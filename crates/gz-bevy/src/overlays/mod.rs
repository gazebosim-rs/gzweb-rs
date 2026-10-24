//! The viewport's overlays (spec §7.3, §11 "VisualizationCapabilities"):
//! grid, collisions, inertia, centre of mass, joints, frames and
//! wireframe, as the Gazebo GUI draws them (DECISIONS.md D-62).
//!
//! Each view has its own set ([`OverlaySet`] on its camera; spec §3.4,
//! DECISIONS.md D-93); [`OverlayCommand`] changes a view's, or every
//! view's, from the viewport keys (G, C, I, J; [`crate::view::controls`])
//! and the full UI's Overlays menu alike. The [`Overlays`] resource is
//! what any view shows, which is what gets drawn; each overlay draws on a
//! render layer of its own ([`Overlay::layer`]), which a view's camera
//! sees while the overlay is on in it ([`view_layers`]). The scene's own
//! `grid` flag ([`GzGrid`]) turns every view's grid on or off. The state
//! lives with the views ([`crate::view::ViewerControls`] registers it);
//! the drawing is [`OverlayRenderPlugin`], added with a renderer.
//!
//! - **Frames** (models and links) and **joints**, **inertia** and the
//!   **centre of mass** are gizmo lines, drawn over the scene
//!   ([`draw`]).
//! - **Collisions** are meshes in Gazebo's translucent orange, children of
//!   their links ([`collisions`]).
//! - Collisions, inertials and joints aren't in the scene message: they
//!   come from the world's SDFormat, asked for the first time one of them
//!   is turned on ([`extras`]).
//! - **Wireframe** draws every visual's triangle edges over it: line
//!   meshes built from the triangles, on the wireframe's layer
//!   ([`wireframe`]).

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

    /// The render layer it draws on: a view sees it while it's on there.
    pub fn layer(self) -> usize {
        OVERLAY_LAYER + Overlay::ALL.iter().position(|o| *o == self).unwrap_or(0)
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

/// The first overlay's render layer ([`Overlay::layer`]).
pub const OVERLAY_LAYER: usize = 40;

/// The overlays that are on: a view's ([`OverlaySet`]), or what any view
/// shows (the resource).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overlays {
    /// Starts as the scene's [`GzGrid`] says.
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

    /// Turns `o` on or off.
    pub fn set(&mut self, o: Overlay, on: bool) {
        *self.flag(o) = on;
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

    /// What's on in either.
    pub fn or(self, other: Overlays) -> Overlays {
        let mut out = self;
        for o in Overlay::ALL {
            if other.is_on(o) {
                *out.flag(o) = true;
            }
        }
        out
    }

    /// The render layers of the overlays that are on.
    pub fn layers(&self) -> impl Iterator<Item = usize> + '_ {
        Overlay::ALL
            .into_iter()
            .filter(|o| self.is_on(*o))
            .map(Overlay::layer)
    }
}

/// A view's own overlays, on its camera.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Deref, DerefMut)]
pub struct OverlaySet(pub Overlays);

/// Turns an overlay on (`Some(true)`), off, or over (`None`), in one view
/// (its camera) or in every view (`None`: over is from what any shows).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayCommand {
    pub overlay: Overlay,
    pub on: Option<bool>,
    pub view: Option<Entity>,
}

impl OverlayCommand {
    /// Over in every view.
    pub fn toggle(overlay: Overlay) -> Self {
        Self {
            overlay,
            on: None,
            view: None,
        }
    }

    /// On or off in every view.
    pub fn set(overlay: Overlay, on: bool) -> Self {
        Self {
            overlay,
            on: Some(on),
            view: None,
        }
    }

    /// The same, in the view on camera `view` only.
    pub fn in_view(self, view: Entity) -> Self {
        Self {
            view: Some(view),
            ..self
        }
    }
}

/// On a view whose grid a workspace set before the scene came: the
/// scene's `grid` flag leaves that view's grid, once.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeepGrid;

/// The views' own overlay sets.
pub type ViewOverlays<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut OverlaySet,
        &'static Camera,
        Has<KeepGrid>,
    ),
    With<crate::view::ViewCamera>,
>;

/// Applies [`OverlayCommand`]s to the views' sets, turns every view's grid
/// on or off when the scene's [`GzGrid`] changes (but a [`KeepGrid`]
/// view's, once), and keeps the
/// [`Overlays`] resource what any showing view has on. Without view sets
/// (no views to keep them on) the commands change the resource itself.
pub fn apply_overlay_commands(
    mut commands: MessageReader<OverlayCommand>,
    mut views: ViewOverlays,
    mut union: ResMut<Overlays>,
    grid: Res<GzGrid>,
    mut cmds: Commands,
) {
    if views.is_empty() {
        for c in commands.read() {
            let now = union.is_on(c.overlay);
            let on = c.on.unwrap_or(!now);
            if on != now {
                *union.flag(c.overlay) = on;
            }
        }
        if grid.is_changed() && union.grid != grid.visible {
            union.grid = grid.visible;
        }
        return;
    }
    if grid.is_changed() {
        for (e, mut set, _, keep) in &mut views {
            if keep {
                // The resource's start isn't the scene's flag.
                if !grid.is_added() {
                    cmds.entity(e).remove::<KeepGrid>();
                }
                continue;
            }
            if set.grid != grid.visible {
                set.grid = grid.visible;
            }
        }
    }
    for c in commands.read() {
        let all_on = union.is_on(c.overlay);
        for (e, mut set, ..) in &mut views {
            if c.view.is_some_and(|v| v != e) {
                continue;
            }
            let now = set.is_on(c.overlay);
            let on =
                c.on.unwrap_or(if c.view.is_some() { !now } else { !all_on });
            if on != now {
                *set.flag(c.overlay) = on;
            }
        }
    }
    let any = views
        .iter()
        .filter(|(_, _, camera, _)| camera.is_active)
        .fold(Overlays::default(), |acc, (_, set, ..)| acc.or(set.0));
    union.set_if_neq(any);
}

/// A view camera's layers: the scene's, its own, and its overlays'.
#[allow(clippy::type_complexity)]
pub fn view_layers(
    mut cameras: Query<
        (
            &crate::view::ViewId,
            &OverlaySet,
            &mut bevy::camera::visibility::RenderLayers,
        ),
        Or<(Changed<crate::view::ViewId>, Changed<OverlaySet>)>,
    >,
) {
    for (id, set, mut layers) in &mut cameras {
        let want = set.layers().fold(id.layers(), |l, n| l.with(n));
        layers.set_if_neq(want);
    }
}

/// The overlay gizmo groups draw on their overlays' layers (a renderer's
/// app with views only: without views the one camera sees the scene's).
pub fn overlay_gizmo_layers(store: Option<ResMut<GizmoConfigStore>>) {
    fn on<T: GizmoConfigGroup>(store: &mut GizmoConfigStore, o: Overlay) {
        if let Some((config, _)) = store.get_config_mut::<T>() {
            config.render_layers = bevy::camera::visibility::RenderLayers::layer(o.layer());
        }
    }
    let Some(mut store) = store else { return };
    on::<crate::render::GridGizmos>(&mut store, Overlay::Grid);
    on::<draw::FrameGizmos>(&mut store, Overlay::Frames);
    on::<draw::JointGizmos>(&mut store, Overlay::Joints);
    on::<draw::InertiaGizmos>(&mut store, Overlay::Inertia);
    on::<draw::ComGizmos>(&mut store, Overlay::CenterOfMass);
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
            .init_gizmo_group::<draw::FrameGizmos>()
            .init_gizmo_group::<draw::JointGizmos>()
            .init_gizmo_group::<draw::InertiaGizmos>()
            .init_gizmo_group::<draw::ComGizmos>()
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
            // Mesh files instance after Update: layered before the views
            // check what they see, so they never show in every view.
            .add_systems(
                PostUpdate,
                collisions::layer_collisions
                    .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
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
