//! What every inspector body shares (spec §3.5 items 2 to 6): the title
//! row (type tag, name, more actions), the breadcrumb, the quick actions,
//! the groups with their live dot, and the link rows; and the "(removed)"
//! state (§2 states) with its Dismiss.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::overlays::{Overlay, OverlayCommand};
use gz_bevy::spawn::{GzEntity, GzWorldRoot};
use gz_bevy::view::{Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use super::{InspectKind, Inspected};
use crate::assets::text;
use crate::assets::IconId;
use crate::format;
use crate::theme::{
    border, radius, ColorRole, Fill, FontFamily, Ink, Opacity, Stroke, Theme, TypeStyle, SPACE,
};
use crate::widgets::layout::{self, column, display, flex_spacer, group_header, property_row, row};
use crate::widgets::text::set_text;
use crate::widgets::{button, icon_button, rounded, ButtonVariant, IconButtonSize, Tooltip};

/// The type tag's type: mono 11/500 (§3.5).
pub(crate) const TAG_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 500);
/// The breadcrumb's type: mono 11 `text_3`.
pub(crate) const CRUMB_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// The JOINT, LIGHT and SENSOR tags' fill: their colour at 12%.
pub(crate) const TINT_OPACITY: f32 = 0.12;
/// The live dot at a group header's right (§3.5: 6px `ok`).
pub(crate) const LIVE_DOT: f32 = 6.0;

/// A type tag (§3.5): its label, text colour, and fill (tinted: the fill
/// colour at [`TINT_OPACITY`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeTag {
    pub(crate) label: &'static str,
    pub(crate) ink: ColorRole,
    pub(crate) fill: ColorRole,
    pub(crate) tinted: bool,
}

impl TypeTag {
    pub(crate) fn of(kind: InspectKind) -> TypeTag {
        let tag = |label, ink, fill, tinted| TypeTag {
            label,
            ink,
            fill,
            tinted,
        };
        match kind {
            InspectKind::World => tag("WORLD", ColorRole::Text2, ColorRole::EntityBadgeBg, false),
            InspectKind::Model => tag("MODEL", ColorRole::AccentText, ColorRole::AccentSoft, false),
            InspectKind::Link => tag("LINK", ColorRole::Text2, ColorRole::EntityBadgeBg, false),
            InspectKind::Joint => tag(
                "JOINT",
                ColorRole::EntityJoint,
                ColorRole::EntityJoint,
                true,
            ),
            InspectKind::Light => tag(
                "LIGHT",
                ColorRole::EntityLight,
                ColorRole::EntityLight,
                true,
            ),
            InspectKind::Sensor => tag(
                "SENSOR",
                ColorRole::EntitySensor,
                ColorRole::EntitySensor,
                true,
            ),
            InspectKind::Multi => tag(
                "SELECTION",
                ColorRole::Text2,
                ColorRole::EntityBadgeBg,
                false,
            ),
        }
    }
}

/// The title row's name.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InspectorTitle(pub(crate) String);

/// The breadcrumb's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct InspectorCrumb;

/// A quick action (disabled while the subject is removed).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct QuickAction;

/// The title row's Dismiss, shown while the subject is removed.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DismissButton;

/// Title row (padding 14 16 10): the type tag, the name, Dismiss (while
/// removed) and more actions.
pub(crate) fn title_row(t: &Theme, kind: InspectKind, name: &str) -> impl Bundle {
    let tag = TypeTag::of(kind);
    let t = t.clone();
    let name = name.to_string();
    (
        Node {
            padding: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                top: px(SPACE[7]),
                bottom: px(SPACE[2]),
            },
            flex_shrink: 0.0,
            ..row(SPACE[4])
        },
        Children::spawn(SpawnWith(move |r: &mut ChildSpawner| {
            r.spawn(type_tag(tag));
            r.spawn((
                text(TypeStyle::ENTITY_NAME, ColorRole::Text, name.clone()),
                InspectorTitle(name),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            r.spawn(flex_spacer());
            r.spawn((button(&t, "Dismiss", ButtonVariant::Normal), DismissButton))
                .insert(Node {
                    display: Display::None,
                    height: px(t.s.button_h),
                    padding: UiRect::horizontal(px(SPACE[4])),
                    border: UiRect::all(px(border::HAIRLINE)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::SM),
                    ..default()
                });
            r.spawn(icon_button(
                &t,
                IconId::More,
                IconButtonSize::Small,
                Tooltip::new("More actions"),
            ));
        })),
    )
}

/// The type tag: mono 11/500 on its fill, radius 4, padding 2 6.
fn type_tag(tag: TypeTag) -> impl Bundle {
    let node = Node {
        padding: UiRect::axes(px(SPACE[3]), px(SPACE[1])),
        flex_shrink: 0.0,
        border_radius: rounded(radius::XS),
        ..default()
    };
    let fill = (
        Fill(tag.fill),
        Opacity(if tag.tinted { TINT_OPACITY } else { 1.0 }),
        Node {
            border_radius: rounded(radius::XS),
            ..layout::overlay()
        },
    );
    (node, children![fill, text(TAG_TYPE, tag.ink, tag.label)])
}

/// The breadcrumb (mono 11 `text_3`).
pub(crate) fn breadcrumb(crumb: impl Into<String>) -> impl Bundle {
    (
        text(CRUMB_TYPE, ColorRole::Text3, crumb.into()),
        InspectorCrumb,
        Node {
            margin: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                bottom: px(SPACE[6]),
                ..default()
            },
            ..default()
        },
    )
}

/// The quick actions' row; spawn three [`quick_action`]s in it.
pub(crate) fn quick_actions() -> impl Bundle {
    (
        Node {
            padding: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                bottom: px(SPACE[7]),
                ..default()
            },
            flex_shrink: 0.0,
            border: UiRect::bottom(px(border::HAIRLINE)),
            ..row(SPACE[3])
        },
        Stroke(ColorRole::Line),
    )
}

/// One of three equal quick actions (never destructive, Q3); `unavailable`
/// disables it with that reason.
pub(crate) fn quick_action(t: &Theme, label: &str, unavailable: Option<&str>) -> impl Bundle {
    let label = label.to_string();
    let tooltip = match unavailable {
        Some(why) => Tooltip::new(label.clone()).detail(why.to_string()),
        None => Tooltip::new(label.clone()),
    };
    (
        button(t, label, ButtonVariant::Normal),
        QuickAction,
        Disabled(unavailable.is_some()),
        tooltip,
    )
}

/// A quick action's size: one of three equal buttons.
pub(crate) fn quick_action_node(t: &Theme) -> Node {
    Node {
        height: px(t.s.button_h),
        flex_grow: 1.0,
        flex_basis: px(0),
        border: UiRect::all(px(border::HAIRLINE)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: rounded(radius::SM),
        ..default()
    }
}

/// A quick action that is unavailable whatever the subject's state.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Disabled(pub(crate) bool);

/// A group's heading with the live dot (§3.5: fields that stream from the
/// sim).
pub(crate) fn live_header(title: &str) -> impl Bundle {
    (
        row(SPACE[3]),
        children![
            text(TypeStyle::HEADING, ColorRole::Text, title.to_string()),
            flex_spacer(),
            (
                Node {
                    width: px(LIVE_DOT),
                    height: px(LIVE_DOT),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                Fill(ColorRole::Ok),
                Tooltip::new("Live").detail("Streams from the simulation"),
            )
        ],
    )
}

/// A group of rows of label and value, the values "—" until filled, each
/// carrying `marker(i)`.
pub(crate) fn group<M: Component>(
    body: &mut ChildSpawner,
    title: &str,
    live: bool,
    rows: &[&str],
    marker: impl Fn(usize) -> M,
) {
    body.spawn(layout::group()).with_children(|g| {
        if live {
            g.spawn(live_header(title));
        } else {
            g.spawn(group_header(title, None));
        }
        for (i, label) in rows.iter().enumerate() {
            let value = text(TypeStyle::MONO_SMALL, ColorRole::Text, "—");
            g.spawn(property_row(*label, (value, marker(i))));
        }
    });
}

/// The link rows at the bottom (§3.5 item 6): each opens a sub-view, the
/// last is All components. None opens yet: they show what they will be.
pub(crate) fn links(body: &mut ChildSpawner, names: &[&str]) {
    body.spawn(Node {
        padding: UiRect::axes(px(SPACE[8]), px(SPACE[4])),
        flex_shrink: 0.0,
        ..column(0.0)
    })
    .with_children(|links| {
        for name in names {
            links
                .spawn((
                    Node {
                        height: px(36),
                        flex_shrink: 0.0,
                        ..row(0.0)
                    },
                    Tooltip::new(*name).detail("Not available yet"),
                ))
                .with_children(|row| {
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text3, name.to_string()));
                    row.spawn(flex_spacer());
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text3, "›"));
                });
        }
    });
}

/// Which pose a [`pose_group`] shows: in the world, or in its parent's
/// frame (a link's in its model's).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PoseFrame {
    World,
    Parent,
}

/// One of a pose group's six values: x, y, z, roll, pitch, yaw.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PoseValue(pub(crate) PoseFrame, pub(crate) usize);

/// The Pose group (§3.5): live, X Y Z in m and Roll Pitch Yaw in degrees
/// (Gazebo's fixed-axis roll, pitch, yaw), in `frame`.
pub(crate) fn pose_group(body: &mut ChildSpawner, title: &str, frame: PoseFrame) {
    group(
        body,
        title,
        true,
        &["X", "Y", "Z", "Roll", "Pitch", "Yaw"],
        |i| PoseValue(frame, i),
    );
}

/// The subject's pose in Gazebo's frame: its own transform for the
/// parent's frame (the world root turns Gazebo into Bevy, so everything
/// under it is in Gazebo axes), or relative to the root for the world.
pub(crate) fn gazebo_pose(
    frame: PoseFrame,
    local: &Transform,
    global: &GlobalTransform,
    root: Option<&GlobalTransform>,
) -> Transform {
    match (frame, root) {
        (PoseFrame::Parent, _) => *local,
        (PoseFrame::World, Some(root)) => global.reparented_to(root),
        (PoseFrame::World, None) => global.compute_transform(),
    }
}

/// The six values of a pose: metres and degrees.
pub(crate) fn pose_texts(pose: &Transform) -> [String; 6] {
    let (yaw, pitch, roll) = pose.rotation.to_euler(EulerRot::ZYX);
    let t = pose.translation;
    [
        format::metres(t.x),
        format::metres(t.y),
        format::metres(t.z),
        format::degrees(roll),
        format::degrees(pitch),
        format::degrees(yaw),
    ]
}

/// The pose groups follow the subject as the sim moves it (§3.5: live
/// values stream while unfocused; these are read-only until M5c).
pub(crate) fn pose_values(
    inspected: Res<Inspected>,
    subjects: Query<(Ref<Transform>, Ref<GlobalTransform>)>,
    roots: Query<&GlobalTransform, With<GzWorldRoot>>,
    added: Query<(), Added<PoseValue>>,
    mut values: Query<(&PoseValue, &mut Text)>,
) {
    let Some((local, global)) = inspected.entity.and_then(|e| subjects.get(e).ok()) else {
        return;
    };
    // The world pose follows the propagated transform, a step after the
    // local one.
    if !local.is_changed() && !global.is_changed() && added.is_empty() && !inspected.is_changed() {
        return;
    }
    let root = roots.iter().next();
    let mut cache: [Option<[String; 6]>; 2] = [None, None];
    for (value, mut text) in &mut values {
        let slot = &mut cache[value.0 as usize];
        let texts =
            slot.get_or_insert_with(|| pose_texts(&gazebo_pose(value.0, &local, &global, root)));
        set_text(&mut text, &texts[value.1]);
    }
}

/// While the subject is removed: the name in `text_3` with "(removed)",
/// the quick actions disabled, Dismiss shown (§2 states).
pub(crate) fn removed_state(
    inspected: Res<Inspected>,
    added: Query<(), Added<InspectorTitle>>,
    mut titles: Query<(&InspectorTitle, &mut Text, &mut Ink)>,
    mut dismiss: Query<&mut Node, With<DismissButton>>,
    actions: Query<(Entity, &Disabled, Has<InteractionDisabled>), With<QuickAction>>,
    mut commands: Commands,
) {
    if !inspected.is_changed() && added.is_empty() {
        return;
    }
    let removed = inspected.removed;
    for (title, mut text, mut ink) in &mut titles {
        let want = if removed {
            format!("{} (removed)", title.0)
        } else {
            title.0.clone()
        };
        set_text(&mut text, &want);
        ink.set_if_neq(Ink(if removed {
            ColorRole::Text3
        } else {
            ColorRole::Text
        }));
    }
    for mut node in &mut dismiss {
        let want = display(removed);
        if node.display != want {
            node.display = want;
        }
    }
    for (e, disabled, has) in &actions {
        let want = removed || disabled.0;
        if want != has {
            if want {
                commands.entity(e).insert(InteractionDisabled);
            } else {
                commands.entity(e).remove::<InteractionDisabled>();
            }
        }
    }
}

/// Dismiss: back to the World inspector.
pub(crate) fn on_dismiss(
    ev: On<Activate>,
    buttons: Query<(), With<DismissButton>>,
    mut inspected: ResMut<Inspected>,
    mut selection: ResMut<Selection>,
) {
    if buttons.contains(ev.entity) {
        *inspected = Inspected::default();
        if !selection.entities.is_empty() {
            selection.entities.clear();
        }
    }
}

/// "warehouse_demo › amr_01 › base_link": the world, then the scene path.
pub(crate) fn crumb(world: Option<&str>, path: &str) -> String {
    world
        .into_iter()
        .chain(path.split('/'))
        .collect::<Vec<_>>()
        .join(" › ")
}

/// What an entity inspector's quick actions do.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntityAction {
    /// Frame the subject in the views (F).
    Focus,
    /// Frame the whole selection in the views.
    FocusSelection,
    /// Select the link's model.
    SelectModel,
    /// Turn the inertia overlay on or off.
    ShowInertia,
    /// Turn the collisions overlay on or off.
    ShowCollision,
    /// Turn the joints overlay on or off (a joint's Show axis).
    ShowJoints,
    /// Hide or show the subject.
    Hide,
}

/// The quick actions. Focus frames the subject: it selects it first if it
/// isn't (pinned), since focus frames the selection.
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_entity_action(
    ev: On<Activate>,
    actions: Query<&EntityAction, Without<InteractionDisabled>>,
    inspected: Res<Inspected>,
    parents: Query<&ChildOf>,
    kinds: Query<&GzEntity>,
    cameras: Query<Entity, With<MainCamera>>,
    mut visibility: Query<&mut Visibility>,
    mut selection: ResMut<Selection>,
    mut views: MessageWriter<ViewCommand>,
    mut overlays: MessageWriter<OverlayCommand>,
) {
    let Ok(action) = actions.get(ev.entity) else {
        return;
    };
    if *action == EntityAction::FocusSelection {
        for view in &cameras {
            views.write(ViewCommand {
                view,
                op: ViewOp::Focus { at: None },
            });
        }
        return;
    }
    let Some(subject) = inspected.entity else {
        return;
    };
    match action {
        EntityAction::Focus => {
            if selection.entities != [subject] {
                selection.entities = vec![subject];
            }
            for view in &cameras {
                views.write(ViewCommand {
                    view,
                    op: ViewOp::Focus { at: None },
                });
            }
        }
        EntityAction::FocusSelection => {}
        EntityAction::SelectModel => {
            let mut e = subject;
            while let Ok(parent) = parents.get(e) {
                e = parent.parent();
                if kinds.get(e).is_ok_and(|g| g.kind == NodeKind::Model) {
                    selection.entities = vec![e];
                    break;
                }
            }
        }
        EntityAction::ShowInertia => {
            overlays.write(OverlayCommand::toggle(Overlay::Inertia));
        }
        EntityAction::ShowCollision => {
            overlays.write(OverlayCommand::toggle(Overlay::Collisions));
        }
        EntityAction::ShowJoints => {
            overlays.write(OverlayCommand::toggle(Overlay::Joints));
        }
        EntityAction::Hide => {
            if let Ok(mut v) = visibility.get_mut(subject) {
                let hidden = *v == Visibility::Hidden;
                v.set_if_neq(if hidden {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}

/// A quick action that selects the entity at this scene path (a joint's
/// parent or child link).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectPath(pub(crate) String);

pub(crate) fn on_select_path(
    ev: On<Activate>,
    actions: Query<&SelectPath, Without<InteractionDisabled>>,
    entities: Query<(Entity, &GzEntity)>,
    mut selection: ResMut<Selection>,
) {
    let Ok(SelectPath(path)) = actions.get(ev.entity) else {
        return;
    };
    if let Some((e, _)) = entities.iter().find(|(_, g)| &g.path == path) {
        gz_bevy::view::select(
            &mut selection.reborrow(),
            gz_bevy::view::SelectMode::Replace,
            &[e],
        );
    }
}

/// A row of `label` and a value text, for definitions (not live).
pub(crate) fn value_row(label: &str, value: impl Into<String>) -> impl Bundle {
    property_row(
        label.to_string(),
        text(TypeStyle::MONO_SMALL, ColorRole::Text, value.into()),
    )
}

/// A group of rows of label and value (definitions, from the scene and
/// the world's SDFormat).
pub(crate) fn definition_group(body: &mut ChildSpawner, title: &str, rows: Vec<(String, String)>) {
    body.spawn(layout::group()).with_children(|g| {
        g.spawn(group_header(title, None));
        for (label, value) in rows {
            g.spawn(value_row(&label, value));
        }
    });
}

/// A small Gazebo scene for the inspectors' tests: the world root (Gazebo
/// to Bevy), a model `cart` at (1, 2, 0.5) turned 90° about z with a link
/// `base` 0.1 above it holding a box visual and a lidar, a model `crate`,
/// a directional light `sun`, and their SDFormat extras.
#[cfg(test)]
pub(crate) mod fixture {
    use bevy::prelude::*;
    use gz_bevy::overlays::extras::SdfExtras;
    use gz_bevy::spawn::{
        gz_to_bevy, GzEntity, GzIndex, GzLight, GzSensors, GzVisual, GzWorldRoot,
    };
    use gz_core::scene::{LightDesc, LightType, NodeKind, Pose, SensorDesc, VisualDesc};
    use gz_core::sdf::WorldExtras;

    use crate::test_support::frames;

    pub(crate) struct Scene {
        pub(crate) cart: Entity,
        pub(crate) base: Entity,
        pub(crate) krate: Entity,
        pub(crate) sun: Entity,
    }

    pub(crate) const SDF: &str = r#"<sdf version="1.9"><world name="w">
      <model name="cart">
        <static>true</static>
        <plugin filename="gz-sim-diff-drive-system" name="gz::sim::systems::DiffDrive"/>
        <link name="base">
          <kinematic>true</kinematic>
          <inertial><mass>2.5</mass><inertia><ixx>0.1</ixx><iyy>0.2</iyy><izz>0.3</izz></inertia></inertial>
          <collision name="hull"><geometry><box><size>1 1 1</size></box></geometry></collision>
          <sensor name="lidar" type="gpu_lidar"><topic>/scan</topic><update_rate>10</update_rate></sensor>
        </link>
        <link name="wheel"/>
        <joint name="hinge" type="revolute"><parent>base</parent><child>wheel</child>
          <axis><xyz>0 1 0</xyz><limit><lower>-1</lower><upper>1</upper></limit></axis></joint>
      </model>
      <model name="crate"><link name="l"/></model>
    </world></sdf>"#;

    fn node(
        app: &mut App,
        parent: Entity,
        name: &str,
        path: &str,
        kind: NodeKind,
        t: Transform,
    ) -> Entity {
        app.world_mut()
            .spawn((
                GzEntity {
                    name: name.into(),
                    id: 0,
                    kind,
                    path: path.into(),
                },
                t,
                Visibility::default(),
                ChildOf(parent),
            ))
            .id()
    }

    pub(crate) fn scene(app: &mut App) -> Scene {
        let root = app
            .world_mut()
            .spawn((
                GzWorldRoot,
                Transform::from_rotation(gz_to_bevy()),
                Visibility::default(),
            ))
            .id();
        {
            let mut index = app.world_mut().resource_mut::<GzIndex>();
            index.root = Some(root);
            index.scene = Some(gz_core::scene::SceneDesc::from_msg(
                &Default::default(),
                Default::default(),
            ));
        }
        let cart = node(
            app,
            root,
            "cart",
            "cart",
            NodeKind::Model,
            Transform::from_xyz(1.0, 2.0, 0.5)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        );
        let base = node(
            app,
            cart,
            "base",
            "cart/base",
            NodeKind::Link,
            Transform::from_xyz(0.0, 0.0, 0.1),
        );
        app.world_mut()
            .entity_mut(base)
            .insert(GzSensors(vec![SensorDesc {
                name: "lidar".into(),
                id: 0,
                ty: "gpu_lidar".into(),
                pose: Pose::IDENTITY,
            }]));
        let body = node(
            app,
            base,
            "body",
            "cart/base/body",
            NodeKind::Visual,
            Transform::default(),
        );
        app.world_mut()
            .entity_mut(body)
            .insert(GzVisual(VisualDesc {
                proto_type: "box".into(),
                geometry: gz_core::scene::GeometryDesc::Box { size: [1.0; 3] },
                material: gz_core::scene::MaterialDesc::absent(),
                cast_shadows: true,
                transparency: 0.0,
            }));
        node(
            app,
            cart,
            "wheel",
            "cart/wheel",
            NodeKind::Link,
            Transform::default(),
        );
        let krate = node(
            app,
            root,
            "crate",
            "crate",
            NodeKind::Model,
            Transform::default(),
        );
        let sun = node(
            app,
            root,
            "sun",
            "sun",
            NodeKind::Light,
            Transform::from_xyz(0.0, 0.0, 10.0),
        );
        app.world_mut().entity_mut(sun).insert(GzLight(LightDesc {
            ty: LightType::Directional,
            color: [0.8; 3],
            intensity: 1.0,
            range: 0.0,
            angle: None,
            inner_angle: None,
            penumbra: None,
            decay: None,
            cast_shadows: true,
            direction: Some([0.0, 0.0, -1.0]),
            msg_intensity: 1.0,
            specular: [0.2; 3],
            attenuation: [1.0, 0.0, 0.0],
        }));
        let mut sdf = SdfExtras::default();
        sdf.extras = Some(WorldExtras::parse(SDF).unwrap());
        sdf.generation = 1;
        app.insert_resource(sdf);
        frames(app, 2);
        Scene {
            cart,
            base,
            krate,
            sun,
        }
    }
}
