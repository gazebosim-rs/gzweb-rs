//! The Assets panel's systems: arming and placing, the `create` requests
//! and their answers, the Fuel search, the preview in the views (always
//! on), and while it shows, the tabs, tiles, results and inserts in place
//! (DECISIONS.md D-102).

use bevy::a11y::AccessibilityNode;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::assets::asset_path;
use gz_bevy::connection::{GzCommand, GzServiceReply, GzStatus};
use gz_bevy::edit::{create_reply, create_request, edits_allowed};
use gz_bevy::fuel::{FuelSearch, FuelState, GzFuel};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::gizmo::GizmoState;
use gz_bevy::view::place::{PlaceAt, PlaceHover, Placing};
use gz_bevy::view::NoViewPick;
use gz_core::fuel::FuelModel;
use gz_core::insert::{model_name, Insertable, LightKind, Shape};

use super::model::{AssetsState, Inserts, SEARCH_DELAY_SECS};
use super::view::{
    fuel_row, insert_row, AssetsNote, Dismiss, FuelBar, FuelButton, FuelButtonBox, FuelInsert,
    FuelMessage, FuelQuery, FuelRows, Indeterminate, InsertRows, LocalInsert, LocalUri, RowBar,
    TabChoice, TabPane, Tile,
};
use super::{AssetsStart, ASSETS};
use crate::shell::{CurrentPhase, Phase, Slot, UiCommand};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Theme};
use crate::widgets::layout::set_shown;
use crate::widgets::text::set_text;
use crate::widgets::TextSubmit;

/// Why nothing can be inserted, when that holds.
pub(crate) const NOT_CONNECTED: &str = "Not connected: connect to insert into a world";
pub(crate) const NO_EDITS: &str =
    "This server doesn't take edits, so nothing can be inserted (it needs the patched \
     websocket server)";

/// What the panel's message says before a search.
pub(crate) const SEARCH_PROMPT: &str = "Search Fuel for models";
pub(crate) const UNREACHABLE: &str = "Fuel is unreachable";
pub(crate) const NO_MATCHES: &str = "No models match";

/// How fast a bar's segment crosses, in widths a second.
const BAR_SPEED: f32 = 0.9;

/// What the Fuel rows and the insert rows were built for.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct BuiltRows {
    fuel: Option<Vec<String>>,
    inserts: Option<Vec<(u32, String, bool)>>,
}

/// Whether the world takes inserts now, and if not, why.
#[derive(SystemParam)]
pub(super) struct Inserting<'w> {
    phase: Res<'w, CurrentPhase>,
    status: Option<Res<'w, GzStatus>>,
}

impl Inserting<'_> {
    /// The world inserts go to, or why there's none.
    pub(super) fn world(&self) -> Result<String, &'static str> {
        if self.phase.0 != Phase::Live {
            return Err(NOT_CONNECTED);
        }
        let status = self.status.as_deref();
        if !edits_allowed(status) {
            return Err(NO_EDITS);
        }
        status.and_then(|s| s.world.clone()).ok_or(NOT_CONNECTED)
    }

    fn changed(&self) -> bool {
        self.phase.is_changed() || self.status.as_ref().is_some_and(Res::is_changed)
    }
}

/// An [`AssetsStart`]: opens the panel on its tab, and searches its
/// query.
pub(super) fn apply_start(
    start: Option<Res<AssetsStart>>,
    mut state: ResMut<AssetsState>,
    mut ui: MessageWriter<UiCommand>,
    mut searches: MessageWriter<FuelSearch>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    ui.write(UiCommand::OpenPanel {
        panel: ASSETS,
        slot: Slot::Primary,
    });
    state.tab = start.tab;
    if !start.query.is_empty() {
        state.query.clone_from(&start.query);
        searches.write(FuelSearch {
            query: start.query.clone(),
            page: 1,
        });
    }
    commands.remove_resource::<AssetsStart>();
}

/// Clicks in the views place while something is armed; Esc, or a world
/// that can't take it, disarms.
pub(super) fn arm(
    keys: Res<ButtonInput<KeyCode>>,
    inserting: Inserting,
    gizmo: Option<Res<GizmoState>>,
    mut state: ResMut<AssetsState>,
    mut placing: ResMut<Placing>,
) {
    // A gizmo drag takes this Esc (it puts the selection back): one step
    // per Esc.
    let esc = keys.just_pressed(KeyCode::Escape) && !gizmo.is_some_and(|g| g.takes_escape());
    if state.armed.is_some() && (esc || inserting.world().is_err()) {
        state.armed = None;
    }
    placing.set_if_neq(Placing(state.armed.is_some()));
}

/// Each place sends a `create` for what is armed, at the point; Shift
/// keeps it armed.
pub(super) fn place(
    time: Res<Time<Real>>,
    inserting: Inserting,
    index: Option<Res<GzIndex>>,
    mut places: MessageReader<PlaceAt>,
    mut state: ResMut<AssetsState>,
    mut inserts: ResMut<Inserts>,
    mut out: MessageWriter<GzCommand>,
) {
    for p in places.read() {
        let (Some(what), Ok(world)) = (state.armed.clone(), inserting.world()) else {
            continue;
        };
        let taken = index
            .as_deref()
            .map(|i| {
                scene_names(i)
                    .filter(|n| what.is_named(n))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        out.write(create_request(&world, &what, p.point, p.normal));
        inserts.sent(what, time.elapsed_secs_f64(), taken);
        if !p.keep {
            state.armed = None;
        }
    }
}

/// The scene's model and light names.
fn scene_names(index: &GzIndex) -> impl Iterator<Item = &str> + Clone {
    index.by_name.keys().map(String::as_str)
}

/// The server's answers to `create`, in order.
pub(super) fn replies(
    time: Res<Time<Real>>,
    mut replies: MessageReader<GzServiceReply>,
    mut inserts: ResMut<Inserts>,
) {
    for ok in replies.read().filter_map(create_reply) {
        inserts.answered(ok, time.elapsed_secs_f64());
    }
}

/// An accepted insert ends when it shows in the scene; one that waits too
/// long fails.
pub(super) fn settle(
    time: Res<Time<Real>>,
    index: Option<Res<GzIndex>>,
    mut inserts: ResMut<Inserts>,
    mut ticked: Local<f64>,
) {
    let waiting = inserts
        .bypass_change_detection()
        .list
        .iter()
        .any(|i| !i.failed());
    if !waiting {
        return;
    }
    let now = time.elapsed_secs_f64();
    // The scene changes only on a spawn or deletion, and the timeouts
    // need a second's look at most.
    let looked = index.as_ref().is_some_and(Res::is_changed) || inserts.is_changed();
    if !looked && now - *ticked < 1.0 {
        return;
    }
    *ticked = now;
    let names: Vec<&str> = index
        .as_deref()
        .map(|i| scene_names(i).collect())
        .unwrap_or_default();
    let before = inserts.bypass_change_detection().clone();
    let mut after = before.clone();
    after.settle(names.iter().copied(), now);
    if after != before {
        *inserts = after;
    }
}

/// Searches Fuel a moment after the last keystroke in its field.
pub(super) fn search_typed(
    time: Res<Time<Real>>,
    mut state: ResMut<AssetsState>,
    mut searches: MessageWriter<FuelSearch>,
) {
    let Some(at) = state.typed_at else {
        return;
    };
    if time.elapsed_secs_f64() - at >= SEARCH_DELAY_SECS {
        state.typed_at = None;
        searches.write(FuelSearch {
            query: state.query.clone(),
            page: 1,
        });
    }
}

/// The preview: what is armed, at the point under the pointer, in
/// translucent `accent` (a ring for a model, whose shape isn't known).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ghost(pub Insertable);

/// The preview follows the pointer while something is armed.
#[allow(clippy::too_many_arguments)]
pub(super) fn ghost(
    state: Res<AssetsState>,
    hover: Res<PlaceHover>,
    theme: Res<Theme>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut ghosts: Query<(
        Entity,
        &Ghost,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut commands: Commands,
) {
    let want = state.armed.as_ref();
    let color = ghost_color(&theme);
    for (e, g, mut t, mut vis, material) in &mut ghosts {
        if Some(&g.0) != want {
            commands.entity(e).despawn();
            continue;
        }
        // A new theme's accent.
        if theme.is_changed() {
            if let Some(mut m) = materials.as_mut().and_then(|m| m.get_mut(&material.0)) {
                m.base_color = color;
            }
        }
        match hover.0 {
            Some((at, normal)) => {
                // Off the surface along its normal, upright as it will be
                // inserted.
                #[allow(clippy::cast_possible_truncation)]
                let lift = g.0.lift() as f32;
                let to = at + normal * lift;
                if t.translation != to {
                    t.translation = to;
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    let (Some(what), Some(mut meshes), Some(mut materials)) = (want, meshes, materials) else {
        return;
    };
    if ghosts.iter().any(|(_, g, ..)| Some(&g.0) == want) {
        return;
    }
    let material = materials.add(StandardMaterial {
        base_color: color,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.spawn((
        Ghost(what.clone()),
        Mesh3d(meshes.add(ghost_mesh(what))),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        NoViewPick,
        NotShadowCaster,
        Name::new("Insert preview"),
    ));
}

/// The preview's colour: `accent`, translucent.
fn ghost_color(theme: &Theme) -> Color {
    let mut color = theme.color(ColorRole::Accent);
    color.set_alpha(0.35);
    color
}

/// The preview's mesh, in Bevy's axes (Y up, as Gazebo's Z).
fn ghost_mesh(what: &Insertable) -> Mesh {
    match what {
        Insertable::Shape(Shape::Box) => Cuboid::new(1.0, 1.0, 1.0).into(),
        Insertable::Shape(Shape::Sphere) => Sphere::new(0.5).into(),
        Insertable::Shape(Shape::Cylinder) => Cylinder::new(0.5, 1.0).into(),
        Insertable::Shape(Shape::Capsule) => Capsule3d::new(0.2, 0.6).into(),
        Insertable::Shape(Shape::Cone) => Cone::new(0.5, 1.0).into(),
        Insertable::Shape(Shape::Plane) => Plane3d::new(Vec3::Y, Vec2::splat(1.0)).into(),
        Insertable::Light(LightKind::Directional | LightKind::Point | LightKind::Spot) => {
            Sphere::new(0.15).into()
        }
        Insertable::Model { .. } => Torus::new(0.35, 0.45).into(),
    }
}

/// The Fuel field's text is the query (searched a moment later); the
/// Local field's is the URI.
pub(super) fn query_text(
    time: Res<Time<Real>>,
    inputs: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    queries: Query<(), With<FuelQuery>>,
    uris: Query<(), With<LocalUri>>,
    mut state: ResMut<AssetsState>,
) {
    for (text, parent) in &inputs {
        let value = text.value().to_string();
        if queries.contains(parent.parent()) && state.query != value {
            state.query = value;
            state.typed_at = Some(time.elapsed_secs_f64());
        } else if uris.contains(parent.parent()) && state.uri != value {
            state.uri = value;
        }
    }
}

/// The chosen tab is checked and its content shows.
pub(super) fn tab_states(
    state: Res<AssetsState>,
    tabs: Query<(Entity, &TabChoice, Has<Checked>)>,
    mut panes: Query<(&TabPane, &mut Node)>,
    added: Query<(), Added<TabChoice>>,
    mut commands: Commands,
) {
    if !state.is_changed() && added.is_empty() {
        return;
    }
    for (e, tab, checked) in &tabs {
        let want = tab.0 == state.tab;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
    for (pane, mut node) in &mut panes {
        set_shown(&mut node, pane.0 == state.tab);
    }
}

/// The armed tile shows active; tiles and Insert buttons are inert while
/// the world can't take an insert.
#[allow(clippy::type_complexity)]
pub(super) fn tile_states(
    state: Res<AssetsState>,
    inserting: Inserting,
    mut tiles: Query<(Entity, &Tile, Has<Active>, &mut AccessibilityNode)>,
    buttons: Query<
        (Entity, Has<InteractionDisabled>),
        Or<(With<Tile>, With<FuelInsert>, With<LocalInsert>)>,
    >,
    added: Query<(), Or<(Added<Tile>, Added<FuelInsert>)>>,
    mut commands: Commands,
) {
    if !state.is_changed() && !inserting.changed() && added.is_empty() {
        return;
    }
    for (e, tile, active, mut access) in &mut tiles {
        let want = state.armed.as_ref() == Some(&tile.0);
        // Armed, it's pressed to AccessKit too.
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
    let can = inserting.world().is_ok();
    for (e, disabled) in &buttons {
        if can == disabled {
            if can {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}

/// Says why nothing can be inserted, while that holds.
pub(super) fn note(
    inserting: Inserting,
    mut notes: Query<(&mut Text, &mut Node), With<AssetsNote>>,
    added: Query<(), Added<AssetsNote>>,
) {
    if !inserting.changed() && added.is_empty() {
        return;
    }
    let why = inserting.world().err();
    for (mut text, mut node) in &mut notes {
        set_text(&mut text, why.unwrap_or_default());
        set_shown(&mut node, why.is_some());
    }
}

/// The Fuel tab's bar while searching, its message (the prompt, "No
/// models match", "Fuel is unreachable" with Retry), and Load more while
/// there may be more.
#[allow(clippy::type_complexity)]
pub(super) fn fuel_states(
    fuel: Res<GzFuel>,
    mut bars: Query<&mut Node, (With<FuelBar>, Without<FuelMessage>, Without<FuelButtonBox>)>,
    mut messages: Query<(&mut Text, &mut Node), (With<FuelMessage>, Without<FuelButtonBox>)>,
    mut boxes: Query<(&FuelButtonBox, &mut Node), Without<FuelMessage>>,
    added: Query<(), Added<FuelBar>>,
) {
    if !fuel.is_changed() && added.is_empty() {
        return;
    }
    let searching = fuel.state == FuelState::Searching;
    for mut node in &mut bars {
        set_shown(&mut node, searching);
    }
    let message = match &fuel.state {
        FuelState::Failed {
            unreachable: true, ..
        } => Some(UNREACHABLE.to_string()),
        FuelState::Failed { message, .. } => {
            Some(format!("Fuel answered with an error: {message}"))
        }
        FuelState::Idle if fuel.query.is_empty() => Some(SEARCH_PROMPT.to_string()),
        FuelState::Idle if fuel.models.is_empty() => Some(NO_MATCHES.to_string()),
        FuelState::Searching | FuelState::Idle => None,
    };
    for (mut text, mut node) in &mut messages {
        set_text(&mut text, message.as_deref().unwrap_or_default());
        set_shown(&mut node, message.is_some());
    }
    let failed = matches!(fuel.state, FuelState::Failed { .. });
    for (b, mut node) in &mut boxes {
        let show = match b.0 {
            FuelButton::Retry => failed,
            FuelButton::More => fuel.more && fuel.state == FuelState::Idle,
        };
        set_shown(&mut node, show);
    }
}

/// The Fuel results' rows, rebuilt when the results change; thumbnails
/// load through the HTTPS asset source.
pub(super) fn sync_fuel_rows(
    theme: Res<Theme>,
    fuel: Res<GzFuel>,
    inserts: Res<Inserts>,
    server: Res<AssetServer>,
    lists: Query<(Entity, Ref<FuelRows>)>,
    mut built: ResMut<BuiltRows>,
    mut commands: Commands,
) {
    let Ok((list, added)) = lists.single() else {
        return;
    };
    if !added.is_added() && !fuel.is_changed() {
        return;
    }
    let key: Vec<String> = fuel.models.iter().map(FuelModel::uri).collect();
    if !added.is_added() && built.fuel.as_ref() == Some(&key) {
        return;
    }
    commands.entity(list).despawn_children();
    let rows: Vec<_> = fuel
        .models
        .iter()
        .map(|m| {
            let thumb = m.thumbnail().map(|u| server.load(asset_path(&u)));
            fuel_row(&theme, m, thumb, inserts.loading(&m.uri()))
        })
        .collect();
    commands.entity(list).with_children(|l| {
        for row in rows {
            l.spawn(row);
        }
    });
    built.fuel = Some(key);
}

/// A result's bar shows while its model is on its way.
pub(super) fn row_bars(inserts: Res<Inserts>, mut bars: Query<(&RowBar, &mut Node)>) {
    if !inserts.is_changed() {
        return;
    }
    for (bar, mut node) in &mut bars {
        set_shown(&mut node, inserts.loading(&bar.0));
    }
}

/// The inserts' rows, rebuilt when one changes stage.
pub(super) fn sync_inserts(
    theme: Res<Theme>,
    inserts: Res<Inserts>,
    lists: Query<(Entity, Ref<InsertRows>)>,
    mut built: ResMut<BuiltRows>,
    mut commands: Commands,
) {
    let Ok((list, added)) = lists.single() else {
        return;
    };
    let key: Vec<(u32, String, bool)> = inserts
        .list
        .iter()
        .map(|i| (i.id, i.status(), i.failed()))
        .collect();
    if !added.is_added() && built.inserts.as_ref() == Some(&key) {
        return;
    }
    commands.entity(list).despawn_children();
    let rows: Vec<_> = inserts
        .list
        .iter()
        .map(|i| insert_row(&theme, i.id, i.label(), i.status(), i.failed()))
        .collect();
    commands.entity(list).with_children(|l| {
        for row in rows {
            l.spawn(row);
        }
    });
    built.inserts = Some(key);
}

/// Moves each bar's segment across, and round again.
pub(super) fn move_bars(
    time: Res<Time<Real>>,
    mut segments: Query<(&mut Node, &ChildOf), With<Indeterminate>>,
    bars: Query<&ComputedNode>,
) {
    let t = time.elapsed_secs() * BAR_SPEED;
    // From off the left (-30%) to off the right (100%).
    let left = (t.fract() * 130.0) - 30.0;
    for (mut node, parent) in &mut segments {
        // A hidden bar's (or tab's) has no width: leave its layout be.
        if bars.get(parent.parent()).is_ok_and(|b| b.size().x > 0.0) {
            node.left = percent(left);
        }
    }
}

/// A tab click picks the tab.
pub(super) fn on_tab(
    ev: On<ValueChange<Entity>>,
    tabs: Query<&TabChoice>,
    mut state: ResMut<AssetsState>,
) {
    if let Ok(tab) = tabs.get(ev.value) {
        if state.tab != tab.0 {
            state.tab = tab.0;
        }
    }
}

/// Arms `what`, or disarms it if it already is.
fn toggle_armed(state: &mut AssetsState, what: Insertable) {
    state.armed = if state.armed.as_ref() == Some(&what) {
        None
    } else {
        Some(what)
    };
}

/// A tile arms its shape or light.
pub(super) fn on_tile(ev: On<Activate>, tiles: Query<&Tile>, mut state: ResMut<AssetsState>) {
    if let Ok(tile) = tiles.get(ev.entity) {
        toggle_armed(&mut state, tile.0.clone());
    }
}

/// A result's Insert says which model it inserts, to AccessKit.
pub(super) fn label_insert(
    ev: On<Add<FuelInsert>>,
    mut buttons: Query<(&FuelInsert, &mut AccessibilityNode)>,
) {
    if let Ok((insert, mut access)) = buttons.get_mut(ev.entity) {
        access.set_label(format!("Insert {}", insert.0.name));
    }
}

/// A result's Insert arms its model.
pub(super) fn on_fuel_insert(
    ev: On<Activate>,
    rows: Query<&FuelInsert>,
    mut state: ResMut<AssetsState>,
) {
    if let Ok(row) = rows.get(ev.entity) {
        let model = &row.0;
        toggle_armed(
            &mut state,
            Insertable::Model {
                uri: model.uri(),
                name: model_name(&model.name),
            },
        );
    }
}

/// Retry searches again; Load more asks for the next page.
pub(super) fn on_fuel_button(
    ev: On<Activate>,
    buttons: Query<&FuelButton>,
    fuel: Res<GzFuel>,
    state: Res<AssetsState>,
    mut searches: MessageWriter<FuelSearch>,
) {
    let Ok(b) = buttons.get(ev.entity) else {
        return;
    };
    let query = if fuel.query.is_empty() {
        state.query.clone()
    } else {
        fuel.query.clone()
    };
    // Retry asks again for the page that failed; Load more for the next.
    let _ = b;
    searches.write(FuelSearch {
        query,
        page: fuel.page + 1,
    });
}

/// A model by URI, named after its last part.
pub(super) fn local_model(uri: &str) -> Option<Insertable> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    // The model's directory, past a model file, a Fuel version (`3`,
    // `tip`) or Fuel's `files`.
    let skip = |s: &str| {
        s.is_empty()
            || s.strip_prefix("model.")
                .is_some_and(|ext| ext == "sdf" || ext == "config")
            || s == "tip"
            || s == "files"
            || s.chars().all(|c| c.is_ascii_digit())
    };
    let last = uri.rsplit(['/', ':']).find(|s| !skip(s)).unwrap_or(uri);
    let last = last.strip_suffix(".sdf").unwrap_or(last);
    Some(Insertable::Model {
        uri: uri.to_string(),
        name: model_name(&last.replace("%20", " ")),
    })
}

/// The Local tab's Insert arms its URI's model.
pub(super) fn on_local_insert(
    ev: On<Activate>,
    buttons: Query<(), With<LocalInsert>>,
    mut state: ResMut<AssetsState>,
) {
    if buttons.contains(ev.entity) {
        if let Some(what) = local_model(&state.uri) {
            toggle_armed(&mut state, what);
        }
    }
}

/// Enter in the Fuel field searches now; in the Local field it arms.
pub(super) fn on_submit(
    ev: On<TextSubmit>,
    queries: Query<(), With<FuelQuery>>,
    uris: Query<(), With<LocalUri>>,
    mut state: ResMut<AssetsState>,
    mut searches: MessageWriter<FuelSearch>,
) {
    if queries.contains(ev.entity) {
        state.query.clone_from(&ev.value);
        state.typed_at = None;
        searches.write(FuelSearch {
            query: ev.value.clone(),
            page: 1,
        });
    } else if uris.contains(ev.entity) {
        state.uri.clone_from(&ev.value);
        if let Some(what) = local_model(&ev.value) {
            state.armed = Some(what);
        }
    }
}

/// A failed insert's dismiss button removes its row.
pub(super) fn on_dismiss(ev: On<Activate>, dismiss: Query<&Dismiss>, mut inserts: ResMut<Inserts>) {
    if let Ok(d) = dismiss.get(ev.entity) {
        inserts.dismiss(d.0);
    }
}
