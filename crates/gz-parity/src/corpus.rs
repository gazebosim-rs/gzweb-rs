//! Corpus format (`.agent/PARITY.md` §1): `manifest.json` + `frames.bin`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use gz_core::frame;
use gz_core::msgs;
use prost::Message as _;
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
        let manifest: Manifest =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut bytes = Vec::new();
        std::fs::File::open(dir.join("frames.bin"))?.read_to_end(&mut bytes)?;
        let mut frames = Vec::new();
        let mut i = 0;
        while i + 4 <= bytes.len() {
            let len =
                u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
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

/// Builds a synthetic corpus: what a Gazebo server would have sent for a
/// given scene. Used by tests that need worlds the recorded corpora lack.
#[derive(Debug, Default)]
pub struct Builder {
    corpus: Corpus,
}

impl Builder {
    /// A corpus for `world`, with placeholder proto definitions and replies
    /// to `topics-types` and `worlds`.
    pub fn new(world: &str) -> Builder {
        let mut b = Builder::default();
        b.corpus.manifest.format = FORMAT.into();
        b.corpus.manifest.world = world.into();
        b.corpus.manifest.protos = Some(b.corpus.push(b"syntax = \"proto3\";".to_vec()));
        let topics = msgs::Publishers::default();
        b.reply(
            "topics-types",
            "gz.msgs.Publishers",
            &topics.encode_to_vec(),
        );
        let worlds = msgs::StringMsgV {
            data: vec![world.into()],
            ..Default::default()
        };
        b.reply("worlds", "gz.msgs.StringMsg_V", &worlds.encode_to_vec());
        b
    }

    fn reply(&mut self, op: &str, msg_type: &str, payload: &[u8]) {
        let idx = self
            .corpus
            .push(frame::build_binary("pub", op, msg_type, payload));
        self.corpus.manifest.responses.insert(op.into(), idx);
    }

    /// The reply to `scene,<world>`.
    pub fn scene(mut self, scene: &msgs::Scene) -> Builder {
        self.reply("scene", "gz.msgs.Scene", &scene.encode_to_vec());
        self
    }

    /// Serves `bytes` for an `asset` request for `uri`.
    pub fn asset(mut self, uri: &str, bytes: &[u8]) -> Builder {
        let payload = msgs::Bytes {
            data: bytes.to_vec(),
            ..Default::default()
        };
        let idx = self.corpus.push(frame::build_binary(
            "asset",
            uri,
            "gz.msgs.Bytes",
            &payload.encode_to_vec(),
        ));
        self.corpus.manifest.assets.insert(uri.into(), idx);
        self
    }

    /// Appends a message to `topic`'s stream, sent after a `sub` for it.
    pub fn publish(mut self, topic: &str, msg_type: &str, msg: &impl prost::Message) -> Builder {
        let idx = self.corpus.push(frame::build_binary(
            "pub",
            topic,
            msg_type,
            &msg.encode_to_vec(),
        ));
        self.corpus
            .manifest
            .streams
            .entry(topic.into())
            .or_default()
            .push(idx);
        self
    }

    /// Appends a raw frame to `topic`'s stream (for malformed-input tests).
    pub fn publish_raw(mut self, topic: &str, frame: &[u8]) -> Builder {
        let idx = self.corpus.push(frame.to_vec());
        self.corpus
            .manifest
            .streams
            .entry(topic.into())
            .or_default()
            .push(idx);
        self
    }

    pub fn build(self) -> Corpus {
        self.corpus
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

    #[test]
    fn builder_lays_out_a_server_session() {
        let scene = msgs::Scene {
            name: "w".into(),
            ..Default::default()
        };
        let pose = msgs::PoseV::default();
        let c = Builder::new("w")
            .scene(&scene)
            .asset("/m.obj", b"v 0 0 0")
            .publish("/t", "gz.msgs.Pose_V", &pose)
            .publish_raw("/t", b"raw")
            .build();
        let m = &c.manifest;
        assert_eq!(m.format, FORMAT);
        assert_eq!(m.world, "w");
        assert_eq!(
            c.frame(m.protos.unwrap()),
            Some(&b"syntax = \"proto3\";"[..])
        );

        let worlds = frame::Frame::parse(c.frame(m.responses["worlds"]).unwrap()).unwrap();
        assert_eq!((worlds.op, worlds.topic), ("pub", "worlds"));
        let names = msgs::StringMsgV::decode(worlds.payload).unwrap();
        assert_eq!(names.data, ["w"]);
        assert!(m.responses.contains_key("topics-types"));
        let sc = frame::Frame::parse(c.frame(m.responses["scene"]).unwrap()).unwrap();
        assert_eq!(msgs::Scene::decode(sc.payload).unwrap().name, "w");

        let asset = frame::Frame::parse(c.frame(m.assets["/m.obj"]).unwrap()).unwrap();
        assert_eq!(
            (asset.op, asset.topic, asset.msg_type),
            ("asset", "/m.obj", "gz.msgs.Bytes")
        );
        assert_eq!(msgs::Bytes::decode(asset.payload).unwrap().data, b"v 0 0 0");

        let stream = &m.streams["/t"];
        assert_eq!(stream.len(), 2);
        assert_eq!(c.frame(stream[1]), Some(&b"raw"[..]));
        assert_eq!(c.frame(999), None);
    }

    #[test]
    fn load_tolerates_a_truncated_last_frame_and_rejects_bad_manifests() {
        let dir = std::env::temp_dir().join(format!("gz-parity-corpus-bad-{}", std::process::id()));
        Builder::new("w").build().save(&dir).unwrap();
        // A length prefix promising more bytes than remain, plus a stray byte.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("frames.bin"))
            .unwrap();
        f.write_all(&100u32.to_le_bytes()).unwrap();
        f.write_all(b"abc").unwrap();
        drop(f);
        let c = Corpus::load(&dir).unwrap();
        assert_eq!(c.frames.last().unwrap(), b"abc");

        std::fs::write(dir.join("manifest.json"), "{ not json").unwrap();
        let err = Corpus::load(&dir).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Corpus::load(&dir).is_err());
    }
}
