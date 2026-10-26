//! The Parts tab (spec §6.6; DECISIONS.md D-113): while a model is
//! edited, the Assets panel adds parts to it. Link, Joint, Visual,
//! Collision and Sensor arm placing (`gz_bevy::view::place`): a click in
//! the view adds a link there, or a visual, collision or sensor to the
//! link clicked; a joint takes two clicks, its parent link then its child,
//! with a rubber band from the parent to the pointer in `entity_joint`.
//! Plugin names a library and a class. The shape, joint and sensor selects
//! say what each adds. A shape from the Shapes tab adds a link of that
//! shape too. Each part is a `PartCommand` (`gz_bevy::view::editing::
//! parts`); undo takes it back.

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as Headless};
use gz_bevy::overlays::draw::JointGizmos;
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::{gz_to_bevy, GzEntity, GzSensors};
use gz_bevy::view::editing::parts::{EditParts, PartCommand, PartsDiscarded};
use gz_bevy::view::editing::{is_under, EditingModel};
use gz_bevy::view::place::{PlaceAt, PlaceHover};
use gz_bevy::view::NoViewPick;
use gz_core::insert::{Insertable, Shape};
use gz_core::parts::{
    joint_label, joint_problem, Draft, Part, SensorKind, JOINT_KINDS, LINK_SIZE, PART_SHAPES,
    PART_SIZE,
};
use gz_core::scene::{NodeKind, Pose};
use gz_core::sdf::{Joint, WorldExtras};

use super::model::{AssetTab, AssetsState, PartTool, PartsState, Reparenting, JOINT_CHILD_HINT};
use super::view::{TabChoice, TabPane, TILE_H};
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::inspectors::common::Disabled;
use crate::inspectors::Inspected;
use crate::shell::status_bar::readouts::StatusNote;
use crate::theme::visuals::{Active, Content, Visuals};
use crate::theme::{border, radius, ColorRole, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, display, property_row, set_shown};
use crate::widgets::text::set_text;
use crate::widgets::{
    button, clickable, rounded, select, text_field, ButtonVariant, SelectChanged, TextSubmit,
    Tooltip,
};

/// A Parts tile.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PartTile(pub(crate) PartTool);

/// Which choice a Parts select makes.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PartChoice {
    Shape,
    Joint,
    Sensor,
}

/// The hint under the tiles.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PartsHint;

/// The plugin's fields and Add, shown while Plugin is chosen.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PluginForm;

/// The plugin's library field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PluginFile;

/// The plugin's class field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PluginName;

/// The plugin's Add.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PluginAdd;

/// The preview of a link, visual or collision about to be added.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PartGhost {
    pub shape: Shape,
    pub size: f64,
}

/// A Link or Joint inspector quick action that changes the model being
/// edited (§6.6): Add visual, Add collision and Add sensor put the Parts
/// tab's shape or sensor at the link's origin; Reparent has the next click
/// pick the joint's new parent link.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartAction {
    Visual,
    Collision,
    Sensor,
    Reparent,
}

impl PartAction {
    /// What its button says.
    pub(crate) fn label(self) -> &'static str {
        match self {
            PartAction::Visual => "Add visual",
            PartAction::Collision => "Add collision",
            PartAction::Sensor => "Add sensor",
            PartAction::Reparent => "Reparent",
        }
    }

    /// Why it can't run outside editing.
    pub(crate) fn unavailable(self) -> &'static str {
        match self {
            PartAction::Reparent => "Edit the model to reparent its joints",
            _ => "Edit the model to add parts",
        }
    }
}

/// What the status bar says while a joint's new parent is picked.
pub(crate) const REPARENT_HINT: &str = "Click the joint's new parent link · Esc cancels";

/// The hint before a part is chosen.
pub(crate) const PARTS_HINT: &str = "Choose a part, then click in the view";

/// What a click that adds nothing says (status bar).
pub(crate) const NOT_A_LINK: &str = "Click a link of the model being edited";

/// What a joint or collision says before the model's SDFormat comes.
pub(crate) const NO_SDF: &str = "The model's SDFormat hasn't come yet: try again in a moment";

fn tool_icon(t: PartTool) -> IconId {
    match t {
        PartTool::Link => IconId::ShapeBox,
        PartTool::Joint => IconId::OverlayJoints,
        PartTool::Visual => IconId::Eye,
        PartTool::Collision => IconId::OverlayCollision,
        PartTool::Sensor => IconId::ActivitySensors,
        PartTool::Plugin => IconId::Settings,
    }
}

/// The tab: the tiles, three to a row, the selects, the plugin's fields
/// and the hint.
pub(super) fn spawn_parts(c: &mut ChildSpawnerCommands, t: &Theme, state: &AssetsState) {
    let parts = &state.parts;
    c.spawn((
        TabPane(AssetTab::Parts),
        Node {
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[4])),
            display: display(state.tab == AssetTab::Parts),
            ..column(SPACE[4])
        },
    ))
    .with_children(|p| {
        p.spawn(Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::flex(3, 1.0),
            column_gap: px(SPACE[3]),
            row_gap: px(SPACE[3]),
            ..default()
        })
        .with_children(|g| {
            for tool in PartTool::ALL {
                let mut e = g.spawn((part_tile(t, tool), PartTile(tool)));
                if parts.armed == Some(tool) {
                    e.insert(Active);
                }
            }
        });
        let width = 150.0;
        let labels = |v: Vec<&str>| v.into_iter().map(str::to_string).collect::<Vec<_>>();
        let shape = PART_SHAPES.iter().position(|s| *s == parts.shape);
        p.spawn(property_row(
            "Shape",
            (
                select(
                    t,
                    "Part shape",
                    labels(PART_SHAPES.iter().map(|s| s.label()).collect()),
                    shape.unwrap_or(0),
                    width,
                ),
                PartChoice::Shape,
            ),
        ));
        let joint = JOINT_KINDS.iter().position(|k| *k == parts.joint);
        p.spawn(property_row(
            "Joint",
            (
                select(
                    t,
                    "Joint type",
                    labels(JOINT_KINDS.iter().map(|k| joint_label(*k)).collect()),
                    joint.unwrap_or(0),
                    width,
                ),
                PartChoice::Joint,
            ),
        ));
        let sensor = SensorKind::ALL.iter().position(|k| *k == parts.sensor);
        p.spawn(property_row(
            "Sensor",
            (
                select(
                    t,
                    "Sensor type",
                    labels(SensorKind::ALL.iter().map(|k| k.label()).collect()),
                    sensor.unwrap_or(0),
                    width,
                ),
                PartChoice::Sensor,
            ),
        ));
        p.spawn((
            PluginForm,
            Node {
                display: display(parts.armed == Some(PartTool::Plugin)),
                ..column(SPACE[3])
            },
        ))
        .with_children(|f| {
            f.spawn((
                text_field(t, "Plugin library", parts.plugin_file.clone()),
                PluginFile,
            ));
            f.spawn((
                text_field(t, "Plugin class", parts.plugin_name.clone()),
                PluginName,
            ));
            f.spawn((button(t, "Add plugin", ButtonVariant::Normal), PluginAdd));
        });
        p.spawn((
            PartsHint,
            text(TypeStyle::SMALL, ColorRole::Text2, hint(parts.armed, false)),
        ));
    });
}

/// A 64 px tile, as the Shapes tab's.
fn part_tile(theme: &Theme, tool: PartTool) -> impl Bundle {
    let visuals = Visuals::new(ColorRole::Surface, ColorRole::Text).stroked(ColorRole::Line);
    (
        Node {
            height: px(TILE_H),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::MD),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..column(SPACE[2])
        },
        Headless,
        clickable(visuals, Role::Button, format!("Add {}", tool.label())),
        Tooltip::new(format!("Add a {}", tool.label().to_lowercase())).detail(tool.hint()),
        children![
            content_icon(tool_icon(tool), theme.s.icon, ColorRole::Text),
            (
                text(TypeStyle::SMALL, ColorRole::Text, tool.label()),
                Content
            ),
        ],
    )
}

/// The hint for what's armed (`child`: a joint's parent is chosen).
fn hint(armed: Option<PartTool>, child: bool) -> &'static str {
    match armed {
        Some(PartTool::Joint) if child => JOINT_CHILD_HINT,
        Some(t) => t.hint(),
        None => PARTS_HINT,
    }
}

/// The model being edited, if one is.
fn edited(editing: Option<&EditingModel>) -> Option<Entity> {
    editing.and_then(EditingModel::model)
}

/// The Parts tab shows while a model is edited: entering opens it,
/// leaving goes back to Shapes and disarms its parts.
pub(super) fn parts_tab(
    editing: Option<Res<EditingModel>>,
    mut state: ResMut<AssetsState>,
    mut tabs: Query<(&TabChoice, &mut Node)>,
    added: Query<(), Added<TabChoice>>,
) {
    let changed = editing.as_ref().is_some_and(Res::is_changed);
    if !changed && added.is_empty() {
        return;
    }
    let on = edited(editing.as_deref()).is_some();
    for (tab, mut node) in &mut tabs {
        if tab.0 == AssetTab::Parts {
            set_shown(&mut node, on);
        }
    }
    if !changed {
        return;
    }
    if on && state.tab != AssetTab::Parts {
        state.tab = AssetTab::Parts;
    }
    if !on && (state.tab == AssetTab::Parts || state.parts.armed.is_some()) {
        if state.tab == AssetTab::Parts {
            state.tab = AssetTab::Shapes;
        }
        state.parts.armed = None;
        state.parts.parent = None;
        state.parts.reparent = None;
    }
}

/// A joint's parent that is no longer a link of the model being edited
/// (another model is, or undo took it back) is forgotten: the next click
/// chooses the parent again. So is a joint to reparent that the model no
/// longer has.
pub(super) fn forget_parent(model: EditedModel, mut state: ResMut<AssetsState>) {
    if let Some(parent) = state.parts.parent {
        if model.link_of(Some(parent)) != Some(parent) {
            state.parts.parent = None;
        }
    }
    if let Some(r) = &state.parts.reparent {
        let owner = model.at_path(&r.model);
        let still = owner.and_then(|o| model.reparenting(o, &r.joint));
        if still.as_ref() != Some(r) {
            state.parts.reparent = None;
        }
    }
}

/// The armed tile shows active, the hint says what a click does, and the
/// plugin's fields show while Plugin is chosen.
pub(super) fn part_states(
    state: Res<AssetsState>,
    mut tiles: Query<(Entity, &PartTile, Has<Active>, &mut AccessibilityNode)>,
    mut hints: Query<&mut Text, With<PartsHint>>,
    mut forms: Query<&mut Node, With<PluginForm>>,
    added: Query<(), Added<PartTile>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    let parts = &state.parts;
    for (e, tile, active, mut access) in &mut tiles {
        let want = parts.armed == Some(tile.0);
        // Armed, it's pressed to AccessKit too, as a Shapes tile is.
        let pressed = want.then_some(accesskit::Toggled::True);
        if access.toggled() != pressed {
            match pressed {
                Some(t) => access.set_toggled(t),
                None => access.clear_toggled(),
            }
        }
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
    for mut t in &mut hints {
        set_text(&mut t, hint(parts.armed, parts.parent.is_some()));
    }
    for mut node in &mut forms {
        set_shown(&mut node, parts.armed == Some(PartTool::Plugin));
    }
}

/// A Parts tile arms its part, or disarms it.
pub(super) fn on_part_tile(
    ev: On<Activate>,
    tiles: Query<&PartTile, Without<InteractionDisabled>>,
    mut state: ResMut<AssetsState>,
) {
    let Ok(tile) = tiles.get(ev.entity) else {
        return;
    };
    let parts = &mut state.parts;
    parts.armed = if parts.armed == Some(tile.0) {
        None
    } else {
        Some(tile.0)
    };
    parts.parent = None;
    parts.reparent = None;
    state.armed = None;
}

/// A Parts select picks what its part adds.
pub(super) fn on_part_choice(
    ev: On<SelectChanged>,
    choices: Query<&PartChoice>,
    mut state: ResMut<AssetsState>,
) {
    let Ok(choice) = choices.get(ev.entity) else {
        return;
    };
    let parts = &mut state.parts;
    match choice {
        PartChoice::Shape => {
            if let Some(s) = PART_SHAPES.get(ev.index) {
                parts.shape = *s;
            }
        }
        PartChoice::Joint => {
            if let Some(k) = JOINT_KINDS.get(ev.index) {
                parts.joint = *k;
            }
        }
        PartChoice::Sensor => {
            if let Some(k) = SensorKind::ALL.get(ev.index) {
                parts.sensor = *k;
            }
        }
    }
}

/// The plugin's fields' text.
pub(super) fn plugin_text(
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    files: Query<(), With<PluginFile>>,
    names: Query<(), With<PluginName>>,
    mut state: ResMut<AssetsState>,
) {
    for (text, parent) in &inputs {
        let value = text.value().to_string();
        let p = &mut state.parts;
        if files.contains(parent.parent()) && p.plugin_file != value {
            p.plugin_file = value;
        } else if names.contains(parent.parent()) && p.plugin_name != value {
            p.plugin_name = value;
        }
    }
}

/// What adding a part reads of the edited model.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct EditedModel<'w, 's> {
    editing: Option<Res<'w, EditingModel>>,
    entities: Query<'w, 's, (&'static GzEntity, Option<&'static GzSensors>)>,
    named: Query<'w, 's, (Entity, &'static GzEntity)>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    extras: Option<Res<'w, SdfExtras>>,
    parts: Option<Res<'w, EditParts>>,
}

impl EditedModel<'_, '_> {
    /// The model being edited, and its scene path.
    fn model(&self) -> Option<(Entity, String)> {
        let m = edited(self.editing.as_deref())?;
        self.entities.get(m).ok().map(|(g, _)| (m, g.path.clone()))
    }

    /// The edited model's link `hit` is in, if it is in one.
    fn link_of(&self, hit: Option<Entity>) -> Option<Entity> {
        let (model, _) = self.model()?;
        let hit = hit?;
        std::iter::once(hit)
            .chain(self.parents.iter_ancestors(hit))
            .find(|e| {
                self.entities
                    .get(*e)
                    .is_ok_and(|(g, _)| g.kind == NodeKind::Link)
                    && is_under(&self.parents, *e, model)
            })
    }

    fn path(&self, e: Entity) -> Option<String> {
        self.entities.get(e).ok().map(|(g, _)| g.path.clone())
    }

    /// The entity at scene path `path`.
    fn at_path(&self, path: &str) -> Option<Entity> {
        self.named
            .iter()
            .find(|(_, g)| g.path == path)
            .map(|(e, _)| e)
    }

    /// A visual, collision or sensor (`tool`) on link `l`, at `at` in its
    /// frame (a shape `lift` further), named free on the link, as `parts`
    /// chooses.
    fn part_on(
        &self,
        tool: PartTool,
        l: Entity,
        at: Vec3,
        lift: Vec3,
        parts: &PartsState,
    ) -> Option<Part> {
        let link = self.path(l)?;
        let taken = self.link_names(l);
        let free = |base: &str| Draft::free_name(base, |n| taken.iter().any(|t| t == n));
        Some(match tool {
            PartTool::Visual => Part::Visual {
                link,
                name: free("visual"),
                shape: parts.shape,
                pose: pose_at(at + lift),
            },
            PartTool::Collision => Part::Collision {
                link,
                name: free("collision"),
                shape: parts.shape,
                pose: pose_at(at + lift),
            },
            PartTool::Sensor => Part::Sensor {
                link,
                name: free(parts.sensor.ty()),
                kind: parts.sensor,
                pose: pose_at(at),
            },
            PartTool::Link | PartTool::Joint | PartTool::Plugin => return None,
        })
    }

    /// Joint `joint` of `owner` (the edited model or one nested in it), to
    /// reparent: none unless it is in the model being edited and the
    /// server's SDFormat has it.
    fn reparenting(&self, owner: Entity, joint: &str) -> Option<Reparenting> {
        let (model, _) = self.model()?;
        if !is_under(&self.parents, owner, model) {
            return None;
        }
        let path = self.path(owner)?;
        let j = self.known_extras()?.joint(&path, joint)?;
        Some(Reparenting {
            joint: joint.to_string(),
            child: format!("{}/{}", j.model, j.child.replace("::", "/")),
            model: path,
        })
    }

    /// `r`'s joint with link `l` as its parent: a link of the joint's model,
    /// not its parent already, and not one that would close a loop.
    fn reparent(&self, r: &Reparenting, l: Entity) -> Result<Part, String> {
        let (_, top) = self.model().ok_or(NOT_A_LINK)?;
        let to = self.path(l).ok_or(NOT_A_LINK)?;
        if !to.starts_with(&format!("{}/", r.model)) {
            let model = r.model.rsplit('/').next().unwrap_or(&r.model);
            return Err(format!("Click a link of {model}"));
        }
        let (joints, _) = self.joints(&top).ok_or(NO_SDF)?;
        let leaf = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
        if joints.iter().any(|(p, c)| *c == r.child && *p == to) {
            return Err(format!("{} is {}'s parent already", leaf(&to), r.joint));
        }
        let others: Vec<(String, String)> =
            joints.into_iter().filter(|(_, c)| *c != r.child).collect();
        if let Some(why) = joint_problem(&to, &r.child, &others) {
            return Err(why);
        }
        Ok(Part::Reparent {
            joint: r.joint.clone(),
            model: r.model.clone(),
            child: r.child.clone(),
            parent: to,
        })
    }

    /// The names of `e`'s children that are Gazebo entities.
    fn child_names(&self, e: Entity) -> Vec<String> {
        self.children
            .get(e)
            .map(|c| {
                c.iter()
                    .filter_map(|c| self.entities.get(c).ok().map(|(g, _)| g.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Where Gazebo world point `point` (and normal `normal`) is in `e`'s
    /// frame.
    fn local(&self, e: Entity, point: [f64; 3], normal: [f64; 3]) -> Option<(Vec3, Vec3)> {
        let g = self.globals.get(e).ok()?;
        let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        let inverse = g.affine().inverse();
        let at = inverse.transform_point3(gz_to_bevy() * v(point));
        let n = inverse
            .transform_vector3(gz_to_bevy() * v(normal))
            .normalize_or(Vec3::Z);
        Some((at, n))
    }

    /// The world's SDFormat, once the server's has come (the draft's parts
    /// added): until then the model's joints and collisions aren't known.
    fn known_extras(&self) -> Option<&WorldExtras> {
        let x = self.extras.as_deref()?;
        let known = self.parts.as_deref().is_none_or(|p| p.server_known(x));
        known.then_some(x.extras.as_ref()).flatten()
    }

    /// The joints in the model and its nested models, each `(parent,
    /// child)` by scene path (`world` for the world), and the model's own
    /// joints' names; none until the server's SDFormat comes.
    #[allow(clippy::type_complexity)]
    fn joints(&self, model: &str) -> Option<(Vec<(String, String)>, Vec<String>)> {
        let x = self.known_extras()?;
        let prefix = format!("{model}/");
        let joints: Vec<_> = x
            .links
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .flat_map(|(_, l)| &l.joints)
            .filter(|j| j.model == model || j.model.starts_with(&prefix))
            .collect();
        let path = |j: &Joint, name: &str| match name {
            "world" => "world".to_string(),
            _ => format!("{}/{}", j.model, name.replace("::", "/")),
        };
        Some((
            joints
                .iter()
                .map(|j| (path(j, &j.parent), path(j, &j.child)))
                .collect(),
            joints
                .iter()
                .filter(|j| j.model == model)
                .map(|j| j.name.clone())
                .collect(),
        ))
    }

    /// The names link `link`'s visuals, collisions and sensors have (a
    /// collision's only once the server's SDFormat comes).
    fn link_names(&self, link: Entity) -> Vec<String> {
        let mut out = self.child_names(link);
        if let Ok((_, Some(s))) = self.entities.get(link) {
            out.extend(s.0.iter().map(|s| s.name.clone()));
        }
        let path = self.path(link).unwrap_or_default();
        if let Some(l) = self
            .extras
            .as_deref()
            .and_then(|x| x.extras.as_ref())
            .and_then(|x| x.link(&path))
        {
            out.extend(l.collisions.iter().map(|c| c.name.clone()));
            out.extend(l.sensors.iter().map(|s| s.name.clone()));
        }
        out
    }

    /// The rotation (Bevy world) that sets a preview's shape upright in
    /// `e`'s frame, as a part added to it is.
    fn upright_in(&self, e: Entity) -> Option<Quat> {
        let g = self.globals.get(e).ok()?;
        Some(g.rotation() * gz_to_bevy().inverse())
    }
}

/// `at`, its axes the frame's.
fn pose_at(at: Vec3) -> Pose {
    Pose {
        pos: [f64::from(at.x), f64::from(at.y), f64::from(at.z)],
        quat: [0.0, 0.0, 0.0, 1.0],
    }
}

/// Each click while a part (or, editing, a shape) is armed adds it: a
/// link where it lands, resting on the surface; a visual, collision or
/// sensor on the link clicked; a joint's parent, then its child. Shift
/// keeps it armed. A click that can't add says why.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_part(
    time: Res<Time<Real>>,
    model: EditedModel,
    mut places: MessageReader<PlaceAt>,
    mut state: ResMut<AssetsState>,
    mut note: ResMut<StatusNote>,
    mut out: MessageWriter<PartCommand>,
) {
    let Some((edited, path)) = model.model() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    for p in places.read() {
        // Reparenting a joint: the link clicked is its new parent.
        if let Some(r) = state.parts.reparent.clone() {
            let Some(l) = model.link_of(p.hit) else {
                note.show(NOT_A_LINK, now);
                continue;
            };
            match model.reparent(&r, l) {
                Ok(part) => {
                    out.write(PartCommand::Add(part));
                    state.parts.reparent = None;
                }
                Err(why) => note.show(why, now),
            }
            continue;
        }
        let shape_link = match &state.armed {
            Some(Insertable::Shape(s)) if *s != Shape::Plane => Some(*s),
            _ => None,
        };
        let Some(tool) = state.parts.armed.or(shape_link.map(|_| PartTool::Link)) else {
            continue;
        };
        let parts = state.parts.clone();
        let link = model.link_of(p.hit);
        // A collision's name, and a joint's links, need the model's
        // SDFormat: the server's may not have come yet.
        let needs_sdf = matches!(tool, PartTool::Collision | PartTool::Joint);
        if needs_sdf && link.is_some() && model.known_extras().is_none() {
            note.show(NO_SDF, now);
            continue;
        }
        let part = match tool {
            PartTool::Link => {
                let shape = shape_link.unwrap_or(parts.shape);
                let Some((at, n)) = model.local(edited, p.point, p.normal) else {
                    continue;
                };
                let taken = model.child_names(edited);
                #[allow(clippy::cast_possible_truncation)]
                let lift = (LINK_SIZE / 2.0) as f32;
                Some(Part::Link {
                    name: Draft::free_name("link", |n| taken.iter().any(|t| t == n)),
                    shape,
                    pose: pose_at(at + n * lift),
                })
            }
            PartTool::Visual | PartTool::Collision | PartTool::Sensor => link.and_then(|l| {
                let (at, n) = model.local(l, p.point, p.normal)?;
                #[allow(clippy::cast_possible_truncation)]
                let lift = (PART_SIZE / 2.0) as f32;
                model.part_on(tool, l, at, n * lift, &parts)
            }),
            PartTool::Joint => {
                let Some(l) = link else {
                    note.show(NOT_A_LINK, now);
                    continue;
                };
                let Some(parent) = parts.parent else {
                    state.parts.parent = Some(l);
                    continue;
                };
                let (Some(from), Some(to)) = (model.path(parent), model.path(l)) else {
                    continue;
                };
                let Some((joints, names)) = model.joints(&path) else {
                    continue;
                };
                if let Some(why) = joint_problem(&from, &to, &joints) {
                    note.show(why, now);
                    continue;
                }
                state.parts.parent = None;
                Some(Part::Joint {
                    name: Draft::free_name("joint", |n| names.iter().any(|t| t == n)),
                    kind: parts.joint,
                    parent: from,
                    child: to,
                })
            }
            PartTool::Plugin => None,
        };
        let Some(part) = part else {
            note.show(NOT_A_LINK, now);
            continue;
        };
        out.write(PartCommand::Add(part));
        if !p.keep {
            state.parts.armed = None;
            state.armed = None;
        }
    }
}

/// The plugin's Add (or Enter in its class field) adds it, once both
/// fields are filled.
pub(super) fn on_plugin_add(
    ev: On<Activate>,
    adds: Query<(), With<PluginAdd>>,
    state: ResMut<AssetsState>,
    out: MessageWriter<PartCommand>,
) {
    if adds.contains(ev.entity) {
        add_plugin(state, out);
    }
}

/// Enter in the plugin's class field adds it.
pub(super) fn on_plugin_submit(
    ev: On<TextSubmit>,
    names: Query<(), With<PluginName>>,
    mut state: ResMut<AssetsState>,
    out: MessageWriter<PartCommand>,
) {
    if names.contains(ev.entity) {
        state.parts.plugin_name.clone_from(&ev.value);
        add_plugin(state, out);
    }
}

fn add_plugin(mut state: ResMut<AssetsState>, mut out: MessageWriter<PartCommand>) {
    let p = &state.parts;
    let (filename, name) = (p.plugin_file.trim(), p.plugin_name.trim());
    if filename.is_empty() || name.is_empty() {
        return;
    }
    out.write(PartCommand::Add(Part::Plugin {
        name: name.to_string(),
        filename: filename.to_string(),
    }));
    state.parts.armed = None;
}

/// Leaving with parts added says they weren't kept (writing them back
/// comes later).
pub(super) fn parts_discarded(
    time: Res<Time<Real>>,
    mut dropped: MessageReader<PartsDiscarded>,
    mut note: ResMut<StatusNote>,
) {
    for PartsDiscarded(n) in dropped.read() {
        // A reparent is a change, not a part.
        let changes = if *n == 1 { "change" } else { "changes" };
        note.show(
            format!("{n} {changes} not kept: writing parts to the model comes later"),
            time.elapsed_secs_f64(),
        );
    }
}

/// The shape a click would add now, and its size, if it adds one.
fn ghost_of(state: &AssetsState) -> Option<PartGhost> {
    match (&state.armed, state.parts.armed) {
        (Some(Insertable::Shape(s)), _) if *s != Shape::Plane => Some(PartGhost {
            shape: *s,
            size: LINK_SIZE,
        }),
        (_, Some(PartTool::Link)) => Some(PartGhost {
            shape: state.parts.shape,
            size: LINK_SIZE,
        }),
        (_, Some(PartTool::Visual | PartTool::Collision)) => Some(PartGhost {
            shape: state.parts.shape,
            size: PART_SIZE,
        }),
        _ => None,
    }
}

/// While a model is edited, the preview of the link, visual or collision
/// a click would add follows the pointer, resting on the surface, upright
/// in the model (a link) or the link under the pointer.
#[allow(clippy::too_many_arguments)]
pub(super) fn part_ghost(
    model: EditedModel,
    state: Res<AssetsState>,
    hover: Res<PlaceHover>,
    theme: Res<Theme>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut ghosts: Query<(Entity, &PartGhost, &mut Transform, &mut Visibility)>,
    mut commands: Commands,
) {
    let edited = model.model().map(|(m, _)| m);
    let want = ghost_of(&state).filter(|_| edited.is_some());
    let on_link = matches!(
        state.parts.armed,
        Some(PartTool::Visual | PartTool::Collision)
    );
    for (e, g, mut t, mut vis) in &mut ghosts {
        if Some(*g) != want {
            commands.entity(e).despawn();
            continue;
        }
        match hover.0 {
            Some((at, normal, hit)) => {
                #[allow(clippy::cast_possible_truncation)]
                let to = at + normal * (g.size / 2.0) as f32;
                let frame = if on_link { model.link_of(hit) } else { None };
                let up = frame
                    .or(edited)
                    .and_then(|f| model.upright_in(f))
                    .unwrap_or(Quat::IDENTITY);
                if t.translation != to || t.rotation != up {
                    t.translation = to;
                    t.rotation = up;
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    let (Some(want), Some(mut meshes), Some(mut materials)) = (want, meshes, materials) else {
        return;
    };
    if ghosts.iter().any(|(_, g, ..)| *g == want) {
        return;
    }
    let mut color = theme.color(ColorRole::Accent);
    color.set_alpha(0.35);
    let material = materials.add(StandardMaterial {
        base_color: color,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    #[allow(clippy::cast_possible_truncation)]
    let s = want.size as f32;
    let mesh: Mesh = match want.shape {
        Shape::Box | Shape::Plane => Cuboid::new(s, s, s).into(),
        Shape::Sphere => Sphere::new(s / 2.0).into(),
        Shape::Cylinder => Cylinder::new(s / 2.0, s).into(),
        Shape::Capsule => Capsule3d::new(0.2 * s, 0.6 * s).into(),
        Shape::Cone => Cone::new(s / 2.0, s).into(),
    };
    commands.spawn((
        want,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        NoViewPick,
        NotShadowCaster,
        Name::new("Part preview"),
    ));
}

/// Choosing a joint's child, a line in `entity_joint` runs from its
/// parent link to the pointer; choosing a joint's new parent, from its
/// child link.
pub(super) fn joint_band(
    state: Res<AssetsState>,
    hover: Res<PlaceHover>,
    theme: Res<Theme>,
    model: EditedModel,
    gizmos: Option<Gizmos<JointGizmos>>,
) {
    let child = || {
        let r = state.parts.reparent.as_ref()?;
        model.at_path(&r.child)
    };
    let (Some(mut gizmos), Some(from)) = (gizmos, state.parts.parent.or_else(child)) else {
        return;
    };
    let Ok(from) = model.globals.get(from).map(GlobalTransform::translation) else {
        return;
    };
    let color = theme.color(ColorRole::EntityJoint);
    gizmos.sphere(Isometry3d::from_translation(from), 0.04, color);
    if let Some((to, ..)) = hover.0 {
        gizmos.line(from, to, color);
    }
}

/// An inspector's [`PartAction`]: a visual, collision or sensor on the
/// inspected link, or the inspected joint's next click picking its parent.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_part_action(
    ev: On<Activate>,
    actions: Query<&PartAction, Without<InteractionDisabled>>,
    inspected: Res<Inspected>,
    time: Res<Time<Real>>,
    model: EditedModel,
    mut state: ResMut<AssetsState>,
    mut note: ResMut<StatusNote>,
    mut out: MessageWriter<PartCommand>,
) {
    let Ok(action) = actions.get(ev.entity) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let tool = match action {
        PartAction::Visual => PartTool::Visual,
        PartAction::Collision => PartTool::Collision,
        PartAction::Sensor => PartTool::Sensor,
        PartAction::Reparent => {
            let (Some(owner), Some(joint)) = (inspected.entity, inspected.part.as_ref()) else {
                return;
            };
            let Some(r) = model.reparenting(owner, &joint.name) else {
                note.show(NO_SDF, now);
                return;
            };
            let parts = &mut state.parts;
            parts.armed = None;
            parts.parent = None;
            parts.reparent = Some(r);
            state.armed = None;
            return;
        }
    };
    let Some(link) = inspected
        .entity
        .filter(|l| model.link_of(Some(*l)) == Some(*l))
    else {
        note.show(NOT_A_LINK, now);
        return;
    };
    if tool == PartTool::Collision && model.known_extras().is_none() {
        note.show(NO_SDF, now);
        return;
    }
    if let Some(part) = model.part_on(tool, link, Vec3::ZERO, Vec3::ZERO, &state.parts) {
        out.write(PartCommand::AddKept(part));
    }
}

/// The inspector's [`PartAction`]s run while their link or joint is in
/// the model being edited; outside it they say to edit it first.
#[allow(clippy::type_complexity)]
pub(super) fn part_action_states(
    inspected: Res<Inspected>,
    model: EditedModel,
    added: Query<(), Added<PartAction>>,
    mut actions: Query<(Entity, &PartAction, &mut Disabled, &mut Tooltip)>,
    mut commands: Commands,
) {
    let changed = inspected.is_changed()
        || model.editing.as_ref().is_some_and(Res::is_changed)
        || model.extras.as_ref().is_some_and(Res::is_changed);
    if !changed && added.is_empty() {
        return;
    }
    for (e, action, mut disabled, mut tooltip) in &mut actions {
        let live = match action {
            PartAction::Reparent => inspected
                .entity
                .zip(inspected.part.as_ref())
                .is_some_and(|(o, j)| model.reparenting(o, &j.name).is_some()),
            _ => inspected
                .entity
                .is_some_and(|l| model.link_of(Some(l)) == Some(l)),
        };
        let off = !live || inspected.removed;
        if disabled.0 != off {
            disabled.0 = off;
            if off {
                commands.entity(e).insert(InteractionDisabled);
            } else {
                commands.entity(e).remove::<InteractionDisabled>();
            }
        }
        let detail = match (live, inspected.removed) {
            (false, _) => Some(action.unavailable().to_string()),
            (true, true) => Some("It was removed".to_string()),
            (true, false) => None,
        };
        if tooltip.detail != detail {
            tooltip.detail = detail;
        }
    }
}

/// The Parts tab's systems, while the panel shows.
pub(super) fn tab_systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (part_states, plugin_text).chain()
}

/// Registers the tab's observers.
pub(super) fn observers(app: &mut App) {
    app.add_observer(on_part_tile)
        .add_observer(on_part_action)
        .add_observer(on_part_choice)
        .add_observer(on_plugin_add)
        .add_observer(on_plugin_submit);
}
