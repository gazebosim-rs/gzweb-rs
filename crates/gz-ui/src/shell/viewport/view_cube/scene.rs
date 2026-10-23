//! The view cubes' own little scenes, one per view: the cube's 54 face
//! patches and 12 edges, and the perspective camera that renders them into
//! the cube node's texture. Each on its view's layer from [`CUBE_LAYER`]
//! at [`CUBE_ORIGIN`], far from the world, unlit and casting no shadows,
//! and passed through by the views' picks ([`NoViewPick`]).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::ui::ComputedNode;
use bevy::window::PrimaryWindow;
use gz_bevy::spawn::gz_to_bevy;
use gz_bevy::view::{view_texture, NoViewPick};

use super::pick::{self, CubeRegion, Shade, CAMERA_DISTANCE, EDGE};
use super::systems::CubeHover;
use super::{CubeImage, CubeShadow, ViewCube};
use crate::shell::viewport::ViewCameras;
use crate::theme::{elevation, ColorRole, Theme};

/// The first view's cube layer: only its cube camera sees it (view `n`'s
/// is `n` more).
pub(crate) const CUBE_LAYER: usize = 30;
/// Where the cubes sit, far below the world.
pub(crate) const CUBE_ORIGIN: Vec3 = Vec3::new(0.0, -10_000.0, 0.0);

/// A cube node's scene: its texture, camera and parts' root. Despawned
/// with the node.
#[derive(Component, Debug, Clone)]
pub(crate) struct CubeScene {
    pub image: Handle<Image>,
    pub camera: Entity,
    pub root: Entity,
}

/// One face patch: its region, its face, its material and the role it
/// shows.
#[derive(Component, Debug, Clone)]
pub(crate) struct CubePatch {
    pub region: CubeRegion,
    pub face: IVec3,
    pub role: Option<ColorRole>,
}

/// A part with one role: the edges (`cube_edge`).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CubeInk {
    pub role: ColorRole,
}

/// A material per part, so each can change on its own.
fn material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        unlit: true,
        ..default()
    })
}

/// A quad from four counter-clockwise corners.
fn quad(c: [Vec3; 4]) -> Mesh {
    let n = (c[1] - c[0]).cross(c[2] - c[0]).normalize_or_zero();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, c.to_vec())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n; 4])
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// The cube's parts' common components.
fn part(
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    parent: Entity,
    layer: usize,
) -> impl Bundle {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material),
        RenderLayers::layer(layer),
        NotShadowCaster,
        NotShadowReceiver,
        NoViewPick,
        ChildOf(parent),
    )
}

/// Gives each new cube node its scene: the cube, its edges, and its
/// camera rendering into a 1 × 1 texture until the node is laid out.
pub(crate) fn spawn_cube_scenes(
    cubes: Query<Entity, Added<ViewCube>>,
    views: ViewCameras,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    images: Option<ResMut<Assets<Image>>>,
    mut commands: Commands,
) {
    if cubes.is_empty() {
        return;
    }
    let (Some(mut meshes), Some(mut materials), Some(mut images)) = (meshes, materials, images)
    else {
        return;
    };
    for cube in &cubes {
        let view = views.id_of(cube).map_or(0, |id| usize::from(id.0));
        let scene = spawn_cube_scene(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            CUBE_LAYER + view,
        );
        commands.entity(cube).insert(scene);
    }
}

/// A cube node's scene goes with it.
pub(crate) fn on_cube_removed(
    ev: On<Remove<CubeScene>>,
    scenes: Query<&CubeScene>,
    mut commands: Commands,
) {
    if let Ok(scene) = scenes.get(ev.entity) {
        for e in [scene.root, scene.camera] {
            if let Ok(mut e) = commands.get_entity(e) {
                e.despawn();
            }
        }
    }
}

/// The cube, its edges and its camera on `layer`.
fn spawn_cube_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    layer: usize,
) -> CubeScene {
    let image = images.add(view_texture(UVec2::ONE));
    // Gazebo axes inside.
    let root = commands
        .spawn((
            Name::new("view cube scene"),
            Transform::from_translation(CUBE_ORIGIN).with_rotation(gz_to_bevy()),
            Visibility::default(),
        ))
        .id();
    for n in CubeRegion::FACES {
        for (region, corners) in pick::face_patches(n) {
            let m = material(materials);
            commands.spawn((
                part(meshes.add(quad(corners)), m, root, layer),
                CubePatch {
                    region,
                    face: n,
                    role: None,
                },
            ));
        }
    }
    // The twelve edges: along each axis, at each pair of signs of the
    // other two.
    let edge_material = material(materials);
    for axis in 0..3 {
        for (sa, sb) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut centre = Vec3::ZERO;
            centre[a] = sa;
            centre[b] = sb;
            let mut size = Vec3::splat(EDGE);
            size[axis] = 2.0 + EDGE;
            commands.spawn((
                part(
                    meshes.add(Cuboid::from_size(size)),
                    edge_material.clone(),
                    root,
                    layer,
                ),
                Transform::from_translation(centre),
                CubeInk {
                    role: ColorRole::CubeEdge,
                },
            ));
        }
    }
    let camera = commands
        .spawn((
            Name::new("view cube camera"),
            Camera3d::default(),
            Camera {
                // Before the views and the UI, which shows its texture.
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: image.clone(),
                scale_factor: 1.0,
            }),
            Projection::Perspective(PerspectiveProjection {
                fov: 2.0 * pick::tan_half_fov().atan(),
                aspect_ratio: 1.0,
                near: 0.1,
                far: 2.0 * CAMERA_DISTANCE,
                ..default()
            }),
            // The tokens' colours, as they are.
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Sample4,
            RenderLayers::layer(layer),
            Transform::from_translation(CUBE_ORIGIN + Vec3::Z * CAMERA_DISTANCE),
        ))
        .id();
    CubeScene {
        image,
        camera,
        root,
    }
}

/// Sizes each cube's texture to its node's physical pixels (after UI
/// layout, so a scale change shows at once).
pub(crate) fn size_cube_texture(
    images: Option<ResMut<Assets<Image>>>,
    nodes: Query<(&ComputedNode, &CubeScene), With<ViewCube>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut targets: Query<&mut RenderTarget>,
) {
    let Some(mut images) = images else { return };
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    for (node, scene) in &nodes {
        let size = node.size().round().as_uvec2().max(UVec2::ONE);
        if images.get(&scene.image).is_some_and(|i| i.size() != size) {
            if let Some(mut image) = images.get_mut(&scene.image) {
                image.resize(bevy::render::render_resource::Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                });
            }
        }
        if let Ok(mut target) = targets.get_mut(scene.camera) {
            let stale = !matches!(&*target, RenderTarget::Image(t) if t.scale_factor == scale);
            if stale {
                *target = RenderTarget::Image(ImageRenderTarget {
                    handle: scene.image.clone(),
                    scale_factor: scale,
                });
            }
        }
    }
}

/// The cube node's image and its drop shadow.
type CubeImages = Or<(With<CubeImage>, With<CubeShadow>)>;

/// Shows the cube's texture in the node's image and, tinted, in its drop
/// shadow: the texture's colour times the shadow's (near black), its
/// alpha times the shadow's alpha.
#[allow(clippy::type_complexity)]
pub(crate) fn show_cube_texture(
    scenes: Query<&CubeScene>,
    theme: Res<Theme>,
    mut shown: Query<(Entity, &ChildOf, Option<&mut ImageNode>, Has<CubeShadow>), CubeImages>,
    mut commands: Commands,
) {
    let tint = theme.color(elevation::CUBE.color.unwrap_or(ColorRole::Shadow));
    // An image node's own colour leaves the texture as it is.
    let plain = ImageNode::default().color;
    for (e, cube, image_node, shadow) in &mut shown {
        let Ok(scene) = scenes.get(cube.parent()) else {
            continue;
        };
        let color = if shadow { tint } else { plain };
        match image_node {
            Some(mut i) => {
                if i.color != color {
                    i.color = color;
                }
            }
            None => {
                commands
                    .entity(e)
                    .insert(ImageNode::new(scene.image.clone()).with_color(color));
            }
        }
    }
}

/// Turns each cube camera as its view's camera is turned.
pub(crate) fn turn_cube(
    cubes: Query<(Entity, &CubeScene)>,
    views: ViewCameras,
    mut transforms: Query<&mut Transform, Without<gz_bevy::camera::MainCamera>>,
) {
    for (cube, scene) in &cubes {
        let Some((_, _, t, _)) = views.of(cube) else {
            continue;
        };
        let rotation = t.rotation;
        if let Ok(mut camera) = transforms.get_mut(scene.camera) {
            let want =
                Transform::from_translation(CUBE_ORIGIN + rotation * Vec3::Z * CAMERA_DISTANCE)
                    .with_rotation(rotation);
            if *camera != want {
                *camera = want;
            }
        }
    }
}

/// The role a patch shows: the hovered region in `accent_ring`, the
/// snapped face in `cube_snapped`, else its shade.
pub(crate) fn patch_role(
    patch: &CubePatch,
    rot: Quat,
    hover: Option<CubeRegion>,
    snapped: Option<CubeRegion>,
) -> ColorRole {
    if hover == Some(patch.region) {
        ColorRole::AccentRing
    } else if snapped.is_some_and(|s| s.0 == patch.face) {
        ColorRole::CubeSnapped
    } else {
        match pick::shade(rot, patch.face.as_vec3()) {
            Shade::Top => ColorRole::CubeTop,
            Shade::Right => ColorRole::CubeRight,
            Shade::Left => ColorRole::CubeLeft,
        }
    }
}

/// Colours each cube's patches (when its view turns, the hover or the
/// snap changes) and edges (new, or with the theme).
#[allow(clippy::type_complexity)]
pub(crate) fn paint_cube(
    cubes: Query<(Entity, &CubeScene, &CubeHover)>,
    views: ViewCameras,
    theme: Res<Theme>,
    mut patches: Query<(&ChildOf, &mut CubePatch, &MeshMaterial3d<StandardMaterial>)>,
    inks: Query<(&CubeInk, &MeshMaterial3d<StandardMaterial>, Ref<CubeInk>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut painted_theme: Local<Option<bool>>,
) {
    let new_theme = *painted_theme != Some(theme.is_dark) || theme.is_changed();
    for (cube, scene, hover) in &cubes {
        let Some((_, v, t, _)) = views.of(cube) else {
            continue;
        };
        let rot = pick::gz_rotation(t.rotation);
        let snapped = v.snapped.map(CubeRegion::of_snap);
        for (parent, mut patch, m) in &mut patches {
            if parent.parent() != scene.root {
                continue;
            }
            let role = patch_role(&patch, rot, hover.0, snapped);
            if patch.role != Some(role) || new_theme {
                patch.role = Some(role);
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.base_color = theme.color(role);
                }
            }
        }
    }
    for (ink, m, added) in &inks {
        if new_theme || added.is_added() {
            if let Some(mut mat) = materials.get_mut(&m.0) {
                mat.base_color = theme.color(ink.role);
            }
        }
    }
    *painted_theme = Some(theme.is_dark);
}
