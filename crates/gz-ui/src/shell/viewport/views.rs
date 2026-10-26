//! Split views (spec §3.4, §7.4; DECISIONS.md D-90, D-91): the layout
//! presets, the grid of view nodes they make with the dividers between
//! them, the floor rule for several views (§2.1), Maximize and Close view,
//! and finding a view's camera from its UI.
//!
//! Each view is a node ([`ViewNode`]) naming its view ([`ViewId`]); its
//! camera renders into a texture the node shows (§9.3), and its controls
//! (cube, rail, pills) are its children. The grid is flex rows and columns,
//! so the views and their 2 px gaps (`viewport_divider`, the area's `bg`)
//! are laid out in the frame the area is; a divider's 6 px hit area
//! (`viewport_divider_hit`, a [`Splitter`]) drags its share, or moves it
//! with the keys, double-click resets it, and it turns `accent` while
//! dragging. Views the layout drops keep their cameras, off, so a preset
//! brought back shows them as they were.

use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use gz_bevy::camera::MainCamera;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{ViewCamera, ViewId, ViewSlots};

use super::view::ViewNode;
use crate::shell::layout::ShellLayout;
use crate::shell::regions::pass_through;
use crate::theme::{ColorRole, Fill, Theme};
use crate::widgets::fade::{fade_tree, FadeQueries};
use crate::widgets::layout::{column, row};
use crate::widgets::splitter::SplitterDrag;
use crate::widgets::{splitter_control, SplitAxis, Splitter, SplitterMoved, SplitterReset};

/// A layout preset (§3.4, §7.4).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutPreset {
    Single,
    TwoColumns,
    TwoRows,
    OnePlusTwo,
    Quad,
}

impl LayoutPreset {
    pub(crate) const ALL: [LayoutPreset; 5] = [
        LayoutPreset::Single,
        LayoutPreset::TwoColumns,
        LayoutPreset::TwoRows,
        LayoutPreset::OnePlusTwo,
        LayoutPreset::Quad,
    ];

    /// Its command-line name (`gzweb --layout`).
    pub fn id(self) -> &'static str {
        match self {
            LayoutPreset::Single => "single",
            LayoutPreset::TwoColumns => "two-columns",
            LayoutPreset::TwoRows => "two-rows",
            LayoutPreset::OnePlusTwo => "one-plus-two",
            LayoutPreset::Quad => "quad",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            LayoutPreset::Single => "Single",
            LayoutPreset::TwoColumns => "Two columns",
            LayoutPreset::TwoRows => "Two rows",
            LayoutPreset::OnePlusTwo => "One plus two",
            LayoutPreset::Quad => "Quad",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// How many views it shows.
    pub(crate) fn count(self) -> u8 {
        match self {
            LayoutPreset::Single => 1,
            LayoutPreset::TwoColumns | LayoutPreset::TwoRows => 2,
            LayoutPreset::OnePlusTwo => 3,
            LayoutPreset::Quad => 4,
        }
    }

    /// Its columns and rows at the most (for the floor rule).
    fn grid(self) -> (u8, u8) {
        match self {
            LayoutPreset::Single => (1, 1),
            LayoutPreset::TwoColumns => (2, 1),
            LayoutPreset::TwoRows => (1, 2),
            LayoutPreset::OnePlusTwo | LayoutPreset::Quad => (2, 2),
        }
    }

    /// The column and row dividers' default shares: Two columns is 3:2,
    /// the rest even (§3.4).
    pub(crate) fn default_shares(self) -> [f32; 2] {
        match self {
            LayoutPreset::TwoColumns => [0.6, 0.5],
            _ => [0.5, 0.5],
        }
    }

    /// Its views as (left, top, width, height) in percent, at the default
    /// shares (the Layout popover's thumbnails).
    pub(crate) fn panes(self) -> Vec<[f32; 4]> {
        let [c, r] = self.default_shares().map(|s| s * 100.0);
        match self {
            LayoutPreset::Single => vec![[0.0, 0.0, 100.0, 100.0]],
            LayoutPreset::TwoColumns => vec![[0.0, 0.0, c, 100.0], [c, 0.0, 100.0 - c, 100.0]],
            LayoutPreset::TwoRows => vec![[0.0, 0.0, 100.0, r], [0.0, r, 100.0, 100.0 - r]],
            LayoutPreset::OnePlusTwo => vec![
                [0.0, 0.0, c, 100.0],
                [c, 0.0, 100.0 - c, r],
                [c, r, 100.0 - c, 100.0 - r],
            ],
            LayoutPreset::Quad => vec![
                [0.0, 0.0, c, r],
                [c, 0.0, 100.0 - c, r],
                [0.0, r, c, 100.0 - r],
                [c, r, 100.0 - c, 100.0 - r],
            ],
        }
    }

    /// The preset with one view fewer (Close view).
    fn without_one(self) -> LayoutPreset {
        match self {
            LayoutPreset::Quad => LayoutPreset::OnePlusTwo,
            LayoutPreset::OnePlusTwo => LayoutPreset::TwoColumns,
            _ => LayoutPreset::Single,
        }
    }

    /// The preset a split right or down makes of this one.
    fn split(self, down: bool) -> LayoutPreset {
        match (self, down) {
            (LayoutPreset::Single, false) => LayoutPreset::TwoColumns,
            (LayoutPreset::Single, true) => LayoutPreset::TwoRows,
            (LayoutPreset::TwoColumns, true) => LayoutPreset::OnePlusTwo,
            (LayoutPreset::TwoRows | LayoutPreset::OnePlusTwo, _) => LayoutPreset::Quad,
            (p, _) => p,
        }
    }
}

impl std::str::FromStr for LayoutPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL.into_iter().find(|p| p.id() == s).ok_or_else(|| {
            let names: Vec<&str> = Self::ALL.iter().map(|p| p.id()).collect();
            format!("unknown layout '{s}' ({})", names.join(", "))
        })
    }
}

/// The layout to start in (`gzweb --layout`, for a capture); applied
/// once, then removed.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartLayout(pub LayoutPreset);

/// Applies [`StartLayout`].
fn apply_start_layout(
    start: Option<Res<StartLayout>>,
    mut layout: ResMut<ViewLayout>,
    mut commands: Commands,
) {
    if let Some(start) = start {
        layout.set_preset(start.0);
        commands.remove_resource::<StartLayout>();
    }
}

/// The viewport's layout (§3.4): the preset, each preset's divider shares
/// (column, row), a maximized view, and whether the floor rule has
/// fallen back to Single (§2.1; the preset comes back when space does).
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct ViewLayout {
    pub(crate) preset: LayoutPreset,
    shares: [[f32; 2]; 5],
    pub(crate) maximized: Option<ViewId>,
    pub(crate) fallback: bool,
}

impl Default for ViewLayout {
    fn default() -> Self {
        Self {
            preset: LayoutPreset::Single,
            shares: LayoutPreset::ALL.map(LayoutPreset::default_shares),
            maximized: None,
            fallback: false,
        }
    }
}

/// What the grid shows: one view, or the preset with its shares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shown {
    One(ViewId),
    Split(LayoutPreset, [f32; 2]),
}

impl ViewLayout {
    /// The current preset's shares.
    pub(crate) fn shares(&self) -> [f32; 2] {
        self.shares[self.preset.index()]
    }

    /// What the grid shows now.
    pub(crate) fn shown(&self) -> Shown {
        match (self.maximized, self.fallback, self.preset) {
            (Some(id), ..) => Shown::One(id),
            (None, true, _) | (None, _, LayoutPreset::Single) => Shown::One(ViewId(0)),
            (None, false, p) => Shown::Split(p, self.shares()),
        }
    }

    /// The views showing.
    pub(crate) fn ids(&self) -> Vec<ViewId> {
        match self.shown() {
            Shown::One(id) => vec![id],
            Shown::Split(p, _) => (0..p.count()).map(ViewId).collect(),
        }
    }

    /// The size the preset's views need at `min` each, with `gap`s.
    fn floor(preset: LayoutPreset, min: f32, gap: f32) -> Vec2 {
        let (c, r) = preset.grid();
        let n = |k: u8| f32::from(k) * min + f32::from(k - 1) * gap;
        Vec2::new(n(c), n(r))
    }

    /// Picks a preset: its views show, none maximized.
    pub(crate) fn set_preset(&mut self, preset: LayoutPreset) {
        self.preset = preset;
        self.maximized = None;
    }

    /// Sets the current preset's shares (a workspace's, §8), each kept
    /// within the dividers' range.
    pub(crate) fn set_shares(&mut self, shares: [f32; 2]) {
        self.shares[self.preset.index()] = shares.map(|s| s.clamp(0.1, 0.9));
    }

    /// Sets divider `axis`'s share (0 columns, 1 rows).
    fn set_share(&mut self, axis: usize, share: f32) {
        self.shares[self.preset.index()][axis] = share;
    }

    fn reset_share(&mut self, axis: usize) {
        let d = self.preset.default_shares()[axis];
        self.set_share(axis, d);
    }
}

/// What the Layout popover and the view keys do to the layout.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutCommand {
    Preset(LayoutPreset),
    /// Maximize a view (the rail's, from its popover; else the key view),
    /// or restore the layout (⇧Space).
    Maximize(Option<ViewId>),
    /// Close a view (the rail's; else the key view, ⌘W): one view fewer.
    Close(Option<ViewId>),
    /// Split the view right or down (⌥⌘→, ⌥⌘↓).
    Split {
        down: bool,
    },
}

/// The node holding the views; [`build_grid`] fills it.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ViewGrid;

/// What the grid was built for.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridShape(Option<LayoutPreset>, Option<ViewId>);

/// Which divider's share a pane takes (`first`), or the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Share {
    pub(crate) axis: usize,
    pub(crate) first: bool,
}

/// A pane's share, if it follows a divider.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shares(pub(crate) Option<Share>);

/// A divider's 2 px gap.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DividerGap(pub(crate) usize);

/// A divider's hit area: it drags the share of `axis` (0: the column
/// divider, 1: the row divider).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewDivider(pub(crate) usize);

/// The views' cameras, found by [`ViewId`] or from a UI node in a view's
/// node (its controls, their menus).
#[derive(SystemParam)]
pub(crate) struct ViewCameras<'w, 's> {
    cameras: Query<
        'w,
        's,
        (
            Entity,
            &'static ViewCamera,
            &'static Transform,
            &'static Projection,
            &'static ViewId,
        ),
        With<MainCamera>,
    >,
    nodes: Query<'w, 's, &'static ViewNode>,
    parents: Query<'w, 's, &'static ChildOf>,
}

/// A view's camera entity, state, placement and projection.
pub(crate) type ViewRef<'a> = (Entity, &'a ViewCamera, &'a Transform, &'a Projection);

impl ViewCameras<'_, '_> {
    /// View `id`'s camera.
    pub(crate) fn get(&self, id: ViewId) -> Option<ViewRef<'_>> {
        self.cameras
            .iter()
            .find(|c| *c.4 == id)
            .map(|(e, v, t, p, _)| (e, v, t, p))
    }

    /// The view `ui` is in: its own node or an ancestor's.
    pub(crate) fn id_of(&self, ui: Entity) -> Option<ViewId> {
        std::iter::once(ui)
            .chain(self.parents.iter_ancestors(ui))
            .find_map(|e| self.nodes.get(e).ok().map(|n| n.0))
    }

    /// The camera of the view `ui` is in.
    pub(crate) fn of(&self, ui: Entity) -> Option<ViewRef<'_>> {
        self.get(self.id_of(ui)?)
    }

    /// The view node `ui` is in.
    pub(crate) fn node_of(&self, ui: Entity) -> Option<Entity> {
        std::iter::once(ui)
            .chain(self.parents.iter_ancestors(ui))
            .find(|e| self.nodes.contains(*e))
    }
}

/// A row of panes, stretched across it.
fn panes_row() -> Node {
    Node {
        align_items: AlignItems::Stretch,
        ..row(0.0)
    }
}

/// The grid node: filling the viewport area, under its overlays.
pub(crate) fn view_grid() -> impl Bundle {
    (
        ViewGrid,
        Node {
            align_items: AlignItems::Stretch,
            ..crate::widgets::layout::overlay()
        },
        pass_through(),
    )
}

/// A box that grows by its share (or the rest), from nothing.
pub(crate) fn pane(share: Option<Share>, grow: f32, node: Node) -> impl Bundle {
    (
        Node {
            flex_grow: grow,
            flex_basis: px(0),
            min_width: px(0),
            min_height: px(0),
            ..node
        },
        pass_through(),
        Shares(share),
    )
}

/// A divider: the 2 px gap, with the 6 px hit area centred on it.
fn divider(theme: &Theme, axis: usize) -> impl Bundle {
    let (gap, hit) = (theme.s.viewport_divider, theme.s.viewport_divider_hit);
    let off = px(-(hit - gap) / 2.0);
    let (gap_node, hit_node, split) = if axis == 0 {
        (
            Node {
                width: px(gap),
                flex_shrink: 0.0,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                bottom: px(0),
                left: off,
                width: px(hit),
                ..default()
            },
            SplitAxis::Columns,
        )
    } else {
        (
            Node {
                height: px(gap),
                flex_shrink: 0.0,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: off,
                height: px(hit),
                ..default()
            },
            SplitAxis::Rows,
        )
    };
    (
        DividerGap(axis),
        gap_node,
        pass_through(),
        children![(
            ViewDivider(axis),
            hit_node,
            // The first pane is before the divider.
            splitter_control(Splitter::new(split, 1.0), "Resize views"),
            GlobalZIndex(5),
        )],
    )
}

/// Spawns `id`'s view node.
type ViewSpawner = Box<dyn Fn(&mut ChildSpawner, ViewId, Option<Share>, f32) + Send + Sync>;

/// Spawns what `shown` shows under the grid: views and dividers in rows
/// and columns.
fn spawn_shown(p: &mut ChildSpawner, theme: &Theme, shown: Shown, view: &ViewSpawner) {
    let first = |axis| Some(Share { axis, first: true });
    let rest = |axis| Some(Share { axis, first: false });
    let (preset, [c, r]) = match shown {
        Shown::One(id) => {
            view(p, id, None, 1.0);
            return;
        }
        Shown::Split(preset, shares) => (preset, shares),
    };
    let t = theme;
    match preset {
        LayoutPreset::Single => view(p, ViewId(0), None, 1.0),
        LayoutPreset::TwoColumns => {
            view(p, ViewId(0), first(0), c);
            p.spawn(divider(t, 0));
            view(p, ViewId(1), rest(0), 1.0 - c);
        }
        LayoutPreset::TwoRows => {
            p.spawn(pane(None, 1.0, column(0.0))).with_children(|col| {
                view(col, ViewId(0), first(1), r);
                col.spawn(divider(t, 1));
                view(col, ViewId(1), rest(1), 1.0 - r);
            });
        }
        LayoutPreset::OnePlusTwo => {
            view(p, ViewId(0), first(0), c);
            p.spawn(divider(t, 0));
            p.spawn(pane(rest(0), 1.0 - c, column(0.0)))
                .with_children(|col| {
                    view(col, ViewId(1), first(1), r);
                    col.spawn(divider(t, 1));
                    view(col, ViewId(2), rest(1), 1.0 - r);
                });
        }
        LayoutPreset::Quad => {
            p.spawn(pane(None, 1.0, column(0.0))).with_children(|col| {
                for (top, ids) in [(true, [0, 1]), (false, [2, 3])] {
                    let (share, grow) = if top {
                        (first(1), r)
                    } else {
                        (rest(1), 1.0 - r)
                    };
                    col.spawn(pane(share, grow, panes_row()))
                        .with_children(|rw| {
                            view(rw, ViewId(ids[0]), first(0), c);
                            rw.spawn(divider(t, 0));
                            view(rw, ViewId(ids[1]), rest(0), 1.0 - c);
                        });
                    if top {
                        col.spawn(divider(t, 1));
                    }
                }
            });
        }
    }
}

/// (Re)builds the grid when what it shows changes (not for a share: the
/// panes follow it, [`follow_shares`]), and tells `gz-bevy` how many views
/// there are.
pub(crate) fn build_grid(
    layout: Res<ViewLayout>,
    theme: Res<Theme>,
    map: Res<InputMap>,
    grids: Query<(Entity, Option<&GridShape>), With<ViewGrid>>,
    slots: Option<ResMut<ViewSlots>>,
    mut commands: Commands,
) {
    let shown = layout.shown();
    let shape = match shown {
        Shown::One(id) => GridShape(None, Some(id)),
        Shown::Split(p, _) => GridShape(Some(p), None),
    };
    if let Some(mut slots) = slots {
        let want = layout
            .preset
            .count()
            .max(layout.maximized.map_or(0, |id| id.0 + 1));
        // Never fewer cameras than views ever shown: dropped views keep
        // theirs.
        if want > slots.0 {
            slots.0 = want;
        }
    }
    for (grid, built) in &grids {
        if built == Some(&shape) {
            continue;
        }
        let t = theme.clone();
        let m = map.clone();
        let view: ViewSpawner = Box::new(move |p, id, share, grow| {
            p.spawn(super::view_node(&t, &m, id, share, grow));
        });
        let theme = theme.clone();
        commands
            .entity(grid)
            .despawn_related::<Children>()
            .insert(shape)
            .insert(Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                spawn_shown(p, &theme, shown, &view);
            })));
    }
}

/// The panes grow by their divider's share as it moves, kept so that
/// each side is at least `viewport_min` in its row or column as it is now
/// laid out (§2.1: never below the floor; a narrower area or an opened
/// panel moves it, the stored share comes back with the room). Runs every
/// frame: the rows' sizes change without the layout.
pub(crate) fn follow_shares(
    layout: Res<ViewLayout>,
    theme: Res<Theme>,
    sizes: Query<&ComputedNode>,
    mut panes: Query<(&Shares, &ChildOf, &mut Node)>,
) {
    let shares = layout.shares();
    for (share, parent, mut node) in &mut panes {
        let Some(share) = share.0 else {
            continue;
        };
        let length = sizes.get(parent.parent()).map_or(0.0, |n| {
            let size = n.size() * n.inverse_scale_factor();
            if share.axis == 0 {
                size.x
            } else {
                size.y
            }
        });
        let s = fit_share(
            shares[share.axis],
            length - theme.s.viewport_divider,
            theme.s.viewport_min,
        );
        let grow = if share.first { s } else { 1.0 - s };
        if node.flex_grow != grow {
            node.flex_grow = grow;
        }
    }
}

/// `share` of `length` (the views', the gap left out) kept to at least
/// `min` a side, or half each when they can't both have it; as it is
/// before the first layout (`length` 0).
pub(crate) fn fit_share(share: f32, length: f32, min: f32) -> f32 {
    if length <= 0.0 {
        return share;
    }
    if length < 2.0 * min {
        return 0.5;
    }
    let lo = min / length;
    share.clamp(lo, 1.0 - lo)
}

/// The length a divider's views share along its row or column (UI
/// pixels, the gap left out).
fn divider_length(
    theme: &Theme,
    d: &ViewDivider,
    gap: &ChildOf,
    gaps: &Query<&ChildOf, With<DividerGap>>,
    nodes: &Query<&ComputedNode>,
) -> Option<f32> {
    let size = gaps
        .get(gap.parent())
        .ok()
        .and_then(|row| nodes.get(row.parent()).ok())
        .map(|n| n.size() * n.inverse_scale_factor())?;
    Some(if d.0 == 0 { size.x } else { size.y } - theme.s.viewport_divider)
}

/// Keeps each divider's size its first pane's length, and its range each
/// side at least `viewport_min` (an even split when they can't both be).
pub(crate) fn divider_ranges(
    layout: Res<ViewLayout>,
    theme: Res<Theme>,
    gaps: Query<&ChildOf, With<DividerGap>>,
    nodes: Query<&ComputedNode>,
    mut dividers: Query<(&ViewDivider, &ChildOf, &mut Splitter)>,
) {
    let min = theme.s.viewport_min;
    for (d, gap, mut s) in &mut dividers {
        let Some(length) = divider_length(&theme, d, gap, &gaps, &nodes) else {
            continue;
        };
        let length = length.max(0.0);
        let share = fit_share(layout.shares()[d.0], length, min);
        let (lo, hi) = if length < 2.0 * min {
            (length / 2.0, length / 2.0)
        } else {
            (min, length - min)
        };
        let want = s.with_range(share * length, lo, hi);
        s.set_if_neq(want);
    }
}

/// A drag or a key sets the divider's share (§3.4).
pub(crate) fn on_divider_moved(
    ev: On<SplitterMoved>,
    theme: Res<Theme>,
    dividers: Query<(&ViewDivider, &ChildOf)>,
    gaps: Query<&ChildOf, With<DividerGap>>,
    nodes: Query<&ComputedNode>,
    mut layout: ResMut<ViewLayout>,
) {
    let Ok((d, gap)) = dividers.get(ev.entity) else {
        return;
    };
    let Some(length) = divider_length(&theme, d, gap, &gaps, &nodes).filter(|l| *l > 0.0) else {
        return;
    };
    let share = fit_share(ev.size / length, length, theme.s.viewport_min);
    if layout.shares()[d.0] != share {
        layout.set_share(d.0, share);
    }
}

/// A double-click resets the share (§3.4), as the shell's splitters do.
pub(crate) fn on_divider_reset(
    ev: On<SplitterReset>,
    dividers: Query<&ViewDivider>,
    mut layout: ResMut<ViewLayout>,
) {
    if let Ok(d) = dividers.get(ev.entity) {
        layout.reset_share(d.0);
    }
}

/// A divider's gap turns `accent` while it drags.
pub(crate) fn divider_fills(
    dividers: Query<(&ChildOf, Has<SplitterDrag>), With<ViewDivider>>,
    mut commands: Commands,
    fills: Query<Option<&Fill>, With<DividerGap>>,
) {
    for (gap, dragging) in &dividers {
        let Ok(fill) = fills.get(gap.parent()) else {
            continue;
        };
        match (dragging, fill) {
            (true, None) => {
                commands
                    .entity(gap.parent())
                    .insert(Fill(ColorRole::Accent));
            }
            (false, Some(_)) => {
                commands.entity(gap.parent()).remove::<Fill>();
            }
            _ => {}
        }
    }
}

/// The floor rule for several views (§2.1): the area must hold every view
/// at `viewport_min`. The side panels collapse first (inspector, then
/// sidebar; they come back when there is room), then the layout falls back
/// to Single until the preset fits again.
pub(crate) fn view_floor(
    theme: Res<Theme>,
    grids: Query<&ComputedNode, With<ViewGrid>>,
    mut layout: ResMut<ViewLayout>,
    mut shell: ResMut<ShellLayout>,
) {
    // A maximized view is one view: it needs one view's room.
    let shown = if layout.maximized.is_some() {
        LayoutPreset::Single
    } else {
        layout.preset
    };
    let need = ViewLayout::floor(shown, theme.s.viewport_min, theme.s.viewport_divider);
    if shell.floor_w != need.x {
        shell.set_floor(need.x);
    }
    let height = grids
        .iter()
        .next()
        .map_or(0.0, |n| n.size().y * n.inverse_scale_factor());
    // Not laid out yet: no fallback on its account.
    let short = height > 0.0 && height < need.y;
    let fallback = shown != LayoutPreset::Single && (shell.viewport_w() < need.x || short);
    if layout.fallback != fallback {
        layout.fallback = fallback;
    }
}

/// The active view (with more than one showing) has a 2 px `accent` inset
/// outline (§3.4).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ActiveFrame;

/// A hidden 2 px `accent` frame inside the view node.
pub(crate) fn active_frame() -> impl Bundle {
    (
        ActiveFrame,
        Node {
            display: Display::None,
            border: UiRect::all(px(2.0)),
            ..crate::widgets::layout::overlay()
        },
        crate::theme::Stroke(ColorRole::Accent),
        pass_through(),
    )
}

/// Shows the frame on the active view while several show.
pub(crate) fn active_frames(
    routing: Res<ViewRouting>,
    layout: Res<ViewLayout>,
    cameras: ViewCameras,
    mut frames: Query<(Entity, &mut Node), With<ActiveFrame>>,
) {
    let several = layout.ids().len() > 1;
    let active = routing.active.unwrap_or(ViewId(0));
    for (e, mut node) in &mut frames {
        let want = several && cameras.id_of(e) == Some(active);
        crate::widgets::layout::set_shown(&mut node, want);
    }
}

/// How far a view's controls show, 1 (the cube and the rail) to 0 (the
/// inactive view's face marker), easing over [`REVEAL_SECS`] (§3.4).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reveal(pub(crate) f32);

impl Default for Reveal {
    fn default() -> Self {
        Self(1.0)
    }
}

/// How long the hover reveal fades (§3.4: 100 ms).
pub(crate) const REVEAL_SECS: f32 = 0.1;
/// The face marker's side and opacity (§3.4: 34 px at 80%).
pub(crate) const MARKER_PX: f32 = 34.0;
pub(crate) const MARKER_OPACITY: f32 = 0.8;

/// An inactive view's flat face marker, where its cube is.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct FaceMarker;

/// The face marker's label (TOP, N, …).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct FaceMarkerLabel;

/// A hidden face marker: a 34 px `cube_top` square with a `cube_edge`
/// border, centred where the cube is, reading the face the view looks at.
pub(crate) fn face_marker(theme: &Theme) -> impl Bundle {
    let s = &theme.s;
    // Centred on the cube: its right inset plus half of it, less half of
    // the marker.
    let right = super::view_cube::CUBE_RIGHT + (s.view_cube - MARKER_PX) / 2.0;
    let top = s.viewport_inset + (s.view_cube - MARKER_PX) / 2.0;
    (
        FaceMarker,
        Name::new("face marker"),
        Node {
            position_type: PositionType::Absolute,
            top: px(top),
            right: px(right),
            width: px(MARKER_PX),
            height: px(MARKER_PX),
            display: Display::None,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(crate::theme::border::HAIRLINE)),
            border_radius: crate::widgets::rounded(crate::theme::radius::XS),
            ..default()
        },
        Fill(ColorRole::CubeTop),
        crate::theme::Stroke(ColorRole::CubeEdge),
        pass_through(),
        children![(
            FaceMarkerLabel,
            crate::assets::fonts::text(
                super::view_cube::systems::label_type(theme),
                ColorRole::CubeLabel,
                "TOP",
            ),
            pass_through(),
        )],
    )
}

/// The view a view's controls show for: shown in full while it is the
/// active or the hovered view (or the only one), else its face marker
/// (§3.4 "Inactive view"), easing between the two. The cube and rail
/// fade (the target chip with the rail) and go once hidden, so they take
/// no pointer; the marker fades the other way, to 80%.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn reveal_views(
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    routing: Res<ViewRouting>,
    layout: Res<ViewLayout>,
    mut views: Query<(&ViewNode, &mut Reveal, &Children, Ref<ViewNode>)>,
    mut controls: Query<
        (&mut Node, Has<FaceMarker>),
        Or<(
            With<super::rail::Rail>,
            With<super::view_cube::ViewCube>,
            With<FaceMarker>,
        )>,
    >,
    (menus, parents, scenes, mut cameras): (
        Query<Entity, With<crate::widgets::menu::Menu>>,
        Query<&ChildOf>,
        Query<&super::view_cube::scene::CubeScene>,
        Query<&mut Camera>,
    ),
    mut fades: FadeQueries,
    mut commands: Commands,
) {
    let one = layout.ids().len() < 2;
    let step = theme.motion_dt(time.delta_secs(), REVEAL_SECS) / REVEAL_SECS;
    for (node, mut reveal, children, added) in &mut views {
        let full = one || routing.active == Some(node.0) || routing.hovered == Some(node.0);
        let want = if full { 1.0 } else { 0.0 };
        let now = if one {
            1.0
        } else if reveal.0 < want {
            (reveal.0 + step).min(want)
        } else {
            (reveal.0 - step).max(want)
        };
        // A hidden cube renders nothing. Checked every frame: a cube's
        // scene spawns through commands, so its camera can miss the frame
        // the view's reveal reaches 0 (one long frame does it).
        for &c in children {
            if let Ok(scene) = scenes.get(c) {
                if let Ok(mut camera) = cameras.get_mut(scene.camera) {
                    if camera.is_active != (now > 0.0) {
                        camera.is_active = now > 0.0;
                    }
                }
            }
        }
        if now == reveal.0 && !added.is_added() {
            continue;
        }
        // Going inactive: its rail's menus close (one opened from the keys
        // would outlive the rail).
        if now < reveal.0 && reveal.0 >= 1.0 {
            for menu in &menus {
                if parents.iter_ancestors(menu).any(|a| children.contains(&a)) {
                    commands.trigger(bevy::ui_widgets::MenuEvent {
                        source: menu,
                        action: bevy::ui_widgets::MenuAction::CloseAll,
                    });
                }
            }
        }
        reveal.0 = now;
        for &c in children {
            let Ok((mut n, marker)) = controls.get_mut(c) else {
                continue;
            };
            // Hidden, the controls have their own opacities back, so a
            // widget's look changing meanwhile is what it comes back with.
            let (shown, alpha) = if marker {
                (now < 1.0, Some(MARKER_OPACITY * (1.0 - now)))
            } else {
                (now > 0.0, (now > 0.0 && now < 1.0).then_some(now))
            };
            crate::widgets::layout::set_shown(&mut n, shown);
            fade_tree(&mut commands, &mut fades, c, alpha);
        }
    }
}

/// The marker reads the face its view looks at: the snapped one, else the
/// one facing the camera most.
pub(crate) fn face_labels(
    cameras: ViewCameras,
    mut labels: Query<(Entity, &mut Text), With<FaceMarkerLabel>>,
) {
    use super::view_cube::pick::{self, CubeRegion};
    for (e, mut text) in &mut labels {
        let Some((_, v, t, _)) = cameras.of(e) else {
            continue;
        };
        let face = match v.snapped {
            Some(s) => CubeRegion::of_snap(s).0,
            None => {
                let to_camera = pick::gz_rotation(t.rotation) * Vec3::Z;
                CubeRegion::FACES
                    .into_iter()
                    .max_by(|a, b| {
                        a.as_vec3()
                            .dot(to_camera)
                            .total_cmp(&b.as_vec3().dot(to_camera))
                    })
                    .unwrap_or(IVec3::Z)
            }
        };
        crate::widgets::text::set_text(&mut text, pick::face_label(face));
    }
}

/// Applies [`LayoutCommand`]s. Closing a view renumbers the ones after it
/// (their cameras take the ids down), so the views keep their order; it
/// waits while a mouse button is down (a drag keeps its view by id).
/// Closing and splitting need the preset showing (not the floor's
/// fallback, nor a maximized view).
pub(crate) fn apply_layout_commands(
    mut reader: MessageReader<LayoutCommand>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut layout: ResMut<ViewLayout>,
    mut routing: ResMut<ViewRouting>,
    mut cameras: Query<&mut ViewId, With<MainCamera>>,
) {
    for cmd in reader.read() {
        let key = routing.key_view().unwrap_or(ViewId(0));
        let split = layout.preset.count() > 1 && !layout.fallback;
        let showing = !layout.fallback && layout.maximized.is_none();
        match *cmd {
            LayoutCommand::Preset(p) => layout.set_preset(p),
            LayoutCommand::Split { down } => {
                if showing {
                    let p = layout.preset.split(down);
                    layout.set_preset(p);
                }
            }
            LayoutCommand::Maximize(view) => {
                layout.maximized = match layout.maximized {
                    None if split => Some(view.unwrap_or(key)),
                    _ => None,
                };
                // The maximized view is the one the tools apply to.
                if let Some(id) = layout.maximized {
                    routing.active = Some(id);
                }
            }
            LayoutCommand::Close(view) => {
                let count = layout.preset.count();
                if count < 2 || !showing || buttons.get_pressed().next().is_some() {
                    continue;
                }
                let closed = view.unwrap_or(key).0.min(count - 1);
                // Their layers follow (`overlays::view_layers`).
                for mut id in &mut cameras {
                    let now = match id.0 {
                        n if n == closed => count - 1,
                        n if n > closed && n < count => n - 1,
                        _ => continue,
                    };
                    *id = ViewId(now);
                }
                let p = layout.preset.without_one();
                layout.set_preset(p);
                routing.active = Some(ViewId(closed.min(p.count() - 1)));
                if routing.hovered.is_some_and(|h| h.0 >= p.count()) {
                    routing.hovered = None;
                }
            }
        }
    }
}

/// The active view is always one that shows: after a layout change, the
/// floor's fallback or a restore, one that went is replaced by the first.
pub(crate) fn keep_active(layout: Res<ViewLayout>, mut routing: ResMut<ViewRouting>) {
    if !layout.is_changed() {
        return;
    }
    let ids = layout.ids();
    if routing.active.is_none_or(|a| !ids.contains(&a)) {
        routing.active = ids.first().copied();
    }
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<ViewLayout>()
        .init_resource::<ViewSlots>()
        .add_message::<LayoutCommand>()
        .add_systems(
            Update,
            (
                apply_start_layout,
                // Renumbering before a new environment respawns the
                // cameras from their ids.
                apply_layout_commands.before(gz_bevy::render::apply_environment),
                view_floor,
                keep_active,
                build_grid,
                follow_shares,
                divider_ranges,
            )
                .chain()
                .in_set(crate::GzUiSystems::Behaviour),
        )
        .add_systems(
            Update,
            (active_frames, divider_fills, face_labels)
                .in_set(crate::shell::ShellSystems::Readouts),
        )
        // After the widgets' looks, so the fade wins.
        .add_systems(Update, reveal_views.after(crate::GzUiSystems::Visuals))
        .add_observer(on_divider_moved)
        .add_observer(on_divider_reset);
}

#[cfg(test)]
mod tests;
