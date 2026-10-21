//! The viewport's overlays (spec §7.3, §11 "VisualizationCapabilities"):
//! grid, collisions, inertia, centre of mass, joints, frames and
//! wireframe, as the Gazebo GUI draws them (DECISIONS.md D-62).
//!
//! [`Overlays`] is what is on; [`OverlayCommand`] changes it, from the
//! viewport keys (G, C, I, J; [`crate::view::controls`]) and the full UI's
//! Overlays menu alike. The grid stays [`GzGrid`], which the scene's own
//! `grid` flag sets; [`Overlays::grid`] mirrors it. The state lives with
//! the views ([`crate::view::ViewerControls`] registers it); the drawing is
//! [`OverlayRenderPlugin`], added with a renderer.
//!
//! - **Frames** (models and links) and **joints**, **inertia** and the
//!   **centre of mass** are gizmo lines, drawn over the scene
//!   ([`draw`]).
//! - **Collisions** are meshes in Gazebo's translucent orange, children of
//!   their links ([`collisions`]).
//! - Collisions, inertials and joints aren't in the scene message: they
//!   come from the world's SDFormat, asked for the first time one of them
//!   is turned on ([`extras`]).
//! - **Wireframe** draws every visual's triangle edges over it: Bevy's
//!   `WireframePlugin` where the GPU has line polygons, else (the browser)
//!   line meshes built from the triangles ([`wireframe`]).
//!
//! In M3 there is one view, so the overlays are one set; split views (M6)
//! make them per view.

pub mod collisions;
pub mod draw;
pub mod extras;
pub mod wireframe;

use bevy::prelude::*;

use crate::render::GzGrid;

/// An overlay the viewport can show (spec §7.3; lidar and sensor data
/// come with the Sensors panel, M7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overlay {
    Grid,
    Collisions,
    Inertia,
    CenterOfMass,
    Joints,
    Frames,
    Wireframe,
}

impl Overlay {
    /// The Overlays menu's order (spec §7.3).
    pub const ALL: [Overlay; 7] = [
        Overlay::Grid,
        Overlay::Collisions,
        Overlay::Inertia,
        Overlay::CenterOfMass,
        Overlay::Joints,
        Overlay::Frames,
        Overlay::Wireframe,
    ];

    /// Its name in the Overlays menu.
    pub fn label(self) -> &'static str {
        match self {
            Overlay::Grid => "Grid",
            Overlay::Collisions => "Collisions",
            Overlay::Inertia => "Inertia",
            Overlay::CenterOfMass => "Center of mass",
            Overlay::Joints => "Joints",
            Overlay::Frames => "Frames",
            Overlay::Wireframe => "Wireframe",
        }
    }

    /// Its `shortcuts.json` binding, if it has one.
    pub fn binding(self) -> Option<&'static str> {
        match self {
            Overlay::Grid => Some("overlay.grid"),
            Overlay::Collisions => Some("overlay.collisions"),
            Overlay::Inertia => Some("overlay.inertia"),
            Overlay::Joints => Some("overlay.joints"),
            Overlay::CenterOfMass | Overlay::Frames | Overlay::Wireframe => None,
        }
    }

    /// Its name on the command line and in captures (`center_of_mass`).
    pub fn id(self) -> &'static str {
        match self {
            Overlay::Grid => "grid",
            Overlay::Collisions => "collisions",
            Overlay::Inertia => "inertia",
            Overlay::CenterOfMass => "center_of_mass",
            Overlay::Joints => "joints",
            Overlay::Frames => "frames",
            Overlay::Wireframe => "wireframe",
        }
    }

    /// Drawn from the world's SDFormat ([`extras`]).
    pub fn needs_sdf(self) -> bool {
        matches!(
            self,
            Overlay::Collisions | Overlay::Inertia | Overlay::CenterOfMass | Overlay::Joints
        )
    }
}

impl std::str::FromStr for Overlay {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Overlay::ALL
            .into_iter()
            .find(|o| o.id() == s || (s == "com" && *o == Overlay::CenterOfMass))
            .ok_or_else(|| {
                let names: Vec<&str> = Overlay::ALL.iter().map(|o| o.id()).collect();
                format!("unknown overlay '{s}' ({})", names.join(", "))
            })
    }
}

/// The overlays that are on.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overlays {
    /// Mirrors [`GzGrid::visible`].
    pub grid: bool,
    pub collisions: bool,
    pub inertia: bool,
    pub center_of_mass: bool,
    pub joints: bool,
    pub frames: bool,
    pub wireframe: bool,
}

impl Overlays {
    pub fn is_on(&self, o: Overlay) -> bool {
        match o {
            Overlay::Grid => self.grid,
            Overlay::Collisions => self.collisions,
            Overlay::Inertia => self.inertia,
            Overlay::CenterOfMass => self.center_of_mass,
            Overlay::Joints => self.joints,
            Overlay::Frames => self.frames,
            Overlay::Wireframe => self.wireframe,
        }
    }

    fn flag(&mut self, o: Overlay) -> &mut bool {
        match o {
            Overlay::Grid => &mut self.grid,
            Overlay::Collisions => &mut self.collisions,
            Overlay::Inertia => &mut self.inertia,
            Overlay::CenterOfMass => &mut self.center_of_mass,
            Overlay::Joints => &mut self.joints,
            Overlay::Frames => &mut self.frames,
            Overlay::Wireframe => &mut self.wireframe,
        }
    }

    /// How many are on (the rail's Overlays badge, spec §3.4).
    pub fn count(&self) -> usize {
        Overlay::ALL.iter().filter(|o| self.is_on(**o)).count()
    }

    /// One of the overlays drawn from the world's SDFormat is on.
    pub fn need_sdf(&self) -> bool {
        Overlay::ALL.iter().any(|o| o.needs_sdf() && self.is_on(*o))
    }
}

/// Turns an overlay on (`Some(true)`), off, or over (`None`).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayCommand {
    pub overlay: Overlay,
    pub on: Option<bool>,
}

impl OverlayCommand {
    pub fn toggle(overlay: Overlay) -> Self {
        Self { overlay, on: None }
    }

    pub fn set(overlay: Overlay, on: bool) -> Self {
        Self {
            overlay,
            on: Some(on),
        }
    }
}

/// Applies [`OverlayCommand`]s (the grid's to [`GzGrid`]) and keeps
/// [`Overlays::grid`] in step with the grid the scene asked for.
pub fn apply_overlay_commands(
    mut commands: MessageReader<OverlayCommand>,
    mut overlays: ResMut<Overlays>,
    mut grid: ResMut<GzGrid>,
) {
    for c in commands.read() {
        if c.overlay == Overlay::Grid {
            let on = c.on.unwrap_or(!grid.visible);
            grid.set_if_neq(GzGrid { visible: on });
        } else {
            let now = overlays.is_on(c.overlay);
            let on = c.on.unwrap_or(!now);
            if on != now {
                *overlays.flag(c.overlay) = on;
            }
        }
    }
    if overlays.grid != grid.visible {
        overlays.grid = grid.visible;
    }
}

/// Draws the overlays: gizmos, collision meshes, wireframes, and the
/// SDFormat request they need. Added with a renderer, when the app has
/// views; [`apply_overlay_commands`] is the views' (`ViewerControls`).
pub struct OverlayRenderPlugin;

impl Plugin for OverlayRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Overlays>()
            .init_resource::<extras::SdfExtras>()
            .init_resource::<extras::SdfWanted>()
            .init_resource::<wireframe::WireframeLines>()
            .init_gizmo_group::<draw::OverlayGizmos>()
            .add_plugins(bevy::pbr::wireframe::WireframePlugin::default())
            .add_systems(Startup, draw::setup_overlay_gizmos)
            .add_systems(
                Update,
                (
                    extras::receive_extras,
                    extras::request_extras,
                    collisions::spawn_collisions,
                    collisions::show_collisions,
                )
                    .chain()
                    .after(crate::spawn::apply_scene),
            )
            .add_systems(
                Update,
                (wireframe::apply_wireframe, wireframe::build_lines).chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    draw::draw_frames,
                    draw::draw_joints,
                    draw::draw_inertia,
                    draw::draw_center_of_mass,
                )
                    .after(TransformSystems::Propagate),
            );
    }
}

#[cfg(test)]
mod tests;
