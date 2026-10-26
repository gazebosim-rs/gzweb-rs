//! The spec's own checks, computed from its data files (DESIGN.md §4.4,
//! §13): the generated tables match `tokens.json`, the §13 contrast pairs
//! pass in both themes, no colour literals outside `theme/tokens.rs`, the
//! embedded fonts and icons are the ones the spec lists, and the icon pairs
//! §13 names differ.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde_json::Value;

use crate::assets::fonts::FONT_FILES;
use crate::assets::icons::IconId;
use crate::theme::{
    border, elevation, motion, radius, ColorRole, FontFamily, Palette, Sizes, TypeStyle, SPACE,
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
    // Radii, borders and durations: every entry, by name (each module's
    // `ALL` lists its constants).
    for (group, all) in [("radius", &radius::ALL[..]), ("border", &border::ALL[..])] {
        let want = t[group].as_object().unwrap();
        assert_eq!(all.len(), want.len(), "{group}");
        for (name, v) in want {
            let got = all.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
            assert_eq!(got, Some(v.as_f64().unwrap() as f32), "{group}.{name}");
        }
    }
    let motion_json = t["motion"].as_object().unwrap();
    let durations: Vec<_> = motion_json.iter().filter(|(_, v)| v.is_number()).collect();
    assert_eq!(motion::ALL.len(), durations.len());
    for (name, v) in durations {
        let got = motion::ALL.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(got, Some(v.as_f64().unwrap() as f32), "motion.{name}");
    }
    assert_eq!(motion::EASING, motion_json["easing"]);
    assert_eq!(motion::REDUCE_MOTION, motion_json["reduce_motion"]);
    // Elevation: blur, offset and colour.
    let elevation_json = t["elevation"].as_object().unwrap();
    assert_eq!(elevation::ALL.len(), elevation_json.len());
    for (name, v) in elevation_json {
        let (_, e) = elevation::ALL.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(e.blur, v["blur"].as_f64().unwrap() as f32, "{name}");
        assert_eq!(e.y, v["y"].as_f64().unwrap() as f32, "{name}");
        assert_eq!(
            e.color.and_then(ColorRole::token),
            v.get("color").and_then(Value::as_str),
            "elevation.{name}.color"
        );
    }
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
        let c = |n: &str| {
            let c = parse_hex(t["color"][theme][n].as_str().unwrap());
            // The formula is for opaque colours; a translucent token would
            // need compositing over its background first.
            assert_eq!(c[3], 1.0, "{theme}.{n} is translucent");
            c
        };
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

/// `#` then 3, 4, 6 or 8 hex digits, not followed by another word
/// character.
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
        matches!(n, 3 | 4 | 6 | 8)
            && !after.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    })
}

/// The text without `//` comments (a `//` inside a string literal stays).
fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            let mut in_string = false;
            let mut prev = ' ';
            for (i, c) in line.char_indices() {
                if c == '"' && prev != '\\' {
                    in_string = !in_string;
                }
                if !in_string && c == '/' && prev == '/' {
                    return &line[..i - 1];
                }
                prev = c;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Identifiers and `::` in order, across lines.
fn tokens(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        if word(b[i]) {
            let start = i;
            while i < b.len() && word(b[i]) {
                i += 1;
            }
            out.push(&code[start..i]);
        } else if b[i..].starts_with(b"::") {
            out.push("::");
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// Colour construction outside the theme: a colour-space type (`Srgba`,
/// `Oklcha`, …, constructors, struct literals and constants alike), the
/// palettes, any `Color::…` other than `Color::NONE`, a renamed `Color`,
/// and the colour components' constants (`TextColor::WHITE`).
fn colour_tokens(code: &str) -> Vec<String> {
    // `$` keeps this file from matching itself.
    let spaces: Vec<String> = [
        "S$rgba",
        "LinearR$gba",
        "H$sla",
        "H$sva",
        "H$wba",
        "L$aba",
        "L$cha",
        "Okl$aba",
        "Okl$cha",
        "X$yza",
        "pal$ettes",
    ]
    .iter()
    .map(|s| s.replace('$', ""))
    .collect();
    let t = tokens(code);
    let mut found = Vec::new();
    for (i, &tok) in t.iter().enumerate() {
        let next = |k: usize| t.get(i + k).copied().unwrap_or("");
        let hit = spaces.iter().any(|s| s == tok)
            || (tok == "Color" && next(1) == "::" && next(2) != "NONE")
            || (tok == "Color" && next(1) == "as")
            || (matches!(tok, "TextColor" | "BackgroundColor" | "BorderColor")
                && next(1) == "::"
                && next(2).starts_with(|c: char| c.is_ascii_uppercase()));
        if hit {
            found.push(format!("{tok} {} {}", next(1), next(2)));
        }
    }
    found
}

#[test]
fn no_colour_literals_outside_the_theme_tokens() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 20);
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("src/theme/tokens.rs")) {
        let code = strip_comments(&std::fs::read_to_string(file).unwrap());
        for (n, line) in code.lines().enumerate() {
            if has_hex_literal(line) {
                found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
        for hit in colour_tokens(&code) {
            found.push(format!("{}: {hit}", file.display()));
        }
    }
    assert!(found.is_empty(), "colour literals:\n{}", found.join("\n"));

    // The detectors themselves (built from pieces so this file doesn't
    // match itself).
    let h = |s: &str| has_hex_literal(&s.replace('$', "#"));
    assert!(h("\"$1C64F2\""));
    assert!(h("$12151A1F)"));
    assert!(h("\"$FFF\""));
    assert!(h("$fff8,"));
    assert!(!h("#[derive(Debug)]"));
    assert!(!h("$1C64F2FF00"));
    assert!(!h("$1C64F"));
    let c = |s: &str| !colour_tokens(&s.replace('$', "")).is_empty();
    for bad in [
        "Col$or::srgb(1.0, 0.0, 0.0)",
        "Col$or::\n    srgb(1.0, 0.0, 0.0)",
        "Col$or::WHITE",
        "Col$or::oklcha(0.5, 0.1, 30.0, 1.0)",
        "Col$or::hsv(0.0, 1.0, 1.0)",
        "S$rgba::hex(\"1C64F2\")",
        "S$rgba::rgb(1.0, 0.0, 0.0)",
        "S$rgba::rgb_u8(1, 2, 3)",
        "S$rgba::WHITE",
        "S$rgba { red: 1.0, green: 0.0, blue: 0.0, alpha: 1.0 }",
        "LinearR$gba::rgb(1.0, 0.0, 0.0)",
        "H$sla::new(0.0, 1.0, 0.5, 1.0)",
        "Okl$cha::new(0.5, 0.1, 30.0, 1.0)",
        "use bevy::color::pal$ettes::css::RED;",
        "use bevy::prelude::Col$or as C;",
        "TextCol$or::WHITE",
        "BackgroundCol$or::DEFAULT",
    ] {
        assert!(c(bad), "{bad}");
    }
    for fine in [
        "Outline::new(px(2), px(0), Color::$NONE)",
        "let c = color.to_srgba();",
        "fn hex(color: Color) -> String",
        "BorderColor::all(c)",
        "TextColor(c)",
    ] {
        assert!(!c(fine), "{fine}");
    }
    assert_eq!(
        strip_comments("let a = 1; // Col$or::srgb\nlet b = \"http://x\";").replace('$', ""),
        "let a = 1; \nlet b = \"http://x\";"
    );
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

/// An icon's glyph in `icons.json`: its shapes (paths, rects, circles)
/// and fill, without its note.
fn glyph(icons: &Value, name: &str) -> String {
    let mut shape = icons["icons"][name]
        .as_object()
        .unwrap_or_else(|| panic!("no icon {name}"))
        .clone();
    shape.remove("note");
    assert!(
        ["paths", "rects", "circles"]
            .iter()
            .any(|k| shape.contains_key(*k)),
        "{name} has no shapes"
    );
    Value::Object(shape).to_string()
}

/// §13: no glyph does two different actions. The pairs the spec names
/// have different shapes (Scene against the menu's "more", the nearest
/// it has to a hamburger), and so do the icons the UI gives those actions:
/// the palette's commands, the tool, the World menu's item and the panels.
/// Two pairs in `icons.json` itself share a glyph, revert and undo, track
/// and eye; they wait on the design (WORKLOG M10e).
#[test]
fn the_named_icon_pairs_differ() {
    use crate::panels::{PanelRegistry, SCENE, TELEOP};
    use crate::shell::palette::command_icon;
    use crate::shell::top_bar::world_menu::WorldItem;
    use crate::shell::{Tool, WORKSPACES_PANEL};
    let icons = json("icons.json");
    let differ = |group: &[&str]| {
        for (i, a) in group.iter().enumerate() {
            for b in &group[i + 1..] {
                assert_ne!(
                    glyph(&icons, a),
                    glyph(&icons, b),
                    "{a} and {b} share a glyph"
                );
            }
        }
    };
    let groups: [&[&str]; 4] = [
        &["reset", "rotate", "revert"],
        &["activity_scene", "more"],
        &["activity_workspaces", "layout_split"],
        &["focus", "track", "activity_teleop"],
    ];
    for group in groups {
        differ(group);
    }
    // The icons the actions show.
    let app = crate::test_support::shell::shell_app((1440, 900), 1.0);
    let panels = app.world().resource::<PanelRegistry>();
    let panel = |id| panels.get(id).expect("registered").icon.name();
    let revert = WorldItem::Revert.parts().1.expect("an icon").name();
    differ(&[
        command_icon("sim.reset").name(),
        Tool::Rotate.icon().name(),
        revert,
    ]);
    differ(&[panel(SCENE), "more"]);
    differ(&[
        panel(WORKSPACES_PANEL),
        command_icon("view.split_right").name(),
    ]);
    differ(&[
        command_icon("view.focus").name(),
        command_icon("view.track").name(),
        panel(TELEOP),
    ]);
}

/// The string literals in `code`, in order (escapes left as written):
/// plain and byte strings, and raw ones (`r"…"`, `r#"…"#`). A scanner,
/// not a parser (comments are stripped before it; of char literals it
/// knows only `'"'`), which is enough for the binding-id check: a stray
/// literal shaped like a binding fails it loudly rather than passing.
fn string_literals(code: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let b = code.as_bytes();
    let ident = |i: usize| i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
    let mut i = 0;
    while i < b.len() {
        // A raw string: r, then hashes, then a quote; it ends at a quote
        // followed by as many hashes, and has no escapes.
        let byte = i > 0 && b[i - 1] == b'b' && !ident(i - 1);
        if b[i] == b'r' && (!ident(i) || byte) {
            let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
            let open = i + 1 + hashes;
            if b.get(open) == Some(&b'"') {
                let start = open + 1;
                let close = format!("\"{}", "#".repeat(hashes));
                let end = code[start..].find(&close).map_or(b.len(), |n| start + n);
                out.push((start, &code[start..end]));
                i = end + close.len();
                continue;
            }
        }
        if b[i] == b'"' && (i == 0 || b[i - 1] != b'\'') {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            out.push((start, &code[start..j.min(b.len())]));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// The scanner finds plain, byte and raw strings, and isn't fooled by a
/// quote inside a raw string or a char literal.
#[test]
fn string_literals_include_raw_strings() {
    let code = r##"let a = "x.y"; let q = '"'; let b = r"panel.scene"; let c = r#"say "hi""#; let d = br#"z"#;"##;
    let found: Vec<&str> = string_literals(code).into_iter().map(|s| s.1).collect();
    assert_eq!(found, ["x.y", "panel.scene", "say \"hi\"", "z"]);
}

/// A binding id's shape: `group.name`, lowercase (`panel.scene`).
fn binding_shaped(s: &str) -> bool {
    let Some((group, name)) = s.split_once('.') else {
        return false;
    };
    let word = |w: &str, digits: bool| {
        w.starts_with(|c: char| c.is_ascii_lowercase())
            && w.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || (digits && c.is_ascii_digit()))
    };
    word(group, false)
        && word(name, true)
        && !["json", "png", "rs", "ttf", "svg", "wesl"].contains(&name)
}

/// Every shortcut binding the code names is one `shortcuts.json` has: a
/// wrong id gives no shortcut and no tooltip hint, silently. The panels'
/// bindings (activity bar, header actions) and the tools' are checked
/// from the registry and `Tool::ALL`; every other one from the source:
/// any `group.name` literal outside the tests is a binding id, except an
/// action id (`ActionId::new("…")`).
#[test]
fn every_binding_id_is_in_shortcuts_json() {
    use gz_bevy::view::input::InputMap;

    use crate::panels::{PanelPlugins, PanelRegistry};
    use crate::shell::Tool;

    let map = InputMap::builtin();
    let mut app = App::new();
    app.add_plugins(PanelPlugins);
    let panels = app.world().resource::<PanelRegistry>();
    let mut ids: Vec<String> = Vec::new();
    for p in panels.iter() {
        ids.extend(p.binding.map(str::to_string));
        ids.extend(
            p.header_actions
                .iter()
                .filter_map(|a| a.binding)
                .map(str::to_string),
        );
    }
    ids.extend(Tool::ALL.into_iter().map(|t| t.binding().to_string()));
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let test_code = |f: &Path| {
        f.ends_with("tests.rs")
            || f.ends_with("spec_checks.rs")
            || f.components().any(|c| c.as_os_str() == "test_support")
    };
    let mut found = 0;
    for file in files.iter().filter(|f| !test_code(f)) {
        let code = strip_comments(&std::fs::read_to_string(file).unwrap());
        for (at, lit) in string_literals(&code) {
            let action = code[..at].trim_end_matches('"').ends_with("ActionId::new(");
            if binding_shaped(lit) && !action {
                ids.push(format!("{lit} ({})", file.display()));
                found += 1;
            }
        }
    }
    assert!(found > 10, "the scan finds the bindings ({found})");
    let unknown: Vec<&String> = ids
        .iter()
        .filter(|id| map.binding(id.split(' ').next().unwrap()).is_none())
        .collect();
    assert!(unknown.is_empty(), "not in shortcuts.json: {unknown:?}");

    // The scanner itself.
    assert_eq!(
        string_literals(r#"a("x.y", '"', "q\"r") ActionId::new("p.q")"#)
            .iter()
            .map(|l| l.1)
            .collect::<Vec<_>>(),
        ["x.y", r#"q\"r"#, "p.q"]
    );
    assert!(binding_shaped("ui.workspace_1") && binding_shaped("tree.expand_all"));
    assert!(!binding_shaped("tokens.json") && !binding_shaped("2.457"));
    assert!(!binding_shaped("Open world…") && !binding_shaped("a.b.c"));
}
