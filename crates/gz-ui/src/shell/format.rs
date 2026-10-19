//! Text the shell shows for sim and connection state (spec §3.1, §3.6,
//! §7.10).

use gz_bevy::connection::SimStats;

use crate::theme::ColorRole;

/// The sim clock, `HH:MM:SS.mmm` (§3.1). Hours keep counting past 99.
pub fn clock(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    let (h, rest) = (ms / 3_600_000, ms % 3_600_000);
    let (m, rest) = (rest / 60_000, rest % 60_000);
    let (s, ms) = (rest / 1000, rest % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// RTF thresholds (§3.1): `ok` at or above, `warn` from [`RTF_WARN`].
pub const RTF_OK: f64 = 0.9;
/// Below this the RTF dot is `danger`.
pub const RTF_WARN: f64 = 0.5;

/// The RTF readout and its dot's colour (§3.1): "RTF 98%", "paused" (dot
/// `disabled`), or "RTF —" with no stats.
pub fn rtf(stats: Option<&SimStats>) -> (String, ColorRole) {
    match stats {
        None => ("RTF —".into(), ColorRole::Disabled),
        Some(s) if s.paused => ("paused".into(), ColorRole::Disabled),
        Some(s) => {
            let r = s.real_time_factor;
            let role = if r >= RTF_OK {
                ColorRole::Ok
            } else if r >= RTF_WARN {
                ColorRole::Warn
            } else {
                ColorRole::Danger
            };
            (format!("RTF {:.0}%", r * 100.0), role)
        }
    }
}

/// `n` with comma thousands separators.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "iter 134,380" (§3.6 item 4), "iter —" with no stats.
pub fn iterations(stats: Option<&SimStats>) -> String {
    stats.map_or_else(
        || "iter —".into(),
        |s| format!("iter {}", thousands(s.iterations)),
    )
}

/// A step size: "1 ms", "0.5 ms", "1 s".
pub fn step(secs: f64) -> String {
    let trim = |v: f64| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if secs >= 1.0 {
        format!("{} s", trim(secs))
    } else {
        format!("{} ms", trim(secs * 1000.0))
    }
}

/// The physics line (§3.6 item 2): engine and step, each "—" when the
/// server doesn't say (the websocket server reports no engine).
pub fn physics(engine: Option<&str>, step_secs: Option<f64>) -> String {
    match (engine, step_secs.filter(|s| *s > 0.0)) {
        (None, None) => "—".into(),
        (e, s) => format!(
            "{} · {}",
            e.unwrap_or("—"),
            s.map_or_else(|| "—".into(), step)
        ),
    }
}

/// The frame rate (§3.6 item 3).
pub fn fps(fps: Option<f64>) -> String {
    fps.map_or_else(|| "— fps".into(), |f| format!("{f:.0} fps"))
}

/// The server address without the scheme: "localhost:11345".
pub fn address(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.trim_end_matches('/').to_string()
}

/// A signed number with a real minus sign (U+2212), as the boards print
/// negative values.
pub fn signed(v: f32, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v.abs());
    let zero = s.chars().all(|c| c == '0' || c == '.');
    if v < 0.0 && !zero {
        format!("\u{2212}{s}")
    } else {
        s
    }
}

/// "cursor 2.41, −0.73" (§3.6 item 9), "cursor —" off the ground.
pub fn cursor(at: Option<[f32; 2]>) -> String {
    at.map_or_else(
        || "cursor —".into(),
        |[x, y]| format!("cursor {}, {}", signed(x, 2), signed(y, 2)),
    )
}

/// The RTF tooltip's second line (§3.1): real time, sim time, iterations
/// and step size.
pub fn rtf_detail(stats: Option<&SimStats>) -> String {
    stats.map_or_else(
        || "No statistics yet".into(),
        |s| {
            format!(
                "Real {} · sim {} · {} iterations · step {}",
                clock(s.real_time),
                clock(s.sim_time),
                thousands(s.iterations),
                if s.step_size > 0.0 {
                    step(s.step_size)
                } else {
                    "—".into()
                }
            )
        },
    )
}
