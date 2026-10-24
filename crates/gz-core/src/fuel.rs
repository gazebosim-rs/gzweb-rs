//! Gazebo Fuel's model search, as the Assets panel uses it (DECISIONS.md
//! D-102): the search URL, the reply's models, and each model's URI and
//! thumbnail. Fuel answers any origin (`Access-Control-Allow-Origin: *`),
//! so the browser can ask it directly.

use serde::Deserialize;

use crate::uri::{FUEL_HOST, FUEL_VERSION};

/// The Fuel server's API root.
pub fn api_root() -> String {
    format!("https://{FUEL_HOST}/{FUEL_VERSION}")
}

/// Where people browse Fuel (the panel's section header names it).
pub const BROWSE_HOST: &str = "app.gazebosim.org";

/// How many models a page of results holds.
pub const PAGE_SIZE: u32 = 20;

/// One model in a search reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct FuelModel {
    pub name: String,
    pub owner: String,
    #[serde(default)]
    pub description: String,
    /// Its download's size in bytes.
    #[serde(default)]
    pub filesize: u64,
    /// Its thumbnail's path under the API root, if it has one.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

impl FuelModel {
    /// The model's URL, which the server includes (it fetches it into its
    /// own Fuel cache).
    pub fn uri(&self) -> String {
        format!(
            "{}/{}/models/{}",
            api_root(),
            encode(&self.owner),
            encode(&self.name)
        )
    }

    /// Its thumbnail's URL.
    pub fn thumbnail(&self) -> Option<String> {
        let path = self.thumbnail_url.as_deref().filter(|p| !p.is_empty())?;
        Some(format!("{}{path}", api_root()).replace(' ', "%20"))
    }
}

/// The search for `query`'s `page` (from 1): models whose name, tags or
/// description match.
pub fn search_url(query: &str, page: u32) -> String {
    format!(
        "{}/models?q={}&page={page}&per_page={PAGE_SIZE}",
        api_root(),
        encode(query.trim())
    )
}

/// A search reply's models.
pub fn parse_models(body: &[u8]) -> Result<Vec<FuelModel>, String> {
    serde_json::from_slice(body).map_err(|e| format!("not a Fuel reply: {e}"))
}

/// A download size as the row shows it ("640 KB", "2.1 MB").
pub fn size_label(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e7 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply's models parse, with or without a thumbnail; the URI and
    /// thumbnail are escaped; the search URL encodes its query.
    #[test]
    fn a_search_reply_reads_as_models() {
        let body = br#"[{"name":"Euro pallet","owner":"OpenRobotics","filesize":25056,
            "description":"A wooden pallet.","likes":1,
            "thumbnail_url":"/OpenRobotics/models/Euro%20pallet/tip/files/thumbnails/1.png"},
            {"name":"bare","owner":"me"}]"#;
        let models = parse_models(body).unwrap();
        assert_eq!(models.len(), 2);
        let m = &models[0];
        assert_eq!(
            (m.name.as_str(), m.owner.as_str(), m.filesize),
            ("Euro pallet", "OpenRobotics", 25056)
        );
        assert_eq!(
            m.uri(),
            "https://fuel.gazebosim.org/1.0/OpenRobotics/models/Euro%20pallet"
        );
        assert_eq!(
            m.thumbnail().unwrap(),
            "https://fuel.gazebosim.org/1.0/OpenRobotics/models/Euro%20pallet/tip/files/thumbnails/1.png"
        );
        assert_eq!(models[1].thumbnail(), None);
        assert!(parse_models(b"{\"error\":1}").is_err());
        assert_eq!(
            search_url(" euro pallet&x ", 2),
            "https://fuel.gazebosim.org/1.0/models?q=euro%20pallet%26x&page=2&per_page=20"
        );
    }

    #[test]
    fn sizes_read_in_units() {
        assert_eq!(size_label(310_000), "310 KB");
        assert_eq!(size_label(2_100_000), "2.1 MB");
        assert_eq!(size_label(12_000_000), "12 MB");
        assert_eq!(size_label(999), "999 B");
        assert_eq!(size_label(3_000_000_000), "3.0 GB");
    }
}
