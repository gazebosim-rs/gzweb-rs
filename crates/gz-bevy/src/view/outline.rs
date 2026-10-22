//! The selection's and the hovered entity's silhouettes, for the full UI's
//! outline (spec §6.4, §9.3): each of their meshes gets a clone on its own
//! render layer in a flat colour (red for the selection, green for the
//! hover), and in each view a mask camera that follows the view's renders
//! only those layers into the view's [`OutlineMask`] texture. The UI draws
//! the outline around the mask.
//!
//! The clones' layers have no lights, so the mask camera adds no shadow
//! cascades (Bevy 0.20 skips directional lights whose layers miss a
//! view's), and it is inactive while nothing is outlined.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget, RenderTargetInfo};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::controls::ViewPlacementSystems;
use super::{view_texture, Selection, ViewPlacement};
use crate::camera::MainCamera;
use crate::overlays::collisions::CollisionOverlay;
use crate::overlays::wireframe::WireLines;

/// The selection's clones.
pub const SELECTED_LAYER: usize = 28;
/// The hovered entity's clones.
pub const HOVERED_LAYER: usize = 29;

/// The entity the pointer is over in the full UI's Scene tree, which every
/// view outlines faintly (spec §6.4).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hovered(pub Option<Entity>);

/// Whether anything is outlined (in every view).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outlining(pub bool);

/// A view's mask: its texture, whether anything is outlined, and where on
/// the view (logical pixels; `None`: anywhere), so the UI draws the
/// outline over that much of the view only. On the view's camera.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct OutlineMask {
    pub image: Option<Handle<Image>>,
    pub active: bool,
    pub rect: Option<Rect>,
}

/// Which outline a clone draws into.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutlineClone {
    Selected,
    Hovered,
}

impl OutlineClone {
    fn layer(self) -> usize {
        match self {
            OutlineClone::Selected => SELECTED_LAYER,
            OutlineClone::Hovered => HOVERED_LAYER,
        }
    }

    /// The mask's channel: red or green.
    fn colour(self) -> Color {
        match self {
            OutlineClone::Selected => Color::linear_rgb(1.0, 0.0, 0.0),
            OutlineClone::Hovered => Color::linear_rgb(0.0, 1.0, 0.0),
        }
    }
}

/// A view's mask camera (a child of its camera, so it has its pose).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct MaskCamera;

/// Meshes that aren't outlined: the collision and wireframe overlays', and
/// the clones.
type NotOutlined = Or<(With<CollisionOverlay>, With<WireLines>, With<OutlineClone>)>;
/// A mesh of the scene's own.
type OwnMesh = (With<Mesh3d>, Without<OutlineClone>);

/// The clones' two flat materials.
#[derive(Resource)]
struct CloneMaterials {
    selected: Handle<StandardMaterial>,
    hovered: Handle<StandardMaterial>,
}

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hovered>()
            .init_resource::<Outlining>()
            .add_systems(Update, sync_clones)
            .add_systems(
                PostUpdate,
                (
                    (spawn_mask_camera, place_mask)
                        .chain()
                        .in_set(ViewPlacementSystems)
                        .after(super::apply_view_placement),
                    outline_rect
                        .in_set(OutlineRectSystems)
                        .before(bevy::ui::UiSystems::Layout),
                ),
            );
    }
}

/// The meshes under `e` that are its own (not the collision or wireframe
/// overlays', nor other clones).
fn meshes_under(
    e: Entity,
    children: &Query<&Children>,
    meshes: &Query<(), OwnMesh>,
    skip: &Query<(), NotOutlined>,
    out: &mut Vec<Entity>,
) {
    if skip.contains(e) {
        return;
    }
    if meshes.contains(e) {
        out.push(e);
    }
    for c in children.get(e).into_iter().flatten() {
        meshes_under(*c, children, meshes, skip, out);
    }
}

/// Keeps one clone per outlined mesh: the selection's meshes, then the
/// hovered entity's that aren't selected. Runs when either changes or a
/// mesh spawns (a model's glTF arriving after it was selected).
#[allow(clippy::too_many_arguments)]
fn sync_clones(
    selection: Res<Selection>,
    hovered: Res<Hovered>,
    added: Query<(), (Added<Mesh3d>, Without<OutlineClone>)>,
    children: Query<&Children>,
    meshes: Query<(), OwnMesh>,
    skip: Query<(), NotOutlined>,
    sources: Query<&Mesh3d, Without<OutlineClone>>,
    clones: Query<(Entity, &OutlineClone, &ChildOf)>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut held: Local<Option<CloneMaterials>>,
    mut outlining: ResMut<Outlining>,
    mut commands: Commands,
) {
    if !selection.is_changed() && !hovered.is_changed() && added.is_empty() {
        return;
    }
    let Some(mut materials) = materials else {
        return;
    };
    let held = held.get_or_insert_with(|| {
        let flat = |kind: OutlineClone| StandardMaterial {
            base_color: kind.colour(),
            unlit: true,
            ..default()
        };
        CloneMaterials {
            selected: materials.add(flat(OutlineClone::Selected)),
            hovered: materials.add(flat(OutlineClone::Hovered)),
        }
    });
    // Each mesh once: a selected model and its own selected links share
    // meshes, and the selection wins over the hover.
    let mut want: HashMap<Entity, OutlineClone> = HashMap::new();
    let mut found = Vec::new();
    for e in &selection.entities {
        meshes_under(*e, &children, &meshes, &skip, &mut found);
    }
    want.extend(found.drain(..).map(|m| (m, OutlineClone::Selected)));
    if let Some(h) = hovered.0 {
        meshes_under(h, &children, &meshes, &skip, &mut found);
        for m in found.drain(..) {
            want.entry(m).or_insert(OutlineClone::Hovered);
        }
    }
    let active = !want.is_empty();
    for (clone, kind, parent) in &clones {
        if want.get(&parent.parent()) == Some(kind) {
            want.remove(&parent.parent());
        } else {
            commands.entity(clone).despawn();
        }
    }
    for (mesh, kind) in want {
        let Ok(source) = sources.get(mesh) else {
            continue;
        };
        let material = match kind {
            OutlineClone::Selected => held.selected.clone(),
            OutlineClone::Hovered => held.hovered.clone(),
        };
        commands.spawn((
            Name::new("outline mask"),
            kind,
            Mesh3d(source.0.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::Inherited,
            RenderLayers::layer(kind.layer()),
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(mesh),
        ));
    }
    if outlining.0 != active {
        outlining.0 = active;
    }
}

/// Gives each placed view its mask and mask camera (again after the
/// environment respawns the view's camera).
#[allow(clippy::type_complexity)]
fn spawn_mask_camera(
    views: Query<(Entity, Option<&Children>, Has<OutlineMask>), With<ViewPlacement>>,
    masks: Query<(), With<MaskCamera>>,
    mut commands: Commands,
) {
    for (view, children, masked) in &views {
        if !masked {
            commands.entity(view).insert(OutlineMask::default());
        }
        let has = children.is_some_and(|c| c.iter().any(|e| masks.contains(e)));
        if !has {
            commands.spawn((
                Name::new("outline mask camera"),
                MaskCamera,
                Camera3d::default(),
                Camera {
                    // Before the views, which are 0 and up.
                    order: -2,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    is_active: false,
                    ..default()
                },
                Msaa::Off,
                Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                RenderLayers::from_layers(&[SELECTED_LAYER, HOVERED_LAYER]),
                Transform::IDENTITY,
                ChildOf(view),
            ));
        }
    }
}

/// Renders each view's mask at the view's size and projection, into a
/// texture of its own, while anything is outlined and the view shows.
#[allow(clippy::type_complexity)]
fn place_mask(
    outlining: Res<Outlining>,
    images: Option<ResMut<Assets<Image>>>,
    mut views: Query<
        (&Projection, &ViewPlacement, &mut OutlineMask),
        (With<MainCamera>, Without<MaskCamera>),
    >,
    mut masks: Query<(&mut Camera, &mut Projection, &mut RenderTarget, &ChildOf), With<MaskCamera>>,
) {
    let Some(mut images) = images else {
        return;
    };
    for (mut camera, mut projection, mut target, parent) in &mut masks {
        let Ok((view, placement, mut mask)) = views.get_mut(parent.parent()) else {
            continue;
        };
        let size = placement.physical_size();
        let active = outlining.0 && size.is_some();
        if mask.active != active {
            mask.active = active;
        }
        if camera.is_active != active {
            camera.is_active = active;
        }
        let Some(size) = size else {
            continue;
        };
        let scale = placement.scale.max(f32::EPSILON);
        let handle = match mask.image.clone() {
            Some(h) => {
                if images.get(&h).is_some_and(|i| i.size() != size) {
                    if let Some(mut image) = images.get_mut(&h) {
                        image.resize(bevy::render::render_resource::Extent3d {
                            width: size.x,
                            height: size.y,
                            depth_or_array_layers: 1,
                        });
                    }
                }
                h
            }
            None => {
                let h = images.add(view_texture(size));
                mask.image = Some(h.clone());
                h
            }
        };
        let placed = matches!(&*target, RenderTarget::Image(t) if t.handle == handle && t.scale_factor == scale);
        if !placed {
            *target = RenderTarget::Image(ImageRenderTarget {
                handle: handle.clone(),
                scale_factor: scale,
            });
        }
        let logical = size.as_vec2() / scale;
        let mut want = view.clone();
        want.update(logical.x, logical.y);
        let clip = want.get_clip_from_view();
        if camera.computed.clip_from_view != clip {
            *projection = want;
            camera.computed.clip_from_view = clip;
        }
        let info = RenderTargetInfo {
            physical_size: size,
            scale_factor: scale,
        };
        let stale = camera
            .computed
            .target_info
            .as_ref()
            .is_none_or(|t| t.physical_size != size || t.scale_factor != scale);
        if stale {
            camera.computed.target_info = Some(info);
        }
    }
}

/// Where [`outline_rect`] writes [`OutlineMask::rect`]: the UI sizes its
/// overlay after it, before layout.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineRectSystems;

/// The screen rectangle the outlined meshes cover in each view (their
/// bounds as last placed, through the camera as it is now), or `None`
/// while one has no bounds yet.
pub fn outline_rect(
    outlining: Res<Outlining>,
    mut views: Query<(&Transform, &Projection, &ViewPlacement, &mut OutlineMask), With<MainCamera>>,
    clones: Query<(Option<&Aabb>, &GlobalTransform), With<OutlineClone>>,
) {
    for (t, p, placement, mut mask) in &mut views {
        let want = (|| {
            if !outlining.0 {
                return None;
            }
            let size = placement.rect?.size();
            let mut out: Option<Rect> = None;
            for (aabb, global) in &clones {
                let aabb = aabb?;
                let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
                let corners = std::array::from_fn(|i| {
                    let s = Vec3::new(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    global.transform_point(c + h * s)
                });
                if let Some(r) = super::select::screen_rect((t, p), size, corners) {
                    out = Some(out.map_or(r, |o| o.union(r)));
                }
            }
            out
        })();
        if mask.rect != want {
            mask.rect = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::MainCamera;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_resource::<Selection>()
            .configure_sets(PostUpdate, ViewPlacementSystems)
            .add_systems(PostUpdate, super::super::apply_view_placement)
            .add_plugins(OutlinePlugin);
        app
    }

    /// A model with a link holding two meshes, one inside a collision
    /// overlay: (model, link mesh, overlay mesh).
    fn model(app: &mut App) -> (Entity, Entity, Entity) {
        let world = app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let model = world
            .spawn((Transform::default(), Visibility::default()))
            .id();
        let link = world.spawn((Transform::default(), ChildOf(model))).id();
        let shown = world.spawn((Mesh3d(mesh.clone()), ChildOf(link))).id();
        let overlay = world.spawn((CollisionOverlay, ChildOf(link))).id();
        let hidden = world.spawn((Mesh3d(mesh), ChildOf(overlay))).id();
        (model, shown, hidden)
    }

    fn clones(app: &mut App) -> Vec<(Entity, OutlineClone, usize)> {
        let world = app.world_mut();
        let mut out: Vec<_> = world
            .query::<(&ChildOf, &OutlineClone, &RenderLayers)>()
            .iter(world)
            .map(|(p, k, l)| (p.parent(), *k, l.iter().next().unwrap()))
            .collect();
        out.sort_by_key(|c| (c.2, c.0.index()));
        out
    }

    #[test]
    fn the_selection_and_the_hover_get_mask_clones() {
        let mut app = app();
        let (a, a_mesh, _) = model(&mut app);
        let (b, b_mesh, _) = model(&mut app);
        app.update();
        assert!(clones(&mut app).is_empty());
        assert!(!app.world().resource::<Outlining>().0);
        // The model's own mesh, not its collision overlay's.
        app.world_mut().resource_mut::<Selection>().entities = vec![a];
        app.update();
        assert_eq!(
            clones(&mut app),
            [(a_mesh, OutlineClone::Selected, SELECTED_LAYER)]
        );
        assert!(app.world().resource::<Outlining>().0);
        // A hovered model that is selected keeps its selection clone.
        app.world_mut().resource_mut::<Hovered>().0 = Some(a);
        app.update();
        assert_eq!(clones(&mut app).len(), 1);
        app.world_mut().resource_mut::<Hovered>().0 = Some(b);
        app.update();
        assert_eq!(
            clones(&mut app),
            [
                (a_mesh, OutlineClone::Selected, SELECTED_LAYER),
                (b_mesh, OutlineClone::Hovered, HOVERED_LAYER),
            ]
        );
        // Selecting it turns the hover clone into a selection clone.
        app.world_mut().resource_mut::<Selection>().entities = vec![b];
        app.update();
        assert_eq!(
            clones(&mut app),
            [(b_mesh, OutlineClone::Selected, SELECTED_LAYER)]
        );
        app.world_mut().resource_mut::<Selection>().entities.clear();
        app.world_mut().resource_mut::<Hovered>().0 = None;
        app.update();
        assert!(clones(&mut app).is_empty());
        assert!(!app.world().resource::<Outlining>().0);
    }

    #[test]
    fn a_mesh_that_arrives_after_the_selection_is_outlined_too() {
        let mut app = app();
        let (a, _, _) = model(&mut app);
        app.world_mut().resource_mut::<Selection>().entities = vec![a];
        app.update();
        let world = app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Sphere::default());
        let late = world.spawn((Mesh3d(mesh), ChildOf(a))).id();
        app.update();
        assert!(clones(&mut app).iter().any(|c| c.0 == late));
    }

    #[test]
    fn the_mask_camera_renders_at_the_views_size_and_projection() {
        let mut app = app();
        let view = app
            .world_mut()
            .spawn((
                MainCamera,
                Camera3d::default(),
                Projection::Orthographic(OrthographicProjection::default_3d()),
                ViewPlacement {
                    rect: Some(Rect::new(0.0, 0.0, 400.0, 300.0)),
                    scale: 2.0,
                    image: None,
                },
            ))
            .id();
        app.update();
        app.update();
        let world = app.world_mut();
        let (camera, projection, target, parent) = world
            .query_filtered::<(&Camera, &Projection, &RenderTarget, &ChildOf), With<MaskCamera>>()
            .single(world)
            .unwrap();
        assert_eq!(parent.parent(), view);
        assert!(!camera.is_active, "nothing is outlined");
        assert!(matches!(projection, Projection::Orthographic(_)));
        let info = camera.computed.target_info.as_ref().unwrap();
        assert_eq!(info.physical_size, UVec2::new(800, 600));
        let mask = world
            .get::<OutlineMask>(view)
            .unwrap()
            .image
            .clone()
            .unwrap();
        assert!(
            matches!(target, RenderTarget::Image(t) if t.handle == mask && t.scale_factor == 2.0)
        );
        let size = world.resource::<Assets<Image>>().get(&mask).unwrap().size();
        assert_eq!(size, UVec2::new(800, 600));
    }

    #[test]
    fn a_selection_without_meshes_outlines_nothing() {
        let mut app = app();
        // A light: an entity with no meshes under it.
        let light = app.world_mut().spawn(Transform::default()).id();
        app.world_mut().resource_mut::<Selection>().entities = vec![light];
        app.update();
        assert!(!app.world().resource::<Outlining>().0);
        // A model and its own link selected: one clone per mesh.
        let (model, mesh, _) = model(&mut app);
        let link = app.world().get::<ChildOf>(mesh).unwrap().parent();
        app.world_mut().resource_mut::<Selection>().entities = vec![model, link];
        app.update();
        assert_eq!(clones(&mut app).len(), 1);
    }

    #[test]
    fn the_wireframe_overlay_leaves_the_mask_clones_alone() {
        use crate::overlays::wireframe::{apply_wireframe, Wired};
        use crate::overlays::Overlays;
        use crate::spawn::GzWorldRoot;
        let mut app = app();
        app.init_resource::<Overlays>()
            .add_systems(Update, apply_wireframe.after(sync_clones));
        let (model, mesh, _) = model(&mut app);
        let root = app.world_mut().spawn(GzWorldRoot).id();
        app.world_mut().entity_mut(model).insert(ChildOf(root));
        app.world_mut().resource_mut::<Selection>().entities = vec![model];
        app.update();
        app.world_mut().resource_mut::<Overlays>().wireframe = true;
        app.update();
        app.update();
        assert!(app.world().get::<Wired>(mesh).is_some(), "the model's mesh");
        let world = app.world_mut();
        let wired_clones = world
            .query_filtered::<(), (With<OutlineClone>, With<Wired>)>()
            .iter(world)
            .count();
        assert_eq!(wired_clones, 0);
    }

    #[test]
    fn the_outline_rect_covers_the_outlined_meshes_on_screen() {
        let mut app = app();
        app.add_systems(
            PostUpdate,
            bevy::camera::visibility::calculate_bounds.before(OutlineRectSystems),
        );
        let view = app
            .world_mut()
            .spawn((
                MainCamera,
                Camera3d::default(),
                Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                Projection::Perspective(PerspectiveProjection::default()),
                ViewPlacement {
                    rect: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
                    scale: 1.0,
                    image: None,
                },
            ))
            .id();
        let (model, ..) = model(&mut app);
        app.world_mut().resource_mut::<Selection>().entities = vec![model];
        for _ in 0..3 {
            app.update();
        }
        let rect = |app: &App| app.world().get::<OutlineMask>(view).unwrap().rect;
        let r = rect(&app).expect("bounds");
        // A unit cube at the origin, 10 m away: a small square mid-view.
        assert!(r.contains(Vec2::new(400.0, 300.0)), "{r:?}");
        assert!(r.width() < 200.0 && r.width() > 10.0, "{r:?}");
        app.world_mut().resource_mut::<Selection>().entities.clear();
        app.update();
        assert_eq!(rect(&app), None);
    }
}
