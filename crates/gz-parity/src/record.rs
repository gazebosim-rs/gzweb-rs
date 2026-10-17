//! Records a corpus from a live Gazebo websocket server by performing the
//! client handshake itself (no browser needed), fetching every asset the
//! scene references, and capturing the subscribed streams for a while.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use gz_core::frame::Frame;
use gz_core::{msgs, uri};
use prost::Message as _;
use tungstenite::{stream::MaybeTlsStream, Message};

use crate::corpus::{Corpus, Manifest, FORMAT};

pub struct RecordOptions {
    pub url: String,
    pub duration: Duration,
    pub max_stream_frames: usize,
    pub world_file: Option<String>,
    /// How long to wait for each handshake reply and for asset replies.
    pub reply_timeout: Duration,
}

pub fn record(opts: &RecordOptions) -> Result<Corpus, String> {
    let (mut ws, _) = tungstenite::connect(opts.url.as_str()).map_err(crate::message)?;
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        tcp.set_read_timeout(Some(Duration::from_millis(50)))
            .map_err(crate::message)?;
    }
    let mut corpus = Corpus {
        manifest: Manifest {
            format: FORMAT.into(),
            ..Default::default()
        },
        frames: Vec::new(),
    };
    if let Some(w) = &opts.world_file {
        corpus
            .manifest
            .source
            .insert("world_file".into(), w.clone());
    }

    let send = |ws: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>, s: &str| {
        ws.send(Message::Text(s.to_string().into()))
            .map_err(crate::message)
    };
    let read = |ws: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
                deadline: Instant|
     -> Result<Option<Vec<u8>>, String> {
        loop {
            if Instant::now() > deadline {
                return Ok(None);
            }
            match ws.read() {
                Ok(Message::Binary(b)) => return Ok(Some(b.to_vec())),
                Ok(Message::Text(t)) => return Ok(Some(t.as_bytes().to_vec())),
                Ok(Message::Close(_)) => return Err("server closed the connection".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    };
    let timeout = || Instant::now() + opts.reply_timeout;

    // 1. protos: the first frame after the request.
    send(&mut ws, "protos,,,")?;
    let protos = read(&mut ws, timeout())?.ok_or("no reply to protos")?;
    corpus.manifest.protos = Some(corpus.push(protos));

    // 2. topics-types + worlds, then scene.
    send(&mut ws, "topics-types,,,")?;
    send(&mut ws, "worlds,,,")?;
    let mut scene_msg: Option<msgs::Scene> = None;
    let deadline = timeout();
    while scene_msg.is_none() {
        let data = read(&mut ws, deadline)?.ok_or("timed out waiting for worlds/scene")?;
        let Some(f) = Frame::parse(&data) else {
            continue;
        };
        if f.op != "pub" {
            continue;
        }
        match f.topic {
            "topics-types" => {
                let idx = corpus.push(data.clone());
                corpus.manifest.responses.insert("topics-types".into(), idx);
            }
            "worlds" => {
                let worlds = msgs::StringMsgV::decode(f.payload).map_err(crate::message)?;
                let world = worlds
                    .data
                    .first()
                    .cloned()
                    .ok_or("server reported no worlds")?;
                corpus.manifest.world = world.clone();
                let idx = corpus.push(data.clone());
                corpus.manifest.responses.insert("worlds".into(), idx);
                send(&mut ws, &format!("scene,{world},,"))?;
            }
            "scene" => {
                scene_msg = Some(msgs::Scene::decode(f.payload).map_err(crate::message)?);
                let idx = corpus.push(data.clone());
                corpus.manifest.responses.insert("scene".into(), idx);
            }
            _ => {}
        }
    }
    let scene = scene_msg.ok_or("server sent no scene")?;

    // 3. Every non-HTTP resource the scene references, under the URI both
    //    clients will request (gzweb strips file:// and model://).
    let mut wanted = BTreeSet::new();
    collect_uris(&scene, &mut wanted);
    for u in &wanted {
        send(&mut ws, &format!("asset,,,{u}"))?;
    }

    // 4. Streams, in gzweb's subscription order.
    let world = corpus.manifest.world.clone();
    let topics = [
        format!("/world/{world}/dynamic_pose/info"),
        format!("/world/{world}/scene/info"),
    ];
    for t in &topics {
        send(&mut ws, &format!("sub,{t},,"))?;
    }

    let end = Instant::now() + opts.duration;
    let asset_deadline = timeout();
    loop {
        let assets_done = wanted
            .iter()
            .all(|u| corpus.manifest.assets.contains_key(u));
        if Instant::now() > end && (assets_done || Instant::now() > asset_deadline) {
            break;
        }
        let Some(data) = read(&mut ws, Instant::now() + Duration::from_millis(100))? else {
            continue;
        };
        let Some(f) = Frame::parse(&data) else {
            continue;
        };
        match f.op {
            "asset" => {
                let key = f.topic.to_string();
                let idx = corpus.push(data.clone());
                corpus.manifest.assets.insert(key, idx);
            }
            "pub" => {
                let topic = f.topic.to_string();
                let n = corpus.manifest.streams.get(&topic).map_or(0, Vec::len);
                if n < opts.max_stream_frames {
                    let idx = corpus.push(data.clone());
                    corpus.manifest.streams.entry(topic).or_default().push(idx);
                }
            }
            _ => {}
        }
    }
    let missing: Vec<_> = wanted
        .iter()
        .filter(|u| !corpus.manifest.assets.contains_key(*u))
        .collect();
    if !missing.is_empty() {
        eprintln!("warning: no reply for assets {missing:?}");
    }
    let _ = ws.close(None);
    Ok(corpus)
}

fn collect_uris(scene: &msgs::Scene, out: &mut BTreeSet<String>) {
    fn visual(v: &msgs::Visual, out: &mut BTreeSet<String>) {
        if let Some(m) = v.geometry.as_ref().and_then(|g| g.mesh.as_ref()) {
            out.insert(uri::mesh_load_uri(&m.filename));
        }
        if let Some(pbr) = v.material.as_ref().and_then(|m| m.pbr.as_ref()) {
            for map in [
                &pbr.albedo_map,
                &pbr.normal_map,
                &pbr.roughness_map,
                &pbr.metalness_map,
                &pbr.emissive_map,
            ] {
                if !map.is_empty() {
                    out.insert(uri::mesh_load_uri(map));
                }
            }
        }
    }
    fn model(m: &msgs::Model, out: &mut BTreeSet<String>) {
        m.visual.iter().for_each(|v| visual(v, out));
        for l in &m.link {
            l.visual.iter().for_each(|v| visual(v, out));
        }
        m.model.iter().for_each(|n| model(n, out));
    }
    scene.model.iter().for_each(|m| model(m, out));
    out.retain(|u| !u.is_empty() && !uri::is_http(u));
}
