//! Resource URI rules, ported from gzweb `FuelServer.ts` (`createFuelUri`) and
//! `SDFParser.ts` `createGeom` (mesh branch).

pub const FUEL_HOST: &str = "fuel.gazebosim.org";
pub const IGN_FUEL_HOST: &str = "fuel.ignitionrobotics.org";
pub const FUEL_VERSION: &str = "1.0";

/// gzweb `createFuelUri`: turns a local Fuel-cache path into a Fuel HTTPS URL.
///
/// `/home/u/.gz/fuel/fuel.gazebosim.org/openrobotics/models/foo/2/meshes/a.dae`
/// becomes
/// `https://fuel.gazebosim.org/1.0/openrobotics/models/foo/2/files/meshes/a.dae`.
/// Anything else is returned unchanged.
pub fn create_fuel_uri(uri: &str) -> String {
    if uri.starts_with(&format!("https://{FUEL_HOST}"))
        || uri.starts_with(&format!("https://{IGN_FUEL_HOST}"))
    {
        return uri.to_string();
    }

    // gzweb tests `indexOf(host) > 0`, so a string that *starts* with the host
    // is left alone.
    let host = if uri.find(FUEL_HOST).is_some_and(|i| i > 0) {
        FUEL_HOST
    } else if uri.find(IGN_FUEL_HOST).is_some_and(|i| i > 0) {
        IGN_FUEL_HOST
    } else {
        return uri.to_string();
    };

    let mut parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();
    // The host can only be missing here if it is embedded in a longer segment
    // (e.g. "xfuel.gazebosim.org"); JS `splice(0, -1)` would then drop nothing.
    if let Some(i) = parts.iter().position(|p| *p == host) {
        parts.drain(..i);
    }
    parts.insert(1.min(parts.len()), FUEL_VERSION);
    parts.insert(6.min(parts.len()), "files");
    format!("https://{}", parts.join("/"))
}

/// The URI gzweb loads for a `<mesh><uri>` (and passes to the websocket
/// `asset` request if the HTTP fetch fails): `file://` and `model://` schemes
/// are stripped, then [`create_fuel_uri`] is applied.
pub fn mesh_load_uri(uri: &str) -> String {
    let stripped = match uri.find("://") {
        Some(i) if matches!(&uri[..i], "file" | "model") => &uri[i + 3..],
        _ => uri,
    };
    create_fuel_uri(stripped)
}

/// Lower-cased extension of the last path segment, without the dot.
pub fn extension(uri: &str) -> String {
    let name = uri.rsplit('/').next().unwrap_or(uri);
    let name = name.split(['?', '#']).next().unwrap_or(name);
    match name.rfind('.') {
        Some(i) => name[i + 1..].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Whether a load URI must be fetched over HTTP(S) rather than requested from
/// the Gazebo websocket server.
pub fn is_http(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_cache_path_to_url() {
        assert_eq!(
            create_fuel_uri(
                "/home/u/.gz/fuel/fuel.gazebosim.org/openrobotics/models/foo/2/meshes/a.dae"
            ),
            "https://fuel.gazebosim.org/1.0/openrobotics/models/foo/2/files/meshes/a.dae"
        );
        assert_eq!(
            create_fuel_uri("/tmp/fuel.ignitionrobotics.org/owner/models/robot/1/meshes/body.glb"),
            "https://fuel.ignitionrobotics.org/1.0/owner/models/robot/1/files/meshes/body.glb"
        );
    }

    #[test]
    fn fuel_passthrough() {
        let url = "https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.stl";
        assert_eq!(create_fuel_uri(url), url);
        assert_eq!(
            create_fuel_uri("/home/u/model/a.stl"),
            "/home/u/model/a.stl"
        );
        // indexOf(host) == 0 is not > 0
        assert_eq!(
            create_fuel_uri("fuel.gazebosim.org/a"),
            "fuel.gazebosim.org/a"
        );
    }

    #[test]
    fn mesh_uris() {
        assert_eq!(mesh_load_uri("file:///home/u/a.dae"), "/home/u/a.dae");
        assert_eq!(
            mesh_load_uri("model://pkg/meshes/a.dae"),
            "pkg/meshes/a.dae"
        );
        assert_eq!(mesh_load_uri("package://pkg/a.dae"), "package://pkg/a.dae");
        assert_eq!(
            mesh_load_uri("file:///root/.gz/fuel/fuel.gazebosim.org/o/models/m/3/meshes/x.obj"),
            "https://fuel.gazebosim.org/1.0/o/models/m/3/files/meshes/x.obj"
        );
    }

    #[test]
    fn extensions() {
        assert_eq!(extension("a/b/c.DAE"), "dae");
        assert_eq!(extension("https://h/x.glb?v=1"), "glb");
        assert_eq!(extension("noext"), "");
        assert_eq!(extension("dir.d/noext"), "");
    }

    #[test]
    fn host_inside_a_longer_segment() {
        // indexOf finds the host inside "xfuel.gazebosim.org"; JS splice(0, -1)
        // then removes nothing, so the version and "files" go in by index.
        // Expected values are gzweb's createFuelUri output (checked in node).
        assert_eq!(
            create_fuel_uri("/c/xfuel.gazebosim.org/o/models/m/1/a.dae"),
            "https://c/1.0/xfuel.gazebosim.org/o/models/m/files/1/a.dae"
        );
        // Short paths: JS splice past the end appends.
        assert_eq!(
            create_fuel_uri("/x/fuel.gazebosim.org"),
            "https://fuel.gazebosim.org/1.0/files"
        );
    }

    #[test]
    fn ignition_fuel_passthrough() {
        let url = "https://fuel.ignitionrobotics.org/1.0/o/models/m/1/files/a.stl";
        assert_eq!(create_fuel_uri(url), url);
        assert_eq!(mesh_load_uri(url), url);
    }

    #[test]
    fn more_extensions() {
        assert_eq!(extension("/a/b/Mesh.DAE"), "dae");
        assert_eq!(extension("https://h/m.glb?raw=1"), "glb");
        assert_eq!(extension("https://h/m.obj#frag"), "obj");
        assert_eq!(extension("/a.b/noext"), "");
        assert_eq!(extension("plain"), "");
        assert_eq!(extension("archive.tar.gz"), "gz");
    }

    #[test]
    fn http_detection() {
        assert!(is_http("https://fuel.gazebosim.org/a"));
        assert!(is_http("http://localhost:8000/a.obj"));
        assert!(!is_http("/home/u/a.obj"));
        assert!(!is_http("file:///a.obj"));
        assert!(!is_http("httpx://a"));
    }
}
