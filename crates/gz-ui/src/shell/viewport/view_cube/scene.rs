//! The view cube's own little scene: the cube's 54 face patches and 12
//! edges, a shadow, and the orthographic camera that renders them into
//! the cube node's texture. All on [`CUBE_LAYER`] at [`CUBE_ORIGIN`], far
//! from the world, unlit and casting no shadows, and passed through by the
//! views' picks ([`NoViewPick`]).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::ui::ComputedNode;
use bevy::window::PrimaryWindow;
use gz_bevy::spawn::gz_to_bevy;
use gz_bevy::view::{view_texture, NoViewPick};

use super::pick::{self, CubeRegion, Shade, VIEW_HALF};
use super::systems::CubeHover;
use super::ViewCube;
use crate::shell::viewport::MainView;
use crate::theme::{ColorRole, Theme};

/// The render layer only the cube camera sees.
pub(crate) const CUBE_LAYER: usize = 30;
/// Where the cube sits, far below the world.
pub(crate) const CUBE_ORIGIN: Vec3 = Vec3::new(0.0, -10_000.0, 0.0);
/// The edges' thickness, in cube units (about a pixel at 56 px).
const EDGE: f32 = 0.05;
/// The camera's distance from the cube.
const CAMERA_DISTANCE: f32 = 6.0;

/// The cube's texture, its camera and its shadow.
#[derive(Resource, Debug, Clone)]
pub(crate) struct CubeScene {
    pub image: Handle<Image>,
    pub camera: Entity,
    pub shadow: Entity,
}

/// One face patch: its region, its face, its material and the role it
/// shows.
#[derive(Component, Debug, Clone)]
pub(crate) struct CubePatch {
    pub region: CubeRegion,
    pub face: IVec3,
    pub role: Option<ColorRole>,
}

/// A part with one role: the edges (`cube_edge`) and the shadow.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CubeInk {
    pub role: ColorRole,
    pub alpha: Option<f32>,
}

/// A material per part, so each can change on its own.
fn material(materials: &mut Assets<StandardMaterial>, alpha: bool) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        unlit: true,
        alpha_mode: if alpha {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
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
fn part(mesh: Handle<Mesh>, material: Handle<StandardMaterial>, parent: Entity) -> impl Bundle {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material),
        RenderLayers::layer(CUBE_LAYER),
        NotShadowCaster,
        NotShadowReceiver,
        NoViewPick,
        ChildOf(parent),
    )
}

/// Spawns the cube, its edges and shadow, and its camera rendering into a
/// 1 × 1 texture until the node is laid out.
pub(crate) fn spawn_cube_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
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
            let m = material(&mut materials, false);
            commands.spawn((
                part(meshes.add(quad(corners)), m, root),
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
    let edge_material = material(&mut materials, false);
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
                ),
                Transform::from_translation(centre),
                CubeInk {
                    role: ColorRole::CubeEdge,
                    alpha: None,
                },
            ));
        }
    }
    // The drop shadow (`elevation.cube`: 1 px down): the cube's silhouette
    // in `shadow`, a pixel lower, behind it.
    let shadow = commands
        .spawn((
            part(
                meshes.add(Cuboid::from_size(Vec3::splat(2.0 + EDGE))),
                material(&mut materials, true),
                root,
            ),
            Transform::default(),
            CubeInk {
                role: ColorRole::Shadow,
                alpha: None,
            },
        ))
        .id();
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
            Projection::Orthographic(OrthographicProjection {
                near: 0.1,
                far: 2.0 * CAMERA_DISTANCE,
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 2.0 * VIEW_HALF,
                },
                ..OrthographicProjection::default_3d()
            }),
            // The tokens' colours, as they are.
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Sample4,
            RenderLayers::layer(CUBE_LAYER),
            Transform::from_translation(CUBE_ORIGIN + Vec3::Z * CAMERA_DISTANCE),
        ))
        .id();
    commands.insert_resource(CubeScene {
        image,
        camera,
        shadow,
    });
}

/// Sizes the cube's texture to its node's physical pixels (after UI
/// layout, so a scale change shows at once) and shows it in the node.
pub(crate) fn size_cube_texture(
    scene: Option<Res<CubeScene>>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<(Entity, &ComputedNode, Option<&mut ImageNode>), With<ViewCube>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut targets: Query<&mut RenderTarget>,
    mut commands: Commands,
) {
    let Some(scene) = scene else { return };
    let Ok((e, node, image_node)) = nodes.single_mut() else {
        return;
    };
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
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    if let Ok(mut target) = targets.get_mut(scene.camera) {
        let stale = !matches!(&*target, RenderTarget::Image(t) if t.scale_factor == scale);
        if stale {
            *target = RenderTarget::Image(ImageRenderTarget {
                handle: scene.image.clone(),
                scale_factor: scale,
            });
        }
    }
    if image_node.is_none() {
        commands
            .entity(e)
            .insert(ImageNode::new(scene.image.clone()));
    }
}

/// Turns the cube camera as the view's camera is turned, and keeps the
/// shadow a pixel below the cube on screen.
pub(crate) fn turn_cube(
    scene: Option<Res<CubeScene>>,
    view: MainView,
    nodes: Query<&ComputedNode, With<ViewCube>>,
    mut transforms: Query<&mut Transform, Without<gz_bevy::camera::MainCamera>>,
) {
    let (Some(scene), Some((_, _, t, _))) = (scene, view.get()) else {
        return;
    };
    let rotation = t.rotation;
    if let Ok(mut camera) = transforms.get_mut(scene.camera) {
        let want = Transform::from_translation(CUBE_ORIGIN + rotation * Vec3::Z * CAMERA_DISTANCE)
            .with_rotation(rotation);
        if *camera != want {
            *camera = want;
        }
    }
    // A pixel, in cube units, down the screen and a little back.
    let px = nodes
        .single()
        .map_or(1.0, |n| n.size().y.max(1.0) * n.inverse_scale_factor());
    let offset = rotation * Vec3::new(0.0, -2.0 * VIEW_HALF / px, -0.1);
    if let Ok(mut shadow) = transforms.get_mut(scene.shadow) {
        let want = Transform::from_translation(gz_to_bevy().inverse() * offset);
        if *shadow != want {
            *shadow = want;
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

/// Colours the patches (when the view turns, the hover or the snap
/// changes) and the edges and shadow (with the theme).
pub(crate) fn paint_cube(
    view: MainView,
    hover: Res<CubeHover>,
    theme: Res<Theme>,
    mut patches: Query<(&mut CubePatch, &MeshMaterial3d<StandardMaterial>)>,
    inks: Query<(&CubeInk, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut painted_theme: Local<Option<bool>>,
) {
    let Some((_, v, t, _)) = view.get() else {
        return;
    };
    let rot = pick::gz_rotation(t.rotation);
    let snapped = v.snapped.map(CubeRegion::of_snap);
    let new_theme = *painted_theme != Some(theme.is_dark) || theme.is_changed();
    for (mut patch, m) in &mut patches {
        let role = patch_role(&patch, rot, hover.0, snapped);
        if patch.role != Some(role) || new_theme {
            patch.role = Some(role);
            if let Some(mut mat) = materials.get_mut(&m.0) {
                mat.base_color = theme.color(role);
            }
        }
    }
    if new_theme {
        for (ink, m) in &inks {
            if let Some(mut mat) = materials.get_mut(&m.0) {
                let c = theme.color(ink.role);
                mat.base_color = ink.alpha.map_or(c, |a| c.with_alpha(a));
            }
        }
        *painted_theme = Some(theme.is_dark);
    }
}
