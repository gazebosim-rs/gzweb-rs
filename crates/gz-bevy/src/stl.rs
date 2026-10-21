//! A Bevy `AssetLoader` for ASCII and binary STL.
//!
//! The format is told apart as three.js's `STLLoader` does: binary when the
//! size matches the face count in the header (CAD exporters often start a
//! binary header with `solid`), ASCII when the file starts with `solid`
//! (here in any case), binary otherwise.
//!
//! Faces are flat shaded, three vertices each, as three.js builds them. A
//! face's stored normal is used when it has one; many exporters write zeros,
//! so those faces get the normal of their winding instead.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

#[derive(Default, TypePath)]
pub struct StlLoader;

/// Why an `.stl` failed to load.
#[derive(Debug)]
pub enum StlError {
    /// The file couldn't be read.
    Io(std::io::Error),
    /// The file isn't a well-formed STL.
    Invalid(String),
}

impl std::fmt::Display for StlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "couldn't read STL: {e}"),
            Self::Invalid(why) => write!(f, "invalid STL: {why}"),
        }
    }
}

impl std::error::Error for StlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Invalid(_) => None,
        }
    }
}

impl AssetLoader for StlLoader {
    type Asset = Mesh;
    type Settings = ();
    type Error = StlError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _cx: &mut LoadContext<'_>,
    ) -> Result<Mesh, StlError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(StlError::Io)?;
        Ok(mesh(&parse(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["stl"]
    }
}

/// One triangle: its stored normal and its vertices.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Facet {
    normal: Vec3,
    vertices: [Vec3; 3],
}

/// Header size of a binary STL: 80 bytes, then a little-endian face count.
const HEADER: usize = 84;
/// A binary face: normal and three vertices (12 `f32`s), and 2 attribute bytes.
const RECORD: usize = 50;

fn invalid(why: impl Into<String>) -> StlError {
    StlError::Invalid(why.into())
}

fn parse(bytes: &[u8]) -> Result<Vec<Facet>, StlError> {
    if bytes.len() >= HEADER && face_count(bytes) * RECORD as u64 == (bytes.len() - HEADER) as u64 {
        return parse_binary(bytes);
    }
    // `solid` may follow a byte-order mark (three.js looks 5 bytes in).
    match (0..5).find(|&i| {
        bytes
            .get(i..i + 5)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"solid"))
    }) {
        Some(start) => parse_ascii(&String::from_utf8_lossy(&bytes[start..])),
        None => parse_binary(bytes),
    }
}

fn face_count(bytes: &[u8]) -> u64 {
    u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]).into()
}

/// Binary STL. Bytes after the last face are ignored, as by three.js.
fn parse_binary(bytes: &[u8]) -> Result<Vec<Facet>, StlError> {
    if bytes.len() < HEADER {
        return Err(invalid(format!("{} bytes is too short", bytes.len())));
    }
    let faces = face_count(bytes);
    let body = &bytes[HEADER..];
    if (body.len() as u64) < faces * RECORD as u64 {
        return Err(invalid(format!(
            "{faces} faces need {} bytes, found {}",
            faces * RECORD as u64,
            body.len()
        )));
    }
    let f = |r: &[u8], i: usize| f32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
    let v = |r: &[u8], i: usize| Vec3::new(f(r, i), f(r, i + 4), f(r, i + 8));
    Ok(body
        .as_chunks::<RECORD>()
        .0
        .iter()
        .take(faces as usize)
        .map(|r| Facet {
            normal: v(r, 0),
            vertices: [v(r, 12), v(r, 24), v(r, 36)],
        })
        .collect())
}

/// ASCII STL: `facet [normal x y z] [outer loop] vertex x y z (×3) [endloop]
/// endfacet`, keywords in any case and laid out freely. `solid` and
/// `endsolid` lines (with any name) are skipped, so several solids load as
/// one mesh, as in three.js.
fn parse_ascii(text: &str) -> Result<Vec<Facet>, StlError> {
    let is_solid_line = |line: &str| {
        line.split_ascii_whitespace().next().is_some_and(|word| {
            let word = word.to_ascii_lowercase();
            word.starts_with("solid") || word.starts_with("endsolid")
        })
    };
    let mut tokens = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_solid_line(line))
        .flat_map(|(i, line)| line.split_ascii_whitespace().map(move |t| (i + 1, t)));

    let mut facets = Vec::new();
    while let Some((line, word)) = tokens.next() {
        if !word.eq_ignore_ascii_case("facet") {
            return Err(invalid(format!(
                "line {line}: expected `facet`, found `{word}`"
            )));
        }
        let mut normal = Vec3::ZERO;
        let mut vertices = Vec::with_capacity(3);
        loop {
            let Some((line, word)) = tokens.next() else {
                return Err(invalid("ends inside a facet"));
            };
            match word.to_ascii_lowercase().as_str() {
                "normal" => normal = vector(&mut tokens, line)?,
                "vertex" => vertices.push(vector(&mut tokens, line)?),
                "outer" | "loop" | "endloop" => {}
                "endfacet" => break,
                _ => return Err(invalid(format!("line {line}: unexpected `{word}`"))),
            }
        }
        let vertices: [Vec3; 3] = vertices.try_into().map_err(|v: Vec<_>| {
            invalid(format!("line {line}: a facet has {} vertices", v.len()))
        })?;
        facets.push(Facet { normal, vertices });
    }
    Ok(facets)
}

/// The three numbers after a `normal` or `vertex` on `line`.
fn vector<'a>(
    tokens: &mut impl Iterator<Item = (usize, &'a str)>,
    line: usize,
) -> Result<Vec3, StlError> {
    let mut v = [0.0; 3];
    for c in &mut v {
        let (line, word) = tokens.next().unwrap_or((line, ""));
        *c = word
            .parse()
            .map_err(|_| invalid(format!("line {line}: expected a number, found `{word}`")))?;
    }
    Ok(Vec3::from_array(v))
}

/// A non-indexed, flat-shaded triangle list, three vertices per face.
fn mesh(facets: &[Facet]) -> Mesh {
    let n = facets.len() * 3;
    let mut positions = Vec::with_capacity(n);
    let mut normals = Vec::with_capacity(n);
    for face in facets {
        let [a, b, c] = face.vertices;
        let normal = face
            .normal
            .try_normalize()
            .unwrap_or_else(|| (b - a).cross(c - a).normalize_or_zero());
        positions.extend(face.vertices.map(|v| v.to_array()));
        normals.extend([normal.to_array(); 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use std::error::Error;

    fn normals(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) => n.clone(),
            other => panic!("{other:?}"),
        }
    }

    /// A binary STL with `header` and `faces` copies of one triangle.
    fn binary(header: &[u8], faces: u32) -> Vec<u8> {
        let mut bytes = header.to_vec();
        bytes.resize(80, b' ');
        bytes.extend(faces.to_le_bytes());
        for _ in 0..faces {
            for f in [
                0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ] {
                bytes.extend(f.to_le_bytes());
            }
            bytes.extend([0, 0]);
        }
        bytes
    }

    const TRIANGLE: Facet = Facet {
        normal: Vec3::Z,
        vertices: [Vec3::ZERO, Vec3::X, Vec3::Y],
    };

    #[test]
    fn ascii_faces_keep_stored_normals_or_use_their_winding() {
        let text = "solid t
facet normal 0 0 2
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 0 1 0
vertex 0 0 1
endloop
endfacet
facet normal inf 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid t
";
        let m = mesh(&parse(text.as_bytes()).unwrap());
        assert_eq!(m.count_vertices(), 9);
        assert!(m.indices().is_none());
        let n = normals(&m);
        assert_eq!(n[0], [0.0, 0.0, 1.0]);
        assert_eq!(n[3], [1.0, 0.0, 0.0]);
        // An infinite stored normal can't be normalised; the winding is used.
        assert_eq!(n[6], [0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_binary_header_may_start_with_solid() {
        // 10 faces: the count's low byte is '\n', so the header reads as a
        // `solid part` line of valid UTF-8.
        let bytes = binary(b"solid part", 10);
        assert_eq!(bytes[80], b'\n');
        assert_eq!(parse(&bytes).unwrap(), vec![TRIANGLE; 10]);
        // Without `solid`, the size isn't checked; trailing bytes are ignored.
        let mut padded = binary(b"exported", 1);
        padded.extend([0; 7]);
        assert_eq!(parse(&padded).unwrap(), vec![TRIANGLE]);
    }

    #[test]
    fn ascii_solid_lines_may_be_bare_uppercase_or_repeated() {
        let facet = "facet normal 0 0 1\n outer loop\n  vertex 0 0 0\n  vertex 1 0 0\n  \
                     vertex 0 1 0\n endloop\nendfacet\n";
        for text in [
            format!("solid\n{facet}endsolid\n"),
            format!("\u{feff}solid\r\n{}endsolid", facet.replace('\n', "\r\n")),
            format!("SOLID PART\n{}ENDSOLID PART\n", facet.to_uppercase()),
            format!("solidname\n{facet}endsolid name\nsolid b\n{facet}endsolid b\n"),
            // No `outer loop` or normal, all on one line, no `endsolid`.
            "solid x\nfacet vertex 0 0 0 vertex 1 0 0 vertex 0 1 0 endfacet".into(),
        ] {
            let facets = parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert!(!facets.is_empty(), "{text:?}");
            assert_eq!(facets[0].vertices, TRIANGLE.vertices, "{text:?}");
        }
        assert_eq!(parse(b"solid empty\nendsolid empty\n").unwrap(), []);
    }

    #[test]
    fn malformed_or_truncated_files_are_errors() {
        let mut short = binary(b"part", 3);
        short.truncate(short.len() - 1);
        let cases: [(&[u8], &str); 9] = [
            (b"", "0 bytes is too short"),
            (b"binary", "too short"),
            (&short, "3 faces need 150 bytes, found 149"),
            (
                b"solid x\nnonsense",
                "line 2: expected `facet`, found `nonsense`",
            ),
            (
                b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0",
                "line 4: expected a number, found ``",
            ),
            (
                b"solid x\nfacet normal 0 0 1\nvertex 0 0 0\n",
                "ends inside a facet",
            ),
            (
                b"solid x\nfacet normal a 0 1\n",
                "line 2: expected a number, found `a`",
            ),
            (
                b"solid x\nfacet\nvertex 0 0 0\nendfacet\n",
                "line 2: a facet has 1 vertices",
            ),
            (b"solid x\nfacet\nwhat\n", "line 3: unexpected `what`"),
        ];
        for (bytes, why) in cases {
            match parse(bytes) {
                Err(StlError::Invalid(e)) => assert!(e.contains(why), "{e} / {why}"),
                other => panic!("{bytes:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn errors_describe_themselves() {
        let io = StlError::Io(std::io::Error::other("gone"));
        assert_eq!(io.to_string(), "couldn't read STL: gone");
        assert_eq!(io.source().unwrap().to_string(), "gone");
        let bad = invalid("no faces");
        assert_eq!(bad.to_string(), "invalid STL: no faces");
        assert!(bad.source().is_none());
    }

    #[test]
    fn loads_binary_stl_through_the_asset_server() {
        use bevy::asset::io::memory::{Dir, MemoryAssetReader};
        use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};

        let dir = Dir::default();
        dir.insert_asset(std::path::Path::new("t.stl"), binary(b"", 1));
        dir.insert_asset(
            std::path::Path::new("bad.stl"),
            b"solid x\nnonsense".to_vec(),
        );

        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: dir.clone() })),
        )
        .add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset_loader::<StlLoader>();
        let server = app.world().resource::<AssetServer>().clone();
        let good: Handle<Mesh> = server.load("t.stl");
        let bad: Handle<Mesh> = server.load("bad.stl");
        crate::test_util::wait_until(|| {
            app.update();
            server.is_loaded(&good) && server.load_state(&bad).is_failed()
        });
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert_eq!(meshes.get(&good).unwrap().count_vertices(), 3);
        assert!(server.load_state(&bad).is_failed());
    }
}
