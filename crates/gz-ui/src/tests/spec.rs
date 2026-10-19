//! The spec's own checks, computed from its data files (DESIGN.md §4.4,
//! §13): the generated tables match `tokens.json`, the §13 contrast pairs
//! pass in both themes, no colour literals outside `theme.rs`, and the
//! embedded fonts and icons are the ones the spec lists.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde_json::Value;

use crate::fonts::FONT_FILES;
use crate::icons::IconId;
use crate::theme::{
    border, motion, radius, ColorRole, FontFamily, Palette, Sizes, TypeStyle, SPACE,
};

fn design() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/ui")
}

fn json(name: &str) -> Value {
    let text = std::fs::read_to_string(design().join(name)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// `#RRGGBB[AA]` → sRGB components in 0..=1.
fn parse_hex(s: &str) -> [f32; 4] {
    let h = s.trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f32 / 255.0;
    let a = if h.len() == 8 { b(6) } else { 1.0 };
    [b(0), b(2), b(4), a]
}

#[test]
fn generated_palettes_match_tokens_json() {
    let t = json("tokens.json");
    for (theme, palette) in [("light", Palette::light()), ("dark", Palette::dark())] {
        let colors = t["color"][theme].as_object().unwrap();
        assert_eq!(colors.len(), ColorRole::TOKENS.len(), "{theme}");
        for role in ColorRole::TOKENS {
            let name = role.token().unwrap();
            let want = parse_hex(colors[name].as_str().unwrap());
            let got = palette.role(role).to_srgba();
            let got = [got.red, got.green, got.blue, got.alpha];
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 1e-6, "{theme}.{name}: {got:?} vs {want:?}");
            }
        }
    }
    assert_eq!(ColorRole::Clear.token(), None);
    assert_eq!(Palette::light().role(ColorRole::Clear), Color::NONE);
}

#[test]
fn generated_sizes_spacing_and_type_match_tokens_json() {
    let t = json("tokens.json");
    let sizes = Sizes::default();
    let reflected = sizes.reflect_ref();
    let bevy::reflect::ReflectRef::Struct(s) = reflected else {
        panic!("Sizes is a struct")
    };
    let want = t["size"].as_object().unwrap();
    assert_eq!(s.field_len(), want.len());
    for (name, v) in want {
        let got = s
            .field(name)
            .and_then(|f| f.try_downcast_ref::<f32>())
            .copied();
        assert_eq!(got, Some(v.as_f64().unwrap() as f32), "size.{name}");
    }
    for (k, v) in t["space"].as_object().unwrap() {
        let i: usize = k.parse().unwrap();
        assert_eq!(SPACE[i], v.as_f64().unwrap() as f32, "space.{k}");
    }
    assert_eq!(SPACE[0], 0.0);
    let r = &t["radius"];
    assert_eq!(radius::XS, r["xs"].as_f64().unwrap() as f32);
    assert_eq!(radius::SM, r["sm"].as_f64().unwrap() as f32);
    assert_eq!(radius::MD, r["md"].as_f64().unwrap() as f32);
    assert_eq!(radius::LG, r["lg"].as_f64().unwrap() as f32);
    assert_eq!(radius::XL, r["xl"].as_f64().unwrap() as f32);
    assert_eq!(radius::PILL, r["pill"].as_f64().unwrap() as f32);
    assert_eq!(border::HAIRLINE, 1.0);
    assert_eq!(border::FOCUS_RING, 2.0);
    assert_eq!(border::FOCUS_HALO, 3.0);
    assert_eq!(motion::CAMERA_SNAP_MS, 250.0);
    assert_eq!(motion::HOVER_MS, 80.0);
    let ty = |name: &str| {
        let v = &t["type"][name];
        TypeStyle {
            font: if v["font"] == "mono" {
                FontFamily::Mono
            } else {
                FontFamily::Ui
            },
            size: v["size"].as_f64().unwrap() as f32,
            weight: v["weight"].as_u64().unwrap() as u16,
            uppercase: v["uppercase"].as_bool().unwrap_or(false),
            tracking: v["tracking"].as_f64().unwrap_or(0.0) as f32,
        }
    };
    assert_eq!(t["type"].as_object().unwrap().len(), 13);
    for (name, style) in [
        ("caps", TypeStyle::CAPS),
        ("section", TypeStyle::SECTION),
        ("small", TypeStyle::SMALL),
        ("mono_small", TypeStyle::MONO_SMALL),
        ("body", TypeStyle::BODY),
        ("label", TypeStyle::LABEL),
        ("row", TypeStyle::ROW),
        ("heading", TypeStyle::HEADING),
        ("entity_name", TypeStyle::ENTITY_NAME),
        ("clock", TypeStyle::CLOCK),
        ("dialog_title", TypeStyle::DIALOG_TITLE),
        ("micro", TypeStyle::MICRO),
        ("tiny", TypeStyle::TINY),
    ] {
        assert_eq!(style, ty(name), "type.{name}");
    }
}

/// WCAG 2 relative luminance of an sRGB colour.
fn luminance(c: [f32; 4]) -> f64 {
    let lin = |v: f32| {
        let v = f64::from(v);
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn contrast(a: [f32; 4], b: [f32; 4]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The §13 pairs: (foreground, background).
const PAIRS: [(&str, &str); 16] = [
    ("text", "surface"),
    ("text_2", "surface"),
    ("text_3", "surface"),
    ("text_3", "bg"),
    ("text_3", "field"),
    ("text_3", "hover"),
    ("accent_text", "accent_soft"),
    ("warn_text", "warn_soft"),
    ("danger", "surface"),
    ("on_primary", "primary"),
    ("tooltip_text", "tooltip"),
    ("tooltip_text_2", "tooltip"),
    ("surface", "warn_badge"),
    ("axis_x_text", "field"),
    ("axis_y_text", "field"),
    ("axis_z_text", "field"),
];

#[test]
fn contrast_pairs_pass_in_both_themes() {
    let t = json("tokens.json");
    let mut lowest = f64::MAX;
    for theme in ["light", "dark"] {
        let c = |n: &str| parse_hex(t["color"][theme][n].as_str().unwrap());
        for (fg, bg) in PAIRS {
            let r = contrast(c(fg), c(bg));
            assert!(r >= 4.5, "{theme}: {fg} on {bg} is {r:.2}:1");
            lowest = lowest.min(r);
        }
    }
    // §4.4: the lowest pair is dark text_3 on hover at 4.53.
    assert!((lowest - 4.53).abs() < 0.01, "{lowest}");
    // The formula itself: black on white is 21:1.
    assert!((contrast([0.0; 4], [1.0; 4]) - 21.0).abs() < 1e-9);
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `#` then 6 or 8 hex digits, not followed by another word character.
fn has_hex_literal(line: &str) -> bool {
    let b = line.as_bytes();
    (0..b.len()).any(|i| {
        if b[i] != b'#' {
            return false;
        }
        let n = b[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        let after = b.get(i + 1 + n);
        (n == 6 || n == 8) && !after.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    })
}

#[test]
fn no_colour_literals_outside_theme_rs() {
    // Built from pieces so this file doesn't match itself.
    let calls: Vec<String> = [
        "srgb(",
        "srgba(",
        "srgb_u8(",
        "srgba_u8(",
        "rgb(",
        "rgba(",
        "rgb_u8(",
        "rgba_u8(",
        "linear_rgb(",
        "linear_rgba(",
        "hsl(",
        "hsla(",
        "oklch(",
        "oklab(",
        "WHITE",
        "BLACK",
    ]
    .iter()
    .map(|c| format!("Color{}{c}", "::"))
    .chain(
        ["Srgba", "LinearRgba"]
            .iter()
            .map(|t| format!("{t}{}new(", "::")),
    )
    .chain([format!("{}{}", "palettes", "::")])
    .collect();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 20);
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("src/theme.rs")) {
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if calls.iter().any(|c| line.contains(c.as_str())) || has_hex_literal(line) {
                found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "colour literals:\n{}", found.join("\n"));
    // The detector itself.
    assert!(has_hex_literal(&format!("\"{}1C64F2\"", '#')));
    assert!(has_hex_literal(&format!("{}12151A1F)", '#')));
    assert!(!has_hex_literal("#[derive(Debug)]"));
    assert!(!has_hex_literal(&format!("{}1C64F2FF00", '#')));
}

#[test]
fn embedded_fonts_are_the_ones_tokens_json_lists() {
    let t = json("tokens.json");
    let listed: Vec<String> = ["ui", "mono"]
        .iter()
        .flat_map(|f| t["font"][f]["files"].as_array().unwrap().clone())
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let embedded: Vec<&str> = FONT_FILES.iter().map(|(n, _)| *n).collect();
    assert_eq!(listed, embedded);
    for (name, bytes) in FONT_FILES {
        // TrueType outlines, and the OFL licence string in the name table.
        assert_eq!(&bytes[..4], &[0, 1, 0, 0], "{name}");
        let utf16: Vec<u8> = "SIL Open Font License"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        assert!(
            bytes.windows(utf16.len()).any(|w| w == utf16),
            "{name} names the OFL"
        );
    }
    let ofl =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/OFL.txt"))
            .unwrap();
    assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
    assert!(ofl.contains("The Geist Project Authors"));
}

#[test]
fn icon_ids_follow_icons_json() {
    let icons = json("icons.json");
    let names: Vec<&String> = icons["icons"].as_object().unwrap().keys().collect();
    assert_eq!(names.len(), 89);
    assert_eq!(IconId::ALL.len(), names.len());
    for (i, (id, name)) in IconId::ALL.iter().zip(names).enumerate() {
        assert_eq!(id.name(), name);
        assert_eq!(id.index(), i);
    }
}
