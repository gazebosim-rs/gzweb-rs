#!/usr/bin/env node
// gzweb side of the parity harness: load the real, unmodified gzweb in
// headless Chromium, connect it to a Gazebo websocket server, wait for the
// scene to settle and write a gz-parity-snapshot/1 JSON file.
//
//   node parity/js/snapshot.mjs --url ws://localhost:9012 --out snap.json \
//     [--settle-ms 1000] [--timeout-ms 60000] [--screenshot shot.png] \
//     [--console-log console.txt] [--precision 6] [--gzweb ../gzweb] \
//     [--allow-pending] [--headed]
//
// See .agent/PARITY.md §3 and parity/js/README.md.

import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_GZWEB = path.resolve(HERE, "../../../gzweb");

function die(msg, code = 1) {
  process.stderr.write(`snapshot: error: ${msg}\n`);
  process.exit(code);
}

const { values: args } = parseArgs({
  options: {
    url: { type: "string" },
    out: { type: "string" },
    "settle-ms": { type: "string", default: "1000" },
    "timeout-ms": { type: "string", default: "60000" },
    screenshot: { type: "string" },
    "console-log": { type: "string" },
    precision: { type: "string", default: "6" },
    gzweb: { type: "string", default: DEFAULT_GZWEB },
    key: { type: "string" },
    "http-mirror": { type: "string" },
    "allow-pending": { type: "boolean", default: false },
    headed: { type: "boolean", default: false },
    help: { type: "boolean", short: "h", default: false },
  },
  strict: true,
});

if (args.help || !args.url || !args.out) {
  process.stderr.write(
    "usage: snapshot.mjs --url ws://host:port --out file.json [--settle-ms 1000]\n" +
      "         [--timeout-ms 60000] [--screenshot file.png] [--console-log file]\n" +
      "         [--precision 6] [--gzweb <path to gzweb checkout>] [--key <auth key>]\n" +
      "         [--http-mirror http://127.0.0.1:9101] [--allow-pending] [--headed]\n",
  );
  process.exit(args.help ? 0 : 2);
}

const settleMs = Number(args["settle-ms"]);
const timeoutMs = Number(args["timeout-ms"]);
const precision = Number(args.precision);
const gzwebRoot = path.resolve(args.gzweb);
const gzwebBundle = path.join(gzwebRoot, "dist", "gzweb.module.js");
if (!existsSync(gzwebBundle)) {
  die(
    `${gzwebBundle} not found; run 'npm ci && npm run build' in ${gzwebRoot}`,
  );
}
if (!existsSync(path.join(gzwebRoot, "node_modules", "three"))) {
  die(`${gzwebRoot}/node_modules missing; run 'npm ci' there`);
}

// ------------------------------------------------------------ bundle
// dist/gzweb.module.js leaves three, protobufjs, rxjs, jszip, ... as bare
// imports, several of which are CommonJS only, so an import map cannot serve
// them. Bundle gzweb + its own node_modules (one copy of three) into an IIFE.
async function buildBundle() {
  const esbuild = await import("esbuild");
  const result = await esbuild.build({
    stdin: {
      contents:
        "import * as THREE from 'three';\n" +
        "import * as gzweb from './dist/gzweb.module.js';\n" +
        "window.THREE = THREE;\nwindow.gzweb = gzweb;\n",
      resolveDir: gzwebRoot,
      sourcefile: "gz-parity-entry.js",
    },
    bundle: true,
    format: "iife",
    platform: "browser",
    write: false,
    logLevel: "silent",
    nodePaths: [path.join(gzwebRoot, "node_modules")],
    define: { "process.env.NODE_ENV": '"production"' },
  });
  return result.outputFiles[0].contents;
}

// ------------------------------------------------------------ server
async function startServer(bundle) {
  const files = {
    "/": ["text/html", await readFile(path.join(HERE, "harness.html"))],
    "/bundle.js": ["text/javascript", bundle],
    "/dump.js": ["text/javascript", await readFile(path.join(HERE, "dump.js"))],
  };
  const server = createServer((req, res) => {
    const p = new URL(req.url, "http://x").pathname;
    const f = files[p === "/index.html" ? "/" : p];
    if (!f) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": f[0], "cache-control": "no-store" });
    res.end(f[1]);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return server;
}

// ------------------------------------------------------------ output
// Stable float output: round to `precision` decimals, normalise -0, keep
// numeric arrays on one line.
function round(x) {
  if (typeof x !== "number") return x;
  if (!Number.isFinite(x)) return null;
  const f = 10 ** precision;
  const r = Math.round(x * f) / f;
  return Object.is(r, -0) ? 0 : r;
}

function serialize(v, indent = "") {
  if (v === null || typeof v !== "object") {
    return JSON.stringify(typeof v === "number" ? round(v) : v);
  }
  const inner = indent + "  ";
  if (Array.isArray(v)) {
    if (v.length === 0) return "[]";
    if (v.every((x) => x === null || typeof x !== "object")) {
      return "[" + v.map((x) => serialize(x)).join(", ") + "]";
    }
    return (
      "[\n" +
      v.map((x) => inner + serialize(x, inner)).join(",\n") +
      "\n" +
      indent +
      "]"
    );
  }
  const keys = Object.keys(v);
  if (keys.length === 0) return "{}";
  return (
    "{\n" +
    keys
      .map((k) => `${inner}${JSON.stringify(k)}: ${serialize(v[k], inner)}`)
      .join(",\n") +
    "\n" +
    indent +
    "}"
  );
}

// ------------------------------------------------------------ main
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const { chromium } = await import("playwright");
  const bundle = await buildBundle();
  const server = await startServer(bundle);
  const port = server.address().port;

  const consoleLines = [];
  const browser = await chromium.launch({
    headless: !args.headed,
    args: [
      "--use-angle=swiftshader",
      "--enable-unsafe-swiftshader",
      "--ignore-gpu-blocklist",
      "--enable-webgl",
    ],
  });
  let exitCode = 0;
  try {
    const page = await browser.newPage({
      viewport: { width: 1280, height: 720 },
      deviceScaleFactor: 1,
    });
    page.on("console", (m) => consoleLines.push(`[${m.type()}] ${m.text()}`));
    const pageErrors = [];
    page.on("pageerror", (e) => {
      pageErrors.push(e.message);
      consoleLines.push(`[pageerror] ${e.message}`);
    });
    page.on("requestfailed", (r) =>
      consoleLines.push(
        `[requestfailed] ${r.url()} ${r.failure()?.errorText ?? ""}`,
      ),
    );

    if (args["http-mirror"]) {
      // Send every HTTPS asset through gz-parity's mirror: <mirror>/<host>/<path>.
      const mirror = args["http-mirror"].replace(/\/$/, "");
      await page.route(/^https:\/\//, async (route) => {
        const u = new URL(route.request().url());
        const resp = await fetch(`${mirror}/${u.host}${u.pathname}${u.search}`);
        await route.fulfill({
          status: resp.status,
          headers: {
            "content-type": resp.headers.get("content-type") ?? "application/octet-stream",
            "access-control-allow-origin": "*",
          },
          body: Buffer.from(await resp.arrayBuffer()),
        });
      });
    }

    const q = new URLSearchParams({ ws: args.url });
    if (args.key) q.set("key", args.key);
    await page.goto(`http://127.0.0.1:${port}/?${q}`, {
      waitUntil: "load",
      timeout: timeoutMs,
    });

    const deadline = Date.now() + timeoutMs;
    const getState = () =>
      page.evaluate(() => window.__gzParity.state(window.__gzSceneManager));

    // 1. ready, 2. all loads finished, 3/4. quiet for settle-ms.
    let st;
    let phase = "connect";
    let quietSince = null;
    let lastFp = null;
    for (;;) {
      st = await getState();
      if (st.status === "error" || st.statusHistory.includes("error")) {
        throw new Error(`websocket connection to ${args.url} failed`);
      }
      const ready = st.status === "ready" && st.hasScene;
      const idle =
        st.pendingAssets === 0 &&
        st.pendingLoaders === 0 &&
        st.pendingDefault <= 0;
      if (ready) phase = idle ? "settle" : "loading";
      if (ready && idle) {
        if (st.fingerprint !== lastFp) {
          lastFp = st.fingerprint;
          quietSince = Date.now();
        } else if (Date.now() - quietSince >= settleMs) {
          break;
        }
      } else {
        quietSince = null;
        lastFp = null;
      }
      if (Date.now() > deadline) {
        const detail =
          phase === "connect"
            ? `status=${st.status}, scene received=${st.hasScene}`
            : `pending assets=${st.pendingAssets} [${st.pendingAssetUris.join(" ")}], ` +
              `pending loader items=${st.pendingLoaders}, default manager=${st.pendingDefault}`;
        if (phase === "loading" && args["allow-pending"]) {
          process.stderr.write(
            `snapshot: warning: timed out with loads pending (${detail}); dumping anyway\n`,
          );
          break;
        }
        const errs = [...new Set(pageErrors)].slice(-3);
        throw new Error(
          `timed out after ${timeoutMs} ms in phase '${phase}' (${detail})` +
            (errs.length ? `; page errors: ${errs.join(" | ")}` : ""),
        );
      }
      await sleep(50);
    }

    const { snapshot, diagnostics } = await page.evaluate(() =>
      window.__gzParity.dump(window.__gzSceneManager),
    );
    for (const p of diagnostics.missing) {
      process.stderr.write(
        `snapshot: warning: no gzweb renderer node for '${p}'\n`,
      );
    }
    await writeFile(args.out, serialize(snapshot) + "\n");
    process.stderr.write(
      `snapshot: wrote ${args.out} (${snapshot.nodes.length} nodes, world '${snapshot.world}')\n`,
    );

    if (args.screenshot) {
      await page.screenshot({ path: args.screenshot });
    }
  } catch (e) {
    process.stderr.write(`snapshot: error: ${e.message}\n`);
    exitCode = 1;
  } finally {
    if (args["console-log"]) {
      await writeFile(args["console-log"], consoleLines.join("\n") + "\n");
    }
    await browser.close();
    server.close();
  }
  process.exit(exitCode);
}

main().catch((e) => die(e.stack || String(e)));
