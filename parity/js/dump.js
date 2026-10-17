// gz-parity-snapshot/1 scene dumper. Runs inside the harness page, against an
// unmodified gzweb SceneManager. See .agent/PARITY.md §3.
//
// Exposes:
//   window.__gzParity.state(sceneMgr)  -> readiness / pending-load summary
//   window.__gzParity.dump(sceneMgr)   -> the snapshot object (unrounded)
//
// Everything gzweb keeps "private" in TypeScript is a plain property at
// runtime, so we read it directly (sceneMgr.transport, sceneMgr.scene, ...).
(function () {
  "use strict";
  const THREE = window.THREE;

  // ---------------------------------------------------------------- helpers

  const v3 = (v) => [v.x, v.y, v.z];
  const q4 = (q) => [q.x, q.y, q.z, q.w];
  const rgb = (c) => [c.r, c.g, c.b];

  function decompose(m) {
    const p = new THREE.Vector3();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    m.decompose(p, q, s);
    return { pos: v3(p), quat: q4(q), scale: v3(s) };
  }

  function isLightWrapper(obj) {
    return obj.children.some((c) => c.isLight);
  }

  function renderables(root) {
    const out = [];
    root.traverse((o) => {
      if (o.isMesh) out.push(o);
    });
    return out;
  }

  function vertexCount(mesh) {
    const g = mesh.geometry;
    if (!g || !g.attributes || !g.attributes.position) return 0;
    return g.attributes.position.count;
  }

  // protobufjs gives enums as numbers on decoded Message instances (gzweb
  // decodes with Type.decode, never toObject), but tolerate strings too.
  function enumName(msg, field, value) {
    if (value === null || value === undefined) return null;
    if (typeof value === "string") return value;
    try {
      const f = msg.$type.fields[field];
      f.resolve();
      const name = f.resolvedType.valuesById[value];
      if (name !== undefined) return name;
    } catch (e) {
      /* fall through */
    }
    return String(value);
  }

  function list(x) {
    if (!x) return [];
    return Array.isArray(x) ? x : [x];
  }

  // Map texture objects back to the uri gzweb loaded them from.
  function textureUris(gzScene) {
    const m = new Map();
    const cache = gzScene && gzScene.textureCache;
    if (cache && typeof cache.forEach === "function") {
      cache.forEach((tex, uri) => m.set(tex, uri));
    }
    return m;
  }

  function texUri(tex, uris) {
    if (!tex) return null;
    if (uris.has(tex)) return uris.get(tex);
    if (tex.name) return tex.name;
    const img = tex.image;
    if (img && typeof img.src === "string" && !img.src.startsWith("data:")) {
      return img.src;
    }
    return "<texture>";
  }

  function colorOrNull(c) {
    return c && c.isColor ? rgb(c) : null;
  }

  function numOrNull(x) {
    return typeof x === "number" && isFinite(x) ? x : null;
  }

  // ---------------------------------------------------------------- blocks

  function geometryBlock(visualMsg, obj) {
    const geomMsg = visualMsg.geometry;
    if (!geomMsg) return null;
    const type = enumName(geomMsg, "type", geomMsg.type);
    const meshes = renderables(obj);
    let aabb = null;
    if (meshes.length > 0) {
      const box = new THREE.Box3().setFromObject(obj, true); // precise: exact vertex bounds, as gzweb-rs computes them
      if (!box.isEmpty()) aabb = { min: v3(box.min), max: v3(box.max) };
    }
    return {
      type: type === null ? null : type.toLowerCase(),
      aabb_world: aabb,
      mesh_count: meshes.length,
      vertex_count: meshes.reduce((a, m) => a + vertexCount(m), 0),
    };
  }

  function materialBlock(obj, uris) {
    const mesh = renderables(obj)[0];
    if (!mesh || !mesh.material) return null;
    const mat = Array.isArray(mesh.material) ? mesh.material[0] : mesh.material;
    if (!mat) return null;
    return {
      color: colorOrNull(mat.color),
      opacity: numOrNull(mat.opacity),
      transparent: !!mat.transparent,
      emissive: colorOrNull(mat.emissive),
      metalness: numOrNull(mat.metalness),
      roughness: numOrNull(mat.roughness),
      maps: {
        albedo: texUri(mat.map, uris),
        normal: texUri(mat.normalMap, uris),
      },
    };
  }

  function lightBlock(wrapper) {
    const l = wrapper.children.find((c) => c.isLight);
    if (!l) return null;
    let type = null;
    if (l.isPointLight) type = "point";
    else if (l.isSpotLight) type = "spot";
    else if (l.isDirectionalLight) type = "directional";
    else type = l.type;
    let dir = null;
    if ((l.isSpotLight || l.isDirectionalLight) && l.target) {
      const lp = new THREE.Vector3().setFromMatrixPosition(l.matrixWorld);
      const tp = new THREE.Vector3().setFromMatrixPosition(
        l.target.matrixWorld,
      );
      const d = tp.sub(lp);
      if (d.lengthSq() > 0) dir = v3(d.normalize());
    }
    return {
      type,
      color: rgb(l.color),
      intensity: l.intensity,
      range: numOrNull(l.distance),
      angle: l.isSpotLight ? numOrNull(l.angle) : null,
      penumbra: l.isSpotLight ? numOrNull(l.penumbra) : null,
      decay: l.isSpotLight || l.isPointLight ? numOrNull(l.decay) : null,
      cast_shadow: !!l.castShadow,
      direction_world: dir,
    };
  }

  // ---------------------------------------------------------------- walk

  function dump(sceneMgr) {
    const transport = sceneMgr.transport;
    const gzScene = sceneMgr.scene;
    const threeScene = gzScene.scene;
    const sceneMsg = transport.sceneInfo$.getValue();
    if (!sceneMsg) throw new Error("no scene message received");

    threeScene.updateMatrixWorld(true);
    const uris = textureUris(gzScene);
    const nodes = [];
    const missing = [];

    // Children of a renderer node, matched by name. `want` filters on
    // whether the child is a light wrapper, so a link's light and a visual
    // with the same name do not collide. `nth` picks among same-named
    // candidates in the order gzweb added them.
    function findChild(parentObj, name, wantLight, nth) {
      if (!parentObj) return null;
      let seen = 0;
      for (const c of parentObj.children) {
        if (c.name !== name) continue;
        if (isLightWrapper(c) !== wantLight) continue;
        if (seen === nth) return c;
        seen++;
      }
      return null;
    }

    // Spec path dedup: second sibling with the same name gets "#1", ...
    function makeNamer(prefix) {
      const counts = new Map();
      return (name) => {
        const n = counts.get(name) || 0;
        counts.set(name, n + 1);
        const seg = n === 0 ? name : `${name}#${n}`;
        return prefix ? `${prefix}/${seg}` : seg;
      };
    }

    // Per-parent counter used for renderer lookup (separate from the path
    // namer: lookup counts only siblings gzweb actually creates and of the
    // same light-ness).
    function makeLookup(parentObj) {
      const counts = new Map();
      return (name, wantLight) => {
        const key = (wantLight ? "L:" : "N:") + name;
        const n = counts.get(key) || 0;
        counts.set(key, n + 1);
        return findChild(parentObj, name, wantLight, n);
      };
    }

    function emit(path, kind, obj, msg) {
      if (!obj) {
        missing.push(path);
        nodes.push({
          path,
          kind,
          local: null,
          world: null,
          visible: null,
          geometry: null,
          material: null,
          light: null,
        });
        return;
      }
      nodes.push({
        path,
        kind,
        local: {
          pos: v3(obj.position),
          quat: q4(obj.quaternion),
          scale: v3(obj.scale),
        },
        world: decompose(obj.matrixWorld),
        visible: !!obj.visible,
        geometry: kind === "visual" ? geometryBlock(msg, obj) : null,
        material: kind === "visual" ? materialBlock(obj, uris) : null,
        light: kind === "light" ? lightBlock(obj) : null,
      });
    }

    function walkModel(modelMsg, parentObj, namer, lookup) {
      const name = modelMsg.name || "";
      const path = namer(name);
      const modelObj = lookup(name, false);
      emit(path, "model", modelObj, modelMsg);

      const childNamer = makeNamer(path);
      const childLookup = makeLookup(modelObj);

      // 1. links -> visuals, lights
      for (const link of list(modelMsg.link)) {
        const lname = link.name || "";
        const lpath = childNamer(lname);
        const linkObj = childLookup(lname, false);
        emit(lpath, "link", linkObj, link);
        const linkNamer = makeNamer(lpath);
        const linkLookup = makeLookup(linkObj);
        for (const vis of list(link.visual)) {
          const vname = vis.name || "";
          emit(linkNamer(vname), "visual", linkLookup(vname, false), vis);
        }
        for (const light of list(link.light)) {
          const n = light.name || "";
          emit(linkNamer(n), "light", linkLookup(n, true), light);
        }
      }

      // 2. model's own visuals. gzweb's spawnModelFromSDF never creates
      // these, so they are reported as missing (all-null) nodes; the lookup
      // counter is deliberately not advanced for them.
      for (const vis of list(modelMsg.visual)) {
        const vname = vis.name || "";
        emit(childNamer(vname), "visual", null, vis);
      }

      // 3. nested models
      for (const nested of list(modelMsg.model)) {
        walkModel(nested, modelObj, childNamer, childLookup);
      }
    }

    const topNamer = makeNamer("");
    const topLookup = makeLookup(threeScene);
    for (const model of list(sceneMsg.model)) {
      walkModel(model, threeScene, topNamer, topLookup);
    }
    // 4. top-level lights
    for (const light of list(sceneMsg.light)) {
      const n = light.name || "";
      emit(topNamer(n), "light", topLookup(n, true), light);
    }

    // ------------------------------------------------------------ scene
    const clear = new THREE.Color();
    gzScene.renderer.getClearColor(clear);
    const cam = gzScene.camera;
    const target =
      gzScene.controls && gzScene.controls.target
        ? v3(gzScene.controls.target)
        : null;
    const grid = threeScene.getObjectByName("grid");

    let sky = null;
    if (sceneMsg.sky) {
      let cubemap = null;
      const hdr = sceneMsg.sky.header;
      if (hdr && hdr.data) {
        for (const d of list(hdr.data)) {
          if (d.key === "cubemap_uri" && d.value && d.value.length) {
            cubemap = d.value[0];
          }
        }
      }
      sky = { cubemap };
    }

    return {
      snapshot: {
        format: "gz-parity-snapshot/1",
        source: "gzweb",
        world: transport.getWorld(),
        scene: {
          ambient: rgb(gzScene.ambient.color),
          clear_color: rgb(clear),
          camera: {
            position: v3(cam.position),
            target,
            fov_deg: cam.fov,
            near: cam.near,
            far: cam.far,
          },
          grid_visible: grid ? !!grid.visible : false,
          sky,
        },
        nodes,
      },
      diagnostics: {
        missing,
        sky_background:
          threeScene.background == null
            ? null
            : threeScene.background.isCubeTexture
              ? "cube_texture"
              : threeScene.background.isColor
                ? "color"
                : "other",
      },
    };
  }

  // ---------------------------------------------------------------- state

  function loaderManagers(gzScene) {
    const out = new Set();
    if (!gzScene) return out;
    for (const k of Object.keys(gzScene)) {
      const v = gzScene[k];
      if (v && typeof v === "object" && v.manager && v.manager.itemStart) {
        out.add(v.manager);
      }
    }
    return out;
  }

  function fingerprint(threeScene) {
    let meshes = 0;
    let verts = 0;
    let objects = 0;
    threeScene.traverse((o) => {
      objects++;
      if (o.isMesh) {
        meshes++;
        verts += vertexCount(o);
      }
    });
    return `${objects}/${meshes}/${verts}`;
  }

  function state(sceneMgr) {
    const status = sceneMgr.getConnectionStatus();
    const hasScene = !!sceneMgr.transport.sceneInfo$.getValue();
    const gzScene = sceneMgr.scene;
    let pendingLoaders = 0;
    for (const m of loaderManagers(gzScene)) {
      // WsLoadingManager keeps these as (TS-private) fields.
      if (typeof m.itemsTotal === "number") {
        pendingLoaders += m.itemsTotal - m.itemsLoaded;
      }
    }
    const h = window.__gzParityHooks || {};
    return {
      status,
      hasScene,
      pendingAssets: h.pendingAssets ? h.pendingAssets.size : 0,
      pendingAssetUris: h.pendingAssets ? [...h.pendingAssets] : [],
      pendingLoaders,
      pendingDefault: h.pendingDefault || 0,
      fingerprint: gzScene && gzScene.scene ? fingerprint(gzScene.scene) : "",
      errors: h.errors || [],
      statusHistory: h.statusHistory || [],
    };
  }

  window.__gzParity = { dump, state };
})();
