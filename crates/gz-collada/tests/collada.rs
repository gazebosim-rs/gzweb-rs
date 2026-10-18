//! Scene assembly on hand-written probes and malformed documents (always),
//! and on real Fuel meshes checked against gzweb's three.js loader (when the
//! spike corpus is present: `spikes/collada/run.sh`, then
//! `cargo test -p gz-collada -- --ignored`).
#![allow(clippy::unwrap_used)]

use std::path::Path;

use gz_collada::{parse, Scene, Shading, Texture, UpAxis};

fn data(name: &str) -> Scene {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn near(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn texture(path: &str) -> Option<Texture> {
    Some(Texture {
        path: path.into(),
        repeat: [true; 2],
    })
}

#[test]
fn hierarchy_composes_parent_first_with_the_unit_on_top() {
    let s = data("hierarchy.dae");
    assert_eq!((s.unit, s.up_axis), (0.01, UpAxis::Y));
    let parent = &s.roots[0];
    assert_eq!(parent.name.as_deref(), Some("ParentNode"));
    assert_eq!(parent.children[0].name.as_deref(), Some("ChildNode"));
    // three.js: the triangle lands at y = 1 m (mesh-loader put it at x = 100).
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [-0.01, 1.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.0, 1.01, 0.0], 1e-5), "{max:?}");
}

#[test]
fn instances_materials_and_library_nodes() {
    let s = data("probe.dae");
    let names: Vec<_> = s.roots.iter().map(|n| n.name.as_deref().unwrap()).collect();
    assert_eq!(
        names,
        ["ParentNode", "SecondInstance", "MultiMat", "RefNode"]
    );
    // One triangle mesh shared by three instances (one through
    // <instance_node>), and a two-material geometry split per primitive.
    assert_eq!(s.meshes.len(), 3);
    assert_eq!(s.instances().len(), 5);
    assert_eq!(s.triangle_count(), 6);
    assert!(s.warnings.is_empty());
    let multi = &s.roots[2];
    assert_eq!(multi.primitives.len(), 2);
    assert_ne!(multi.primitives[0].material, multi.primitives[1].material);
    let lib = &s.roots[3].children[0];
    assert_eq!(lib.name.as_deref(), Some("LibNode"));
    assert_eq!(lib.id.as_deref(), Some("LibNode"));

    let textured = &s.materials[multi.primitives[1].material.unwrap()];
    assert_eq!(textured.name.as_deref(), Some("Textured"));
    assert_eq!(textured.shading, Shading::Phong);
    assert_eq!(textured.opacity, 0.5);
    assert_eq!(textured.shininess, 20.0);
    assert_eq!(
        textured.diffuse_texture,
        texture("../materials/textures/probe.png")
    );
    // FCOLLADA <extra><technique><bump>.
    assert_eq!(
        textured.normal_texture,
        texture("../materials/textures/probe_normal.png")
    );
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [-1.0, 0.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.03, 3.01, 2.0], 1e-5), "{max:?}");
}

#[test]
fn unused_geometries_are_not_emitted() {
    // textured.dae has two geometries nobody instances.
    let s = data("textured.dae");
    assert_eq!(s.instances().len(), 1);
    let (min, max) = s.bounds().unwrap();
    assert!(near(min, [0.02, 0.0, 0.0], 1e-5), "{min:?}");
    assert!(near(max, [0.03, 0.01, 0.0], 1e-5), "{max:?}");
}

#[test]
fn bad_documents_are_errors() {
    assert!(parse(b"not xml").is_err());
    assert!(parse(b"<root/>")
        .unwrap_err()
        .to_string()
        .contains("COLLADA"));
    let empty =
        br#"<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1"/>"#;
    let s = parse(empty).unwrap();
    assert!(s.roots.is_empty() && s.bounds().is_none());
    assert_eq!(s.triangle_count(), 0);
}

// ─── Generated documents (the PR #21 review's reproductions) ─────────────────

/// The pieces of a one-geometry document; `Default` is a valid red
/// triangle instanced once.
struct Doc {
    /// The `<vertices>` POSITION source id.
    positions: &'static str,
    /// The position accessor's `count`.
    count: usize,
    /// The primitive element.
    prim: String,
    /// The `<visual_scene>`'s children.
    scene: String,
    library_nodes: String,
    effect: String,
    unit: &'static str,
}

const INSTANCE: &str = r##"<instance_geometry url="#tri"><bind_material><technique_common><instance_material symbol="m" target="#mat"/></technique_common></bind_material></instance_geometry>"##;

const TRIANGLE: &str = r##"<triangles count="1" material="m"><input semantic="VERTEX" source="#vtx" offset="0"/><p>0 1 2</p></triangles>"##;

impl Default for Doc {
    fn default() -> Self {
        Self {
            positions: "pos",
            count: 3,
            prim: TRIANGLE.into(),
            scene: format!(r#"<node id="n" name="n">{INSTANCE}</node>"#),
            library_nodes: String::new(),
            effect: phong(""),
            unit: "1",
        }
    }
}

/// A red Phong effect with `extra` inside its `<technique>`.
fn phong(extra: &str) -> String {
    format!(
        r#"<effect id="e"><profile_COMMON><technique sid="c"><phong><diffuse><color>1 0 0 1</color></diffuse></phong>{extra}</technique></profile_COMMON></effect>"#
    )
}

impl Doc {
    fn xml(&self) -> String {
        let Doc {
            positions,
            count,
            prim,
            scene,
            library_nodes,
            effect,
            unit,
        } = self;
        format!(
            r##"<?xml version="1.0"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset><unit meter="{unit}"/></asset>
  <library_images><image id="img"><init_from>a.png</init_from></image></library_images>
  <library_effects>{effect}</library_effects>
  <library_materials><material id="mat"><instance_effect url="#e"/></material></library_materials>
  <library_geometries><geometry id="tri" name="Tri"><mesh>
    <source id="pos"><float_array id="pos-a" count="15">0 0 0 1 0 0 0 1 0 1 1 0 2 2 0</float_array>
      <technique_common><accessor source="#pos-a" count="{count}" stride="3">
        <param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/>
      </accessor></technique_common></source>
    <source id="ints"><int_array id="ints-a" count="9">0 0 0 1 0 0 0 1 0</int_array>
      <technique_common><accessor source="#ints-a" count="3" stride="3">
        <param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/>
      </accessor></technique_common></source>
    <source id="names"><Name_array id="names-a" count="2">left right</Name_array></source>
    <vertices id="vtx"><input semantic="POSITION" source="#{positions}"/></vertices>
    {prim}
  </mesh></geometry></library_geometries>
  <library_nodes>{library_nodes}</library_nodes>
  <library_visual_scenes><visual_scene id="S">{scene}</visual_scene></library_visual_scenes>
  <scene><instance_visual_scene url="#S"/></scene>
</COLLADA>"##
        )
    }

    fn parse(&self) -> Result<Scene, gz_collada::Error> {
        parse(self.xml().as_bytes())
    }
}

fn polylist(count: usize, vcount: &str, p: &str) -> String {
    format!(
        r##"<polylist count="{count}" material="m"><input semantic="VERTEX" source="#vtx" offset="0"/><vcount>{vcount}</vcount>{p}</polylist>"##
    )
}

fn node_count(s: &Scene) -> usize {
    fn count(n: &gz_collada::Node) -> usize {
        1 + n.children.iter().map(count).sum::<usize>()
    }
    s.roots.iter().map(count).sum()
}

#[track_caller]
fn error(doc: Doc, wanted: &str) {
    let e = doc.parse().unwrap_err().to_string();
    assert!(e.contains(wanted), "{e:?} lacks {wanted:?}");
}

#[test]
fn the_generated_document_is_valid() {
    // It also holds a <Name_array>, which used to hang the parser.
    let s = Doc::default().parse().unwrap();
    assert_eq!(s.triangle_count(), 1);
    assert_eq!(s.materials[0].diffuse, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn malformed_geometry_is_an_error_not_a_panic() {
    error(
        Doc {
            positions: "nope",
            ..Doc::default()
        },
        "no <source> #nope",
    );
    error(
        Doc {
            positions: "ints",
            ..Doc::default()
        },
        "#ints-a is not a <float_array>",
    );
    error(
        Doc {
            count: 99,
            ..Doc::default()
        },
        "reads 297 floats, #pos-a has 15",
    );
    error(
        Doc {
            prim: polylist(1, "3", ""),
            ..Doc::default()
        },
        "<p> has 0 indices, expected 3",
    );
    // Counts too large to reserve memory for (a capacity overflow on wasm32).
    error(
        Doc {
            prim: polylist(4_000_000_000, "3", "<p>0 1 2</p>"),
            ..Doc::default()
        },
        "expected more values",
    );
    error(
        Doc {
            prim: r##"<triangles count="4000000000"><input semantic="VERTEX" source="#vtx" offset="0"/><p>0 1 2</p></triangles>"##.into(),
            ..Doc::default()
        },
        "expected 12000000000",
    );
    // A <float_array> whose count is far past its contents.
    let huge = Doc::default()
        .xml()
        .replace(r#"count="15""#, r#"count="4000000000""#);
    assert!(parse(huge.as_bytes()).is_err());
}

#[test]
fn unused_broken_geometry_is_not_an_error() {
    // As in three.js, only what the scene instances is built.
    let s = Doc {
        positions: "nope",
        scene: r#"<node id="empty"/>"#.into(),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.triangle_count(), 0);
}

#[test]
fn dangling_scene_references_are_errors() {
    error(
        Doc {
            scene: format!("<node>{}</node>", INSTANCE.replace("#tri", "#gone")),
            ..Doc::default()
        },
        "no geometry #gone",
    );
    error(
        Doc {
            scene: format!("<node>{}</node>", INSTANCE.replace("#mat", "#gone")),
            ..Doc::default()
        },
        "no material #gone",
    );
    error(
        Doc {
            scene: r##"<node><instance_node url="#gone"/></node>"##.into(),
            ..Doc::default()
        },
        "no node #gone",
    );
    let no_effect = Doc::default()
        .xml()
        .replace(r##"url="#e""##, r##"url="#gone""##);
    let e = parse(no_effect.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("no effect #gone"), "{e}");
}

#[test]
fn vertex_inputs_read_the_meshs_own_vertices() {
    // three.js ignores the VERTEX source id; mesh-loader hit `todo!()`.
    let s = Doc {
        prim: TRIANGLE.replace("#vtx", "#elsewhere"),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.triangle_count(), 1);
}

#[test]
fn polylists_skip_points_and_lines() {
    let s = Doc {
        count: 5,
        prim: polylist(3, "2 3 1", "<p>3 4 0 1 2 4</p>"),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.triangle_count(), 1);
    assert_eq!(
        s.meshes[0].positions,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
}

#[test]
fn tristrips_alternate_their_winding_and_trifans_fan() {
    let strip = r##"<tristrips count="1" material="m"><input semantic="VERTEX" source="#vtx" offset="0"/><p>0 1 2 3 4</p></tristrips>"##;
    let s = Doc {
        count: 5,
        prim: strip.into(),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    let v = |i: usize| -> [f32; 3] {
        let all = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
        ];
        all[i]
    };
    let faces = |order: &[usize]| order.iter().map(|&i| v(i)).collect::<Vec<_>>();
    assert_eq!(s.meshes[0].positions, faces(&[0, 1, 2, 2, 1, 3, 2, 3, 4]));
    let fan = strip.replace("tristrips", "trifans");
    let s = Doc {
        count: 5,
        prim: fan,
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.meshes[0].positions, faces(&[0, 1, 2, 0, 2, 3, 0, 3, 4]));
}

/// Library nodes N0..Nn, each instancing the next one twice; Nn holds the
/// triangle.
fn dag(n: usize) -> Doc {
    let mut library_nodes: String = (0..n)
        .map(|i| {
            let next = i + 1;
            format!(
                r##"<node id="N{i}"><instance_node url="#N{next}"/><instance_node url="#N{next}"/></node>"##
            )
        })
        .collect();
    library_nodes += &format!(r#"<node id="N{n}">{INSTANCE}</node>"#);
    Doc {
        scene: r##"<node id="root"><instance_node url="#N0"/></node>"##.into(),
        library_nodes,
        ..Doc::default()
    }
}

#[test]
fn instance_node_expansion_is_bounded() {
    let s = dag(10).parse().unwrap();
    assert_eq!(s.triangle_count(), 1 << 10);
    assert_eq!(node_count(&s), (1 << 11) - 1 + 1);
    // 2^21 nodes is past the budget.
    error(dag(20), "more than 1000000 nodes and primitives");
}

#[test]
fn instance_node_cycles_are_cut() {
    for fan_out in [1, 2] {
        let back = r##"<instance_node url="#A"/>"##.repeat(fan_out);
        let s = Doc {
            scene: r##"<node id="root"><instance_node url="#A"/></node>"##.into(),
            library_nodes: format!(r#"<node id="A">{INSTANCE}{back}</node>"#),
            ..Doc::default()
        }
        .parse()
        .unwrap();
        // root → A, once; A's own instances of A are skipped.
        assert_eq!((node_count(&s), s.triangle_count()), (2, 1));
        assert!(s.warnings[0].contains("instance their own ancestor"));
    }
}

#[test]
fn deep_hierarchies_keep_their_geometry_or_fail_cleanly() {
    let nest = |depth: usize| {
        let mut scene = format!(r#"<node id="leaf">{INSTANCE}</node>"#);
        for _ in 0..depth {
            scene = format!("<node>{scene}</node>");
        }
        Doc {
            scene,
            ..Doc::default()
        }
    };
    let s = nest(100).parse().unwrap();
    assert_eq!((node_count(&s), s.triangle_count()), (101, 1));
    // The parser recurses per <node>; a coverage build's frames are larger
    // than a plain debug build's.
    let thread = std::thread::Builder::new().stack_size(8 << 20);
    let deep = move || error(nest(260), "<node>s nested more than 256 deep");
    thread.spawn(deep).unwrap().join().unwrap();
    // Deep enough to overflow the XML parser's stack.
    error(nest(20_000), "elements nested more than 300 deep");
}

#[test]
fn double_sided_is_read_from_extras() {
    let googleearth = phong(
        r#"<extra><technique profile="GOOGLEEARTH"><double_sided>1</double_sided></technique></extra>"#,
    );
    let max3d = r#"<effect id="e"><profile_COMMON><technique sid="c"><lambert/></technique></profile_COMMON><extra><technique profile="MAX3D"><double_sided>1</double_sided></technique></extra></effect>"#;
    for effect in [googleearth, max3d.into()] {
        let s = Doc {
            effect,
            ..Doc::default()
        }
        .parse()
        .unwrap();
        assert!(s.materials[0].double_sided);
    }
    assert!(!Doc::default().parse().unwrap().materials[0].double_sided);
}

#[test]
fn unreadable_extras_are_ignored() {
    // A <bump> without texcoord still counts; a bad <color> is skipped.
    let effect = phong(
        r#"<extra><technique profile="FCOLLADA"><bump><color>1 2</color></bump></technique><technique profile="OpenCOLLADA"><bump><texture texture="img"/></bump></technique></extra>"#,
    );
    let s = Doc {
        effect,
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.materials[0].normal_texture, texture("a.png"));
}

#[test]
fn effect_colours_and_textures() {
    // RGB colours (alpha 1), a <transparent> cut-out texture, and three.js's
    // wrap rules from the texture's <extra>.
    let effect = r#"<effect id="e"><profile_COMMON><technique sid="c"><lambert>
        <emission><texture texture="img" texcoord="UV"><extra><technique profile="MAYA"><wrapU>1</wrapU><wrapV>FALSE</wrapV></technique></extra></texture></emission>
        <diffuse><color>0.5 0.5 0.5</color></diffuse>
        <transparent opaque="A_ONE"><texture texture="img"><extra><technique><repeatU>2</repeatU></technique></extra></texture></transparent>
        </lambert></technique></profile_COMMON></effect>"#;
    let s = Doc {
        effect: effect.into(),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    let m = &s.materials[0];
    assert_eq!(m.diffuse, [0.5, 0.5, 0.5, 1.0]);
    assert_eq!(m.shading, Shading::Lambert);
    let wrap = |t: &Option<Texture>| t.as_ref().unwrap().repeat;
    assert_eq!(wrap(&m.emission_texture), [true, false]);
    assert_eq!(wrap(&m.transparent_texture), [false, false]);
    assert_eq!(m.opacity, 1.0);
    let bad = Doc {
        effect: phong("").replace("1 0 0 1", "1 0"),
        ..Doc::default()
    };
    error(bad, "expected 3 or 4 floats");
}

#[test]
fn bad_units_fall_back_to_metres() {
    for unit in ["0", "-2", "NaN", "inf", "metre"] {
        let s = Doc {
            unit,
            ..Doc::default()
        }
        .parse()
        .unwrap();
        assert_eq!(s.unit, 1.0, "{unit}");
    }
    let s = Doc {
        unit: "0,5",
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.unit, 0.5);
}

#[test]
fn rotation_axes_are_normalised() {
    let s = Doc {
        scene: format!(r#"<node><rotate>0 0 2 90</rotate>{INSTANCE}</node>"#),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    let t = s.roots[0].transform;
    // 90° about +Z: X goes to Y, with no scale.
    let x = [t[0][0], t[1][0], t[2][0]];
    assert!(near(x, [0.0, 1.0, 0.0], 1e-6), "{x:?}");
}

#[test]
fn skinned_meshes_are_reported() {
    let s = Doc {
        scene: r##"<node><instance_controller url="#skin"/></node><node><instance_controller url="#skin2"/></node>"##.into(),
        ..Doc::default()
    }
    .parse()
    .unwrap();
    assert_eq!(s.warnings.len(), 1);
    assert!(
        s.warnings[0].starts_with("2 <instance_controller>s"),
        "{:?}",
        s.warnings
    );
}

// ─── Fuel corpus ─────────────────────────────────────────────────────────────

/// gzweb's three.js loader output for the spike corpus, one JSON line per
/// file (`spikes/collada/out/js.jsonl`).
#[test]
#[ignore = "needs spikes/collada/run.sh (Fuel and Node)"]
fn fuel_corpus_matches_three_js() {
    let spike = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/collada");
    let reference = std::fs::read_to_string(spike.join("out/js.jsonl")).unwrap();
    let mut checked = 0;
    for line in reference.lines() {
        let r: serde_json::Value = serde_json::from_str(line).unwrap();
        let file = r["file"].as_str().unwrap();
        let point = |key: &str| -> [f32; 3] {
            let v: Vec<f32> = r["aabb"][key]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap() as f32)
                .collect();
            [v[0], v[1], v[2]]
        };
        let Ok(bytes) = std::fs::read(spike.join(file)) else {
            continue;
        };
        let s = parse(&bytes).unwrap();
        // Every file, the probes included, is compared in full.
        let triangles = r["total_triangles"].as_u64().unwrap() as usize;
        assert_eq!(s.triangle_count(), triangles, "{file}");
        let (min, max) = s.bounds().unwrap();
        let tol = 1e-3 * (1.0 + max.iter().fold(0.0f32, |m, x| m.max(x.abs())));
        let (want_min, want_max) = (point("min"), point("max"));
        assert!(
            near(min, want_min, tol),
            "{file}: min {min:?} vs {want_min:?}"
        );
        assert!(
            near(max, want_max, tol),
            "{file}: max {max:?} vs {want_max:?}"
        );
        checked += 1;
    }
    assert!(checked >= 14, "only {checked} files checked");
}
