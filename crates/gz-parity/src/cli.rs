//! Command-line front end; see the crate docs for the subcommands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use gz_core::snapshot::Snapshot;

use crate::{compare, corpus, record, replay};

/// Runs the CLI and maps errors to exit code 2.
pub fn main_with(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args: Vec<String> = args.into_iter().collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("gz-parity: {e}");
            ExitCode::from(2)
        }
    }
}

/// `--flag value` pairs plus positional arguments.
fn parse(args: &[String]) -> Result<(HashMap<String, String>, Vec<String>), String> {
    let mut flags = HashMap::new();
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            let v = it.next().ok_or(format!("--{name} needs a value"))?;
            flags.insert(name.to_string(), v.clone());
        } else {
            positional.push(a.clone());
        }
    }
    Ok((flags, positional))
}

fn need<'a>(flags: &'a HashMap<String, String>, name: &str) -> Result<&'a str, String> {
    flags
        .get(name)
        .map(String::as_str)
        .ok_or(format!("missing --{name}"))
}

fn num<T: std::str::FromStr>(
    flags: &HashMap<String, String>,
    name: &str,
    default: T,
) -> Result<T, String> {
    match flags.get(name) {
        Some(v) => v.parse().map_err(|_| format!("bad --{name} value '{v}'")),
        None => Ok(default),
    }
}

fn read_snapshot(path: &str) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

fn write_report<T: serde::Serialize>(
    flags: &HashMap<String, String>,
    report: &T,
) -> Result<(), String> {
    if let Some(path) = flags.get("report") {
        let json = serde_json::to_string_pretty(report).map_err(crate::message)?;
        std::fs::write(path, json + "\n").map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

/// Runs one subcommand; `args` excludes the program name.
pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some((cmd, rest)) = args.split_first() else {
        return Err(
            "usage: gz-parity <record|replay|compare|compare-protocol> ... (see .agent/PARITY.md)"
                .into(),
        );
    };
    let (flags, positional) = parse(rest)?;

    match cmd.as_str() {
        "record" => {
            let out = PathBuf::from(need(&flags, "out")?);
            let opts = record::RecordOptions {
                url: need(&flags, "url")?.to_string(),
                duration: Duration::from_millis(num(&flags, "duration-ms", 3000)?),
                max_stream_frames: num(&flags, "max-stream-frames", 200)?,
                world_file: flags.get("world-file").cloned(),
                reply_timeout: Duration::from_millis(num(&flags, "timeout-ms", 20000)?),
            };
            let corpus = record::record(&opts)?;
            corpus.save(&out).map_err(crate::message)?;
            let m = &corpus.manifest;
            eprintln!(
                "recorded world '{}': {} frames, {} assets, streams: {}",
                m.world,
                corpus.frames.len(),
                m.assets.len(),
                m.streams
                    .iter()
                    .map(|(t, v)| format!("{t} ({})", v.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(ExitCode::SUCCESS)
        }
        "replay" => {
            let corpus = corpus::Corpus::load(&PathBuf::from(need(&flags, "corpus")?))
                .map_err(crate::message)?;
            let opts = replay::ReplayOptions {
                port: num(&flags, "port", 9100)?,
                client_log: flags.get("client-log").map(PathBuf::from),
                stream_limit: flags
                    .get("stream-limit")
                    .map(|v| v.parse())
                    .transpose()
                    .map_err(|_| "bad --stream-limit")?,
                connections: num(&flags, "connections", 0)?,
                key: flags.get("key").cloned(),
            };
            replay::serve(&corpus, &opts)?;
            Ok(ExitCode::SUCCESS)
        }
        "compare" => {
            let expected = read_snapshot(need(&flags, "expected")?)?;
            let actual = read_snapshot(need(&flags, "actual")?)?;
            let allow = match flags.get("allow") {
                Some(p) => compare::load_allowlist(std::path::Path::new(p))?,
                None => Vec::new(),
            };
            let report = compare::compare_snapshots(&expected, &actual, &allow, Default::default());
            write_report(&flags, &report)?;
            for d in &report.suppressed {
                println!(
                    "  suppressed [{}] {} {}: {} vs {}",
                    d.suppressed_by.as_deref().unwrap_or(""),
                    d.path,
                    d.field,
                    d.expected,
                    d.actual
                );
            }
            for d in &report.diffs {
                println!(
                    "  DIFF {} {}: expected {} actual {}",
                    d.path, d.field, d.expected, d.actual
                );
            }
            println!(
                "{}: {} nodes vs {}, {} diffs, {} suppressed",
                if report.passed() { "PASS" } else { "FAIL" },
                report.nodes_expected,
                report.nodes_actual,
                report.diffs.len(),
                report.suppressed.len()
            );
            Ok(if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "compare-protocol" => {
            let [e, a] = positional.as_slice() else {
                return Err("compare-protocol needs EXPECTED.log ACTUAL.log".into());
            };
            let read = |p: &str| std::fs::read_to_string(p).map_err(|err| format!("{p}: {err}"));
            let report = compare::compare_protocol(&read(e)?, &read(a)?);
            write_report(&flags, &report)?;
            if !report.sequence_matches {
                println!(
                    "  sequence differs:\n    expected {:?}\n    actual   {:?}",
                    report.sequence_expected, report.sequence_actual
                );
            }
            for x in &report.assets_only_expected {
                println!("  only in expected: {x}");
            }
            for x in &report.assets_only_actual {
                println!("  only in actual:   {x}");
            }
            println!("{}", if report.passed() { "PASS" } else { "FAIL" });
            Ok(if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        other => Err(format!("unknown command '{other}'")),
    }
}
