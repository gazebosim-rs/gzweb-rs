//! Workspaces (spec §8, §7.8; DECISIONS.md D-110): Build, Run, Debug and
//! Review, each a layout: the sidebar and inspector (open, width), the
//! slots' panels and their split, the pop-out windows, the views' preset
//! and divider shares, and each view's camera (pose, projection) and
//! overlays.
//!
//! Alt+1–4, the status bar's switcher and the command palette switch.
//! Switching keeps the layout being left, changed or not, in memory for
//! the session and shows the next one as it was left, or as saved, or
//! its default (§8's table). Save current layout writes it for good: to
//! `gz/ui/workspaces.json` in the platform's config directory natively,
//! to `localStorage` in the browser, with the current workspace, the
//! theme, the UI scale and the motion setting, which are written whenever
//! they change. The app starts in the
//! workspace it was left in: as saved, or as the app starts when never
//! saved (so start-up stays as it was).

use std::str::FromStr;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gz_bevy::camera::MainCamera;
use gz_bevy::overlays::{KeepGrid, Overlay, OverlaySet};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::editing::EditingModel;
use gz_bevy::view::{SnapView, ViewCamera, ViewCommand, ViewId, ViewOp, ViewPose};
use serde::{Deserialize, Serialize};

use crate::panels::{PanelId, PanelRegistry};
use crate::shell::layout::{PopOut, ShellLayout, Slot, DEFAULT_SPLIT};
use crate::shell::status_bar::readouts::StatusNote;
use crate::shell::viewport::views::{LayoutPreset, ViewLayout};
use crate::theme::{MotionMode, Sizes, Theme, ThemeMode};

/// The workspaces: id (in the file), name and binding.
pub(crate) const WORKSPACES: [(&str, &str, &str); 4] = [
    ("build", "Build", "ui.workspace_1"),
    ("run", "Run", "ui.workspace_2"),
    ("debug", "Debug", "ui.workspace_3"),
    ("review", "Review", "ui.workspace_4"),
];

/// The file's name, in `gz/ui/` under the platform's config directory.
pub(crate) const WORKSPACE_FILE: &str = "workspaces.json";

/// A camera as saved: where it is, or (a default's) an axis view to snap
/// to; and its overlays (by id).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SavedView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<SavedPose>,
    /// "top", "front", …: with a pose, the axis view it sits in; without,
    /// one to snap to (§8's Build: a top ortho view).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlays: Option<Vec<String>>,
}

/// [`ViewPose`] as saved.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SavedPose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub pivot: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ortho: Option<f32>,
}

impl From<ViewPose> for SavedPose {
    fn from(p: ViewPose) -> Self {
        Self {
            translation: p.translation.to_array(),
            rotation: p.rotation.to_array(),
            pivot: p.pivot.to_array(),
            ortho: p.ortho,
        }
    }
}

impl From<SavedPose> for ViewPose {
    fn from(p: SavedPose) -> Self {
        Self {
            translation: Vec3::from_array(p.translation),
            rotation: Quat::from_array(p.rotation),
            pivot: Vec3::from_array(p.pivot),
            ortho: p.ortho,
            snapped: None,
        }
    }
}

/// A pop-out window as saved.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedPopOut {
    pub panel: String,
    /// "primary" or "secondary".
    pub from: String,
    /// x, y, width, height in the viewport area (logical px).
    pub rect: [f32; 4],
}

/// A workspace's layout (§8).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WorkspaceLayout {
    pub sidebar_open: bool,
    pub sidebar_w: f32,
    pub inspector_open: bool,
    pub inspector_w: f32,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub split: f32,
    #[serde(default)]
    pub popouts: Vec<SavedPopOut>,
    /// The views' preset ([`LayoutPreset::id`]) and its divider shares.
    pub preset: String,
    pub shares: [f32; 2],
    /// Its views' cameras, first to last; fewer leave the rest as they
    /// are.
    #[serde(default)]
    pub views: Vec<SavedView>,
}

impl WorkspaceLayout {
    /// §8's defaults: Build (Scene, the inspector, a perspective and a top
    /// ortho view), Run (one view, nothing else), Debug (Topics over Plot,
    /// the inspector), Review (Plot).
    pub fn default_for(index: usize) -> Self {
        let s = Sizes::default();
        let base = Self {
            sidebar_open: true,
            sidebar_w: s.sidebar_w,
            inspector_open: true,
            inspector_w: s.inspector_w,
            primary: None,
            secondary: None,
            split: DEFAULT_SPLIT,
            popouts: vec![],
            preset: LayoutPreset::Single.id().into(),
            shares: LayoutPreset::Single.default_shares(),
            views: vec![],
        };
        match index {
            0 => Self {
                primary: Some("scene".into()),
                preset: LayoutPreset::TwoColumns.id().into(),
                shares: LayoutPreset::TwoColumns.default_shares(),
                views: vec![
                    SavedView::default(),
                    SavedView {
                        snap: Some("top".into()),
                        ..default()
                    },
                ],
                ..base
            },
            1 => Self {
                sidebar_open: false,
                inspector_open: false,
                ..base
            },
            2 => Self {
                primary: Some("topics".into()),
                secondary: Some("plot".into()),
                ..base
            },
            _ => Self {
                primary: Some("plot".into()),
                inspector_open: false,
                ..base
            },
        }
    }
}

/// The Workspaces panel (spec §3.3, §8): no activity button; the
/// switcher's Manage… and ⌘K open it.
pub const WORKSPACES_PANEL: PanelId = PanelId::new("workspaces");

/// One workspace: one of §8's four (Alt+1–4, a default) or the user's
/// own (Save as new…).
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    /// Its id in the file.
    pub id: String,
    pub name: String,
    /// Its place among §8's four; none for the user's.
    pub builtin: Option<usize>,
    /// Its layout as saved (a built-in one's default until it is).
    saved: WorkspaceLayout,
    /// Saved by the user (so written to the file); always, for the user's.
    user_saved: bool,
    /// As left this session (switching away).
    left: Option<WorkspaceLayout>,
}

impl Workspace {
    fn builtin(index: usize) -> Self {
        let (id, name, _) = WORKSPACES[index];
        Self {
            id: id.into(),
            name: name.into(),
            builtin: Some(index),
            saved: WorkspaceLayout::default_for(index),
            user_saved: false,
            left: None,
        }
    }

    fn custom(id: String, name: String, saved: WorkspaceLayout) -> Self {
        Self {
            id,
            name,
            builtin: None,
            saved,
            user_saved: true,
            left: None,
        }
    }

    /// Its layout as saved.
    pub fn saved(&self) -> &WorkspaceLayout {
        &self.saved
    }

    /// Its layout as switching to it shows it: as left, else as saved.
    pub fn shown(&self) -> &WorkspaceLayout {
        self.left.as_ref().unwrap_or(&self.saved)
    }

    /// It was left changed from its layout as saved (the current one's is
    /// [`CurrentLayout::modified`]).
    pub fn left_modified(&self) -> bool {
        self.left
            .as_ref()
            .is_some_and(|l| !same_layout(&self.saved, l))
    }

    /// Its `shortcuts.json` binding (a built-in one's Alt+N).
    pub fn binding(&self) -> Option<&'static str> {
        self.builtin.map(|i| WORKSPACES[i].2)
    }

    /// What Reset puts back: the layout as saved while it's modified, else
    /// a built-in one's default (none when it's that already).
    pub(crate) fn reset_to(&self, now: &WorkspaceLayout) -> Option<WorkspaceLayout> {
        if !same_layout(&self.saved, now) {
            return Some(self.saved.clone());
        }
        let default = WorkspaceLayout::default_for(self.builtin?);
        (!same_layout(&default, now)).then_some(default)
    }
}

/// The workspaces, §8's four first, then the user's; the current one.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Workspaces {
    pub current: usize,
    list: Vec<Workspace>,
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            current: 0,
            list: (0..WORKSPACES.len()).map(Workspace::builtin).collect(),
        }
    }
}

impl Workspaces {
    /// The current workspace's name.
    pub fn name(&self) -> &str {
        &self.list[self.current].name
    }

    /// The current workspace.
    pub fn current(&self) -> &Workspace {
        &self.list[self.current]
    }

    /// Every workspace, in order.
    pub fn list(&self) -> &[Workspace] {
        &self.list
    }

    /// Workspace `index`'s layout as saved.
    pub fn saved(&self, index: usize) -> &WorkspaceLayout {
        &self.list[index].saved
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|w| w.id == id)
    }

    /// `name`, or "name 2", "name 3", … when a workspace has it.
    fn free_name(&self, name: &str) -> String {
        let taken = |n: &str| self.list.iter().any(|w| w.name.eq_ignore_ascii_case(n));
        if !taken(name) {
            return name.to_string();
        }
        (2..)
            .map(|k| format!("{name} {k}"))
            .find(|n| !taken(n))
            .unwrap_or_default()
    }

    /// An id no workspace has: "user_1", "user_2", ….
    fn free_id(&self) -> String {
        (1..)
            .map(|k| format!("user_{k}"))
            .find(|id| self.index_of(id).is_none())
            .unwrap_or_default()
    }
}

/// Whether two layouts look the same: everything but the cameras' poses
/// (a view's overlays and axis view where `a` holds them) and what a closed
/// side panel would hold, within a pixel or a fraction's rounding.
pub(crate) fn same_layout(a: &WorkspaceLayout, b: &WorkspaceLayout) -> bool {
    let near = |x: f32, y: f32, by: f32| (x - y).abs() <= by;
    // The windows by panel: raising one reorders them.
    let by_panel = |w: &WorkspaceLayout| {
        let mut p = w.popouts.clone();
        p.sort_by(|x, y| x.panel.cmp(&y.panel));
        p
    };
    let (pa, pb) = (by_panel(a), by_panel(b));
    let popouts = pa.len() == pb.len()
        && pa.iter().zip(&pb).all(|(p, q)| {
            p.panel == q.panel
                && p.from == q.from
                && p.rect.iter().zip(q.rect).all(|(x, y)| near(*x, y, 1.0))
        });
    let views = a.views.iter().zip(&b.views).all(|(v, w)| {
        let overlays = match (&v.overlays, &w.overlays) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        };
        // The axis view it sits in (a default's to snap to).
        overlays && v.snap == w.snap
    });
    // The sidebar's panels and width count only while it's open (a
    // closed one is laid out with the first panel).
    let sidebar = a.sidebar_open == b.sidebar_open
        && (!a.sidebar_open
            || near(a.sidebar_w, b.sidebar_w, 0.5)
                && a.primary == b.primary
                && a.secondary == b.secondary
                && (a.secondary.is_none() || near(a.split, b.split, 0.005)));
    let inspector = a.inspector_open == b.inspector_open
        && (!a.inspector_open || near(a.inspector_w, b.inspector_w, 0.5));
    sidebar
        && inspector
        && a.preset == b.preset
        && a.shares
            .iter()
            .zip(b.shares)
            .all(|(x, y)| near(*x, y, 0.005))
        && popouts
        && views
}

/// The file's format. A newer one is read as far as it can be and never
/// written over.
pub(crate) const FILE_VERSION: u32 = 1;

/// What the file holds.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorkspaceFile {
    pub(crate) version: u32,
    pub(crate) current: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) theme: Option<String>,
    /// The UI scale (1.0 is 100%; DECISIONS.md D-118).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_scale"
    )]
    pub(crate) ui_scale: Option<f32>,
    /// The motion setting: `system`, `reduce` or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) motion: Option<String>,
    /// The workspaces saved, by id.
    #[serde(default)]
    pub(crate) workspaces: std::collections::BTreeMap<String, WorkspaceLayout>,
    /// The user's own workspaces, in order, with their names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) custom: Vec<CustomName>,
}

/// A user's workspace in the file: its id (in `workspaces`) and name.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CustomName {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Where the workspaces persist; none (tests, captures) keeps them for
/// the session.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct WorkspaceStore {
    pub at: StoreAt,
    /// The saved theme applies at start (not over `gzweb --theme`).
    pub theme: bool,
    /// The saved UI scale applies at start (not over `gzweb --ui-scale`).
    pub scale: bool,
    /// The saved motion setting applies at start (not over `gzweb
    /// --motion`).
    pub motion: bool,
}

/// Where a [`WorkspaceStore`] keeps its file.
#[derive(Clone, Debug, PartialEq)]
pub enum StoreAt {
    /// A file (natively: [`config_file`]).
    File(std::path::PathBuf),
    /// The browser's `localStorage` (`gzweb.workspaces`).
    Browser,
    /// In memory (tests): what was last written.
    Memory(Option<String>),
}

impl WorkspaceStore {
    /// This platform's: the config file natively, `localStorage` on the
    /// web; none without a config directory.
    pub fn platform() -> Option<Self> {
        #[cfg(target_arch = "wasm32")]
        let at = Some(StoreAt::Browser);
        #[cfg(not(target_arch = "wasm32"))]
        let at = config_file().map(StoreAt::File);
        at.map(|at| Self {
            at,
            theme: true,
            scale: true,
            motion: true,
        })
    }

    fn read(&self) -> Result<Option<String>, String> {
        match &self.at {
            StoreAt::File(path) => match std::fs::read_to_string(path) {
                Ok(s) => Ok(Some(s)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            #[cfg(target_arch = "wasm32")]
            StoreAt::Browser => Ok(gz_bevy::view::web::stored(
                gz_bevy::view::web::WORKSPACES_KEY,
            )),
            #[cfg(not(target_arch = "wasm32"))]
            StoreAt::Browser => Ok(None),
            StoreAt::Memory(s) => Ok(s.clone()),
        }
    }

    /// Writes `text`; a file whole or not at all (written beside it, then
    /// moved over it).
    fn write(&mut self, text: &str) -> Result<(), String> {
        match &mut self.at {
            StoreAt::File(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                let part = path.with_file_name(format!("{WORKSPACE_FILE}.part"));
                std::fs::write(&part, text)
                    .and_then(|()| std::fs::rename(&part, &*path))
                    .map_err(|e| e.to_string())
            }
            #[cfg(target_arch = "wasm32")]
            StoreAt::Browser => gz_bevy::view::web::store(gz_bevy::view::web::WORKSPACES_KEY, text)
                .then_some(())
                .ok_or_else(|| "the browser didn't keep it".to_string()),
            #[cfg(not(target_arch = "wasm32"))]
            StoreAt::Browser => Ok(()),
            StoreAt::Memory(s) => {
                *s = Some(text.to_string());
                Ok(())
            }
        }
    }

    /// Keeps `text`, which couldn't be read, apart (`workspaces.json.bad`,
    /// `gzweb.workspaces.bad`), so writing doesn't lose it; where.
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn set_aside(&mut self, text: &str) -> Result<String, String> {
        match &self.at {
            StoreAt::File(path) => {
                let bad = path.with_file_name(format!("{WORKSPACE_FILE}.bad"));
                std::fs::rename(path, &bad).map_err(|e| e.to_string())?;
                Ok(bad.display().to_string())
            }
            #[cfg(target_arch = "wasm32")]
            StoreAt::Browser => {
                let key = gz_bevy::view::web::WORKSPACES_BAD_KEY;
                gz_bevy::view::web::store(key, text)
                    .then(|| key.to_string())
                    .ok_or_else(|| "the browser didn't keep it".to_string())
            }
            #[cfg(not(target_arch = "wasm32"))]
            StoreAt::Browser => Ok(String::new()),
            StoreAt::Memory(_) => Ok("memory".into()),
        }
    }
}

/// `gz/ui/workspaces.json` in the platform's config directory
/// (`$XDG_CONFIG_HOME` or `~/.config` on Linux, `~/Library/Application
/// Support` on macOS, `%APPDATA%` on Windows).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn config_file() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(target_os = "windows") {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(dir.join("gz").join("ui").join(WORKSPACE_FILE))
}

/// A view camera's parts a workspace saves.
type CameraParts = (
    Entity,
    &'static ViewId,
    &'static ViewCamera,
    &'static Transform,
    &'static Projection,
    Option<&'static OverlaySet>,
);

/// What the workspaces read and write of the app.
#[derive(SystemParam)]
pub(crate) struct LayoutState<'w, 's> {
    pub(crate) layout: ResMut<'w, ShellLayout>,
    views: ResMut<'w, ViewLayout>,
    panels: Res<'w, PanelRegistry>,
    cameras: Query<'w, 's, CameraParts, With<MainCamera>>,
    beneath: ResMut<'w, Beneath>,
    editing: Option<Res<'w, EditingModel>>,
}

/// What the Workspaces panel stands in for while it's in a slot: the
/// panel it took the slot from (it moves with the panel from slot to
/// slot) and the sidebar's state before it came. A layout is read and
/// laid out under it, so managing the workspaces doesn't change them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Beneath {
    stand_in: Option<PanelId>,
    sidebar_open: bool,
    /// Its slot and the slots' panels, last look.
    was: (Option<Slot>, Option<PanelId>, Option<PanelId>),
}

impl Beneath {
    /// It never stands in for the panel its other slot shows (a swap, or a
    /// dock that moved that panel down).
    fn settle(&mut self, l: &ShellLayout) {
        let slot = workspaces_slot(l);
        let other = match slot {
            Some(Slot::Primary) => l.secondary,
            Some(Slot::Secondary) => l.primary,
            None => None,
        };
        if self.stand_in.is_some() && self.stand_in == other {
            self.stand_in = None;
        }
        self.was = (slot, l.primary, l.secondary);
    }
}

/// The slot the Workspaces panel is in, if it's in one.
fn workspaces_slot(l: &ShellLayout) -> Option<Slot> {
    let ws = Some(WORKSPACES_PANEL);
    if l.primary == ws {
        Some(Slot::Primary)
    } else if l.secondary == ws {
        Some(Slot::Secondary)
    } else {
        None
    }
}

/// Follows what the Workspaces panel stands in for: coming into a slot,
/// the panel that was there and the sidebar's state.
pub(crate) fn track_beneath(layout: Res<ShellLayout>, mut beneath: ResMut<Beneath>) {
    if !layout.is_changed() {
        return;
    }
    let mut next = *beneath;
    let (was, was_primary, was_secondary) = next.was;
    match (was, workspaces_slot(&layout)) {
        (_, None) => {
            next.stand_in = None;
            next.sidebar_open = layout.sidebar_open;
        }
        (None, Some(Slot::Primary)) => next.stand_in = was_primary,
        (None, Some(Slot::Secondary)) => next.stand_in = was_secondary,
        (Some(_), Some(_)) => {}
    }
    next.settle(&layout);
    beneath.set_if_neq(next);
}

/// The overlays on, by id.
fn overlay_ids(set: &OverlaySet) -> Vec<String> {
    Overlay::ALL
        .into_iter()
        .filter(|o| set.is_on(*o))
        .map(|o| o.id().to_string())
        .collect()
}

impl LayoutState<'_, '_> {
    /// The layout as it is now.
    pub(crate) fn capture(&self) -> WorkspaceLayout {
        let l = &*self.layout;
        let preset = self.views.preset;
        let mut cams: Vec<_> = self.cameras.iter().collect();
        cams.sort_by_key(|(_, id, ..)| id.0);
        let b = &*self.beneath;
        let slot = workspaces_slot(l);
        let (mut primary, mut secondary) = (l.primary, l.secondary);
        match slot {
            Some(Slot::Primary) => primary = b.stand_in,
            Some(Slot::Secondary) => secondary = b.stand_in,
            None => {}
        }
        // Standing in for nothing, it's as if closed: the second panel
        // moves up.
        if primary.is_none() {
            primary = secondary.take();
        }
        // Closed with it, the sidebar is closed; open, it's as it was.
        let sidebar_open = l.sidebar_open && (slot.is_none() || b.sidebar_open);
        WorkspaceLayout {
            sidebar_open,
            sidebar_w: l.sidebar_w,
            inspector_open: l.inspector_open,
            inspector_w: l.inspector_w,
            primary: primary.map(|p| p.name().to_string()),
            secondary: secondary.map(|p| p.name().to_string()),
            split: l.split,
            popouts: l
                .popouts
                .iter()
                .filter(|p| p.panel != WORKSPACES_PANEL)
                .map(|p| SavedPopOut {
                    panel: p.panel.name().to_string(),
                    from: match p.from {
                        Slot::Primary => "primary",
                        Slot::Secondary => "secondary",
                    }
                    .into(),
                    rect: [p.rect.min.x, p.rect.min.y, p.rect.width(), p.rect.height()],
                })
                .collect(),
            preset: preset.id().into(),
            shares: self.views.shares(),
            views: cams
                .into_iter()
                .filter(|(_, id, ..)| id.0 < preset.count())
                .map(|(e, _, v, t, p, set)| {
                    let pose = ViewPose::of(v, t, p);
                    // Without the joints and frames editing a model shows.
                    let own = |s: &OverlaySet| match self.editing.as_deref() {
                        Some(editing) => overlay_ids(&editing.own(e, s)),
                        None => overlay_ids(s),
                    };
                    SavedView {
                        pose: Some(pose.into()),
                        snap: pose.snapped.map(snap_id),
                        overlays: set.map(own),
                    }
                })
                .collect(),
        }
    }

    /// The panel registered as `name`, if one is.
    fn panel(&self, name: Option<&str>) -> Option<PanelId> {
        let name = name?;
        self.panels
            .iter()
            .find(|d| d.id.name() == name)
            .map(|d| d.id)
    }

    /// Lays the app out as `w` says; its views' cameras go to `pending`
    /// (a view the preset adds has no camera yet).
    pub(crate) fn apply(&mut self, w: &WorkspaceLayout, pending: &mut PendingViews) {
        let primary = self
            .panel(w.primary.as_deref())
            .or_else(|| self.panels.default_open());
        let secondary = self.panel(w.secondary.as_deref());
        let popouts: Vec<PopOut> = w
            .popouts
            .iter()
            .filter_map(|p| {
                Some(PopOut {
                    panel: self.panel(Some(&p.panel))?,
                    from: if p.from == "secondary" {
                        Slot::Secondary
                    } else {
                        Slot::Primary
                    },
                    rect: Rect::new(
                        p.rect[0],
                        p.rect[1],
                        p.rect[0] + p.rect[2],
                        p.rect[1] + p.rect[3],
                    ),
                })
            })
            .collect();
        let l = &mut *self.layout;
        let ws = Some(WORKSPACES_PANEL);
        // The Workspaces panel stays where it shows, over the layout (not
        // in a closed sidebar).
        let keep = workspaces_slot(l).filter(|_| l.sidebar_open);
        let kept = l
            .popouts
            .iter()
            .find(|p| p.panel == WORKSPACES_PANEL)
            .cloned();
        l.sidebar_open = w.sidebar_open && primary.is_some();
        l.sidebar_w = w.sidebar_w;
        l.inspector_open = w.inspector_open;
        l.inspector_w = w.inspector_w;
        l.primary = primary;
        l.secondary = secondary.filter(|s| Some(*s) != primary);
        l.split = w.split.clamp(0.1, 0.9);
        l.popouts = popouts;
        l.popouts.extend(kept);
        let beneath = &mut *self.beneath;
        match keep {
            Some(Slot::Primary) => {
                beneath.stand_in = l.primary;
                beneath.sidebar_open = l.sidebar_open;
                l.primary = ws;
                l.sidebar_open = true;
            }
            Some(Slot::Secondary) => {
                beneath.stand_in = l.secondary;
                beneath.sidebar_open = l.sidebar_open;
                l.secondary = ws;
                l.sidebar_open = true;
            }
            None => {}
        }
        beneath.settle(l);
        l.floor_closed = [false; 2];
        l.startup = false;
        l.enforce_floor(None);
        let preset = LayoutPreset::from_str(&w.preset).unwrap_or(LayoutPreset::Single);
        self.views.set_preset(preset);
        self.views.set_shares(w.shares);
        pending.0 = w
            .views
            .iter()
            .take(usize::from(preset.count()))
            .enumerate()
            .map(|(i, v)| (ViewId(i as u8), v.clone()))
            .collect();
    }
}

/// Save as new was asked for outside the Workspaces panel (⌘K): the
/// panel opens with its name field, then the resource is removed.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NameWorkspace;

/// The current layout as the Workspaces panel and ⌘K show it: what Save
/// would keep (the cameras' poses left out), whether it's modified from
/// the workspace as saved, what Reset would do, and each view's kind.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CurrentLayout {
    pub layout: Option<WorkspaceLayout>,
    pub modified: bool,
    /// What Reset puts back: the default (true) or the layout as saved
    /// (false); none when there's nothing to put back.
    pub reset: Option<bool>,
    /// Each view's kind: "Perspective", "Orthographic", or its axis view.
    pub views: Vec<String>,
}

/// Keeps [`CurrentLayout`] up to date.
pub(crate) fn current_layout(
    state: LayoutState,
    workspaces: Res<Workspaces>,
    mut out: ResMut<CurrentLayout>,
) {
    let mut now = state.capture();
    let ws = workspaces.current();
    let reset = ws.reset_to(&now).map(|to| {
        ws.builtin
            .is_some_and(|b| to == WorkspaceLayout::default_for(b))
    });
    let views = now
        .views
        .iter()
        .map(|v| match (&v.snap, v.pose.and_then(|p| p.ortho)) {
            (Some(snap), _) => {
                let mut c = snap.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
            (None, Some(_)) => "Orthographic".to_string(),
            (None, None) => "Perspective".to_string(),
        })
        .collect();
    for v in &mut now.views {
        v.pose = None;
    }
    let next = CurrentLayout {
        modified: !same_layout(&ws.saved, &now),
        layout: Some(now),
        reset,
        views,
    };
    out.set_if_neq(next);
}

/// Cameras to put where a workspace has them, as their views come.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PendingViews(pub(crate) Vec<(ViewId, SavedView)>);

/// Puts each pending view's camera in place once it exists. Before the
/// scene comes, its grid stays as saved when the scene's flag arrives.
pub(crate) fn apply_pending_views(
    mut pending: ResMut<PendingViews>,
    mut cameras: Query<(Entity, &ViewId, Option<&mut OverlaySet>), With<MainCamera>>,
    index: Option<Res<GzIndex>>,
    mut views: MessageWriter<ViewCommand>,
    mut editing: Option<ResMut<EditingModel>>,
    mut commands: Commands,
) {
    if pending.0.is_empty() {
        return;
    }
    let before_scene = index.is_none_or(|i| i.scene.is_none());
    pending.0.retain(|(id, saved)| {
        let Some((view, _, set)) = cameras.iter_mut().find(|(_, i, _)| *i == id) else {
            return true;
        };
        let snap = saved.snap.as_deref().and_then(snap_view);
        if let Some(pose) = saved.pose {
            views.write(ViewCommand {
                view,
                op: ViewOp::Restore(ViewPose {
                    snapped: snap,
                    ..pose.into()
                }),
            });
        } else if let Some(snap) = snap {
            views.write(ViewCommand {
                view,
                op: ViewOp::Snap(snap),
            });
        }
        if let (Some(ids), Some(mut set)) = (&saved.overlays, set) {
            if before_scene {
                commands.entity(view).insert(KeepGrid);
            }
            for o in Overlay::ALL {
                let on = ids.iter().any(|i| i == o.id());
                if set.is_on(o) != on {
                    set.set(o, on);
                }
            }
            // A model being edited shows its joints and frames here again.
            if let Some(editing) = editing.as_mut().filter(|e| e.model().is_some()) {
                editing.relight(view);
            }
        }
        false
    });
}

/// "top" → [`SnapView::Top`], and so on.
fn snap_view(name: &str) -> Option<SnapView> {
    SnapView::ALL
        .into_iter()
        .find(|s| snap_id(*s) == name.to_ascii_lowercase())
}

/// [`SnapView::Top`] → "top", and so on.
fn snap_id(snap: SnapView) -> String {
    format!("{snap:?}").to_ascii_lowercase()
}

/// What the switcher, the palette, the keys and the Workspaces panel ask
/// of the workspaces.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Switch to the workspace at this index.
    Switch(usize),
    /// Keep the layout as the current workspace's.
    Save,
    /// Keep the layout as a new workspace of the user's, by this name, and
    /// switch to it.
    SaveAsNew(String),
    /// Put the current workspace back: as saved while it's modified, else
    /// (a built-in one) its default.
    Reset,
    /// Delete the user's workspace at this index.
    Delete(usize),
}

/// The store has been read (once, at start).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspacesLoaded(bool);

/// What the store holds beyond the app's own state, and what it last
/// had written.
/// A UI scale as the file has it: a factor (`1.25`), or, written by hand,
/// a percentage (`125`, `"125%"`). Anything else is no scale, and the rest
/// of the file still reads.
fn lenient_scale<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(f32),
        Text(String),
        Other(serde::de::IgnoredAny),
    }
    let v: Option<f32> = match Option::<Raw>::deserialize(d)? {
        Some(Raw::Number(n)) => Some(n),
        Some(Raw::Text(t)) => t.trim().trim_end_matches('%').trim().parse().ok(),
        Some(Raw::Other(_)) | None => None,
    };
    // A factor is at most 2 (200%); more than a little past it is a
    // percentage.
    Ok(v.filter(|v| v.is_finite())
        .map(|v| if v > 2.5 { v / 100.0 } else { v }))
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Persisted {
    /// What was last written (or read), to write again only on a change.
    written: Option<WorkspaceFile>,
    /// The file's theme, kept as it is while a theme was asked for
    /// ([`WorkspaceStore::theme`] off); its UI scale and motion likewise.
    theme: Option<String>,
    ui_scale: Option<f32>,
    motion: Option<String>,
    /// It isn't written: it couldn't be read, or a newer gzweb wrote it.
    read_only: bool,
}

impl Persisted {
    /// The store isn't written this session.
    pub(crate) fn read_only(&self) -> bool {
        self.read_only
    }
}

impl Persisted {
    /// What the file holds for `workspaces` and `theme` (its mode, UI
    /// scale and motion).
    fn file_of(
        &self,
        workspaces: &Workspaces,
        theme: &Theme,
        store: &WorkspaceStore,
    ) -> WorkspaceFile {
        let list = workspaces.list();
        WorkspaceFile {
            version: FILE_VERSION,
            current: list[workspaces.current].id.clone(),
            theme: if store.theme {
                Some(theme.mode.id().into())
            } else {
                self.theme.clone()
            },
            ui_scale: if store.scale {
                Some(theme.scale)
            } else {
                self.ui_scale
            },
            motion: if store.motion {
                Some(theme.motion.id().into())
            } else {
                self.motion.clone()
            },
            workspaces: list
                .iter()
                .filter(|w| w.user_saved)
                .map(|w| (w.id.clone(), w.saved.clone()))
                .collect(),
            custom: list
                .iter()
                .filter(|w| w.builtin.is_none())
                .map(|w| CustomName {
                    id: w.id.clone(),
                    name: w.name.clone(),
                })
                .collect(),
        }
    }
}

/// The layouts as Export writes them: every workspace's as saved (the
/// built-in ones' defaults too) and the user's names, in the store's
/// format; no theme.
pub(crate) fn exported(workspaces: &Workspaces) -> String {
    let list = workspaces.list();
    let file = WorkspaceFile {
        version: FILE_VERSION,
        current: list[workspaces.current].id.clone(),
        theme: None,
        ui_scale: None,
        motion: None,
        workspaces: list
            .iter()
            .map(|w| (w.id.clone(), w.saved.clone()))
            .collect(),
        custom: list
            .iter()
            .filter(|w| w.builtin.is_none())
            .map(|w| CustomName {
                id: w.id.clone(),
                name: w.name.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Reads the store on the first frame: the saved workspaces, the
/// current one (laid out as saved, else its default; Build unsaved is the
/// app's start), the theme, the UI scale and the motion. Either way, it's written again only once
/// something in it changes.
///
/// A file it can't parse is kept apart (`.bad`) and the defaults used; if
/// it can't be kept apart, or can't be read at all, it's left as it is
/// and not written this session. A newer version's is read as far as it
/// can be and not written either.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_workspaces(
    store: Option<ResMut<WorkspaceStore>>,
    mut loaded: ResMut<WorkspacesLoaded>,
    mut workspaces: ResMut<Workspaces>,
    mut persisted: ResMut<Persisted>,
    mut theme: ResMut<Theme>,
    mut state: LayoutState,
    mut pending: ResMut<PendingViews>,
) {
    if loaded.0 {
        return;
    }
    loaded.0 = true;
    let Some(mut store) = store else {
        return;
    };
    let text = match store.read() {
        Ok(text) => text,
        Err(e) => {
            warn!("Couldn't read {WORKSPACE_FILE}, using defaults and leaving it as it is: {e}");
            persisted.read_only = true;
            return;
        }
    };
    let parsed = text.map(|t| serde_json::from_str::<WorkspaceFile>(&t).map_err(|e| (t, e)));
    let file = match parsed {
        Some(Ok(file)) => file,
        None => {
            persisted.written = Some(persisted.file_of(&workspaces, &theme, &store));
            return;
        }
        Some(Err((text, e))) => {
            match store.set_aside(&text) {
                Ok(at) => {
                    warn!(
                        "Couldn't read {WORKSPACE_FILE}, using defaults (it's kept as {at}): {e}"
                    );
                }
                Err(why) => {
                    warn!("Couldn't read {WORKSPACE_FILE}, using defaults and leaving it as it is: {e} (couldn't keep it apart: {why})");
                    persisted.read_only = true;
                }
            }
            return;
        }
    };
    if file.version > FILE_VERSION {
        warn!(
            "{WORKSPACE_FILE} is from a newer gzweb (version {}); reading what this one can, not writing it",
            file.version
        );
        persisted.read_only = true;
    }
    // The user's in their order, then any other the file holds (a newer
    // gzweb's), by id.
    let named = file
        .custom
        .iter()
        .map(|c| (c.id.as_str(), Some(c.name.as_str())));
    let rest = file.workspaces.keys().map(|id| (id.as_str(), None));
    for (id, name) in named.chain(rest) {
        let Some(w) = file.workspaces.get(id) else {
            continue;
        };
        match workspaces.index_of(id) {
            Some(i) if workspaces.list[i].builtin.is_some() => {
                let ws = &mut workspaces.list[i];
                ws.saved = w.clone();
                ws.user_saved = true;
            }
            Some(_) => {}
            None => {
                let name = workspaces.free_name(name.unwrap_or(id));
                workspaces
                    .list
                    .push(Workspace::custom(id.into(), name, w.clone()));
            }
        }
    }
    if let Some(i) = workspaces.index_of(&file.current) {
        workspaces.current = i;
        let ws = &workspaces.list[i];
        if i != 0 || ws.user_saved {
            let w = ws.saved.clone();
            state.apply(&w, &mut pending);
        }
    }
    persisted.theme = file.theme.clone();
    if store.theme {
        if let Some(mode) = file
            .theme
            .as_deref()
            .and_then(|t| ThemeMode::from_str(t).ok())
        {
            theme.set_mode(mode);
        }
    }
    persisted.ui_scale = file.ui_scale;
    if let Some(scale) = file.ui_scale.filter(|_| store.scale) {
        theme.set_scale(scale);
    }
    persisted.motion = file.motion.clone();
    if store.motion {
        if let Some(motion) = file
            .motion
            .as_deref()
            .and_then(|m| MotionMode::from_str(m).ok())
        {
            theme.set_motion(motion);
        }
    }
    persisted.written = Some(persisted.file_of(&workspaces, &theme, &store));
}

/// Runs the [`WorkspaceCommand`]s: a switch keeps the layout being left
/// and lays out the next as it was left, or as saved; Save keeps the
/// layout as the current workspace's, and Save as new as a new one's;
/// Reset puts it back; Delete drops one of the user's. Each says so.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_workspace_commands(
    mut commands: MessageReader<WorkspaceCommand>,
    time: Res<Time<Real>>,
    mut workspaces: ResMut<Workspaces>,
    mut state: LayoutState,
    mut pending: ResMut<PendingViews>,
    mut note: ResMut<StatusNote>,
) {
    let now = time.elapsed_secs_f64();
    for cmd in commands.read() {
        match cmd {
            WorkspaceCommand::Switch(i) => {
                switch(&mut workspaces, *i, &mut state, &mut pending);
            }
            WorkspaceCommand::Save => {
                let i = workspaces.current;
                let ws = &mut workspaces.list[i];
                ws.saved = state.capture();
                ws.user_saved = true;
                ws.left = None;
                note.show(format!("Saved the {} layout", ws.name), now);
            }
            WorkspaceCommand::SaveAsNew(name) => {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                let name = workspaces.free_name(name);
                let id = workspaces.free_id();
                let layout = state.capture();
                // The one left keeps what it had changed, for the session.
                let leaving = workspaces.current;
                workspaces.list[leaving].left = Some(layout.clone());
                workspaces
                    .list
                    .push(Workspace::custom(id, name.clone(), layout));
                workspaces.current = workspaces.list.len() - 1;
                note.show(format!("Saved the {name} layout"), now);
            }
            WorkspaceCommand::Reset => {
                let i = workspaces.current;
                let Some(to) = workspaces.list[i].reset_to(&state.capture()) else {
                    continue;
                };
                let ws = &mut workspaces.list[i];
                let default = ws
                    .builtin
                    .is_some_and(|b| to == WorkspaceLayout::default_for(b));
                let saved = ws.user_saved;
                // The default is kept as its layout, so it's there next
                // start too (Build would start as the app does).
                if default {
                    ws.saved = to.clone();
                    ws.user_saved = true;
                }
                ws.left = None;
                state.apply(&to, &mut pending);
                let name = ws.name.clone();
                note.show(
                    match (default, saved) {
                        (true, _) => format!("Reset {name} to its default"),
                        (false, true) => format!("Reset {name} to its saved layout"),
                        (false, false) => format!("Reset {name} to how it started"),
                    },
                    now,
                );
            }
            WorkspaceCommand::Delete(i) => {
                let i = *i;
                if workspaces.list.get(i).is_none_or(|w| w.builtin.is_some()) {
                    continue;
                }
                if workspaces.current == i {
                    switch(&mut workspaces, 0, &mut state, &mut pending);
                }
                let gone = workspaces.list.remove(i);
                if workspaces.current > i {
                    workspaces.current -= 1;
                }
                note.show(format!("Deleted the {} workspace", gone.name), now);
            }
        }
    }
}

/// Build unsaved starts as the app always did, so once start-up ends that
/// layout is Build's as saved (its cameras and overlays free) until it's
/// saved or reset:
/// a first start isn't "modified", and Reset gives §8's default.
pub(crate) fn adopt_start_layout(
    state: LayoutState,
    mut workspaces: ResMut<Workspaces>,
    mut done: Local<bool>,
) {
    if *done || state.layout.startup {
        return;
    }
    *done = true;
    let build = &workspaces.list[0];
    if workspaces.current != 0 || build.user_saved || build.left.is_some() {
        return;
    }
    // The cameras and overlays as they are: the scene's grid flag may not
    // have come yet.
    let mut now = state.capture();
    for v in &mut now.views {
        v.pose = None;
        v.overlays = None;
    }
    workspaces.list[0].saved = now;
}

/// Switches to workspace `i`: keeps the layout being left, and lays out
/// `i` as it was left, else as saved. The current one again does nothing.
fn switch(
    workspaces: &mut Workspaces,
    i: usize,
    state: &mut LayoutState,
    pending: &mut PendingViews,
) {
    if i >= workspaces.list.len() || i == workspaces.current {
        return;
    }
    let leaving = workspaces.current;
    workspaces.list[leaving].left = Some(state.capture());
    let next = workspaces.list[i]
        .left
        .take()
        .unwrap_or_else(|| workspaces.list[i].saved.clone());
    state.apply(&next, pending);
    workspaces.current = i;
}

/// Writes the store when what it holds changes: the saved workspaces, the
/// current one, the theme (its mode, UI scale and motion).
pub(crate) fn persist_workspaces(
    store: Option<ResMut<WorkspaceStore>>,
    loaded: Res<WorkspacesLoaded>,
    workspaces: Res<Workspaces>,
    theme: Res<Theme>,
    mut persisted: ResMut<Persisted>,
) {
    let Some(mut store) = store else {
        return;
    };
    if !loaded.0 || persisted.read_only || !(workspaces.is_changed() || theme.is_changed()) {
        return;
    }
    let file = persisted.file_of(&workspaces, &theme, &store);
    if persisted.written.as_ref() == Some(&file) {
        return;
    }
    let text = serde_json::to_string_pretty(&file).unwrap_or_default();
    match store.write(&text) {
        Ok(()) => persisted.written = Some(file),
        Err(e) => warn!("Couldn't write {WORKSPACE_FILE}: {e}"),
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_message::<WorkspaceCommand>()
        .init_resource::<Workspaces>()
        .init_resource::<WorkspacesLoaded>()
        .init_resource::<Persisted>()
        .init_resource::<PendingViews>()
        .init_resource::<Beneath>()
        .init_resource::<CurrentLayout>()
        // Before Update, so the start flags (`--layout`, `--pop-out`, a
        // panel to open) go over the layout it reads.
        .add_systems(PreUpdate, load_workspaces)
        .add_systems(
            Update,
            (
                track_beneath,
                adopt_start_layout,
                run_workspace_commands,
                apply_pending_views,
                persist_workspaces,
                current_layout,
            )
                .chain()
                .in_set(crate::shell::ShellSystems::Layout)
                .after(crate::shell::commands::apply_ui_commands)
                .before(crate::shell::regions::apply_layout),
        );
}

#[cfg(test)]
mod tests;
