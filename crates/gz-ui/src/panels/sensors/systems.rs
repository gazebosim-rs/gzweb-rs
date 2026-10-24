//! The Sensors panel's systems: the selected model's sensors and their
//! topics, the chosen one, its subscription (only while the panel shows)
//! and its data, then the rows and the preview in place (DECISIONS.md
//! D-103).

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzPublish, GzStatus};
use gz_bevy::overlays::extras::{want_sdf as want, SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzSensors, GzWorldRoot};
use gz_bevy::topics::{GzSubscriptions, GzTopics, Holder};
use gz_bevy::view::{PartKind, Selection};
use gz_core::echo::message_text;
use gz_core::scene::NodeKind;
use gz_core::sensor::{png_size, Scan};

use super::model::{
    badge, sensor_topic, stale_label, Preview, PreviewKind, SensorEntry, SensorsState, PREVIEW_RATE,
};
use super::view::{sensor_row, PreviewPart, SensorRow, SensorRows};
use super::{SensorsStart, ViewSensor, SENSORS};
use crate::panels::topics::clip_lines;
use crate::panels::PanelView;
use crate::raster::Canvas;
use crate::shell::{
    CurrentPhase, PanelStatus, Phase, SetPanelStatus, ShownPanels, Slot, UiCommand,
};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Theme};
use crate::widgets::layout::set_shown;
use crate::widgets::text::set_text;

/// What holds the previewed topic.
pub(crate) const HOLDER: Holder = "sensors_preview";

/// How often a text preview redraws.
const TEXT_HZ: f64 = 10.0;

/// The empty states (§3.3.1).
pub(crate) const SELECT: &str = "Select a model with sensors";
pub(crate) const NONE: &str = "No sensors in this world";
pub(crate) const WAITING: &str = "Waiting for first message…";
pub(crate) const NO_TOPIC: &str = "No topic for this sensor yet";

/// The selected model's sensors.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct SensorList {
    pub model: Option<(Entity, String)>,
    pub entries: Vec<SensorEntry>,
    /// Some model in the world has a sensor.
    pub any: bool,
}

impl SensorList {
    fn find(&self, key: &(Entity, String)) -> Option<&SensorEntry> {
        self.entries
            .iter()
            .find(|e| e.link == key.0 && e.name == key.1)
    }
}

/// A camera frame being decoded off the main thread, and the newest frame
/// waiting for it.
#[derive(Resource, Default)]
pub(crate) struct Decoding {
    task: Option<Task<Option<Image>>>,
    pending: Option<Vec<u8>>,
}

/// What the rows were built for.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltRows(Option<Vec<(Entity, String, String)>>);

/// The lidar view's image, drawn at its size.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct ScanImage {
    handle: Option<Handle<Image>>,
    drawn: Option<(UVec2, bool)>,
}

/// The view: "Not connected", the two empty states, else the list.
pub(super) fn view(live: bool, any: bool, sensors: bool) -> PanelView {
    match (live, sensors) {
        (false, _) => PanelView::empty("Not connected"),
        (true, true) => PanelView::Ready,
        (true, false) if any => PanelView::empty(SELECT),
        (true, false) => PanelView::empty(NONE),
    }
}

/// A scene node's parts the list reads.
type NodeParts = (
    &'static GzEntity,
    Option<&'static GzSensors>,
    Option<&'static Children>,
    Option<&'static ChildOf>,
);

/// What the list is read from.
#[derive(SystemParam)]
pub(super) struct ListSources<'w, 's> {
    selection: Option<Res<'w, Selection>>,
    status: Option<Res<'w, GzStatus>>,
    topics: Res<'w, GzTopics>,
    sdf: Option<Res<'w, SdfExtras>>,
    nodes: Query<'w, 's, NodeParts>,
    roots: Query<'w, 's, (), With<GzWorldRoot>>,
    sensors: Query<'w, 's, &'static GzSensors>,
    changed: Query<'w, 's, (), Changed<GzSensors>>,
}

impl ListSources<'_, '_> {
    fn changed(&self) -> bool {
        self.selection.as_ref().is_some_and(Res::is_changed)
            || self.topics.is_changed()
            || self.sdf.as_ref().is_some_and(Res::is_changed)
            || self.status.as_ref().is_some_and(Res::is_changed)
            || !self.changed.is_empty()
    }

    /// The top-level model `e` is in.
    fn model_of(&self, mut e: Entity) -> Option<Entity> {
        for _ in 0..64 {
            let (_, _, _, parent) = self.nodes.get(e).ok()?;
            let parent = parent?.parent();
            if self.roots.contains(parent) {
                return Some(e);
            }
            e = parent;
        }
        None
    }

    /// `model`'s sensors, in the scene's order.
    fn entries(&self, model: Entity) -> Vec<SensorEntry> {
        let world = self
            .status
            .as_deref()
            .and_then(|s| s.world.clone())
            .unwrap_or_default();
        let extras = self.sdf.as_deref().and_then(|s| s.extras.as_ref());
        let mut out = vec![];
        let mut stack = vec![model];
        while let Some(e) = stack.pop() {
            let Ok((g, sensors, children, _)) = self.nodes.get(e) else {
                continue;
            };
            if let (NodeKind::Link, Some(sensors)) = (g.kind, sensors) {
                let link = extras.and_then(|w| w.link(&g.path));
                for s in &sensors.0 {
                    let sdf = link.and_then(|l| l.sensors.iter().find(|x| x.name == s.name));
                    let topic = sensor_topic(
                        &world,
                        &g.path,
                        &s.name,
                        &s.ty,
                        sdf.and_then(|x| x.topic.as_deref()),
                        &self.topics,
                    );
                    out.push(SensorEntry {
                        link: e,
                        link_path: g.path.clone(),
                        name: s.name.clone(),
                        ty: s.ty.clone(),
                        topic,
                        rate: sdf.map(|x| x.update_rate).filter(|r| *r > 0.0),
                    });
                }
            }
            if let Some(children) = children {
                stack.extend(children.iter().rev());
            }
        }
        out
    }
}

/// The selected model's sensors, when what they come from changes.
pub(super) fn collect(src: ListSources, mut list: ResMut<SensorList>) {
    if !src.changed() && !list.is_added() {
        return;
    }
    let any = src.sensors.iter().any(|s| !s.0.is_empty());
    let model = src
        .selection
        .as_deref()
        .and_then(Selection::first_target)
        .and_then(|e| src.model_of(e));
    let entries = model.map(|m| src.entries(m)).unwrap_or_default();
    let model = model.and_then(|m| Some((m, src.nodes.get(m).ok()?.0.name.clone())));
    list.set_if_neq(SensorList {
        model,
        entries,
        any,
    });
}

/// A [`SensorsStart`]: opens the panel at once, and chooses its sensor
/// once the list has it.
pub(super) fn apply_start(
    start: Option<Res<SensorsStart>>,
    list: Res<SensorList>,
    mut state: ResMut<SensorsState>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    if start.is_added() {
        ui.write(UiCommand::OpenPanel {
            panel: SENSORS,
            slot: Slot::Primary,
        });
    }
    let Some(name) = &start.sensor else {
        commands.remove_resource::<SensorsStart>();
        return;
    };
    if let Some(e) = list.entries.iter().find(|e| e.name == *name) {
        state.chosen = Some(e.key());
        commands.remove_resource::<SensorsStart>();
    }
}

/// The inspector's View in Sensors chooses its sensor.
pub(super) fn view_sensor(mut views: MessageReader<ViewSensor>, mut state: ResMut<SensorsState>) {
    for v in views.read() {
        state.chosen = Some((v.0, v.1.clone()));
    }
}

/// The chosen sensor: a selected sensor of the model, else the one chosen
/// before if the model still has it, else its first camera, lidar or
/// other sensor.
pub(super) fn choose(
    list: Res<SensorList>,
    selection: Option<Res<Selection>>,
    mut state: ResMut<SensorsState>,
) {
    let picked = selection
        .as_ref()
        .filter(|s| s.is_changed())
        .and_then(|s| s.parts.iter().find(|p| p.kind == PartKind::Sensor))
        .map(|p| (p.owner, p.name.clone()))
        .filter(|k| list.find(k).is_some());
    if let Some(k) = picked {
        if state.chosen.as_ref() != Some(&k) {
            state.chosen = Some(k);
        }
        return;
    }
    if !list.is_changed() && !state.is_changed() {
        return;
    }
    if state
        .chosen
        .as_ref()
        .is_some_and(|k| list.find(k).is_some())
    {
        return;
    }
    let rank = |e: &SensorEntry| match crate::inspectors::sensor::message_type(&e.ty) {
        Some("gz.msgs.Image") => 0,
        Some("gz.msgs.LaserScan") => 1,
        _ => 2,
    };
    let first = list
        .entries
        .iter()
        .min_by_key(|e| rank(e))
        .map(SensorEntry::key);
    if state.chosen != first {
        state.chosen = first;
    }
}

/// The view, and the header's count ("4 on amr_01").
pub(super) fn sensors_status(
    phase: Res<CurrentPhase>,
    list: Res<SensorList>,
    mut status: ResMut<PanelStatus>,
) {
    if !phase.is_changed() && !list.is_changed() {
        return;
    }
    let live = phase.0 == Phase::Live;
    let n = list.entries.len();
    status.set_view(SENSORS, view(live, list.any, n > 0));
    let count = list
        .model
        .as_ref()
        .filter(|_| live && n > 0)
        .map(|(_, name)| format!("{n} on {name}"));
    status.set_count(SENSORS, count);
}

/// The world's SDFormat (the sensors' `<topic>`s) is wanted while the
/// panel shows.
pub(super) fn want_sdf(hosts: Res<ShownPanels>, wanted: Option<ResMut<SdfWanted>>) {
    if let Some(mut wanted) = wanted {
        want(&mut wanted, "sensors", hosts.is_shown(SENSORS));
    }
}

/// The preview follows the chosen sensor's topic.
pub(super) fn start_preview(
    list: Res<SensorList>,
    state: Res<SensorsState>,
    topics: Res<GzTopics>,
    mut preview: ResMut<Preview>,
) {
    if !list.is_changed() && !state.is_changed() && !topics.is_changed() {
        return;
    }
    let topic = state
        .chosen
        .as_ref()
        .and_then(|k| list.find(k))
        .and_then(|e| e.topic.clone());
    let chosen = state.chosen.is_some();
    if preview.topic == topic && (chosen || preview.topic.is_none()) {
        return;
    }
    let msg_type = topic
        .as_deref()
        .and_then(|t| topics.get(t))
        .map(|t| t.msg_type.clone())
        .unwrap_or_default();
    preview.start(topic, &msg_type);
}

/// Holds the previewed topic while the panel shows on a live world.
pub(super) fn hold_preview(
    hosts: Res<ShownPanels>,
    phase: Res<CurrentPhase>,
    preview: Res<Preview>,
    mut subs: ResMut<GzSubscriptions>,
) {
    let live = phase.0 == Phase::Live && hosts.is_shown(SENSORS);
    let want = preview.topic.as_deref().filter(|_| live);
    if subs.bypass_change_detection().held_by(HOLDER).eq(want) {
        return;
    }
    let held: Vec<String> = subs.held_by(HOLDER).map(str::to_string).collect();
    for topic in &held {
        subs.release(HOLDER, topic);
    }
    if let Some(topic) = want {
        subs.hold(HOLDER, topic, Some(PREVIEW_RATE));
    }
}

/// The previewed topic's messages: a camera's newest frame waits for the
/// decoder, a scan is read at once, any other message is written as text
/// at most [`TEXT_HZ`] times a second.
pub(super) fn read_preview(
    time: Res<Time<Real>>,
    mut publishes: MessageReader<GzPublish>,
    mut preview: ResMut<Preview>,
    mut decoding: ResMut<Decoding>,
    mut text_at: Local<f64>,
) {
    let Some(topic) = preview.topic.clone() else {
        publishes.clear();
        return;
    };
    let now = time.elapsed_secs_f64();
    let mut newest = None;
    for p in publishes.read().filter(|p| p.topic == topic) {
        newest = Some(p);
    }
    let Some(p) = newest else {
        return;
    };
    let msg_type = if p.msg_type.is_empty() {
        preview.msg_type.clone()
    } else {
        p.msg_type.clone()
    };
    preview.last_at = Some(now);
    match preview.kind {
        PreviewKind::Image => {
            if let Some(size) = png_size(&p.payload) {
                if preview.size != Some(size) {
                    preview.size = Some(size);
                }
            }
            decoding.pending = Some(p.payload.clone());
        }
        PreviewKind::Scan => {
            preview.scan = Scan::decode(&p.payload);
            preview.fresh = true;
        }
        PreviewKind::Text => {
            if now - *text_at >= 1.0 / TEXT_HZ {
                *text_at = now;
                preview.text = Some(match message_text(&msg_type, &p.payload) {
                    Ok(t) => clip_lines(&t),
                    Err(e) => e.to_string(),
                });
                preview.fresh = true;
            }
        }
    }
}

/// Decodes the newest frame off the main thread, one at a time, and shows
/// each decoded one.
pub(super) fn finish_decode(
    mut decoding: ResMut<Decoding>,
    mut preview: ResMut<Preview>,
    mut images: ResMut<Assets<Image>>,
) {
    if let Some(task) = decoding.task.as_mut() {
        let Some(done) = check_ready(task) else {
            return;
        };
        decoding.task = None;
        if let Some(image) = done.filter(|_| preview.kind == PreviewKind::Image) {
            match preview.image.clone() {
                Some(h) => {
                    let _ = images.insert(&h, image);
                }
                None => preview.image = Some(images.add(image)),
            }
            preview.fresh = true;
        }
    }
    if let Some(png) = decoding.pending.take() {
        decoding.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            Image::from_buffer(
                &png,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::linear(),
                RenderAssetUsages::default(),
            )
            .ok()
        }));
    }
}

/// The rows, rebuilt when the sensors change.
pub(super) fn sync_rows(
    theme: Res<Theme>,
    list: Res<SensorList>,
    parents: Query<(Entity, Ref<SensorRows>)>,
    mut built: ResMut<BuiltRows>,
    mut commands: Commands,
) {
    let Ok((parent, added)) = parents.single() else {
        return;
    };
    let key: Vec<(Entity, String, String)> = list
        .entries
        .iter()
        .map(|e| (e.link, e.name.clone(), rate_label(e.rate)))
        .collect();
    if !added.is_added() && built.0.as_ref() == Some(&key) {
        return;
    }
    commands.entity(parent).despawn_children();
    let rows: Vec<_> = list
        .entries
        .iter()
        .map(|e| sensor_row(&theme, e, badge(&e.ty), rate_label(e.rate)))
        .collect();
    commands.entity(parent).with_children(|r| {
        for row in rows {
            r.spawn(row);
        }
    });
    built.0 = Some(key);
}

/// A sensor's update rate ("30 Hz"), or "—".
fn rate_label(rate: Option<f64>) -> String {
    rate.map_or_else(
        || "—".into(),
        |r| format!("{} Hz", crate::format::number(r, 1)),
    )
}

/// The chosen row shows active.
pub(super) fn row_states(
    state: Res<SensorsState>,
    rows: Query<(Entity, &SensorRow, Has<Active>)>,
    added: Query<(), Added<SensorRow>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for (e, row, active) in &rows {
        let want = state.chosen.as_ref() == Some(&row.0);
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
}

/// The preview's section follows the chosen sensor: its name and size, the
/// frame (dimmed until the chosen sensor's first), the scan or the text.
#[allow(clippy::type_complexity)]
pub(super) fn preview_parts(
    list: Res<SensorList>,
    state: Res<SensorsState>,
    preview: Res<Preview>,
    mut parts: Query<(
        &PreviewPart,
        &mut Node,
        Option<&mut Text>,
        Option<&mut ImageNode>,
    )>,
    added: Query<(), Added<PreviewPart>>,
) {
    if !preview.is_changed() && !state.is_changed() && !list.is_changed() && added.is_empty() {
        return;
    }
    let chosen = state.chosen.as_ref().and_then(|k| list.find(k));
    let rate = chosen
        .and_then(|e| e.rate)
        .map(|r| format!(" · {} Hz", crate::format::number(r, 1)));
    let meta = match preview.kind {
        PreviewKind::Image => preview
            .size
            .map(|(w, h)| format!("{w}×{h}{}", rate.clone().unwrap_or_default())),
        PreviewKind::Scan => preview.scan.as_ref().map(|s| {
            format!(
                "{} × {}{}",
                s.count,
                s.vertical_count,
                rate.clone().unwrap_or_default()
            )
        }),
        PreviewKind::Text => Some(
            preview
                .msg_type
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_string(),
        ),
    }
    .unwrap_or_default();
    for (part, mut node, text, image) in &mut parts {
        match part {
            PreviewPart::Section => set_shown(&mut node, chosen.is_some()),
            PreviewPart::Title => {
                if let Some(mut t) = text {
                    set_text(
                        &mut t,
                        &chosen.map(|e| e.name.to_uppercase()).unwrap_or_default(),
                    );
                }
            }
            PreviewPart::Meta => {
                if let Some(mut t) = text {
                    set_text(&mut t, &meta);
                }
            }
            PreviewPart::Image => {
                let shown = preview.kind == PreviewKind::Image && preview.image.is_some();
                set_shown(&mut node, shown);
                if let (Some(mut img), Some(h)) = (image, preview.image.as_ref()) {
                    if img.image != *h {
                        img.image = h.clone();
                    }
                    // The last frame, dimmed, until this sensor's first.
                    let alpha = if preview.fresh { 1.0 } else { 0.5 };
                    if img.color.alpha() != alpha {
                        img.color = img.color.with_alpha(alpha);
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                let aspect = preview.size.map(|(w, h)| w as f32 / h.max(1) as f32);
                if node.aspect_ratio != aspect {
                    node.aspect_ratio = aspect;
                }
            }
            PreviewPart::Scan => set_shown(&mut node, preview.kind == PreviewKind::Scan),
            PreviewPart::TextArea => set_shown(&mut node, preview.kind == PreviewKind::Text),
            PreviewPart::Text => {
                if let Some(mut t) = text {
                    set_text(&mut t, preview.text.as_deref().unwrap_or_default());
                }
            }
            PreviewPart::Status => {}
        }
    }
}

/// The lidar view (§3.3): the scan from above, the sensor at the dot and
/// forward up the view; range rings at the full and half range and the
/// axes in `line_soft`; the returns as a 2 px line in `danger`, broken
/// where a beam returns nothing. Drawn when the scan, the size or the
/// theme changes.
pub(super) fn draw_scan(
    theme: Res<Theme>,
    preview: Res<Preview>,
    mut views: Query<(
        Entity,
        &PreviewPart,
        &ComputedNode,
        &mut ImageNode,
        Option<&mut ScanImage>,
    )>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    for (e, part, computed, mut node, scan_image) in &mut views {
        if *part != PreviewPart::Scan {
            continue;
        }
        let Some(mut drawn) = scan_image else {
            commands.entity(e).insert(ScanImage::default());
            continue;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = computed.size().round().as_uvec2();
        if size.x == 0 || size.y == 0 {
            continue;
        }
        let key = (size, theme.is_dark);
        if !preview.is_changed() && !theme.is_changed() && drawn.drawn == Some(key) {
            continue;
        }
        drawn.drawn = Some(key);
        let scale = 1.0 / computed.inverse_scale_factor;
        let image = scan_view(&theme, preview.scan.as_ref(), size, scale);
        match drawn.handle.clone() {
            Some(h) => {
                let _ = images.insert(&h, image);
            }
            None => {
                let h = images.add(image);
                node.image = h.clone();
                drawn.handle = Some(h);
            }
        }
    }
}

/// The lidar view's pixels.
fn scan_view(theme: &Theme, scan: Option<&Scan>, size: UVec2, scale: f32) -> Image {
    let (w, h) = (size.x as usize, size.y as usize);
    let mut canvas = Canvas::new(w, h);
    #[allow(clippy::cast_precision_loss)]
    let (wf, hf) = (w as f32, h as f32);
    let pad = 8.0 * scale;
    // A forward-looking scan has its sensor low in the view; one that
    // sees all round, in the middle.
    let wide = scan.is_some_and(|s| s.span() > std::f64::consts::PI + 1e-3);
    let (center, radius) = if wide {
        (
            Vec2::new(wf / 2.0, hf / 2.0),
            (wf.min(hf) / 2.0 - pad).max(1.0),
        )
    } else {
        (
            Vec2::new(wf / 2.0, hf - pad),
            (wf / 2.0 - pad).min(hf - 2.0 * pad).max(1.0),
        )
    };
    let soft = theme.color(ColorRole::LineSoft).to_linear().to_f32_array();
    let hair = scale.round().max(1.0);
    canvas.circle(center, radius, hair, soft);
    canvas.circle(center, radius / 2.0, hair, soft);
    canvas.polyline(
        &[
            Vec2::new(center.x - radius, center.y),
            Vec2::new(center.x + radius, center.y),
        ],
        hair,
        soft,
    );
    canvas.polyline(
        &[
            Vec2::new(center.x, center.y - radius),
            Vec2::new(center.x, center.y + radius),
        ],
        hair,
        soft,
    );
    if let Some(scan) = scan.filter(|s| s.range_max > 0.0) {
        let color = theme.color(ColorRole::Danger).to_linear().to_f32_array();
        let mut line: Vec<Vec2> = vec![];
        #[allow(clippy::cast_possible_truncation)]
        for &(a, r) in &scan.beams {
            if !scan.hit(r) {
                if !line.is_empty() {
                    canvas.polyline(&line, 2.0 * scale, color);
                }
                line.clear();
                continue;
            }
            let d = (r / scan.range_max) as f32 * radius;
            let a = a as f32;
            // Forward is up; a positive angle turns left.
            line.push(center + Vec2::new(-a.sin(), -a.cos()) * d);
        }
        if !line.is_empty() {
            canvas.polyline(&line, 2.0 * scale, color);
        }
    }
    canvas.dot(
        center,
        3.0 * scale,
        theme.color(ColorRole::Text).to_linear().to_f32_array(),
    );
    canvas.image()
}

/// Over the preview: "Waiting for first message…" until one comes, "No
/// data for 3.2 s" once it goes quiet (§3.3.1), or why there's no topic.
pub(super) fn preview_status(
    time: Res<Time<Real>>,
    state: Res<SensorsState>,
    preview: Res<Preview>,
    mut parts: Query<(&PreviewPart, &mut Node, &mut Text)>,
) {
    let now = time.elapsed_secs_f64();
    let say = if state.chosen.is_none() {
        None
    } else if preview.topic.is_none() {
        Some(NO_TOPIC.to_string())
    } else if preview.last_at.is_none() {
        Some(WAITING.to_string())
    } else {
        preview.stale(now).map(stale_label)
    };
    for (part, mut node, mut text) in &mut parts {
        if *part == PreviewPart::Status {
            set_text(&mut text, say.as_deref().unwrap_or_default());
            set_shown(&mut node, say.is_some());
        }
    }
}

/// A row's click shows its sensor.
pub(super) fn on_row(ev: On<Activate>, rows: Query<&SensorRow>, mut state: ResMut<SensorsState>) {
    if let Ok(row) = rows.get(ev.entity) {
        if state.chosen.as_ref() != Some(&row.0) {
            state.chosen = Some(row.0.clone());
        }
    }
}
