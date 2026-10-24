//! Overlay tests: the state and its commands, the SDFormat request and
//! reply, the collision meshes, the gizmo maths and drawing, and the
//! wireframe fallback.

use super::collisions::{collision_visual, CollisionOverlay, COLLISION_RGBA};
use super::draw::{com_radius, equivalent_box, symmetric_eigen, FrameGizmos};
use super::extras::SdfExtras;
use super::wireframe::{edge_lines, WireLines, Wired};
use super::*;
use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::material::GzMaterials;
use crate::spawn::{GzEntity, GzIndex, GzLoads, GzWorldRoot};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use gz_core::msgs;
use gz_core::scene::{GeometryDesc, NodeKind, Pose, SceneDesc};
use gz_core::sdf::Inertial;
use gz_core::transport::Status;
use prost::Message as _;

#[derive(Resource, Default)]
struct Sent(Vec<GzCommand>);

fn collect(mut reader: MessageReader<GzCommand>, mut sent: ResMut<Sent>) {
    sent.0.extend(reader.read().cloned());
}

/// The overlays' state and rendering on a headless app with a live world
/// named `w`.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        bevy::mesh::MeshPlugin,
        bevy::gizmos::GizmoPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<WorldAsset>()
    .add_message::<GzCommand>()
    .add_message::<GzServiceReply>()
    .add_message::<OverlayCommand>()
    .init_resource::<GzGrid>()
    .init_resource::<GzLoads>()
    .init_resource::<GzMaterials>()
    .init_resource::<Sent>()
    .insert_resource(GzStatus {
        status: Status::Ready,
        world: Some("w".into()),
        ..GzStatus::new("ws://test")
    })
    .insert_resource(GzIndex {
        scene: Some(SceneDesc::from_msg(&Default::default(), Default::default())),
        ..default()
    })
    .add_plugins(OverlayRenderPlugin)
    .add_systems(Update, apply_overlay_commands)
    .add_systems(Last, collect);
    app.update();
    app
}

fn set(app: &mut App, o: Overlay, on: bool) {
    app.world_mut().write_message(OverlayCommand::set(o, on));
    app.update();
}

fn overlays(app: &App) -> Overlays {
    *app.world().resource::<Overlays>()
}

fn requests(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world_mut().resource_mut::<Sent>().0)
        .into_iter()
        .filter_map(|c| match c {
            GzCommand::Request { service, .. } => Some(service),
            _ => None,
        })
        .collect()
}

const SDF: &str = r#"<sdf version="1.10"><world name="w">
  <model name="robot">
    <link name="base">
      <collision name="body"><pose>0 0 0.5 0 0 0</pose>
        <geometry><box><size>1 1 1</size></box></geometry></collision>
      <collision name="terrain"><geometry><heightmap/></geometry></collision>
      <inertial><mass>2</mass><inertia><ixx>1</ixx><iyy>1</iyy><izz>1</izz></inertia></inertial>
    </link>
    <link name="wheel"/>
    <joint name="j" type="revolute"><child>wheel</child><parent>base</parent>
      <axis><xyz>0 1 0</xyz></axis></joint>
    <joint name="p" type="prismatic"><child>wheel</child><parent>base</parent></joint>
  </model></world></sdf>"#;

fn reply(app: &mut App, service: &str, xml: &str) {
    let payload = msgs::StringMsg {
        data: xml.into(),
        ..Default::default()
    }
    .encode_to_vec();
    app.world_mut().write_message(GzServiceReply {
        service: service.into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload,
    });
    app.update();
}

/// A model `robot` with links `base` and `wheel` under a world root.
fn robot(app: &mut App) -> (Entity, Entity) {
    let world = app.world_mut();
    let root = world
        .spawn((GzWorldRoot, Transform::default(), Visibility::default()))
        .id();
    let gz = |name: &str, kind, path: &str| GzEntity {
        name: name.into(),
        id: 0,
        kind,
        path: path.into(),
    };
    let model = world
        .spawn((
            gz("robot", NodeKind::Model, "robot"),
            Transform::from_xyz(1.0, 0.0, 0.0),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    let base = world
        .spawn((
            gz("base", NodeKind::Link, "robot/base"),
            Transform::default(),
            Visibility::default(),
            ChildOf(model),
        ))
        .id();
    let wheel = world
        .spawn((
            gz("wheel", NodeKind::Link, "robot/wheel"),
            Transform::default(),
            Visibility::default(),
            ChildOf(model),
        ))
        .id();
    (base, wheel)
}

#[test]
fn names_bindings_and_parsing() {
    for o in Overlay::ALL {
        assert_eq!(o.id().parse::<Overlay>(), Ok(o));
        assert!(!o.label().is_empty());
    }
    assert_eq!("COM".parse::<Overlay>(), Ok(Overlay::CenterOfMass));
    assert_eq!(
        "center of mass".parse::<Overlay>(),
        Ok(Overlay::CenterOfMass)
    );
    assert!("lidar".parse::<Overlay>().unwrap_err().contains("grid"));
    assert_eq!(Overlay::Grid.binding(), Some("overlay.grid"));
    assert_eq!(Overlay::Joints.binding(), Some("overlay.joints"));
    assert_eq!(Overlay::Frames.binding(), None);
    let map = crate::view::InputMap::builtin();
    for o in Overlay::ALL {
        if let Some(id) = o.binding() {
            assert!(map.binding(id).is_some(), "{id}");
        }
    }
    let needs: Vec<Overlay> = Overlay::ALL.into_iter().filter(|o| o.needs_sdf()).collect();
    assert_eq!(
        needs,
        [
            Overlay::Collisions,
            Overlay::Inertia,
            Overlay::CenterOfMass,
            Overlay::Joints
        ]
    );
}

#[test]
fn commands_toggle_and_set_and_the_grid_follows_the_scene() {
    let mut app = app();
    assert_eq!(overlays(&app), Overlays::default());
    set(&mut app, Overlay::Frames, true);
    assert!(overlays(&app).frames);
    set(&mut app, Overlay::Frames, true);
    assert!(overlays(&app).frames);
    app.world_mut()
        .write_message(OverlayCommand::toggle(Overlay::Frames));
    app.update();
    assert!(!overlays(&app).frames);
    // G turns the grid on; the scene's flag sets it again.
    app.world_mut()
        .write_message(OverlayCommand::toggle(Overlay::Grid));
    app.update();
    assert!(overlays(&app).grid);
    assert!(!app.world().resource::<GzGrid>().visible, "the scene's");
    app.world_mut().resource_mut::<GzGrid>().visible = false;
    app.update();
    assert!(!overlays(&app).grid);
    for o in Overlay::ALL {
        set(&mut app, o, true);
    }
    assert_eq!(overlays(&app).count(), 7);
    assert!(overlays(&app).need_sdf());
    set(&mut app, Overlay::Wireframe, false);
    assert_eq!(overlays(&app).count(), 6);
}

#[test]
fn the_sdf_is_asked_for_once_per_world_and_model_set() {
    let mut app = app();
    set(&mut app, Overlay::Frames, true);
    assert!(requests(&mut app).is_empty(), "frames need no SDFormat");
    set(&mut app, Overlay::Inertia, true);
    assert_eq!(requests(&mut app), ["/world/w/generate_world_sdf"]);
    set(&mut app, Overlay::Joints, true);
    assert!(requests(&mut app).is_empty());
    // A reply to another service is not ours.
    reply(&mut app, "/world/w/other", SDF);
    assert!(app.world().resource::<SdfExtras>().extras.is_none());
    reply(&mut app, "/world/w/generate_world_sdf", SDF);
    let extras = app.world().resource::<SdfExtras>();
    assert_eq!(extras.generation, 1);
    assert!(extras.extras.as_ref().unwrap().link("robot/base").is_some());
    // A spawned model asks again.
    app.world_mut()
        .resource_mut::<GzIndex>()
        .model_names
        .insert("new".into());
    app.update();
    assert_eq!(requests(&mut app), ["/world/w/generate_world_sdf"]);
    // A server without the service: logged, nothing changes, no retry.
    reply(&mut app, "/world/w/generate_world_sdf", "service_not_found");
    assert_eq!(app.world().resource::<SdfExtras>().generation, 1);
    app.update();
    assert!(requests(&mut app).is_empty());
}

const SDF_REQUEST: &str = "/world/w/generate_world_sdf";

fn model_names(app: &mut App) -> Mut<'_, std::collections::HashSet<String>> {
    app.world_mut()
        .resource_mut::<GzIndex>()
        .map_unchanged(|i| &mut i.model_names)
}

fn set_status(app: &mut App, status: Status) {
    app.world_mut().resource_mut::<GzStatus>().status = status;
    app.update();
}

/// The set of models, not their number: one model replacing another asks
/// again.
#[test]
fn a_replaced_model_asks_again() {
    let mut app = app();
    model_names(&mut app).insert("a".into());
    set(&mut app, Overlay::Joints, true);
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
    reply(&mut app, SDF_REQUEST, SDF);
    let mut names = model_names(&mut app);
    names.remove("a");
    names.insert("b".into());
    app.update();
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
}

/// One request at a time: a change while it is out is asked for when its
/// reply lands, so the last reply used is for the latest set.
#[test]
fn a_change_while_a_request_is_out_is_asked_for_after_its_reply() {
    let mut app = app();
    set(&mut app, Overlay::Joints, true);
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
    model_names(&mut app).insert("late".into());
    app.update();
    assert!(requests(&mut app).is_empty(), "one in flight");
    reply(&mut app, SDF_REQUEST, SDF);
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
    let late = SDF.replace(
        "</world>",
        r#"<model name="late"><link name="l"><inertial><mass>1</mass></inertial></link></model></world>"#,
    );
    reply(&mut app, SDF_REQUEST, &late);
    let extras = app.world().resource::<SdfExtras>();
    assert_eq!(extras.generation, 2);
    assert!(extras.extras.as_ref().unwrap().link("late/l").is_some());
    app.update();
    assert!(requests(&mut app).is_empty());
}

/// A reply lost to a disconnect is asked for again once the connection is
/// back; so is the world after any reconnect.
#[test]
fn a_reconnect_asks_again() {
    let mut app = app();
    set(&mut app, Overlay::Inertia, true);
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
    set_status(&mut app, Status::Disconnected);
    assert!(requests(&mut app).is_empty());
    set_status(&mut app, Status::Ready);
    assert_eq!(requests(&mut app), [SDF_REQUEST], "the lost reply");
    reply(&mut app, SDF_REQUEST, SDF);
    set_status(&mut app, Status::Connecting);
    set_status(&mut app, Status::Ready);
    assert_eq!(requests(&mut app), [SDF_REQUEST], "after a reconnect");
}

fn collisions(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    world
        .query_filtered::<&ChildOf, With<CollisionOverlay>>()
        .iter(world)
        .map(ChildOf::parent)
        .collect()
}

/// Links spawned again under the same paths (the scene after a reconnect,
/// a model deleted and respawned) get their collisions back, from the
/// reply they already have, whether the overlay was on then or not.
#[test]
fn respawned_links_get_their_collisions_back() {
    let mut app = app();
    model_names(&mut app).insert("robot".into());
    let (base, _) = robot(&mut app);
    set(&mut app, Overlay::Collisions, true);
    reply(&mut app, SDF_REQUEST, SDF);
    app.update();
    assert_eq!(collisions(&mut app), [base]);
    let despawn_world = |app: &mut App| {
        let world = app.world_mut();
        let roots: Vec<Entity> = world
            .query_filtered::<Entity, With<GzWorldRoot>>()
            .iter(world)
            .collect();
        for r in roots {
            world.entity_mut(r).despawn();
        }
    };
    despawn_world(&mut app);
    let (base, _) = robot(&mut app);
    app.update();
    assert_eq!(collisions(&mut app), [base]);
    // With the overlay off while they respawn: when it comes back on.
    set(&mut app, Overlay::Collisions, false);
    despawn_world(&mut app);
    let (base, _) = robot(&mut app);
    app.update();
    assert!(collisions(&mut app).is_empty());
    set(&mut app, Overlay::Collisions, true);
    app.update();
    assert_eq!(collisions(&mut app), [base]);
    // Nothing doubles up on a later toggle.
    set(&mut app, Overlay::Collisions, false);
    set(&mut app, Overlay::Collisions, true);
    app.update();
    assert_eq!(collisions(&mut app), [base]);
    assert!(requests(&mut app).iter().all(|r| r == SDF_REQUEST));
}

/// The scene tree wants the SDFormat without an overlay.
#[test]
fn the_sdf_is_asked_for_when_wanted() {
    use super::extras::SdfWanted;
    let mut app = app();
    app.update();
    assert!(requests(&mut app).is_empty());
    let mut wanted = SdfWanted::default();
    wanted.0.insert("test");
    app.insert_resource(wanted);
    app.update();
    assert_eq!(requests(&mut app), [SDF_REQUEST]);
}

#[test]
fn no_request_without_a_world() {
    let mut app = app();
    app.world_mut().resource_mut::<GzIndex>().scene = None;
    set(&mut app, Overlay::Collisions, true);
    assert!(requests(&mut app).is_empty());
    app.world_mut().resource_mut::<GzStatus>().world = None;
    app.world_mut().resource_mut::<GzIndex>().scene =
        Some(SceneDesc::from_msg(&Default::default(), Default::default()));
    app.update();
    assert!(requests(&mut app).is_empty());
}

#[test]
fn collisions_spawn_under_their_links_and_show_with_the_overlay() {
    let mut app = app();
    let (base, _) = robot(&mut app);
    set(&mut app, Overlay::Inertia, true);
    reply(&mut app, "/world/w/generate_world_sdf", SDF);
    let count = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(), With<CollisionOverlay>>()
            .iter(world)
            .count()
    };
    assert_eq!(count(&mut app), 0, "not until the overlay is on");
    set(&mut app, Overlay::Collisions, true);
    app.update();
    // The box; the heightmap isn't drawn.
    assert_eq!(count(&mut app), 1);
    let world = app.world_mut();
    let (e, parent, t, v) = world
        .query_filtered::<(Entity, &ChildOf, &Transform, &Visibility), With<CollisionOverlay>>()
        .single(world)
        .unwrap();
    assert_eq!(parent.parent(), base);
    assert_eq!(t.translation, Vec3::new(0.0, 0.0, 0.5));
    assert_eq!(*v, Visibility::Inherited);
    assert!(world.get::<Children>(e).is_some_and(|c| !c.is_empty()));
    set(&mut app, Overlay::Collisions, false);
    let world = app.world_mut();
    let v = world
        .query_filtered::<&Visibility, With<CollisionOverlay>>()
        .single(world)
        .unwrap();
    assert_eq!(*v, Visibility::Hidden);
    // A new reply replaces them.
    set(&mut app, Overlay::Collisions, true);
    app.world_mut()
        .resource_mut::<GzIndex>()
        .model_names
        .insert("x".into());
    app.update();
    reply(&mut app, "/world/w/generate_world_sdf", SDF);
    app.update();
    assert_eq!(count(&mut app), 1);

    let v = collision_visual(&GeometryDesc::Sphere { radius: 1.0 });
    assert_eq!(v.material.diffuse, Some(COLLISION_RGBA));
    assert!(v.material.present && !v.cast_shadows);
}

#[test]
fn gizmo_overlays_draw_without_panicking() {
    let mut app = app();
    robot(&mut app);
    for o in [
        Overlay::Frames,
        Overlay::Joints,
        Overlay::Inertia,
        Overlay::CenterOfMass,
    ] {
        set(&mut app, o, true);
    }
    reply(&mut app, "/world/w/generate_world_sdf", SDF);
    app.update();
    let store = app.world().resource::<GizmoConfigStore>();
    let (config, _) = store.config::<FrameGizmos>();
    assert_eq!(config.depth_bias, -1.0);
}

#[test]
fn inertia_boxes_match_their_moments() {
    // A 2 × 4 × 6 box of 3 kg.
    let m = 3.0;
    let (w, h, d): (f64, f64, f64) = (2.0, 4.0, 6.0);
    let i = Inertial {
        pose: Pose::IDENTITY,
        mass: m,
        inertia: [
            m * (h * h + d * d) / 12.0,
            0.0,
            0.0,
            m * (w * w + d * d) / 12.0,
            0.0,
            m * (w * w + h * h) / 12.0,
        ],
    };
    let (size, rot) = equivalent_box(&i);
    let mut sorted = size.to_array();
    sorted.sort_by(f32::total_cmp);
    assert!((Vec3::from_array(sorted) - Vec3::new(2.0, 4.0, 6.0)).length() < 1e-3);
    // The axes pair each size with its axis.
    for (k, axis) in [(0, rot * Vec3::X), (1, rot * Vec3::Y), (2, rot * Vec3::Z)] {
        let along = [Vec3::X, Vec3::Y, Vec3::Z]
            .iter()
            .position(|a| a.dot(axis).abs() > 0.99)
            .unwrap();
        assert!((size[k] - [2.0, 4.0, 6.0][along]).abs() < 1e-3);
    }
    // A point mass has no box.
    let point = Inertial {
        inertia: [0.0; 6],
        ..i
    };
    assert_eq!(equivalent_box(&point).0, Vec3::ZERO);
}

#[test]
fn eigen_decomposition_reconstructs_the_matrix() {
    let m = Mat3::from_cols(
        Vec3::new(4.0, 1.0, 0.5),
        Vec3::new(1.0, 3.0, -0.2),
        Vec3::new(0.5, -0.2, 2.0),
    );
    let (values, vectors) = symmetric_eigen(m);
    assert!(vectors.determinant() > 0.99);
    let back = vectors * Mat3::from_diagonal(values) * vectors.transpose();
    for (a, b) in back.to_cols_array().iter().zip(m.to_cols_array()) {
        assert!((a - b).abs() < 1e-4, "{back} vs {m}");
    }
    assert_eq!(symmetric_eigen(Mat3::IDENTITY).0, Vec3::ONE);
}

#[test]
fn centre_of_mass_spheres_are_lead() {
    // 1 kg of lead is a 2.76 cm sphere.
    assert!((com_radius(1.0) - 0.0276).abs() < 1e-3);
    assert_eq!(com_radius(0.0), 0.01);
    assert!(com_radius(1000.0) > com_radius(1.0));
}

fn triangle_mesh(indices: Option<Indices>) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    );
    if let Some(i) = indices {
        m.insert_indices(i);
    }
    m
}

fn line_count(m: &Mesh) -> usize {
    m.indices().map_or(0, |i| i.len() / 2)
}

#[test]
fn edge_lines_list_each_edge_once() {
    // A quad: two triangles sharing a diagonal, five edges.
    let quad = triangle_mesh(Some(Indices::U16(vec![0, 1, 2, 0, 2, 3])));
    let lines = edge_lines(&quad);
    assert_eq!(lines.primitive_topology(), PrimitiveTopology::LineList);
    assert_eq!(line_count(&lines), 5);
    let quad32 = triangle_mesh(Some(Indices::U32(vec![0, 1, 2, 0, 2, 3, 0, 1, 9])));
    // (0, 1) again, and two edges to a vertex that doesn't exist.
    assert_eq!(line_count(&edge_lines(&quad32)), 5);
    // Unindexed: every three vertices; the fourth is left over.
    assert_eq!(line_count(&edge_lines(&triangle_mesh(None))), 3);
    let strip = Mesh::new(PrimitiveTopology::LineStrip, RenderAssetUsages::default());
    assert_eq!(line_count(&edge_lines(&strip)), 0);
}

#[test]
fn wireframes_are_line_meshes_on_their_layer() {
    let mut app = app();
    let (base, _) = robot(&mut app);
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.0, 1.0, 1.0));
    let visual = app
        .world_mut()
        .spawn((Mesh3d(mesh.clone()), Transform::default(), ChildOf(base)))
        .id();
    // Not part of the scene: left alone.
    let other = app
        .world_mut()
        .spawn((Mesh3d(mesh.clone()), Transform::default()))
        .id();
    set(&mut app, Overlay::Wireframe, true);
    app.update();
    let lines = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&ChildOf, With<WireLines>>()
            .iter(world)
            .map(ChildOf::parent)
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&mut app), [visual]);
    // WebGPU refuses a depth bias on line lists (a validation error that
    // quits the app).
    let world = app.world_mut();
    let m = world
        .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<WireLines>>()
        .single(world)
        .unwrap()
        .0
        .clone();
    let m = app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&m)
        .unwrap();
    assert_eq!(m.depth_bias, 0.0);
    assert!(app.world().get::<Wired>(visual).is_some());
    // Only the views showing it see it.
    let world = app.world_mut();
    let layers = world
        .query_filtered::<&bevy::camera::visibility::RenderLayers, With<WireLines>>()
        .single(world)
        .unwrap()
        .clone();
    assert_eq!(
        layers,
        bevy::camera::visibility::RenderLayers::layer(Overlay::Wireframe.layer())
    );
    assert!(app.world().get::<Wired>(other).is_none());
    // A mesh that arrives while it's on gets one too, and shares the
    // cached lines.
    let late = app
        .world_mut()
        .spawn((Mesh3d(mesh), Transform::default(), ChildOf(base)))
        .id();
    app.update();
    app.update();
    assert_eq!(lines(&mut app).len(), 2);
    assert!(lines(&mut app).contains(&late));
    set(&mut app, Overlay::Wireframe, false);
    app.update();
    assert!(lines(&mut app).is_empty());
    assert!(app.world().get::<Wired>(visual).is_none());
}

#[test]
fn each_view_has_its_own_overlays_on_its_own_layers() {
    use super::{view_layers, OverlaySet};
    use crate::view::{ViewCamera, ViewId};
    use bevy::camera::visibility::RenderLayers;
    let mut app = app();
    app.add_systems(Update, view_layers.after(apply_overlay_commands));
    let view = |app: &mut App, id: u8| {
        app.world_mut()
            .spawn((
                Camera::default(),
                ViewCamera::new(Vec3::ZERO, crate::camera::perspective()),
                ViewId(id),
                ViewId(id).layers(),
                OverlaySet::default(),
            ))
            .id()
    };
    let (a, b) = (view(&mut app, 0), view(&mut app, 1));
    let set = |app: &App, e: Entity| app.world().get::<OverlaySet>(e).unwrap().0;
    let layers = |app: &App, e: Entity| app.world().get::<RenderLayers>(e).unwrap().clone();
    // One view's: the other's stays, and what any shows is drawn.
    app.world_mut()
        .write_message(OverlayCommand::set(Overlay::Joints, true).in_view(b));
    app.update();
    assert!(set(&app, b).joints && !set(&app, a).joints);
    assert!(overlays(&app).joints);
    assert_eq!(
        layers(&app, b),
        ViewId(1).layers().with(Overlay::Joints.layer())
    );
    assert_eq!(layers(&app, a), ViewId(0).layers());
    // Every view's: over from what any shows (on somewhere: off all).
    app.world_mut()
        .write_message(OverlayCommand::toggle(Overlay::Joints));
    app.update();
    assert!(!set(&app, a).joints && !set(&app, b).joints && !overlays(&app).joints);
    // The scene's grid flag turns every view's grid.
    app.world_mut().resource_mut::<GzGrid>().visible = true;
    app.update();
    assert!(set(&app, a).grid && set(&app, b).grid);
    // A view that isn't showing counts for nothing drawn.
    app.world_mut()
        .write_message(OverlayCommand::set(Overlay::Frames, true).in_view(b));
    app.world_mut().get_mut::<Camera>(b).unwrap().is_active = false;
    app.update();
    assert!(set(&app, b).frames && !overlays(&app).frames);
}

#[test]
fn collision_meshes_and_overlay_gizmos_draw_on_their_layers() {
    use bevy::camera::visibility::RenderLayers;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = app();
    app.world_mut()
        .run_system_once(overlay_gizmo_layers)
        .unwrap();
    let store = app.world().resource::<GizmoConfigStore>();
    assert_eq!(
        store.config::<FrameGizmos>().0.render_layers,
        RenderLayers::layer(Overlay::Frames.layer())
    );
    // A mesh under a collision root, however late it arrives.
    let root = app
        .world_mut()
        .spawn((CollisionOverlay, Transform::default()))
        .id();
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::default());
    let part = app
        .world_mut()
        .spawn(Transform::default())
        .insert(ChildOf(root))
        .id();
    let shape = app
        .world_mut()
        .spawn((Mesh3d(mesh), Transform::default(), ChildOf(part)))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<RenderLayers>(shape),
        Some(&RenderLayers::layer(Overlay::Collisions.layer()))
    );
}
