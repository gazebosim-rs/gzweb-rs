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
//! to `localStorage` in the browser, with the current workspace and the
//! theme, which are written whenever they change. The app starts in the
//! workspace it was left in: as saved, or as the app starts when never
//! saved (so start-up stays as it was).

use std::str::FromStr;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gz_bevy::camera::MainCamera;
use gz_bevy::overlays::{Overlay, OverlaySet};
use gz_bevy::view::{SnapView, ViewCamera, ViewCommand, ViewId, ViewOp, ViewPose};
use serde::{Deserialize, Serialize};

use crate::panels::{PanelId, PanelRegistry};
use crate::shell::layout::{PopOut, ShellLayout, Slot, DEFAULT_SPLIT};
use crate::shell::status_bar::readouts::StatusNote;
use crate::shell::viewport::views::{LayoutPreset, ViewLayout};
use crate::theme::{Sizes, Theme, ThemeMode};

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
    /// "top", "front", … (§8's Build: a top ortho view).
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

/// The workspaces: the current one, each one's layout as saved (its
/// default until it is), and as left this session (switching away).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Workspaces {
    pub current: usize,
    saved: [WorkspaceLayout; 4],
    /// Saved by the user (so written to the file), not a default.
    user_saved: [bool; 4],
    left: [Option<WorkspaceLayout>; 4],
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            current: 0,
            saved: std::array::from_fn(WorkspaceLayout::default_for),
            user_saved: [false; 4],
            left: Default::default(),
        }
    }
}

impl Workspaces {
    /// The current workspace's name.
    pub fn name(&self) -> &'static str {
        WORKSPACES[self.current].1
    }

    /// Workspace `index`'s layout as saved.
    pub fn saved(&self, index: usize) -> &WorkspaceLayout {
        &self.saved[index]
    }
}

/// What the file holds.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorkspaceFile {
    pub(crate) version: u32,
    pub(crate) current: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) theme: Option<String>,
    /// The workspaces saved, by id.
    #[serde(default)]
    pub(crate) workspaces: std::collections::BTreeMap<String, WorkspaceLayout>,
}

/// Where the workspaces persist; none (tests, captures) keeps them for
/// the session.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct WorkspaceStore {
    pub at: StoreAt,
    /// The saved theme applies at start (not over `gzweb --theme`).
    pub theme: bool,
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
        at.map(|at| Self { at, theme: true })
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

    fn write(&mut self, text: &str) -> Result<(), String> {
        match &mut self.at {
            StoreAt::File(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(path, text).map_err(|e| e.to_string())
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
        WorkspaceLayout {
            sidebar_open: l.sidebar_open,
            sidebar_w: l.sidebar_w,
            inspector_open: l.inspector_open,
            inspector_w: l.inspector_w,
            primary: l.primary.map(|p| p.name().to_string()),
            secondary: l.secondary.map(|p| p.name().to_string()),
            split: l.split,
            popouts: l
                .popouts
                .iter()
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
                .map(|(_, _, v, t, p, set)| SavedView {
                    pose: Some(ViewPose::of(v, t, p).into()),
                    snap: None,
                    overlays: set.map(overlay_ids),
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
        l.sidebar_open = w.sidebar_open && primary.is_some();
        l.sidebar_w = w.sidebar_w;
        l.inspector_open = w.inspector_open;
        l.inspector_w = w.inspector_w;
        l.primary = primary;
        l.secondary = secondary.filter(|s| Some(*s) != primary);
        l.split = w.split.clamp(0.1, 0.9);
        l.popouts = popouts;
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

/// Cameras to put where a workspace has them, as their views come.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PendingViews(pub(crate) Vec<(ViewId, SavedView)>);

/// Puts each pending view's camera in place once it exists.
pub(crate) fn apply_pending_views(
    mut pending: ResMut<PendingViews>,
    mut cameras: Query<(Entity, &ViewId, Option<&mut OverlaySet>), With<MainCamera>>,
    mut views: MessageWriter<ViewCommand>,
) {
    if pending.0.is_empty() {
        return;
    }
    pending.0.retain(|(id, saved)| {
        let Some((view, _, set)) = cameras.iter_mut().find(|(_, i, _)| *i == id) else {
            return true;
        };
        if let Some(pose) = saved.pose {
            views.write(ViewCommand {
                view,
                op: ViewOp::Restore(pose.into()),
            });
        } else if let Some(snap) = saved.snap.as_deref().and_then(snap_view) {
            views.write(ViewCommand {
                view,
                op: ViewOp::Snap(snap),
            });
        }
        if let (Some(ids), Some(mut set)) = (&saved.overlays, set) {
            for o in Overlay::ALL {
                let on = ids.iter().any(|i| i == o.id());
                if set.is_on(o) != on {
                    set.set(o, on);
                }
            }
        }
        false
    });
}

/// "top" → [`SnapView::Top`], and so on.
fn snap_view(name: &str) -> Option<SnapView> {
    SnapView::ALL
        .into_iter()
        .find(|s| format!("{s:?}").eq_ignore_ascii_case(name))
}

/// Switch to a workspace, or save the current one's layout.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Switch(usize),
    Save,
}

/// The store has been read (once, at start).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspacesLoaded(bool);

/// What the file last had written, to write again only on a change.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Written(Option<WorkspaceFile>);

/// What the file holds for `workspaces` and `theme`.
fn file_of(workspaces: &Workspaces, theme: &Theme) -> WorkspaceFile {
    WorkspaceFile {
        version: 1,
        current: WORKSPACES[workspaces.current].0.into(),
        theme: Some(theme.mode.id().into()),
        workspaces: WORKSPACES
            .iter()
            .enumerate()
            .filter(|(i, _)| workspaces.user_saved[*i])
            .map(|(i, (id, ..))| (id.to_string(), workspaces.saved[i].clone()))
            .collect(),
    }
}

/// Reads the store once the shell is up: the saved workspaces, the current
/// one (laid out as saved, if it was) and the theme. A file it can't read
/// leaves the defaults, saying so. Either way, it's written again only
/// once something in it changes.
pub(crate) fn load_workspaces(
    store: Option<Res<WorkspaceStore>>,
    mut loaded: ResMut<WorkspacesLoaded>,
    mut workspaces: ResMut<Workspaces>,
    mut written: ResMut<Written>,
    mut theme: ResMut<Theme>,
    mut state: LayoutState,
    mut pending: ResMut<PendingViews>,
) {
    if loaded.0 {
        return;
    }
    loaded.0 = true;
    let Some(store) = store else {
        return;
    };
    let read = store.read().and_then(|text| {
        text.map(|t| serde_json::from_str::<WorkspaceFile>(&t).map_err(|e| e.to_string()))
            .transpose()
    });
    let file = match read {
        Ok(Some(file)) => file,
        Ok(None) => {
            written.0 = Some(file_of(&workspaces, &theme));
            return;
        }
        Err(e) => {
            warn!("Couldn't read {WORKSPACE_FILE}, using defaults: {e}");
            written.0 = Some(file_of(&workspaces, &theme));
            return;
        }
    };
    for (i, (id, ..)) in WORKSPACES.iter().enumerate() {
        if let Some(w) = file.workspaces.get(*id) {
            workspaces.saved[i] = w.clone();
            workspaces.user_saved[i] = true;
        }
    }
    if let Some(i) = WORKSPACES.iter().position(|(id, ..)| *id == file.current) {
        workspaces.current = i;
        if workspaces.user_saved[i] {
            let w = workspaces.saved[i].clone();
            state.apply(&w, &mut pending);
        }
    }
    if store.theme {
        if let Some(mode) = file
            .theme
            .as_deref()
            .and_then(|t| ThemeMode::from_str(t).ok())
        {
            theme.set_mode(mode);
        }
    }
    written.0 = Some(file_of(&workspaces, &theme));
}

/// Runs the [`WorkspaceCommand`]s: a switch keeps the layout being left
/// and lays out the next as it was left, or as saved; Save keeps the
/// layout as the current workspace's, saying so.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_workspace_commands(
    mut commands: MessageReader<WorkspaceCommand>,
    time: Res<Time<Real>>,
    mut workspaces: ResMut<Workspaces>,
    mut state: LayoutState,
    mut pending: ResMut<PendingViews>,
    mut note: ResMut<StatusNote>,
) {
    for cmd in commands.read() {
        match *cmd {
            WorkspaceCommand::Switch(i) if i < WORKSPACES.len() && i != workspaces.current => {
                let leaving = workspaces.current;
                workspaces.left[leaving] = Some(state.capture());
                let next = workspaces.left[i]
                    .take()
                    .unwrap_or_else(|| workspaces.saved[i].clone());
                state.apply(&next, &mut pending);
                workspaces.current = i;
            }
            WorkspaceCommand::Switch(_) => {}
            WorkspaceCommand::Save => {
                let i = workspaces.current;
                workspaces.saved[i] = state.capture();
                workspaces.user_saved[i] = true;
                workspaces.left[i] = None;
                note.show(
                    format!("Saved the {} layout", WORKSPACES[i].1),
                    time.elapsed_secs_f64(),
                );
            }
        }
    }
}

/// Writes the store when what it holds changes: the saved workspaces, the
/// current one, the theme.
pub(crate) fn persist_workspaces(
    store: Option<ResMut<WorkspaceStore>>,
    loaded: Res<WorkspacesLoaded>,
    workspaces: Res<Workspaces>,
    theme: Res<Theme>,
    mut written: ResMut<Written>,
) {
    let Some(mut store) = store else {
        return;
    };
    if !loaded.0 || !(workspaces.is_changed() || theme.is_changed()) {
        return;
    }
    let file = file_of(&workspaces, &theme);
    if written.0.as_ref() == Some(&file) {
        return;
    }
    let text = serde_json::to_string_pretty(&file).unwrap_or_default();
    match store.write(&text) {
        Ok(()) => written.0 = Some(file),
        Err(e) => warn!("Couldn't write {WORKSPACE_FILE}: {e}"),
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_message::<WorkspaceCommand>()
        .init_resource::<Workspaces>()
        .init_resource::<WorkspacesLoaded>()
        .init_resource::<Written>()
        .init_resource::<PendingViews>()
        .add_systems(
            Update,
            (
                load_workspaces,
                run_workspace_commands,
                apply_pending_views,
                persist_workspaces,
            )
                .chain()
                .in_set(crate::shell::ShellSystems::Layout)
                .after(crate::shell::commands::apply_ui_commands)
                .before(crate::shell::regions::apply_layout),
        );
}

#[cfg(test)]
mod tests;
