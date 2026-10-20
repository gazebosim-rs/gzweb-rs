//! The shell's layout state and rules (spec §2.1, §3.2, §3.3.2), as plain
//! data and functions so they can be tested without an app:
//! panel widths and collapse, the start-up collapse rules, the viewport
//! floor, splitter drags, and the activity bar's slot logic.

use bevy::prelude::*;

use crate::theme::{Sizes, SPACE};

/// The sidebar panels with an activity button (spec §3.2), in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum Panel {
    Scene,
    Assets,
    Topics,
    Plot,
    Sensors,
    Teleop,
    Console,
}

impl Panel {
    /// Every panel, in the activity bar's order (each registers itself:
    /// [`PanelRegistry`](crate::panels::PanelRegistry)).
    pub const ALL: [Panel; 7] = [
        Panel::Scene,
        Panel::Assets,
        Panel::Topics,
        Panel::Plot,
        Panel::Sensors,
        Panel::Teleop,
        Panel::Console,
    ];
}

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
/// The primary slot's default share of the sidebar (§3.3.2).
pub const DEFAULT_SPLIT: f32 = 0.55;
/// A slot is never shorter than this (§3.3.2).
pub const MIN_SLOT_H: f32 = 180.0;

/// Panel widths, collapse state and slots (spec §2.1, §8). The workspace
/// persists it from M8; until then it lives for the session.
#[derive(Resource, Clone, Debug, PartialEq, Reflect)]
pub struct ShellLayout {
    pub sidebar_w: f32,
    pub inspector_w: f32,
    pub sidebar_open: bool,
    pub inspector_open: bool,
    pub primary: Option<Panel>,
    pub secondary: Option<Panel>,
    /// The primary slot's share of the sidebar's height when both slots
    /// are open.
    pub split: f32,
    /// The window width (logical) the layout was last fitted to.
    pub window_w: f32,
    /// Panels the floor rule closed, reopened when space returns (a user
    /// toggle forgets them).
    pub floor_closed: [bool; 2],
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
    /// below 1100, Scene in the primary slot, then the floor rule.
    pub fn for_width(width: f32) -> Self {
        let s = Sizes::default();
        let mut l = Self {
            sidebar_w: s.sidebar_w,
            inspector_w: s.inspector_w,
            sidebar_open: width >= SIDEBAR_START_W,
            inspector_open: width >= INSPECTOR_START_W,
            primary: Some(Panel::Scene),
            secondary: None,
            split: DEFAULT_SPLIT,
            window_w: width,
            floor_closed: [false; 2],
        };
        l.enforce_floor(None);
        l
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
        let min = Sizes::default().viewport_min;
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

    /// Fits the layout to a new window width: the floor rule, then panels
    /// it closed come back (sidebar first) once they fit again.
    pub fn fit(&mut self, window_w: f32) {
        self.window_w = window_w;
        self.enforce_floor(None);
        let min = Sizes::default().viewport_min;
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

    /// Opens (or collapses) a side on the user's request.
    pub fn show(&mut self, side: Side, open: bool) {
        self.floor_closed[index(side)] = false;
        self.set_open(side, open);
        if open {
            self.enforce_floor(Some(side));
        }
    }

    /// The width a splitter drag gives `side`: `start` moved by `dx`
    /// (towards the viewport widens it), within the side's limits and
    /// short of the viewport floor.
    pub fn drag_width(&self, side: Side, start: f32, dx: f32) -> f32 {
        let (_, min, max) = Self::limits(side);
        let sign = match side {
            Side::Sidebar => 1.0,
            Side::Inspector => -1.0,
        };
        let room = self.viewport_w() + self.width(side) - Sizes::default().viewport_min;
        // (`side` is open while it's dragged: no bar of its own to count.)
        (start + sign * dx).min(room).clamp(min, max)
    }

    /// Applies a splitter drag.
    pub fn drag(&mut self, side: Side, start: f32, dx: f32) {
        let w = self.drag_width(side, start, dx);
        self.set_width(side, w);
    }

    /// A splitter double-click: back to the default width (then the floor
    /// rule, keeping this side).
    pub fn reset_width(&mut self, side: Side) {
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

    /// The slot holding `panel`.
    pub fn slot_of(&self, panel: Panel) -> Option<Slot> {
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
    pub fn is_active(&self, panel: Panel) -> bool {
        self.slot_of(panel).is_some()
    }

    /// An activity button click (§3.2): an inactive panel opens in the
    /// primary slot (Shift: the secondary); the active one collapses the
    /// sidebar, or expands it again if it's collapsed.
    pub fn activity_click(&mut self, panel: Panel, shift: bool) {
        if shift {
            self.open_in(panel, Slot::Secondary);
        } else if self.is_active(panel) {
            self.show(Side::Sidebar, !self.sidebar_open);
        } else {
            self.open_in(panel, Slot::Primary);
        }
    }

    /// Opens `panel` in `slot` and shows the sidebar. A panel is in one
    /// slot only: one already in the other slot swaps places with what
    /// `slot` held. A secondary with an empty primary moves up.
    pub fn open_in(&mut self, panel: Panel, slot: Slot) {
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
}
