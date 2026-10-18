//! Snapshot and protocol comparison (`.agent/PARITY.md` §4).

use std::collections::{BTreeMap, HashMap};

use gz_core::snapshot::{Aabb, LightInfo, MaterialInfo, SceneInfo, SnapNode, Snapshot, Xform};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One allow-list entry: diffs at matching `path`/`field` are reported as
/// suppressed. `path` and `field` are globs (`*` matches any run of chars).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Allow {
    pub id: String,
    #[serde(default = "star")]
    pub path: String,
    pub field: String,
    pub reason: String,
}

fn star() -> String {
    "*".into()
}

#[derive(Debug, Default, Deserialize)]
pub struct AllowList {
    #[serde(default)]
    pub allow: Vec<Allow>,
}

/// Reads a `tests/parity/allowlist.toml`-style file.
pub fn load_allowlist(path: &std::path::Path) -> Result<Vec<Allow>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str::<AllowList>(&text)
        .map(|a| a.allow)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub path: String,
    pub field: String,
    pub expected: Value,
    pub actual: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressed_by: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub expected_source: String,
    pub actual_source: String,
    pub nodes_expected: usize,
    pub nodes_actual: usize,
    pub diffs: Vec<Diff>,
    pub suppressed: Vec<Diff>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.diffs.is_empty()
    }
}

pub fn glob(pattern: &str, s: &str) -> bool {
    let Some((head, tail)) = pattern.split_once('*') else {
        return pattern == s;
    };
    let Some(mut rest) = s.strip_prefix(head) else {
        return false;
    };
    let mut middle: Vec<&str> = tail.split('*').collect();
    let last = middle.pop().unwrap_or_default();
    for part in middle {
        let Some(pos) = rest.find(part) else {
            return false;
        };
        rest = &rest[pos + part.len()..];
    }
    rest.ends_with(last)
}

#[derive(Debug, Clone, Copy)]
pub struct Tolerances {
    pub pos_abs: f64,
    pub pos_rel: f64,
    pub quat: f64,
    pub aabb_abs: f64,
    pub aabb_rel: f64,
    pub color: f64,
    pub scalar_rel: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            pos_abs: 1e-5,
            // f32 storage on the Rust side.
            pos_rel: 1e-6,
            quat: 1e-6,
            aabb_abs: 1e-3,
            aabb_rel: 1e-3,
            color: 1.0 / 255.0,
            scalar_rel: 1e-4,
        }
    }
}

struct Ctx<'a> {
    tol: Tolerances,
    allow: &'a [Allow],
    report: Report,
}

impl Ctx<'_> {
    fn diff(&mut self, path: &str, field: &str, expected: Value, actual: Value) {
        let suppressed_by = self
            .allow
            .iter()
            .find(|a| glob(&a.path, path) && glob(&a.field, field))
            .map(|a| a.id.clone());
        let d = Diff {
            path: path.to_string(),
            field: field.to_string(),
            expected,
            actual,
            suppressed_by,
        };
        if d.suppressed_by.is_some() {
            self.report.suppressed.push(d);
        } else {
            self.report.diffs.push(d);
        }
    }

    fn close(a: f64, b: f64, abs: f64, rel: f64) -> bool {
        (a - b).abs() <= abs + rel * a.abs().max(b.abs())
    }

    fn vec(&mut self, path: &str, field: &str, e: &[f64], a: &[f64], abs: f64, rel: f64) {
        if e.len() != a.len() || e.iter().zip(a).any(|(x, y)| !Self::close(*x, *y, abs, rel)) {
            self.diff(path, field, json!(e), json!(a));
        }
    }

    fn quat(&mut self, path: &str, field: &str, e: &[f64; 4], a: &[f64; 4]) {
        let dot: f64 = e.iter().zip(a).map(|(x, y)| x * y).sum();
        if 1.0 - dot.abs() > self.tol.quat {
            self.diff(path, field, json!(e), json!(a));
        }
    }

    fn scalar(&mut self, path: &str, field: &str, e: Option<f64>, a: Option<f64>) {
        match (e, a) {
            (Some(x), Some(y)) if Self::close(x, y, 1e-9, self.tol.scalar_rel) => {}
            (None, None) => {}
            _ => self.diff(path, field, json!(e), json!(a)),
        }
    }

    fn eq<T: PartialEq + Serialize>(&mut self, path: &str, field: &str, e: &T, a: &T) {
        if e != a {
            self.diff(path, field, json!(e), json!(a));
        }
    }

    fn xform(&mut self, path: &str, prefix: &str, e: &Xform, a: &Xform) {
        let t = self.tol;
        self.vec(
            path,
            &format!("{prefix}.pos"),
            &e.pos,
            &a.pos,
            t.pos_abs,
            t.pos_rel,
        );
        self.quat(path, &format!("{prefix}.quat"), &e.quat, &a.quat);
        self.vec(
            path,
            &format!("{prefix}.scale"),
            &e.scale,
            &a.scale,
            t.pos_abs,
            t.pos_rel,
        );
    }
}

pub fn compare_snapshots(
    expected: &Snapshot,
    actual: &Snapshot,
    allow: &[Allow],
    tol: Tolerances,
) -> Report {
    let mut cx = Ctx {
        tol,
        allow,
        report: Report {
            expected_source: expected.source.clone(),
            actual_source: actual.source.clone(),
            nodes_expected: expected.nodes.len(),
            nodes_actual: actual.nodes.len(),
            ..Default::default()
        },
    };

    scene(&mut cx, &expected.scene, &actual.scene);

    let actual_by_path: HashMap<&str, &SnapNode> =
        actual.nodes.iter().map(|n| (n.path.as_str(), n)).collect();
    let expected_paths: HashMap<&str, ()> = expected
        .nodes
        .iter()
        .map(|n| (n.path.as_str(), ()))
        .collect();
    for e in &expected.nodes {
        match actual_by_path.get(e.path.as_str()) {
            Some(a) => node(&mut cx, e, a),
            None => cx.diff(&e.path, "node.missing", json!(e.kind), Value::Null),
        }
    }
    for a in &actual.nodes {
        if !expected_paths.contains_key(a.path.as_str()) {
            cx.diff(&a.path, "node.extra", Value::Null, json!(a.kind));
        }
    }
    cx.report
}

fn scene(cx: &mut Ctx, e: &SceneInfo, a: &SceneInfo) {
    let c = cx.tol.color;
    let t = cx.tol;
    if let (Some(x), Some(y)) = (&e.ambient, &a.ambient) {
        cx.vec("<scene>", "scene.ambient", x, y, c, 0.0);
    }
    if let (Some(x), Some(y)) = (&e.clear_color, &a.clear_color) {
        cx.vec("<scene>", "scene.clear_color", x, y, c, 0.0);
    }
    if let (Some(x), Some(y)) = (&e.camera, &a.camera) {
        cx.vec(
            "<scene>",
            "scene.camera.position",
            &x.position,
            &y.position,
            t.pos_abs,
            t.pos_rel,
        );
        cx.vec(
            "<scene>",
            "scene.camera.target",
            &x.target,
            &y.target,
            t.pos_abs,
            t.pos_rel,
        );
        cx.scalar(
            "<scene>",
            "scene.camera.fov_deg",
            Some(x.fov_deg),
            Some(y.fov_deg),
        );
        cx.scalar("<scene>", "scene.camera.near", Some(x.near), Some(y.near));
        cx.scalar("<scene>", "scene.camera.far", Some(x.far), Some(y.far));
    }
    cx.eq(
        "<scene>",
        "scene.grid_visible",
        &e.grid_visible,
        &a.grid_visible,
    );
    cx.eq("<scene>", "scene.sky", &e.sky, &a.sky);
}

fn node(cx: &mut Ctx, e: &SnapNode, a: &SnapNode) {
    let p = e.path.as_str();
    cx.eq(p, "kind", &e.kind, &a.kind);
    match (&e.local, &a.local) {
        (Some(x), Some(y)) => cx.xform(p, "local", x, y),
        (None, None) => {}
        (x, y) => cx.diff(
            p,
            "node.renderer_object",
            json!(x.is_some()),
            json!(y.is_some()),
        ),
    }
    if let (Some(x), Some(y)) = (&e.world, &a.world) {
        cx.xform(p, "world", x, y);
    }
    if e.local.is_none() || a.local.is_none() {
        // Nothing else to compare for an object one side never created.
        return;
    }
    cx.eq(p, "visible", &e.visible, &a.visible);

    match (&e.geometry, &a.geometry) {
        (Some(x), Some(y)) => {
            cx.eq(p, "geometry.type", &x.ty, &y.ty);
            cx.eq(p, "geometry.mesh_count", &x.mesh_count, &y.mesh_count);
            aabb(cx, p, &x.aabb_world, &y.aabb_world);
        }
        (None, None) => {}
        (x, y) => cx.diff(p, "geometry", json!(x), json!(y)),
    }
    match (&e.material, &a.material) {
        (Some(x), Some(y)) => material(cx, p, x, y),
        (None, None) => {}
        (x, y) => cx.diff(p, "material", json!(x), json!(y)),
    }
    match (&e.light, &a.light) {
        (Some(x), Some(y)) => light(cx, p, x, y),
        (None, None) => {}
        (x, y) => cx.diff(p, "light", json!(x), json!(y)),
    }
}

fn aabb(cx: &mut Ctx, p: &str, e: &Option<Aabb>, a: &Option<Aabb>) {
    let t = cx.tol;
    match (e, a) {
        (Some(x), Some(y)) => {
            let ok = x
                .min
                .iter()
                .chain(&x.max)
                .zip(y.min.iter().chain(&y.max))
                .all(|(u, v)| Ctx::close(*u, *v, t.aabb_abs, t.aabb_rel));
            if !ok {
                cx.diff(p, "geometry.aabb_world", json!(x), json!(y));
            }
        }
        (None, None) => {}
        _ => cx.diff(p, "geometry.aabb_world", json!(e), json!(a)),
    }
}

fn material(cx: &mut Ctx, p: &str, e: &MaterialInfo, a: &MaterialInfo) {
    let c = cx.tol.color;
    cx.vec(p, "material.color", &e.color, &a.color, c, 0.0);
    cx.scalar(p, "material.opacity", Some(e.opacity), Some(a.opacity));
    cx.eq(p, "material.transparent", &e.transparent, &a.transparent);
    cx.vec(p, "material.emissive", &e.emissive, &a.emissive, c, 0.0);
    // `null` on the gzweb side means the material model has no such
    // parameter (three.js Phong); nothing to compare then.
    if e.metalness.is_some() {
        cx.scalar(p, "material.metalness", e.metalness, a.metalness);
    }
    if e.roughness.is_some() {
        cx.scalar(p, "material.roughness", e.roughness, a.roughness);
    }
    cx.eq(
        p,
        "material.maps.albedo",
        &e.maps.albedo.is_some(),
        &a.maps.albedo.is_some(),
    );
    cx.eq(
        p,
        "material.maps.normal",
        &e.maps.normal.is_some(),
        &a.maps.normal.is_some(),
    );
}

fn light(cx: &mut Ctx, p: &str, e: &LightInfo, a: &LightInfo) {
    let c = cx.tol.color;
    cx.eq(p, "light.type", &e.ty, &a.ty);
    cx.vec(p, "light.color", &e.color, &a.color, c, 0.0);
    cx.scalar(p, "light.intensity", Some(e.intensity), Some(a.intensity));
    cx.scalar(p, "light.range", e.range, a.range);
    cx.scalar(p, "light.angle", e.angle, a.angle);
    cx.scalar(p, "light.penumbra", e.penumbra, a.penumbra);
    cx.scalar(p, "light.decay", e.decay, a.decay);
    cx.eq(p, "light.cast_shadow", &e.cast_shadow, &a.cast_shadow);
    match (&e.direction_world, &a.direction_world) {
        (Some(x), Some(y)) => cx.vec(p, "light.direction_world", x, y, 1e-5, 0.0),
        (None, None) => {}
        (x, y) => cx.diff(p, "light.direction_world", json!(x), json!(y)),
    }
}

// ─── Protocol ───────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize)]
pub struct ProtocolReport {
    /// Non-asset frames, which must match in order.
    pub sequence_expected: Vec<String>,
    pub sequence_actual: Vec<String>,
    pub sequence_matches: bool,
    /// Asset requests are compared as multisets: gzweb issues them as HTTP
    /// loads fail, so their order relative to each other is incidental.
    pub assets_only_expected: Vec<String>,
    pub assets_only_actual: Vec<String>,
}

impl ProtocolReport {
    pub fn passed(&self) -> bool {
        self.sequence_matches
            && self.assets_only_expected.is_empty()
            && self.assets_only_actual.is_empty()
    }
}

pub fn compare_protocol(expected: &str, actual: &str) -> ProtocolReport {
    let split = |log: &str| {
        let mut seq = Vec::new();
        let mut assets: BTreeMap<String, usize> = BTreeMap::new();
        for line in log.lines().filter(|l| !l.is_empty()) {
            if line.starts_with("asset,") {
                *assets.entry(line.to_string()).or_default() += 1;
            } else {
                seq.push(line.to_string());
            }
        }
        (seq, assets)
    };
    let (se, ae) = split(expected);
    let (sa, aa) = split(actual);
    let only = |a: &BTreeMap<String, usize>, b: &BTreeMap<String, usize>| {
        a.iter()
            .filter(|(k, n)| b.get(*k).copied().unwrap_or(0) < **n)
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>()
    };
    ProtocolReport {
        sequence_matches: se == sa,
        assets_only_expected: only(&ae, &aa),
        assets_only_actual: only(&aa, &ae),
        sequence_expected: se,
        sequence_actual: sa,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gz_core::snapshot::*;

    fn node(path: &str, pos: [f64; 3]) -> SnapNode {
        let x = Xform {
            pos,
            quat: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        };
        SnapNode {
            path: path.into(),
            kind: "model".into(),
            local: Some(x.clone()),
            world: Some(x),
            visible: true,
            geometry: None,
            material: None,
            light: None,
        }
    }

    fn snap(nodes: Vec<SnapNode>) -> Snapshot {
        Snapshot {
            format: FORMAT.into(),
            source: "t".into(),
            world: "w".into(),
            scene: SceneInfo {
                ambient: None,
                clear_color: None,
                camera: None,
                grid_visible: Some(false),
                sky: None,
            },
            nodes,
        }
    }

    #[test]
    fn globs() {
        assert!(glob("*", "a/b"));
        assert!(glob("light.*", "light.color"));
        assert!(glob("*/visual", "m/l/visual"));
        assert!(glob("a*c*e", "abcde"));
        assert!(!glob("light.*", "material.color"));
        assert!(glob("exact", "exact"));
    }

    #[test]
    fn tolerances_missing_extra_and_allow() {
        let e = snap(vec![node("a", [1.0, 2.0, 3.0]), node("b", [0.0; 3])]);
        let a = snap(vec![node("a", [1.0, 2.0, 3.000001]), node("c", [0.0; 3])]);
        let r = compare_snapshots(&e, &a, &[], Tolerances::default());
        let fields: Vec<_> = r
            .diffs
            .iter()
            .map(|d| (d.path.as_str(), d.field.as_str()))
            .collect();
        assert_eq!(fields, [("b", "node.missing"), ("c", "node.extra")]);

        let allow = vec![Allow {
            id: "x".into(),
            path: "c".into(),
            field: "node.*".into(),
            reason: "test".into(),
        }];
        let r = compare_snapshots(&e, &a, &allow, Tolerances::default());
        assert_eq!(r.diffs.len(), 1);
        assert_eq!(r.suppressed[0].suppressed_by.as_deref(), Some("x"));
    }

    #[test]
    fn quaternion_sign_is_ignored() {
        let mut e = node("a", [0.0; 3]);
        let mut a = node("a", [0.0; 3]);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        e.local.as_mut().unwrap().quat = [0.0, 0.0, h, h];
        a.local.as_mut().unwrap().quat = [0.0, 0.0, -h, -h];
        let r = compare_snapshots(&snap(vec![e]), &snap(vec![a]), &[], Tolerances::default());
        assert!(r.passed(), "{:?}", r.diffs);
    }

    #[test]
    fn protocol() {
        let e = "protos,,,\ntopics-types,,,\nworlds,,,\nasset,,,/a\nasset,,,/b\nsub,/x,,\n";
        let a = "protos,,,\ntopics-types,,,\nworlds,,,\nsub,/x,,\nasset,,,/b\nasset,,,/a\n";
        assert!(compare_protocol(e, a).passed());
        let r = compare_protocol(e, "protos,,,\nworlds,,,\nsub,/x,,\nasset,,,/a\n");
        assert!(!r.sequence_matches);
        assert_eq!(r.assets_only_expected, ["asset,,,/b"]);
    }

    type Mutation<T> = Box<dyn Fn(&mut T)>;

    fn full(path: &str) -> SnapNode {
        SnapNode {
            geometry: Some(GeometryInfo {
                ty: "box".into(),
                aabb_world: Some(Aabb {
                    min: [-1.0; 3],
                    max: [1.0; 3],
                }),
                mesh_count: 1,
                vertex_count: 24,
                corners: vec![],
            }),
            material: Some(MaterialInfo {
                color: [0.5; 3],
                opacity: 1.0,
                transparent: false,
                emissive: [0.0; 3],
                metalness: Some(0.1),
                roughness: Some(0.9),
                maps: Maps::default(),
            }),
            light: Some(LightInfo {
                ty: "spot".into(),
                color: [1.0; 3],
                intensity: 1.0,
                range: Some(10.0),
                angle: Some(0.5),
                penumbra: Some(0.1),
                decay: Some(1.0),
                cast_shadow: true,
                direction_world: Some([0.0, 0.0, -1.0]),
            }),
            ..node(path, [0.0; 3])
        }
    }

    fn full_scene() -> SceneInfo {
        SceneInfo {
            ambient: Some([0.1; 3]),
            clear_color: Some([0.7; 3]),
            camera: Some(CameraInfo {
                position: [-6.0, 0.0, 6.0],
                target: [0.0; 3],
                fov_deg: 60.0,
                near: 0.01,
                far: 1000.0,
            }),
            grid_visible: Some(false),
            sky: None,
        }
    }

    /// Fields that differ when `actual` is `mutate(full)`.
    fn diff_fields(mutate: impl Fn(&mut SnapNode)) -> Vec<String> {
        let e = full("n");
        let mut a = full("n");
        mutate(&mut a);
        let r = compare_snapshots(&snap(vec![e]), &snap(vec![a]), &[], Tolerances::default());
        r.diffs.into_iter().map(|d| d.field).collect()
    }

    #[test]
    fn identical_full_nodes_pass() {
        assert!(diff_fields(|_| {}).is_empty());
    }

    #[test]
    fn every_node_field_is_compared() {
        let cases: Vec<(&str, Mutation<SnapNode>)> = vec![
            ("kind", Box::new(|n| n.kind = "link".into())),
            (
                "local.pos",
                Box::new(|n| n.local.as_mut().unwrap().pos[0] = 1.0),
            ),
            (
                "local.scale",
                Box::new(|n| n.local.as_mut().unwrap().scale = [2.0; 3]),
            ),
            (
                "local.quat",
                Box::new(|n| n.local.as_mut().unwrap().quat = [1.0, 0.0, 0.0, 0.0]),
            ),
            (
                "world.pos",
                Box::new(|n| n.world.as_mut().unwrap().pos[2] = 5.0),
            ),
            ("visible", Box::new(|n| n.visible = false)),
            (
                "geometry.type",
                Box::new(|n| n.geometry.as_mut().unwrap().ty = "sphere".into()),
            ),
            (
                "geometry.mesh_count",
                Box::new(|n| n.geometry.as_mut().unwrap().mesh_count = 2),
            ),
            (
                "geometry.aabb_world",
                Box::new(|n| {
                    n.geometry
                        .as_mut()
                        .unwrap()
                        .aabb_world
                        .as_mut()
                        .unwrap()
                        .max[0] = 3.0;
                }),
            ),
            (
                "geometry.aabb_world",
                Box::new(|n| n.geometry.as_mut().unwrap().aabb_world = None),
            ),
            ("geometry", Box::new(|n| n.geometry = None)),
            (
                "material.color",
                Box::new(|n| n.material.as_mut().unwrap().color = [0.0; 3]),
            ),
            (
                "material.opacity",
                Box::new(|n| n.material.as_mut().unwrap().opacity = 0.5),
            ),
            (
                "material.transparent",
                Box::new(|n| n.material.as_mut().unwrap().transparent = true),
            ),
            (
                "material.emissive",
                Box::new(|n| n.material.as_mut().unwrap().emissive = [1.0; 3]),
            ),
            (
                "material.metalness",
                Box::new(|n| n.material.as_mut().unwrap().metalness = None),
            ),
            (
                "material.roughness",
                Box::new(|n| n.material.as_mut().unwrap().roughness = Some(0.2)),
            ),
            (
                "material.maps.albedo",
                Box::new(|n| n.material.as_mut().unwrap().maps.albedo = Some("a.png".into())),
            ),
            (
                "material.maps.normal",
                Box::new(|n| n.material.as_mut().unwrap().maps.normal = Some("n.png".into())),
            ),
            ("material", Box::new(|n| n.material = None)),
            (
                "light.type",
                Box::new(|n| n.light.as_mut().unwrap().ty = "point".into()),
            ),
            (
                "light.color",
                Box::new(|n| n.light.as_mut().unwrap().color = [0.0; 3]),
            ),
            (
                "light.intensity",
                Box::new(|n| n.light.as_mut().unwrap().intensity = 2.0),
            ),
            (
                "light.range",
                Box::new(|n| n.light.as_mut().unwrap().range = None),
            ),
            (
                "light.angle",
                Box::new(|n| n.light.as_mut().unwrap().angle = Some(1.0)),
            ),
            (
                "light.penumbra",
                Box::new(|n| n.light.as_mut().unwrap().penumbra = Some(0.5)),
            ),
            (
                "light.decay",
                Box::new(|n| n.light.as_mut().unwrap().decay = Some(2.0)),
            ),
            (
                "light.cast_shadow",
                Box::new(|n| n.light.as_mut().unwrap().cast_shadow = false),
            ),
            (
                "light.direction_world",
                Box::new(|n| n.light.as_mut().unwrap().direction_world = Some([1.0, 0.0, 0.0])),
            ),
            (
                "light.direction_world",
                Box::new(|n| n.light.as_mut().unwrap().direction_world = None),
            ),
            ("light", Box::new(|n| n.light = None)),
            ("node.renderer_object", Box::new(|n| n.local = None)),
        ];
        for (field, mutate) in cases {
            assert_eq!(diff_fields(mutate), [field], "mutating {field}");
        }
    }

    #[test]
    fn unrendered_nodes_compare_only_their_kind() {
        let mut e = full("n");
        let mut a = full("n");
        e.local = None;
        a.local = None;
        a.visible = false;
        a.geometry = None;
        let r = compare_snapshots(&snap(vec![e]), &snap(vec![a]), &[], Tolerances::default());
        assert!(r.passed(), "{:?}", r.diffs);
    }

    #[test]
    fn every_scene_field_is_compared() {
        let cases: Vec<(&str, Mutation<SceneInfo>)> = vec![
            ("scene.ambient", Box::new(|s| s.ambient = Some([1.0; 3]))),
            (
                "scene.clear_color",
                Box::new(|s| s.clear_color = Some([0.0; 3])),
            ),
            (
                "scene.camera.position",
                Box::new(|s| s.camera.as_mut().unwrap().position[0] = 1.0),
            ),
            (
                "scene.camera.target",
                Box::new(|s| s.camera.as_mut().unwrap().target[0] = 1.0),
            ),
            (
                "scene.camera.fov_deg",
                Box::new(|s| s.camera.as_mut().unwrap().fov_deg = 45.0),
            ),
            (
                "scene.camera.near",
                Box::new(|s| s.camera.as_mut().unwrap().near = 0.1),
            ),
            (
                "scene.camera.far",
                Box::new(|s| s.camera.as_mut().unwrap().far = 100.0),
            ),
            (
                "scene.grid_visible",
                Box::new(|s| s.grid_visible = Some(true)),
            ),
            (
                "scene.sky",
                Box::new(|s| {
                    s.sky = Some(SkyInfo {
                        cubemap: Some("c.dds".into()),
                    });
                }),
            ),
        ];
        for (field, mutate) in cases {
            let e = Snapshot {
                scene: full_scene(),
                ..snap(vec![])
            };
            let mut a = e.clone();
            mutate(&mut a.scene);
            let r = compare_snapshots(&e, &a, &[], Tolerances::default());
            let fields: Vec<_> = r.diffs.iter().map(|d| d.field.as_str()).collect();
            assert_eq!(fields, [field]);
        }
        // Fields one side doesn't report aren't compared.
        let e = Snapshot {
            scene: full_scene(),
            ..snap(vec![])
        };
        let r = compare_snapshots(&e, &snap(vec![]), &[], Tolerances::default());
        assert!(r.passed(), "{:?}", r.diffs);
    }

    #[test]
    fn more_globs() {
        assert!(!glob("a*b*c", "axxc"));
        assert!(!glob("x*", "abc"));
        assert!(!glob("exact", "other"));
        assert!(glob("*.color", "light.color"));
    }

    #[test]
    fn allowlist_files() {
        let dir = std::env::temp_dir().join(format!("gz-parity-allow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.toml");
        std::fs::write(
            &good,
            "[[allow]]\nid = \"a\"\nfield = \"light.color\"\nreason = \"r\"\n",
        )
        .unwrap();
        let allow = load_allowlist(&good).unwrap();
        assert_eq!(allow.len(), 1);
        // `path` defaults to every path.
        assert_eq!(allow[0].path, "*");

        let bad = dir.join("bad.toml");
        std::fs::write(&bad, "[[allow]]\nid = 3\n").unwrap();
        assert!(load_allowlist(&bad).unwrap_err().contains("bad.toml"));
        assert!(load_allowlist(&dir.join("missing.toml"))
            .unwrap_err()
            .contains("missing.toml"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn protocol_reports_extra_actual_assets() {
        let r = compare_protocol("asset,,,/a\n", "asset,,,/a\nasset,,,/a\n");
        assert!(!r.passed());
        assert!(r.sequence_matches);
        assert_eq!(r.assets_only_actual, ["asset,,,/a"]);
    }
}
