//! The shell's layout state and rules (spec §2.1, §3.2, §3.3.2, §7.11), as
//! plain data and functions so they can be tested without an app:
//! panel widths and collapse, the start-up collapse rules, the viewport
//! floor, splitter widths, the activity bar's slot logic, and the pop-out
//! windows.

use bevy::prelude::*;

use crate::panels::PanelId;
use crate::theme::{Sizes, SPACE};

/// The sidebar's two stacked slots (spec §3.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum Slot {
    Primary,
    Secondary,
}

/// The two collapsible side regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum Side {
    Sidebar,
    Inspector,
}

/// Below this window width the inspector starts collapsed (§2.1).
pub const INSPECTOR_START_W: f32 = 1280.0;
/// Below this window width the sidebar starts collapsed too (§2.1).
pub const SIDEBAR_START_W: f32 = 1100.0;
/// The smallest window the layout is designed for (§2.1).
pub const MIN_WINDOW: (f32, f32) = (1024.0, 640.0);
/// A collapsed side panel leaves a bar this wide with the button that
/// brings it back (DECISIONS.md D-50).
pub const COLLAPSED_BAR_W: f32 = SPACE[10];
/// Start-up lasts this many frames and real seconds, whichever is later:
/// the start-up rules follow the window's size until then
/// ([`ShellLayout::startup`]).
pub const STARTUP_FRAMES: u32 = 30;
/// See [`STARTUP_FRAMES`].
pub const STARTUP_SECS: f64 = 1.0;
/// The primary slot's default share of the sidebar (§3.3.2).
pub const DEFAULT_SPLIT: f32 = 0.55;
/// A slot is never shorter than this (§3.3.2).
pub const MIN_SLOT_H: f32 = 180.0;
/// A pop-out window is never smaller than this (§7.11).
pub const POPOUT_MIN: Vec2 = Vec2::new(320.0, 240.0);
/// A new pop-out window's size.
pub const POPOUT_SIZE: Vec2 = Vec2::new(360.0, 440.0);
/// A new pop-out window's place in the viewport area, and how far each
/// next one steps down and right.
pub(crate) const POPOUT_AT: Vec2 = Vec2::new(SPACE[6], SPACE[6]);
pub(crate) const POPOUT_STEP: f32 = 24.0;

/// Which edges a pop-out's resize moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub struct Edges {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

/// A panel popped out of the sidebar into a window over the viewport area
/// (§7.11): its rect in the area (logical px), and the slot Dock returns
/// it to.
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct PopOut {
    pub panel: PanelId,
    pub from: Slot,
    pub rect: Rect,
}

/// Panel widths, collapse state and slots (spec §2.1, §8). The workspace
/// persists it from M8; until then it lives for the session.
#[derive(Resource, Clone, Debug, PartialEq, Reflect)]
pub struct ShellLayout {
    pub sidebar_w: f32,
    pub inspector_w: f32,
    pub sidebar_open: bool,
    pub inspector_open: bool,
    pub primary: Option<PanelId>,
    pub secondary: Option<PanelId>,
    /// The primary slot's share of the sidebar's height when both slots
    /// are open.
    pub split: f32,
    /// The window width (logical) the layout was last fitted to.
    pub window_w: f32,
    /// Panels the floor rule closed, reopened when space returns (a user
    /// toggle forgets them).
    pub floor_closed: [bool; 2],
    /// The width the viewport area must keep (§2.1 floor): `viewport_min`
    /// for one view, more for a split layout (each view at
    /// `viewport_min`).
    pub floor_w: f32,
    /// The start-up collapse rules still apply to width changes: the
    /// window's first real size can arrive after start (a browser canvas
    /// fitting its page, a tiling window manager). Ends with the user's
    /// first layout change, or once start-up is over (DECISIONS.md D-53).
    pub startup: bool,
    /// The pop-out windows, back to front: the last is on top (§7.11:
    /// they stack by last focused).
    pub popouts: Vec<PopOut>,
    /// The viewport area's size (logical px), which the windows stay in.
    pub area: Vec2,
}

fn index(side: Side) -> usize {
    match side {
        Side::Sidebar => 0,
        Side::Inspector => 1,
    }
}

impl ShellLayout {
    /// The layout a window `width` wide starts with: both panels at their
    /// default widths, the inspector collapsed below 1280, the sidebar too
    /// below 1100 (or with no panel to show), `open` (the registry's
    /// default panel) in the primary slot, then the floor rule.
    pub fn for_width(width: f32, open: Option<PanelId>) -> Self {
        let s = Sizes::default();
        let mut l = Self {
            sidebar_w: s.sidebar_w,
            inspector_w: s.inspector_w,
            sidebar_open: width >= SIDEBAR_START_W && open.is_some(),
            inspector_open: width >= INSPECTOR_START_W,
            primary: open,
            secondary: None,
            split: DEFAULT_SPLIT,
            window_w: width,
            floor_closed: [false; 2],
            floor_w: s.viewport_min,
            startup: true,
            popouts: Vec::new(),
            area: Vec2::ZERO,
        };
        l.enforce_floor(None);
        l
    }

    /// Sets the floor (a split layout's), then the floor rule, and brings
    /// back panels it closed once they fit again.
    pub fn set_floor(&mut self, floor_w: f32) {
        self.floor_w = floor_w;
        let w = self.window_w;
        let startup = std::mem::replace(&mut self.startup, false);
        self.fit(w);
        self.startup = startup;
    }

    pub fn is_open(&self, side: Side) -> bool {
        match side {
            Side::Sidebar => self.sidebar_open,
            Side::Inspector => self.inspector_open,
        }
    }

    fn set_open(&mut self, side: Side, open: bool) {
        match side {
            Side::Sidebar => self.sidebar_open = open,
            Side::Inspector => self.inspector_open = open,
        }
    }

    pub fn width(&self, side: Side) -> f32 {
        match side {
            Side::Sidebar => self.sidebar_w,
            Side::Inspector => self.inspector_w,
        }
    }

    fn set_width(&mut self, side: Side, w: f32) {
        match side {
            Side::Sidebar => self.sidebar_w = w,
            Side::Inspector => self.inspector_w = w,
        }
    }

    /// `(default, min, max)` width of a side.
    pub fn limits(side: Side) -> (f32, f32, f32) {
        let s = Sizes::default();
        match side {
            Side::Sidebar => (s.sidebar_w, s.sidebar_min_w, s.sidebar_max_w),
            Side::Inspector => (s.inspector_w, s.inspector_min_w, s.inspector_max_w),
        }
    }

    /// The viewport area's width: the window less the activity bar, the
    /// open panels and the collapsed panels' bars.
    pub fn viewport_w(&self) -> f32 {
        let s = Sizes::default();
        let mut w = self.window_w - s.activity_bar_w;
        for side in [Side::Sidebar, Side::Inspector] {
            w -= if self.is_open(side) {
                self.width(side)
            } else {
                COLLAPSED_BAR_W
            };
        }
        w
    }

    /// The floor rule (§2.1): while the viewport area is narrower than
    /// `viewport_min`, collapse the inspector, then the sidebar, never the
    /// side in `keep` (the one the user just opened or widened).
    pub fn enforce_floor(&mut self, keep: Option<Side>) {
        let min = self.floor_w;
        for side in [Side::Inspector, Side::Sidebar] {
            if self.viewport_w() >= min {
                return;
            }
            if keep != Some(side) && self.is_open(side) {
                self.set_open(side, false);
                self.floor_closed[index(side)] = true;
            }
        }
    }

    /// Fits the layout to a new window width: during start-up the start-up
    /// rules again (the sidebar only with a panel to show); afterwards the floor rule, then panels it closed come
    /// back (sidebar first) once they fit again.
    pub fn fit(&mut self, window_w: f32) {
        if self.startup {
            self.window_w = window_w;
            self.sidebar_open = window_w >= SIDEBAR_START_W && self.primary.is_some();
            self.inspector_open = window_w >= INSPECTOR_START_W;
            self.floor_closed = [false; 2];
            self.enforce_floor(None);
            return;
        }
        self.window_w = window_w;
        self.enforce_floor(None);
        let min = self.floor_w;
        for side in [Side::Sidebar, Side::Inspector] {
            let i = index(side);
            if self.floor_closed[i] && self.viewport_w() + COLLAPSED_BAR_W - self.width(side) >= min
            {
                self.set_open(side, true);
                self.floor_closed[i] = false;
            }
        }
    }

    /// Opens or collapses a side (⌘B, ⌘⌥I, the chevrons). Opening one that
    /// doesn't fit collapses the other (§2.1: never below the floor).
    pub fn toggle(&mut self, side: Side) {
        let open = !self.is_open(side);
        self.show(side, open);
    }

    /// Opens (or collapses) a side on the user's request. The sidebar
    /// opens only with a panel to show.
    pub fn show(&mut self, side: Side, open: bool) {
        if open && side == Side::Sidebar && self.primary.is_none() {
            return;
        }
        self.startup = false;
        self.floor_closed[index(side)] = false;
        self.set_open(side, open);
        if open {
            self.enforce_floor(Some(side));
        }
    }

    /// The widths a splitter can give `side`: within the side's limits and
    /// short of the viewport floor (`min` when even that is too wide).
    pub fn width_range(&self, side: Side) -> (f32, f32) {
        let (_, min, max) = Self::limits(side);
        // (`side` is open while it's dragged: no bar of its own to count.)
        let room = self.viewport_w() + self.width(side) - self.floor_w;
        (min, max.min(room).max(min))
    }

    /// Applies a splitter's drag or key: `side` `w` wide, within
    /// [`ShellLayout::width_range`].
    pub fn resize(&mut self, side: Side, w: f32) {
        let (min, max) = self.width_range(side);
        self.startup = false;
        self.set_width(side, w.clamp(min, max));
    }

    /// A splitter double-click: back to the default width (then the floor
    /// rule, keeping this side).
    pub fn reset_width(&mut self, side: Side) {
        self.startup = false;
        self.set_width(side, Self::limits(side).0);
        self.enforce_floor(Some(side));
    }

    /// The primary slot's share after a row-splitter drag of `dy` from
    /// `start`, in a sidebar body `height` tall (both slots at least
    /// [`MIN_SLOT_H`]; an even split when they can't both be).
    pub fn split_after(start: f32, dy: f32, height: f32) -> f32 {
        if height < 2.0 * MIN_SLOT_H {
            return 0.5;
        }
        let lo = MIN_SLOT_H / height;
        (start + dy / height).clamp(lo, 1.0 - lo)
    }

    /// The panel in `slot`.
    pub fn slot(&self, slot: Slot) -> Option<PanelId> {
        match slot {
            Slot::Primary => self.primary,
            Slot::Secondary => self.secondary,
        }
    }

    /// The slot holding `panel`.
    pub fn slot_of(&self, panel: PanelId) -> Option<Slot> {
        if self.primary == Some(panel) {
            Some(Slot::Primary)
        } else if self.secondary == Some(panel) {
            Some(Slot::Secondary)
        } else {
            None
        }
    }

    /// The panel's activity button shows the active state: its panel is in
    /// either slot, whether the sidebar is open or collapsed (§3.2; the
    /// selection survives a collapse, DECISIONS.md D-50).
    pub fn is_active(&self, panel: PanelId) -> bool {
        self.slot_of(panel).is_some()
    }

    /// An activity button click (§3.2): an inactive panel opens in the
    /// primary slot (Shift: the secondary); the active one collapses the
    /// sidebar, or expands it again if it's collapsed; a popped-out one
    /// comes to the top.
    pub fn activity_click(&mut self, panel: PanelId, shift: bool) {
        if self.is_popped(panel) {
            self.raise(panel);
        } else if shift {
            self.open_in(panel, Slot::Secondary);
        } else if self.is_active(panel) {
            self.show(Side::Sidebar, !self.sidebar_open);
        } else {
            self.open_in(panel, Slot::Primary);
        }
    }

    /// Opens `panel` in `slot` and shows the sidebar. A panel is in one
    /// slot only: one already in the other slot swaps places with what
    /// `slot` held. A secondary with an empty primary moves up. A
    /// popped-out panel is already showing: it comes to the top instead
    /// (Dock puts it back in a slot).
    pub fn open_in(&mut self, panel: PanelId, slot: Slot) {
        if self.is_popped(panel) {
            self.raise(panel);
            return;
        }
        let (target, other) = match slot {
            Slot::Primary => (self.primary, self.secondary),
            Slot::Secondary => (self.secondary, self.primary),
        };
        let other = if other == Some(panel) { target } else { other };
        match slot {
            Slot::Primary => (self.primary, self.secondary) = (Some(panel), other),
            Slot::Secondary => (self.secondary, self.primary) = (Some(panel), other),
        }
        self.normalize();
        self.show(Side::Sidebar, true);
    }

    /// The panel header's move button, shown with two slots: the panels
    /// swap slots (a panel can be in one slot only, so moving it to the
    /// other slot is a swap; DECISIONS.md).
    pub fn swap_slots(&mut self) {
        if self.primary.is_some() && self.secondary.is_some() {
            std::mem::swap(&mut self.primary, &mut self.secondary);
        }
    }

    /// Closes the secondary slot (§3.3.2): back to one slot.
    pub fn close_secondary(&mut self) {
        self.secondary = None;
    }

    /// Keeps a lone panel in the primary slot.
    fn normalize(&mut self) {
        if self.primary.is_none() {
            self.primary = self.secondary.take();
        }
    }

    /// `panel` is in a pop-out window.
    pub fn is_popped(&self, panel: PanelId) -> bool {
        self.popouts.iter().any(|p| p.panel == panel)
    }

    fn popout_index(&self, panel: PanelId) -> Option<usize> {
        self.popouts.iter().position(|p| p.panel == panel)
    }

    /// Pops `panel` out of its slot into a window on top (§7.11), each new
    /// one a step down and right of the last; the sidebar closes with no
    /// panel left in it. One already popped out comes to the top.
    pub fn pop_out(&mut self, panel: PanelId) {
        if self.is_popped(panel) {
            self.raise(panel);
            return;
        }
        let from = self.slot_of(panel).unwrap_or(Slot::Primary);
        match self.slot_of(panel) {
            Some(Slot::Primary) => self.primary = self.secondary.take(),
            Some(Slot::Secondary) => self.secondary = None,
            None => {}
        }
        if self.primary.is_none() {
            self.sidebar_open = false;
        }
        #[allow(clippy::cast_precision_loss)]
        let step = POPOUT_STEP * self.popouts.len() as f32;
        let min = POPOUT_AT + Vec2::splat(step);
        let rect = Rect::from_corners(min, min + POPOUT_SIZE);
        self.popouts.push(PopOut {
            panel,
            from,
            rect: self.clamped(rect),
        });
    }

    /// Dock (§7.11): the window's panel goes back to the slot it came
    /// from.
    pub fn dock(&mut self, panel: PanelId) {
        if let Some(i) = self.popout_index(panel) {
            let p = self.popouts.remove(i);
            self.open_in(p.panel, p.from);
        }
    }

    /// Close (§7.11): the window goes, and its panel with it.
    pub fn close_popout(&mut self, panel: PanelId) {
        self.popouts.retain(|p| p.panel != panel);
    }

    /// The window comes to the top (a press in it).
    pub fn raise(&mut self, panel: PanelId) {
        if let Some(i) = self.popout_index(panel) {
            let p = self.popouts.remove(i);
            self.popouts.push(p);
        }
    }

    /// The window moves to `rect` (a header drag), kept in the area.
    pub fn move_popout(&mut self, panel: PanelId, at: Vec2) {
        let Some(i) = self.popout_index(panel) else {
            return;
        };
        let size = self.popouts[i].rect.size();
        self.popouts[i].rect = self.clamped(Rect::from_corners(at, at + size));
    }

    /// The window's `edges` move by `delta` from `start` (an edge or
    /// corner drag): never smaller than [`POPOUT_MIN`], kept in the area.
    pub fn resize_popout(&mut self, panel: PanelId, start: Rect, edges: Edges, delta: Vec2) {
        let Some(i) = self.popout_index(panel) else {
            return;
        };
        let (mut min, mut max) = (start.min, start.max);
        let area = self.area_rect();
        if edges.left {
            min.x = (min.x + delta.x).clamp(area.min.x, max.x - POPOUT_MIN.x);
        }
        if edges.right {
            max.x =
                (max.x + delta.x).clamp(min.x + POPOUT_MIN.x, area.max.x.max(min.x + POPOUT_MIN.x));
        }
        if edges.top {
            min.y = (min.y + delta.y).clamp(area.min.y, max.y - POPOUT_MIN.y);
        }
        if edges.bottom {
            max.y =
                (max.y + delta.y).clamp(min.y + POPOUT_MIN.y, area.max.y.max(min.y + POPOUT_MIN.y));
        }
        self.popouts[i].rect = self.clamped(Rect::from_corners(min, max));
    }

    /// The viewport area changed size: the windows stay in it.
    pub fn set_area(&mut self, area: Vec2) {
        self.area = area;
        for i in 0..self.popouts.len() {
            self.popouts[i].rect = self.clamped(self.popouts[i].rect);
        }
    }

    fn area_rect(&self) -> Rect {
        // Before the area's first layout, anywhere.
        let size = if self.area == Vec2::ZERO {
            Vec2::splat(f32::MAX / 4.0)
        } else {
            self.area
        };
        Rect::from_corners(Vec2::ZERO, size)
    }

    /// `rect` kept in the area: at least [`POPOUT_MIN`], no bigger than the
    /// area, moved back in where it hangs out.
    fn clamped(&self, rect: Rect) -> Rect {
        let area = self.area_rect().size();
        let size = rect.size().max(POPOUT_MIN).min(area.max(POPOUT_MIN));
        let at = rect.min.clamp(Vec2::ZERO, (area - size).max(Vec2::ZERO));
        Rect::from_corners(at, at + size)
    }
}
