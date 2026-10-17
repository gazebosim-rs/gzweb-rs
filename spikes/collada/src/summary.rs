//! Summarise what `mesh-loader` produces for a Collada file.

use std::{path::Path, time::Instant};

use mesh_loader::{Material, Scene};
use serde::Serialize;

#[derive(Serialize)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    fn empty() -> Self {
        Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }
    fn add(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }
    fn union(&mut self, o: &Aabb) {
        if o.min[0].is_finite() {
            self.add(o.min);
            self.add(o.max);
        }
    }
    fn rounded(mut self) -> Self {
        let r = |v: f32| (v * 1e5).round() / 1e5;
        self.min = self.min.map(r);
        self.max = self.max.map(r);
        self
    }
}

#[derive(Serialize)]
pub struct MeshSummary {
    pub name: String,
    pub vertices: usize,
    pub triangles: usize,
    pub materials: Vec<String>,
    pub has_normals: bool,
    pub has_uvs: bool,
    pub has_uv2: bool,
    pub has_colors: bool,
    /// Attribute arrays whose length != vertex count (would break a Bevy Mesh).
    pub attribute_mismatch: bool,
    pub aabb: Aabb,
}

#[derive(Serialize)]
pub struct MaterialSummary {
    pub name: String,
    pub kind: Option<String>,
    pub diffuse: Option<[f32; 4]>,
    pub diffuse_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub specular_texture: Option<String>,
    pub emissive_texture: Option<String>,
    pub alpha_texture: Option<String>,
    pub opacity: Option<f32>,
    pub transparent_color: Option<[f32; 4]>,
}

#[derive(Serialize)]
pub struct FileSummary {
    pub file: String,
    pub loader: &'static str,
    /// Median of `runs` parses (`mesh_loader::collada::from_slice`), ms.
    pub parse_ms: f64,
    pub runs: usize,
    pub unit: f32,
    pub up_axis: String,
    /// What mesh-loader applied: it bakes everything into vertex positions.
    pub root_transform: &'static str,
    pub mesh_count: usize,
    pub total_vertices: usize,
    pub total_triangles: usize,
    pub aabb: Aabb,
    pub meshes: Vec<MeshSummary>,
    pub materials: Vec<MaterialSummary>,
}

fn path_str(p: &Option<std::path::PathBuf>) -> Option<String> {
    p.as_ref().map(|p| p.to_string_lossy().into_owned())
}

fn material_summary(m: &Material) -> MaterialSummary {
    MaterialSummary {
        name: m.name.clone(),
        kind: m.shading_model.as_ref().map(|s| format!("{s:?}")),
        diffuse: m.color.diffuse,
        diffuse_texture: path_str(&m.texture.diffuse),
        normal_texture: path_str(&m.texture.normal),
        specular_texture: path_str(&m.texture.specular),
        emissive_texture: path_str(&m.texture.emissive),
        alpha_texture: path_str(&m.texture.opacity),
        opacity: m.opacity,
        transparent_color: m.color.transparent,
    }
}

/// Crude `<asset>` sniffing (mesh-loader does not expose these).
fn sniff_asset(text: &str) -> (f32, String) {
    let unit = text
        .find("<unit")
        .and_then(|i| {
            let tag = &text[i..i + text[i..].find('>')?];
            let j = tag.find("meter=\"")? + 7;
            tag[j..j + tag[j..].find('"')?].trim().parse().ok()
        })
        .unwrap_or(1.0);
    let up = text
        .find("<up_axis>")
        .and_then(|i| {
            let s = &text[i + 9..];
            Some(s[..s.find('<')?].trim().to_string())
        })
        .unwrap_or_else(|| "Y_UP".into());
    (unit, up)
}

pub fn parse(bytes: &[u8]) -> std::io::Result<Scene> {
    // `from_slice` (no path) returns texture paths verbatim. The path-based
    // `Loader::load` instead joins them with the file's dir and *drops* any
    // texture whose file does not exist on disk.
    mesh_loader::collada::from_slice(bytes)
}

pub fn summarize(file: &Path, runs: usize) -> std::io::Result<FileSummary> {
    let bytes = std::fs::read(file)?;
    let (unit, up_axis) = sniff_asset(&String::from_utf8_lossy(&bytes));

    let mut times = Vec::with_capacity(runs);
    let mut scene = None;
    for _ in 0..runs.max(1) {
        let t0 = Instant::now();
        let s = parse(&bytes)?;
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        scene = Some(s);
    }
    times.sort_by(f64::total_cmp);
    let scene = scene.unwrap();

    let mut total = Aabb::empty();
    let meshes: Vec<_> = scene
        .meshes
        .iter()
        .zip(&scene.materials)
        .map(|(m, mat)| {
            let mut bb = Aabb::empty();
            m.vertices.iter().for_each(|&p| bb.add(p));
            total.union(&bb);
            let n = m.vertices.len();
            let bad = |len: usize| len != 0 && len != n;
            MeshSummary {
                name: m.name.clone(),
                vertices: n,
                triangles: m.faces.len(),
                materials: vec![mat.name.clone()],
                has_normals: !m.normals.is_empty(),
                has_uvs: !m.texcoords[0].is_empty(),
                has_uv2: !m.texcoords[1].is_empty(),
                has_colors: !m.colors[0].is_empty(),
                attribute_mismatch: bad(m.normals.len())
                    || bad(m.texcoords[0].len())
                    || bad(m.colors[0].len()),
                aabb: bb.rounded(),
            }
        })
        .collect();

    Ok(FileSummary {
        file: file.to_string_lossy().into_owned(),
        loader: "mesh-loader-0.1.13",
        parse_ms: (times[times.len() / 2] * 1e3).round() / 1e3,
        runs: times.len(),
        unit,
        up_axis,
        root_transform: "unit scale baked into positions via root node; up_axis ignored",
        mesh_count: meshes.len(),
        total_vertices: meshes.iter().map(|m| m.vertices).sum(),
        total_triangles: meshes.iter().map(|m| m.triangles).sum(),
        aabb: total.rounded(),
        meshes,
        materials: scene.materials.iter().map(material_summary).collect(),
    })
}
