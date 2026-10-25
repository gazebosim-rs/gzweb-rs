//! What every inspector body shares (spec §3.5 items 2 to 6): the title
//! row (type tag, name, more actions), the breadcrumb, the quick actions,
//! the groups with their live dot, and the link rows; and the "(removed)"
//! state (§2 states) with its Dismiss.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as Headless};
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::{edits_allowed, movable, HasPlane, Held, SetPose};
use gz_bevy::overlays::{Overlay, OverlayCommand};
use gz_bevy::spawn::{GzEntity, GzWorldRoot};
use gz_bevy::undo::{Change, Record};
use gz_bevy::view::editing::EditCommand;
use gz_bevy::view::gizmo::SEND_SECS;
use gz_bevy::view::{SelectMode, Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use super::{InspectKind, Inspected};
use crate::assets::text;
use crate::assets::IconId;
use crate::format;
use crate::theme::visuals::Visuals;
use crate::theme::{
    border, radius, ColorRole, Fill, FontFamily, Ink, Opacity, Stroke, Theme, TypeStyle, SPACE,
};
use crate::widgets::layout::{self, column, display, flex_spacer, group_header, property_row, row};
use crate::widgets::text::set_text;
use crate::widgets::{
    button, clickable, icon_button, number_field, rounded, AxisTag, ButtonVariant, IconButtonSize,
    NumberCommit, NumberField, Tooltip, Unit,
};

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
    links_with(body, names, &[]);
}

/// [`links`], where the rows named in `live` run their action (the Model
/// inspector's Edit model).
pub(crate) fn links_with(body: &mut ChildSpawner, names: &[&str], live: &[(&str, EntityAction)]) {
    body.spawn(Node {
        padding: UiRect::axes(px(SPACE[8]), px(SPACE[4])),
        flex_shrink: 0.0,
        ..column(0.0)
    })
    .with_children(|links| {
        for name in names {
            let action = live.iter().find(|(n, _)| n == name).map(|(_, a)| *a);
            let node = Node {
                height: px(36),
                flex_shrink: 0.0,
                ..row(0.0)
            };
            let ink = if action.is_some() {
                ColorRole::Text
            } else {
                ColorRole::Text3
            };
            let mut row = match action {
                Some(a) => links.spawn((
                    node,
                    Headless,
                    clickable(
                        Visuals::new(ColorRole::Clear, ColorRole::Text),
                        accesskit::Role::Button,
                        name.to_string(),
                    ),
                    a,
                )),
                None => links.spawn((node, Tooltip::new(*name).detail("Not available yet"))),
            };
            row.with_children(|row| {
                row.spawn(text(TypeStyle::ROW, ink, name.to_string()));
                row.spawn(flex_spacer());
                row.spawn(text(TypeStyle::ROW, ink, "›"));
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

/// A pose's rows (§3.5: Position X Y Z, Rotation R P Yw; yaw is Yw so it
/// never reads as the Y position).
pub(crate) const POSE_LABELS: [&str; 6] = ["X", "Y", "Z", "R", "P", "Yw"];

/// One of a pose group's six values: x, y, z, roll, pitch, yaw.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PoseValue(pub(crate) PoseFrame, pub(crate) usize);

/// The Pose group (§3.5): live, X Y Z in m and Roll Pitch Yaw in degrees
/// (Gazebo's fixed-axis roll, pitch, yaw), in `frame`. `editable` makes
/// the values number fields, Position and Rotation rows of three, that
/// move the subject (M5c).
pub(crate) fn pose_group(
    body: &mut ChildSpawner,
    t: &Theme,
    title: &str,
    frame: PoseFrame,
    editable: bool,
) {
    if !editable {
        group(body, title, true, &POSE_LABELS, |i| PoseValue(frame, i));
        return;
    }
    body.spawn(layout::group()).with_children(|g| {
        g.spawn(live_header(title));
        for (label, first) in [("Position", 0), ("Rotation", 3)] {
            g.spawn(text(TypeStyle::ROW, ColorRole::Text2, label));
            g.spawn(row(SPACE[3])).with_children(|r| {
                for i in first..first + 3 {
                    r.spawn((pose_field(t, label, i), PoseValue(frame, i)));
                }
            });
        }
    });
}

/// Whether the subject's pose is editable: the server takes edits and the
/// subject is one the tools move (a top-level model without a plane).
pub(crate) fn pose_editable(
    status: Option<&GzStatus>,
    subject: Entity,
    nodes: &Query<(&GzEntity, &ChildOf, Has<HasPlane>)>,
    roots: &Query<Entity, With<GzWorldRoot>>,
) -> bool {
    let (Ok((g, parent, plane)), Some(root)) = (nodes.get(subject), roots.iter().next()) else {
        return false;
    };
    edits_allowed(status) && movable(g, parent, root, plane)
}

/// The number field for a pose's value `i`: millimetres and tenths of a
/// degree, as the text shows them.
fn pose_field(t: &Theme, label: &str, i: usize) -> impl Bundle {
    let tag = [
        AxisTag::X,
        AxisTag::Y,
        AxisTag::Z,
        AxisTag::Roll,
        AxisTag::Pitch,
        AxisTag::Yaw,
    ][i]
        .clone();
    let field = if i < 3 {
        NumberField::new(0.0).unit(Unit::Length).precision(3, 0.01)
    } else {
        NumberField::new(0.0).unit(Unit::Angle).precision(1, 0.5)
    };
    let name = format!("{label} {}", tag.name());
    number_field(t, tag, field.named(name), false)
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

/// The six values of a pose as numbers: metres and degrees.
pub(crate) fn pose_numbers(pose: &Transform) -> [f64; 6] {
    let (yaw, pitch, roll) = pose.rotation.to_euler(EulerRot::ZYX);
    let t = pose.translation.as_dvec3();
    let deg = |a: f32| f64::from(a).to_degrees();
    [t.x, t.y, t.z, deg(roll), deg(pitch), deg(yaw)]
}

/// The pose with the six values `v` (metres and degrees) and `scale`.
pub(crate) fn pose_from_numbers(v: [f64; 6], scale: Vec3) -> Transform {
    let rad = |d: f64| d.to_radians() as f32;
    Transform {
        translation: Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32),
        rotation: Quat::from_euler(EulerRot::ZYX, rad(v[5]), rad(v[4]), rad(v[3])),
        scale,
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
/// values stream while unfocused; a focused field keeps its text, D-39).
pub(crate) fn pose_values(
    inspected: Res<Inspected>,
    subjects: Query<(Ref<Transform>, Ref<GlobalTransform>)>,
    roots: Query<&GlobalTransform, With<GzWorldRoot>>,
    added: Query<(), Added<PoseValue>>,
    edit: Res<PoseEdit>,
    mut values: Query<(&PoseValue, &mut Text)>,
    mut fields: Query<(&PoseValue, &mut NumberField)>,
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
    // Fields are only on top-level subjects, whose pose in the world is
    // their own transform: this frame's, where the propagated one lags a
    // frame behind an edit and would pull the field back.
    let numbers = edit.numbers(inspected.entity, &local);
    for (value, mut field) in &mut fields {
        let v = numbers[value.1];
        if field.value != v {
            field.value = v;
        }
    }
}

/// The pose fields' edit under way.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct PoseEdit {
    /// The six numbers the fields last set, with the subject and the pose
    /// they made. They stand while the subject is still at that pose, so a
    /// scrub of pitch past ±90° carries on rather than turning into the
    /// same rotation's other angles (and back) at every step.
    set: Option<(Entity, Transform, [f64; 6])>,
    /// When the last scrub step went out, and a later one not yet sent.
    sent_at: Option<f64>,
    pending: Option<SetPose>,
}

impl PoseEdit {
    /// The subject's pose as six numbers: the fields' own while it is
    /// where they put it.
    fn numbers(&self, subject: Option<Entity>, pose: &Transform) -> [f64; 6] {
        match self.set {
            Some((e, set, v)) if Some(e) == subject && set == *pose => v,
            _ => pose_numbers(pose),
        }
    }
}

/// A pose field's commit moves the subject (a top-level model, so its pose
/// in the world is its pose in its parent's frame): at once
/// here, and in the sim through `set_pose`, at most every
/// [`SEND_SECS`] while the tag is scrubbed (a step held back goes out
/// when that time is up, [`send_pending_pose`]). Its poses from the sim
/// are held meanwhile, as for the gizmo.
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_pose_commit(
    ev: On<NumberCommit>,
    values: Query<&PoseValue>,
    inspected: Res<Inspected>,
    status: Option<Res<GzStatus>>,
    time: Option<Res<Time<Real>>>,
    nodes: Query<(&GzEntity, &ChildOf, Has<HasPlane>)>,
    roots: Query<Entity, With<GzWorldRoot>>,
    mut subjects: Query<&mut Transform, With<GzEntity>>,
    mut held: Option<ResMut<Held>>,
    mut edit: ResMut<PoseEdit>,
    mut out: (MessageWriter<SetPose>, MessageWriter<Record>),
) {
    let (Ok(value), Some(entity)) = (values.get(ev.entity), inspected.entity) else {
        return;
    };
    if !pose_editable(status.as_deref(), entity, &nodes, &roots) {
        return;
    }
    let Ok(mut local) = subjects.get_mut(entity) else {
        return;
    };
    let before = *local;
    let mut v = edit.numbers(Some(entity), &local);
    v[value.1] = ev.value;
    let pose = pose_from_numbers(v, local.scale);
    *local = pose;
    edit.set = Some((entity, pose, v));
    // A scrub is one edit, from where it began (its steps join it).
    out.1.write(Record {
        verb: "move",
        named: true,
        changes: vec![Change::Pose {
            entity,
            before,
            after: pose,
        }],
        scrub: Some(ev.entity),
        open: !ev.is_final,
    });
    let now = time.map_or(0.0, |t| t.elapsed_secs_f64());
    // Each scrub step holds again for a moment, so a scrub cut short (the
    // body rebuilt under it) can't hold the subject for good.
    if let Some(held) = held.as_deref_mut() {
        held.release(entity, now);
    }
    let set_pose = SetPose { entity, pose };
    if ev.is_final || edit.sent_at.is_none_or(|t| now - t >= SEND_SECS) {
        out.0.write(set_pose);
        edit.sent_at = (!ev.is_final).then_some(now);
        edit.pending = None;
    } else {
        edit.pending = Some(set_pose);
    }
}

/// Sends the scrub step held back once [`SEND_SECS`] is up, so a scrub
/// that stops there doesn't spring back to the step sent before it when
/// the hold runs out.
pub(crate) fn send_pending_pose(
    time: Res<Time<Real>>,
    mut edit: ResMut<PoseEdit>,
    mut held: ResMut<Held>,
    mut edits: MessageWriter<SetPose>,
) {
    let now = time.elapsed_secs_f64();
    if edit.pending.is_none() || edit.sent_at.is_some_and(|t| now - t < SEND_SECS) {
        return;
    }
    if let Some(set_pose) = edit.pending.take() {
        edits.write(set_pose);
        held.release(set_pose.entity, now);
        edit.sent_at = Some(now);
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
        if !selection.is_empty() {
            *selection = Selection::default();
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
    /// Edit the model in place (§6.6).
    EditModel,
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
    mut records: MessageWriter<Record>,
    mut editing: MessageWriter<EditCommand>,
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
            gz_bevy::view::select(&mut selection.reborrow(), SelectMode::Replace, &[subject]);
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
                    gz_bevy::view::select(&mut selection.reborrow(), SelectMode::Replace, &[e]);
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
        EntityAction::EditModel => {
            editing.write(EditCommand::Enter(subject));
        }
        EntityAction::Hide => {
            if let Ok(mut v) = visibility.get_mut(subject) {
                let before = *v;
                let hidden = before == Visibility::Hidden;
                let after = if hidden {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                *v = after;
                records.write(Record {
                    verb: if hidden { "show" } else { "hide" },
                    named: true,
                    changes: vec![Change::Visibility {
                        entity: subject,
                        before,
                        after,
                    }],
                    scrub: None,
                    open: false,
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
        gz_bevy::view::select(&mut selection.reborrow(), SelectMode::Replace, &[e]);
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

/// The Hide quick action reads Show while its subject is hidden.
pub(crate) fn hide_label(
    inspected: Res<Inspected>,
    visibility: Query<Ref<Visibility>>,
    actions: Query<(&EntityAction, &Children)>,
    added: Query<(), Added<EntityAction>>,
    mut texts: Query<&mut Text>,
) {
    let Some(v) = inspected.entity.and_then(|e| visibility.get(e).ok()) else {
        return;
    };
    if !v.is_changed() && added.is_empty() {
        return;
    }
    let label = if *v == Visibility::Hidden {
        "Show"
    } else {
        "Hide"
    };
    for (action, children) in &actions {
        if *action == EntityAction::Hide {
            for c in children {
                if let Ok(mut t) = texts.get_mut(*c) {
                    set_text(&mut t, label);
                }
            }
        }
    }
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
        <plugin filename="gz-sim-joint-state-publisher-system"
                name="gz::sim::systems::JointStatePublisher"><topic>/cart/joints</topic></plugin>
        <plugin filename="gz-sim-joint-position-controller-system"
                name="gz::sim::systems::JointPositionController"><joint_name>slide</joint_name></plugin>
        <link name="base">
          <kinematic>true</kinematic>
          <inertial><mass>2.5</mass><inertia><ixx>0.1</ixx><iyy>0.2</iyy><izz>0.3</izz></inertia></inertial>
          <collision name="hull"><geometry><box><size>1 1 1</size></box></geometry></collision>
          <sensor name="lidar" type="gpu_lidar"><topic>/scan</topic><update_rate>10</update_rate></sensor>
        </link>
        <link name="wheel"/>
        <joint name="hinge" type="revolute"><parent>base</parent><child>wheel</child>
          <axis><xyz>0 1 0</xyz><limit><lower>-1</lower><upper>1</upper></limit></axis></joint>
        <link name="rail"/>
        <joint name="slide" type="prismatic"><parent>base</parent><child>rail</child>
          <axis><xyz expressed_in="__model__">1 0 0</xyz><limit><lower>0</lower><upper>0.5</upper><effort>30</effort><velocity>2</velocity></limit></axis></joint>
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
