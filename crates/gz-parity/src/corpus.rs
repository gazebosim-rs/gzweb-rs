//! Corpus format (`.agent/PARITY.md` §1): `manifest.json` + `frames.bin`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "gz-parity-corpus/1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub world: String,
    #[serde(default)]
    pub source: BTreeMap<String, String>,
    /// Frame index of the reply to `protos`.
    pub protos: Option<usize>,
    /// Request op (`topics-types`, `worlds`, `scene`) → reply frame index.
    pub responses: BTreeMap<String, usize>,
    /// Requested asset URI → reply frame index.
    pub assets: BTreeMap<String, usize>,
    /// Topic → frame indices in arrival order.
    pub streams: BTreeMap<String, Vec<usize>>,
}

#[derive(Debug, Default)]
pub struct Corpus {
    pub manifest: Manifest,
    pub frames: Vec<Vec<u8>>,
}

impl Corpus {
    pub fn push(&mut self, frame: Vec<u8>) -> usize {
        self.frames.push(frame);
        self.frames.len() - 1
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut f = std::fs::File::create(dir.join("frames.bin"))?;
        for frame in &self.frames {
            f.write_all(&(frame.len() as u32).to_le_bytes())?;
            f.write_all(frame)?;
        }
        let json = serde_json::to_string_pretty(&self.manifest).expect("manifest serializes");
        std::fs::write(dir.join("manifest.json"), json + "\n")
    }

    pub fn load(dir: &Path) -> std::io::Result<Corpus> {
        let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut bytes = Vec::new();
        std::fs::File::open(dir.join("frames.bin"))?.read_to_end(&mut bytes)?;
        let mut frames = Vec::new();
        let mut i = 0;
        while i + 4 <= bytes.len() {
            let len = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
            let end = (i + len).min(bytes.len());
            frames.push(bytes[i..end].to_vec());
            i = end;
        }
        Ok(Corpus { manifest, frames })
    }

    pub fn frame(&self, idx: usize) -> Option<&[u8]> {
        self.frames.get(idx).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("gz-parity-corpus-{}", std::process::id()));
        let mut c = Corpus::default();
        c.manifest.format = FORMAT.into();
        c.manifest.world = "w".into();
        let p = c.push(b"syntax".to_vec());
        c.manifest.protos = Some(p);
        let s = c.push(b"pub,scene,gz.msgs.Scene,\x00\x01".to_vec());
        c.manifest.responses.insert("scene".into(), s);
        c.save(&dir).unwrap();
        let back = Corpus::load(&dir).unwrap();
        assert_eq!(back.frames, c.frames);
        assert_eq!(back.manifest.responses["scene"], 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
