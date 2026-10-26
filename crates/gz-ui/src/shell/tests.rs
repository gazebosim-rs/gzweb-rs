//! The shell (M2) in a headless app: region rects at 1440×900 and the
//! collapse rules at narrower widths, the collapsed bars, the side
//! splitters, keys and commands, pointer routing, the readouts, the
//! connection phases and the text formats. Each region's own tests are in
//! its folder.

use std::time::Duration;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::view::controls::ViewRouting;
use gz_core::transport::Status;

use super::activity_bar::ActivityFor;
use super::collapsed_bar::ExpandButton;
use super::connection::{dot_role, ConnectionView, Phase};
use super::layout::MIN_SLOT_H;
use super::regions::Region;
use super::splitter::ShellSplitter;
use super::status_bar::StatusItem;
use super::top_bar::tools::ToolChoice;
use crate::panels::PanelBody;

use super::viewport::view::ViewNode;
use crate::panels::{PanelId, CONSOLE, SCENE};
use crate::shell::{ShellLayout, Side, Tool, UiCommand};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Theme, ThemeMode};

#[test]
fn every_region_has_its_size_at_1440_by_900() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let want = [
        (Region::Root, (0.0, 0.0), (1440.0, 900.0)),
        (Region::TopBar, (0.0, 0.0), (1440.0, 44.0)),
        (Region::Body, (0.0, 44.0), (1440.0, 828.0)),
        (Region::ActivityBar, (0.0, 44.0), (56.0, 828.0)),
        (Region::Sidebar, (56.0, 44.0), (300.0, 828.0)),
        (Region::ViewportArea, (356.0, 44.0), (784.0, 828.0)),
        (Region::Inspector, (1140.0, 44.0), (300.0, 828.0)),
        (Region::StatusBar, (0.0, 872.0), (1440.0, 28.0)),
    ];
    for (r, min, size) in want {
        let e = region(&mut app, r);
        assert_eq!(rect(&app, e), (Vec2::from(min), Vec2::from(size)), "{r:?}");
    }
    let view = find::<ViewNode>(&mut app);
    assert_eq!(
        rect(&app, view),
        (Vec2::new(356.0, 44.0), Vec2::new(784.0, 828.0))
    );
    // The splitters are 6 px, centred on the 1 px borders.
    let world = app.world_mut();
    let splitters: Vec<(Entity, Side)> = world
        .query::<(Entity, &ShellSplitter)>()
        .iter(world)
        .map(|(e, s)| (e, s.0))
        .collect();
    for (e, side) in splitters {
        let (min, size) = rect(&app, e);
        assert_eq!(size, Vec2::new(6.0, 828.0));
        let border_centre = match side {
            Side::Sidebar => 355.5,
            Side::Inspector => 1140.5,
        };
        // Layout rounds to whole pixels.
        assert!((min.x + 3.0 - border_centre).abs() <= 0.5, "{side:?} {min}");
    }
    // Panels and slots: Scene alone in the primary slot.
    let l = layout(&app);
    assert_eq!((l.primary, l.secondary), (Some(SCENE), None));
    let world = app.world_mut();
    let bodies: Vec<PanelId> = world
        .query::<&PanelBody>()
        .iter(world)
        .map(|b| b.0)
        .collect();
    assert_eq!(bodies, [SCENE]);
}

#[test]
fn narrow_windows_start_with_panels_collapsed() {
    for (w, sidebar, inspector) in [
        (1440, true, true),
        (1280, true, true),
        (1279, true, false),
        (1100, true, false),
        (1099, false, false),
        (1024, false, false),
    ] {
        let mut app = shell_app((w, 640), 1.0);
        frames(&mut app, 3);
        let l = layout(&app);
        assert_eq!(
            (l.sidebar_open, l.inspector_open),
            (sidebar, inspector),
            "{w}"
        );
        let s = region(&mut app, Region::Sidebar);
        let i = region(&mut app, Region::Inspector);
        assert_eq!(displayed(&app, s), sidebar, "{w}");
        assert_eq!(displayed(&app, i), inspector, "{w}");
        // Collapsed panels leave only their 24 px bar: the viewport fills
        // the rest.
        let v = region(&mut app, Region::ViewportArea);
        let open = |o: bool| if o { 300.0 } else { 24.0 };
        let want = w as f32 - 56.0 - open(sidebar) - open(inspector);
        assert_eq!(rect(&app, v).1.x, want, "{w}");
        // The status bar drops items below 1280 and 1100.
        let world = app.world_mut();
        let items: Vec<(StatusItem, Entity)> = world
            .query::<(&StatusItem, Entity)>()
            .iter(world)
            .map(|(i, e)| (*i, e))
            .collect();
        for (item, e) in items {
            let shown = match item {
                StatusItem::Hint | StatusItem::Cursor => w >= 1280,
                StatusItem::Fps | StatusItem::Iterations => w >= 1100,
                _ => true,
            };
            assert_eq!(displayed(&app, e), shown, "{w} {item:?}");
        }
    }
}

/// A collapsed side panel leaves a 24 px bar with a button that brings it
/// back at its previous width (DECISIONS.md D-50).
#[test]
fn collapsed_panels_leave_a_bar_that_expands_them() {
    use crate::widgets::{Tooltip, TooltipSide};
    // 1024 wide: both start collapsed.
    let mut app = shell_app((1024, 700), 1.0);
    frames(&mut app, 3);
    let l = layout(&app);
    assert!(!l.sidebar_open && !l.inspector_open);
    let bars = [
        (Side::Sidebar, Region::SidebarBar),
        (Side::Inspector, Region::InspectorBar),
    ];
    for (_, r) in bars {
        let bar = region(&mut app, r);
        assert!(displayed(&app, bar));
        assert_eq!(rect(&app, bar).1.x, 24.0);
    }
    // The bars sit beside the view: activity bar, bar, view, bar.
    let mut rect_of = |r| {
        let e = region(&mut app, r);
        rect(&app, e)
    };
    let (s, v, i) = (
        rect_of(Region::SidebarBar),
        rect_of(Region::ViewportArea),
        rect_of(Region::InspectorBar),
    );
    assert_eq!(s.0.x, 56.0);
    assert_eq!(v.0.x, 80.0);
    assert_eq!(v.1.x, 1024.0 - 56.0 - 48.0);
    assert_eq!(i.0.x, 1000.0);
    assert_eq!(layout(&app).viewport_w(), v.1.x);
    let world = app.world_mut();
    let buttons: Vec<(Entity, Side, Tooltip)> = world
        .query::<(Entity, &ExpandButton, &Tooltip)>()
        .iter(world)
        .map(|(e, b, t)| (e, b.0, t.clone()))
        .collect();
    assert_eq!(buttons.len(), 2);
    let find = |side| buttons.iter().find(|b| b.1 == side).unwrap().clone();
    let (sidebar_button, _, tip) = find(Side::Sidebar);
    assert_eq!(tip.title, "Show sidebar");
    assert_eq!(tip.shortcut.as_deref(), Some("Ctrl+B"));
    assert_eq!(tip.side, TooltipSide::Right);
    assert_eq!(
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(sidebar_button)
            .unwrap()
            .label(),
        Some("Show sidebar")
    );
    let (inspector_button, _, tip) = find(Side::Inspector);
    assert_eq!(tip.title, "Show inspector");
    assert_eq!(tip.side, TooltipSide::Left);
    // Scene stays selected while collapsed.
    let world = app.world_mut();
    let scene = world
        .query::<(Entity, &ActivityFor)>()
        .iter(world)
        .find(|(_, a)| a.0 == SCENE)
        .unwrap()
        .0;
    assert!(app.world().get::<Active>(scene).is_some());
    // Widen the window, narrow the sidebar, collapse it, then expand it
    // from its bar: back at 260, the bar gone.
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    app.world_mut().resource_mut::<ShellLayout>().sidebar_w = 260.0;
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    let bar = region(&mut app, Region::SidebarBar);
    assert!(displayed(&app, bar));
    let world = app.world_mut();
    let button = world
        .query::<(Entity, &ExpandButton)>()
        .iter(world)
        .find(|(_, b)| b.0 == Side::Sidebar)
        .unwrap()
        .0;
    app.world_mut().trigger(Activate { entity: button });
    frames(&mut app, 2);
    let l = layout(&app);
    assert!(l.sidebar_open && l.sidebar_w == 260.0);
    assert!(!displayed(&app, bar));
    let sidebar = region(&mut app, Region::Sidebar);
    assert_eq!(rect(&app, sidebar).1.x, 260.0);
    let _ = inspector_button;
}

#[test]
fn the_floor_rule_collapses_the_inspector_then_the_sidebar() {
    let mut l = ShellLayout::for_width(1440.0, Some(SCENE));
    l.startup = false;
    l.sidebar_w = 480.0;
    l.inspector_w = 480.0;
    // 1440 − 56 − 960 = 424 ≥ 240: both stay.
    l.fit(1440.0);
    assert!(l.sidebar_open && l.inspector_open);
    // 1200 − 56 − 960 < 240: the inspector goes first…
    l.fit(1200.0);
    assert!(l.sidebar_open && !l.inspector_open);
    assert!(l.viewport_w() >= 240.0);
    // …and comes back when the space does.
    l.fit(1440.0);
    assert!(l.sidebar_open && l.inspector_open);
    // Opening the inspector where both don't fit closes the sidebar.
    l.fit(1200.0);
    l.show(Side::Inspector, true);
    assert!(!l.sidebar_open && l.inspector_open);
    // The user's own collapse isn't undone by more room.
    l.show(Side::Sidebar, false);
    l.fit(1600.0);
    assert!(!l.sidebar_open);
    // Never below the floor at the minimum window.
    for w in [1024.0, 1100.0, 1279.0] {
        let mut l = ShellLayout::for_width(w, Some(SCENE));
        l.show(Side::Sidebar, true);
        l.show(Side::Inspector, true);
        assert!(l.viewport_w() >= 240.0, "{w}");
    }
    let mut tiny = ShellLayout::for_width(400.0, Some(SCENE));
    tiny.show(Side::Sidebar, true);
    assert!(tiny.sidebar_open && !tiny.inspector_open);
}

/// At UI scale 1.25 a 100 px pointer drag moves the edge 100 window px,
/// which is 80 UI px of width (DECISIONS.md D-41).
#[test]
fn splitter_drags_follow_the_pointer_at_any_ui_scale() {
    let mut app = shell_app((1440, 900), 1.0);
    app.world_mut().resource_mut::<Theme>().scale = 1.25;
    frames(&mut app, 3);
    let start = layout(&app).sidebar_w;
    let world = app.world_mut();
    let bar = world
        .query::<(Entity, &ShellSplitter)>()
        .iter(world)
        .find(|(_, s)| s.0 == Side::Sidebar)
        .unwrap()
        .0;
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    world.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::new(100.0, 0.0),
        delta: Vec2::new(100.0, 0.0),
    });
    frames(&mut app, 2);
    assert_eq!(layout(&app).sidebar_w, start + 80.0);
}

#[test]
fn splitter_drags_clamp_and_double_click_resets() {
    let l = ShellLayout::for_width(1440.0, Some(SCENE));
    // Each side within its limits.
    assert_eq!(l.width_range(Side::Sidebar), (240.0, 480.0));
    assert_eq!(l.width_range(Side::Inspector).0, 260.0);
    // Short of the floor: 1440 − 56 − 300 − 240 = 844 is the most, so at
    // 1000 px only 1000 − 56 − 300 − 240 = 404.
    let mut narrow = ShellLayout {
        window_w: 1000.0,
        inspector_open: true,
        sidebar_open: true,
        ..l
    };
    assert_eq!(narrow.width_range(Side::Sidebar), (240.0, 404.0));
    narrow.resize(Side::Sidebar, 700.0);
    assert_eq!(narrow.sidebar_w, 404.0);
    narrow.resize(Side::Sidebar, 100.0);
    assert_eq!(narrow.sidebar_w, 240.0);
    // No room at all: the minimum.
    let tiny = ShellLayout {
        window_w: 600.0,
        ..narrow.clone()
    };
    assert_eq!(tiny.width_range(Side::Sidebar), (240.0, 240.0));
    // Slot split: both slots at least 180.
    assert_eq!(ShellLayout::split_after(0.55, 0.0, 600.0), 0.55);
    assert_eq!(
        ShellLayout::split_after(0.55, -500.0, 600.0),
        MIN_SLOT_H / 600.0
    );
    assert_eq!(
        ShellLayout::split_after(0.55, 500.0, 600.0),
        1.0 - MIN_SLOT_H / 600.0
    );
    assert_eq!(ShellLayout::split_after(0.55, 10.0, 300.0), 0.5);

    // Through the observers.
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let world = app.world_mut();
    let bar = world
        .query::<(Entity, &ShellSplitter)>()
        .iter(world)
        .find(|(_, s)| s.0 == Side::Sidebar)
        .unwrap()
        .0;
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    world.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: hit.clone(),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::new(80.0, 3.0),
        delta: Vec2::new(80.0, 3.0),
    });
    frames(&mut app, 2);
    assert_eq!(layout(&app).sidebar_w, 380.0);
    let s = region(&mut app, Region::Sidebar);
    assert_eq!(rect(&app, s).1.x, 380.0);
    // The border is accent while dragging.
    assert_eq!(
        app.world().get::<crate::theme::Stroke>(s).unwrap().0,
        ColorRole::Accent
    );
    let world = app.world_mut();
    world.trigger(PointerDragEnd {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    frames(&mut app, 2);
    assert_eq!(
        app.world().get::<crate::theme::Stroke>(s).unwrap().0,
        ColorRole::Line
    );
    // A single click does nothing; a double-click resets.
    for count in [1, 2] {
        app.world_mut().trigger(PointerClick {
            entity: bar,
            pointer: pointer.clone(),
            button: PointerButton::Primary,
            hit: hit.clone(),
            duration: Duration::from_millis(80),
            count,
        });
        frames(&mut app, 1);
        let want = if count == 1 { 380.0 } else { 300.0 };
        assert_eq!(layout(&app).sidebar_w, want, "{count}");
    }
}

#[test]
fn ctrl_b_and_ctrl_alt_i_toggle_the_panels() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(layout(&app).sidebar_open);
    chord(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::KeyI],
    );
    assert!(!layout(&app).inspector_open);
    let i = region(&mut app, Region::Inspector);
    frames(&mut app, 1);
    assert!(!displayed(&app, i));
    // Plain B is the box-select tool (a viewport key), not the sidebar.
    chord(&mut app, &[KeyCode::KeyB]);
    assert!(layout(&app).sidebar_open);
    assert_eq!(*app.world().resource::<Tool>(), Tool::BoxSelect);
    // The tool buttons follow.
    frames(&mut app, 1);
    let world = app.world_mut();
    let checked: Vec<Tool> = world
        .query_filtered::<&ToolChoice, With<bevy::ui::Checked>>()
        .iter(world)
        .map(|c| c.0)
        .collect();
    assert_eq!(checked, [Tool::BoxSelect]);
    // Without the view's keys, B does nothing; Ctrl+B still works.
    *app.world_mut().resource_mut::<ViewRouting>() = ViewRouting {
        pointer: false,
        keys: false,
        ..default()
    };
    // Routing is rewritten each frame from the pointer: hold it off.
    app.world_mut()
        .resource_mut::<crate::shell::routing::ViewFocus>()
        .0 = false;
    chord(&mut app, &[KeyCode::KeyT]);
    assert_eq!(*app.world().resource::<Tool>(), Tool::BoxSelect);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
    // PanelId shortcuts open their panel.
    chord(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyC],
    );
    let l = layout(&app);
    assert!(l.sidebar_open);
    assert_eq!(l.primary, Some(CONSOLE));
}

/// D-119: a shortcut's letter is the key that types it: on Dvorak, B is
/// `KeyN` and `KeyB` types X, with Ctrl or without.
#[test]
fn shortcut_letters_follow_the_keyboard_layout() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    tap_typed(&mut app, KeyCode::KeyB, "x");
    assert_eq!(*app.world().resource::<Tool>(), Tool::Navigate);
    tap_typed(&mut app, KeyCode::KeyN, "b");
    assert_eq!(*app.world().resource::<Tool>(), Tool::BoxSelect);
    key(&mut app, KeyCode::ControlLeft, ButtonState::Pressed);
    tap_typed(&mut app, KeyCode::KeyB, "x");
    assert!(layout(&app).sidebar_open);
    tap_typed(&mut app, KeyCode::KeyN, "b");
    assert!(!layout(&app).sidebar_open);
}

#[test]
fn camera_input_goes_to_the_view_only_when_the_pointer_is_over_it() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let window = find::<Window>(&mut app);
    let at = |app: &mut App, p: Option<Vec2>, over: Option<Entity>| {
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(p);
        let mut hover = app.world_mut().resource_mut::<HoverMap>();
        hover.clear();
        if let Some(e) = over {
            hover
                .entry(PointerId::Mouse)
                .or_default()
                .insert(e, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
        }
        app.update();
        *app.world().resource::<ViewRouting>()
    };
    // Over the view, nothing on top: the camera's.
    let r = at(&mut app, Some(Vec2::new(700.0, 400.0)), None);
    assert!(r.pointer && r.keys);
    // Over the view but under a UI node (the banner, a menu): not.
    let top = region(&mut app, Region::TopBar);
    let r = at(&mut app, Some(Vec2::new(700.0, 400.0)), Some(top));
    assert!(!r.pointer);
    // Over the sidebar: not, but the view keeps its keys…
    let r = at(&mut app, Some(Vec2::new(200.0, 400.0)), Some(top));
    assert!(!r.pointer && r.keys);
    // …until a press elsewhere takes them.
    mouse(&mut app, ButtonState::Pressed);
    let r = at(&mut app, Some(Vec2::new(200.0, 400.0)), Some(top));
    assert!(!r.pointer && !r.keys);
    mouse(&mut app, ButtonState::Released);
    // A press in the view gives them back.
    mouse(&mut app, ButtonState::Pressed);
    let r = at(&mut app, Some(Vec2::new(700.0, 400.0)), None);
    assert!(r.pointer && r.keys);
    mouse(&mut app, ButtonState::Released);
    let r = at(&mut app, Some(Vec2::new(200.0, 400.0)), Some(top));
    assert!(!r.pointer && r.keys);
    // Off the window.
    let r = at(&mut app, None, None);
    assert!(!r.pointer);
    // The cursor readout only over the view.
    assert_eq!(
        app.world()
            .resource::<crate::shell::status_bar::CursorReadout>()
            .0,
        None
    );
}

#[test]
fn readouts_follow_the_sim() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let t = all_text(&mut app);
    assert!(t.contains("No world") && t.contains("RTF —") && t.contains("iter —"));
    go_live(&mut app, false);
    frames(&mut app, 1);
    let t = all_text(&mut app);
    for want in [
        "warehouse_demo",
        ".sdf",
        "00:02:14.380",
        "RTF 98%",
        "iter 134,380",
        "localhost:9004",
        "— · 1 ms",
        Tool::Navigate.hint(),
        "sel: —",
        "0 entities",
    ] {
        assert!(t.contains(want), "{want} in {t}");
    }
    // Translate needs a server that takes edits.
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    app.world_mut()
        .write_message(UiCommand::SetTool(Tool::Translate));
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains(Tool::Translate.hint()));
}

/// The connection phases and the status dot (the panels' states per
/// phase are tested in each panel, the card's line in the viewport).
#[test]
fn the_connection_phases_and_the_status_dot() {
    use Phase::*;
    let lost = Lost { since: 0.0 };

    // The phases themselves.
    let mut view = ConnectionView::default();
    let mut s = GzStatus::new("ws://h:1");
    assert_eq!(view.update(None, false, 0.0), Idle);
    assert_eq!(view.update(Some(&s), false, 0.0), Idle);
    s.attempting = true;
    assert_eq!(view.update(Some(&s), false, 0.0), Connecting);
    s.status = Status::Connected;
    assert_eq!(view.update(Some(&s), false, 0.0), Loading);
    s.status = Status::Ready;
    assert_eq!(view.update(Some(&s), false, 0.0), Loading);
    assert_eq!(view.update(Some(&s), true, 1.0), Live);
    s.status = Status::Error("closed".into());
    s.attempting = false;
    s.reconnect_at = Some(3.0);
    assert_eq!(view.update(Some(&s), true, 2.0), Lost { since: 2.0 });
    assert_eq!(view.update(Some(&s), true, 9.0), Lost { since: 2.0 });
    assert_eq!(dot_role(Lost { since: 2.0 }, Some(&s)), ColorRole::Warn);
    s.reconnect_at = None;
    assert_eq!(dot_role(Lost { since: 2.0 }, Some(&s)), ColorRole::Danger);
    assert_eq!(dot_role(Live, Some(&s)), ColorRole::Ok);
    view.connect_elsewhere();
    assert_eq!(view.update(Some(&s), true, 9.0), Idle);
    assert!(Idle.no_world() && !Live.no_world() && !lost.no_world());
}

#[test]
fn the_theme_command_recolours_the_shell() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let top = region(&mut app, Region::TopBar);
    let light = app.world().get::<BackgroundColor>(top).unwrap().0;
    app.world_mut()
        .write_message(UiCommand::SetTheme(ThemeMode::Dark));
    frames(&mut app, 2);
    let dark = app.world().get::<BackgroundColor>(top).unwrap().0;
    assert_ne!(light, dark);
    assert_eq!(
        dark,
        Theme::new(ThemeMode::Dark, false).color(ColorRole::Surface)
    );
    app.world_mut().write_message(UiCommand::ToggleTheme);
    frames(&mut app, 2);
    assert_eq!(app.world().get::<BackgroundColor>(top).unwrap().0, light);
}

// ---- PR #29 review fixes (DECISIONS.md D-52, D-53, D-56) ----

fn sidebar_splitter(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShellSplitter)>()
        .iter(world)
        .find(|(_, s)| s.0 == Side::Sidebar)
        .unwrap()
        .0
}

fn splitter_click(app: &mut App, bar: Entity, button: PointerButton, count: u8) {
    app.world_mut().trigger(PointerClick {
        entity: bar,
        pointer: pointer(),
        button,
        hit: hit(),
        duration: Duration::from_millis(80),
        count,
    });
    frames(app, 1);
}

/// A drag, released and grabbed again within the double-click time, is
/// no double-click: bevy_picking's click count runs on across drags.
/// Nor is a right-button double-click.
#[test]
fn a_splitter_resets_only_on_a_primary_double_click_without_a_drag() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let bar = sidebar_splitter(&mut app);
    let drag = |app: &mut App, dx: f32| {
        let world = app.world_mut();
        world.trigger(PointerDragStart {
            entity: bar,
            pointer: pointer(),
            button: PointerButton::Primary,
            hit: hit(),
        });
        world.flush();
        world.trigger(PointerDrag {
            entity: bar,
            pointer: pointer(),
            button: PointerButton::Primary,
            distance: Vec2::new(dx, 0.0),
            delta: Vec2::new(dx, 0.0),
        });
        frames(app, 1);
    };
    let drag_end = |app: &mut App| {
        app.world_mut().trigger(PointerDragEnd {
            entity: bar,
            pointer: pointer(),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
        });
        frames(app, 1);
    };
    // Drag and release (the release's click comes before the drag end).
    drag(&mut app, 40.0);
    splitter_click(&mut app, bar, PointerButton::Primary, 1);
    drag_end(&mut app);
    // Grab again at once to fine-tune: the second click counts 2.
    drag(&mut app, 10.0);
    splitter_click(&mut app, bar, PointerButton::Primary, 2);
    drag_end(&mut app);
    assert_eq!(layout(&app).sidebar_w, 350.0, "no reset after drags");
    // A plain click right after a drag isn't a double-click either.
    splitter_click(&mut app, bar, PointerButton::Primary, 3);
    assert_eq!(layout(&app).sidebar_w, 350.0);
    // Right-button double-click: nothing.
    splitter_click(&mut app, bar, PointerButton::Secondary, 1);
    splitter_click(&mut app, bar, PointerButton::Secondary, 2);
    assert_eq!(layout(&app).sidebar_w, 350.0);
    // Two plain primary clicks: reset.
    splitter_click(&mut app, bar, PointerButton::Primary, 1);
    splitter_click(&mut app, bar, PointerButton::Primary, 2);
    assert_eq!(layout(&app).sidebar_w, 300.0);
}

fn side_splitter(app: &mut App, side: Side) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShellSplitter)>()
        .iter(world)
        .find(|(_, s)| s.0 == side)
        .unwrap()
        .0
}

/// The side splitters are tab stops: the arrows move them by a step (the
/// inspector's is after its splitter: Left widens it), Home and End to
/// the side's limits, End short of the floor.
#[test]
fn the_side_splitters_move_with_the_keys() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let (sidebar, inspector) = (
        side_splitter(&mut app, Side::Sidebar),
        side_splitter(&mut app, Side::Inspector),
    );
    let step = crate::widgets::splitter::KEY_STEP;
    let start = layout(&app);
    focused_key(&mut app, inspector, KeyCode::ArrowLeft);
    assert_eq!(layout(&app).inspector_w, start.inspector_w + step);
    focused_key(&mut app, inspector, KeyCode::ArrowRight);
    assert_eq!(layout(&app).inspector_w, start.inspector_w);
    focused_key(&mut app, sidebar, KeyCode::ArrowRight);
    assert_eq!(layout(&app).sidebar_w, start.sidebar_w + step);
    focused_key(&mut app, sidebar, KeyCode::Home);
    assert_eq!(layout(&app).sidebar_w, 240.0);
    // At 1000 px both panels fit, and the floor, not the sidebar's 480,
    // stops End: 1000 − 56 − 300 − 240 = 404.
    let window = find::<Window>(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(1000.0, 900.0);
    frames(&mut app, 2);
    let l = layout(&app);
    assert!(l.sidebar_open && l.inspector_open);
    focused_key(&mut app, sidebar, KeyCode::End);
    let l = layout(&app);
    assert_eq!(l.sidebar_w, 1000.0 - 56.0 - l.inspector_w - 240.0);
    assert_eq!(l.viewport_w(), l.floor_w);
    // The splitter holds the layout's width and range.
    let s = *app
        .world()
        .get::<crate::widgets::Splitter>(sidebar)
        .unwrap();
    assert_eq!((s.size, s.min, s.max), (l.sidebar_w, 240.0, l.sidebar_w));
}

/// A splitter a press focused keeps its arrows, but not their chords:
/// Ctrl+→ still steps the paused sim (the PR #45 review's finding).
#[test]
fn a_focused_splitter_leaves_ctrl_arrow_to_the_sim_step() {
    use gz_bevy::connection::GzCommand;
    let mut app = shell_app((1440, 900), 1.0);
    with_viewer_keys(&mut app);
    frames(&mut app, 2);
    go_live(&mut app, true);
    sent(&mut app);
    let bar = side_splitter(&mut app, Side::Sidebar);
    // A press focuses it, as it does any tab stop.
    click(&mut app, bar);
    frames(&mut app, 1);
    assert_eq!(focused(&app), Some(bar));
    let w = layout(&app).sidebar_w;
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::ArrowRight]);
    frames(&mut app, 1);
    assert_eq!(sent(&mut app), [GzCommand::Step(1)]);
    assert_eq!(layout(&app).sidebar_w, w);
    // The plain arrow is the splitter's.
    chord(&mut app, &[KeyCode::ArrowRight]);
    frames(&mut app, 1);
    assert!(sent(&mut app).is_empty());
    assert_eq!(
        layout(&app).sidebar_w,
        w + crate::widgets::splitter::KEY_STEP
    );
}

/// The start-up collapse rules apply to the window's first real size
/// (it can come a few frames late); afterwards only the floor rule.
#[test]
fn the_start_up_rules_follow_the_first_real_window_size() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        10,
    )));
    frames(&mut app, 2);
    assert!(layout(&app).inspector_open);
    let window = find::<Window>(&mut app);
    let resize = |app: &mut App, w: f32| {
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(w, 900.0);
        frames(app, 1);
    };
    // The browser's canvas turns out 1200 wide: as if it had started so.
    resize(&mut app, 1200.0);
    let l = layout(&app);
    assert!(l.startup && l.sidebar_open && !l.inspector_open);
    // Start-up ends (30 frames and a second); then the floor rule only:
    // 1279 − 56 − 300 − 24 leaves room, and the inspector stays shut.
    frames(&mut app, 100);
    assert!(!layout(&app).startup);
    resize(&mut app, 1279.0);
    assert!(!layout(&app).inspector_open);
    resize(&mut app, 1440.0);
    assert!(
        !layout(&app).inspector_open,
        "not reopened by the floor rule"
    );
    // A user's change ends start-up at once.
    let mut l = ShellLayout::for_width(1440.0, Some(SCENE));
    l.show(Side::Inspector, true);
    l.fit(1200.0);
    assert!(
        l.inspector_open,
        "the floor rule keeps it: 1200 − 656 ≥ 240"
    );
}

/// Esc in an open menu closes the menu and nothing else (spec §13).
#[test]
fn esc_closes_a_menu_before_it_changes_the_tool() {
    use crate::shell::top_bar::transport::StepsTrigger;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    // Translate needs a server that takes edits.
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    app.world_mut()
        .write_message(UiCommand::SetTool(Tool::Translate));
    let trigger = find::<StepsTrigger>(&mut app);
    app.world_mut().trigger(bevy::ui_widgets::MenuEvent {
        source: trigger,
        action: bevy::ui_widgets::MenuAction::Open(
            bevy::input_focus::tab_navigation::NavAction::First,
        ),
    });
    frames(&mut app, 2);
    // The pointer is over the view, so its Esc (Navigate) is live.
    let window = find::<Window>(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(700.0, 400.0)));
    frames(&mut app, 1);
    assert!(app.world().resource::<ViewRouting>().keys);
    let esc = |app: &mut App| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state,
                text: None,
                repeat: false,
                window,
            });
            app.update();
        }
    };
    esc(&mut app);
    assert_eq!(*app.world().resource::<Tool>(), Tool::Translate);
    esc(&mut app);
    assert_eq!(*app.world().resource::<Tool>(), Tool::Navigate);
}

fn slot_action(app: &mut App, which: crate::shell::sidebar::SlotAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == which)
        .unwrap()
        .0
}

/// The sidebar header's collapse chevron: clicked, then Space; or focused
/// from the keyboard and hidden by ⌘B, then Space. Neither reopens the
/// sidebar.
#[test]
fn space_after_a_header_chevron_doesnt_reopen_the_panel() {
    use crate::shell::sidebar::SlotAction;
    let mut app = shell_app((1440, 900), 1.0);
    // 16 ms frames: a Space tap is the same length however slow the machine.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    frames(&mut app, 3);
    let chevron = slot_action(&mut app, SlotAction::Collapse);
    click(&mut app, chevron);
    frames(&mut app, 1);
    assert!(!layout(&app).sidebar_open);
    chord(&mut app, &[KeyCode::Space]);
    assert!(!layout(&app).sidebar_open);
    // From the keyboard: focus stays on a visible control…
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(layout(&app).sidebar_open);
    tab_to(&mut app, chevron);
    frames(&mut app, 1);
    assert_eq!(focused(&app), Some(chevron), "Tab focus stays");
    // …and leaves it when it hides.
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
    assert_eq!(focused(&app), None);
    chord(&mut app, &[KeyCode::Space]);
    assert!(!layout(&app).sidebar_open);
}

/// Click the active Scene button (collapsing the sidebar), then Space:
/// the button isn't pressed again.
#[test]
fn space_after_an_activity_button_doesnt_press_it_again() {
    let mut app = shell_app((1440, 900), 1.0);
    // 16 ms frames: a Space tap is the same length however slow the machine.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    frames(&mut app, 3);
    let world = app.world_mut();
    let scene = world
        .query::<(Entity, &ActivityFor)>()
        .iter(world)
        .find(|(_, a)| a.0 == SCENE)
        .unwrap()
        .0;
    click(&mut app, scene);
    frames(&mut app, 1);
    assert!(!layout(&app).sidebar_open);
    for key in [KeyCode::Space, KeyCode::Enter] {
        chord(&mut app, &[key]);
        assert!(!layout(&app).sidebar_open, "{key:?}");
    }
}

/// A focused control takes Enter and Space from the keyboard (Tab focus is
/// kept for M10's keyboard navigation).
#[test]
fn a_tab_focused_button_still_takes_enter() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let chevron = slot_action(&mut app, crate::shell::sidebar::SlotAction::Collapse);
    tab_to(&mut app, chevron);
    frames(&mut app, 1);
    chord(&mut app, &[KeyCode::Enter]);
    assert!(!layout(&app).sidebar_open);
}

fn transport(app: &mut App, which: super::top_bar::transport::Transport) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &super::top_bar::transport::Transport)>()
        .iter(world)
        .find(|(_, t)| **t == which)
        .unwrap()
        .0
}

/// Tab, then Space: the focused button takes Space, and the global
/// play/pause chord (gz-bevy's Space tap) stands down (DECISIONS.md
/// D-58a). Reset resets once without toggling the sim; Play/Pause
/// toggles exactly once; with nothing focused Space still toggles.
#[test]
fn a_focused_button_takes_space_from_the_play_pause_chord() {
    use super::top_bar::transport::Transport;
    use gz_bevy::connection::GzCommand;
    let mut app = shell_app((1440, 900), 1.0);
    // 16 ms frames: a Space tap is the same length however slow the machine.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    with_viewer_keys(&mut app);
    frames(&mut app, 2);
    go_live(&mut app, false);
    sent(&mut app);
    let space = |app: &mut App| {
        chord(app, &[KeyCode::Space]);
        frames(app, 1);
        sent(app)
    };
    assert_eq!(space(&mut app), [GzCommand::Pause], "no focus: the chord");
    let reset = transport(&mut app, Transport::Reset);
    tab_to(&mut app, reset);
    assert_eq!(space(&mut app), [GzCommand::Reset]);
    assert_eq!(focused(&app), Some(reset));
    let play = transport(&mut app, Transport::PlayPause);
    tab_to(&mut app, play);
    assert_eq!(space(&mut app), [GzCommand::Pause]);
    // Held: the repeats are the button's too, and the release is no tap.
    key(&mut app, KeyCode::Space, ButtonState::Pressed);
    frames(&mut app, 3);
    key(&mut app, KeyCode::Space, ButtonState::Released);
    frames(&mut app, 2);
    assert_eq!(sent(&mut app), [GzCommand::Pause]);
    // Focus gone: Space is the chord's again.
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    assert_eq!(space(&mut app), [GzCommand::Pause]);
    // A shortcut on another key still works with a button focused.
    tab_to(&mut app, reset);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
}

/// Tab pressed for real to Reset, then Space: one reset, no pause.
#[test]
fn tab_to_reset_then_space_resets_once() {
    use super::top_bar::transport::Transport;
    use gz_bevy::connection::GzCommand;
    let mut app = shell_app((1440, 900), 1.0);
    // 16 ms frames: a Space tap is the same length however slow the machine.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    with_viewer_keys(&mut app);
    frames(&mut app, 2);
    go_live(&mut app, false);
    let reset = transport(&mut app, Transport::Reset);
    for _ in 0..50 {
        chord(&mut app, &[KeyCode::Tab]);
        if focused(&app) == Some(reset) {
            break;
        }
    }
    assert_eq!(focused(&app), Some(reset));
    sent(&mut app);
    chord(&mut app, &[KeyCode::Space]);
    frames(&mut app, 1);
    assert_eq!(sent(&mut app), [GzCommand::Reset]);
}

/// Reset works in the top bar and the inspector alike: with a live world,
/// stats or not. Play/Pause needs the stats.
#[test]
fn reset_is_enabled_alike_in_the_top_bar_and_the_inspector() {
    use crate::shell::top_bar::transport::Transport;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, true);
    app.world_mut().resource_mut::<GzStatus>().sim = None;
    frames(&mut app, 1);
    // The World inspector's Reset, by its name inside the inspector.
    let inspector = region(&mut app, Region::Inspector);
    let reset = descendants(app.world(), inspector)
        .into_iter()
        .find(|e| {
            app.world()
                .get::<bevy::a11y::AccessibilityNode>(*e)
                .is_some_and(|a| a.label() == Some("Reset"))
        })
        .expect("the inspector's Reset");
    let mut transport = |which: Transport| {
        let world = app.world_mut();
        world
            .query::<(Entity, &Transport)>()
            .iter(world)
            .find(|(_, t)| **t == which)
            .unwrap()
            .0
    };
    let (top, play) = (transport(Transport::Reset), transport(Transport::PlayPause));
    let enabled = |app: &App, e| app.world().get::<InteractionDisabled>(e).is_none();
    assert!(enabled(&app, reset) && enabled(&app, top));
    assert!(!enabled(&app, play));
    let s = stats(true);
    assert!(Transport::Step.enabled(Phase::Live, Some(&s)));
    assert!(!Transport::Reset.enabled(Phase::Loading, Some(&s)));
}

/// The shell is built from the UI's theme, fonts and widgets: without
/// `GzUiPlugin` it says so rather than failing later.
#[test]
#[should_panic(expected = "ShellPlugin needs GzUiPlugin")]
fn the_shell_needs_the_ui_plugin() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, crate::shell::ShellPlugins));
    app.finish();
}

/// Tab pressed for real walks the shell's regions in the spec's order
/// (§6.5: top bar → activity bar → sidebar → viewport → inspector →
/// status bar) and comes round again, never landing in a region that
/// isn't displayed: a collapsed sidebar gives its turn to its bar, and
/// gets it back when reopened. Shift+Tab walks the other way; the second
/// slot's buttons are stops while it is open, and the walk is as before
/// once it closes.
#[test]
fn tab_walks_the_regions_in_order_and_skips_hidden_ones() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    // From no focus, Tab by Tab until focus comes back to the first stop.
    let walk = |app: &mut App, back: bool| -> Vec<(Entity, Region)> {
        app.world_mut()
            .resource_mut::<bevy::input_focus::InputFocus>()
            .clear();
        let mut seen: Vec<(Entity, Region)> = Vec::new();
        for _ in 0..200 {
            if back {
                key(app, KeyCode::ShiftLeft, ButtonState::Pressed);
            }
            key(app, KeyCode::Tab, ButtonState::Pressed);
            app.update();
            key(app, KeyCode::Tab, ButtonState::Released);
            if back {
                key(app, KeyCode::ShiftLeft, ButtonState::Released);
            }
            app.update();
            let e = focused(app).expect("Tab focuses something");
            if seen.first().is_some_and(|s| s.0 == e) {
                break;
            }
            let world = app.world();
            let region =
                std::iter::successors(Some(e), |x| world.get::<ChildOf>(*x).map(ChildOf::parent))
                    .find_map(|a| world.get::<Region>(a).copied())
                    .expect("in a region");
            seen.push((e, region));
        }
        seen
    };
    let regions = |seen: &[(Entity, Region)]| {
        let mut r: Vec<Region> = seen.iter().map(|s| s.1).collect();
        r.dedup();
        r
    };
    use Region::*;
    let all = [
        TopBar,
        ActivityBar,
        Sidebar,
        ViewportArea,
        Inspector,
        StatusBar,
    ];
    let first = walk(&mut app, false);
    assert_eq!(regions(&first), all);
    // Disabled controls are no stops (Scene's Expand all, Collapse all).
    for (e, _) in &first {
        assert!(app.world().get::<InteractionDisabled>(*e).is_none());
    }
    // Shift+Tab: the same stops the other way round.
    let mut back = walk(&mut app, true);
    back.reverse();
    assert_eq!(back, first);
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert_eq!(
        regions(&walk(&mut app, false)),
        [
            TopBar,
            ActivityBar,
            SidebarBar,
            ViewportArea,
            Inspector,
            StatusBar
        ]
    );
    // Reopened: the sidebar is visible to Tab again.
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert_eq!(walk(&mut app, false), first);
    // A second slot adds its header's buttons, and takes them away again.
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: CONSOLE,
        slot: crate::shell::Slot::Secondary,
    });
    frames(&mut app, 2);
    let two = walk(&mut app, false);
    assert_eq!(regions(&two), all);
    assert!(two.len() > first.len());
    let secondary = |app: &App, e: Entity| {
        let world = app.world();
        std::iter::successors(Some(e), |x| world.get::<ChildOf>(*x).map(ChildOf::parent)).any(|a| {
            world.get::<crate::shell::sidebar::SlotView>(a)
                == Some(&crate::shell::sidebar::SlotView(
                    crate::shell::Slot::Secondary,
                ))
        })
    };
    assert!(two.iter().any(|(e, _)| secondary(&app, *e)));
    app.world_mut().write_message(UiCommand::CloseSecondary);
    frames(&mut app, 2);
    let closed = walk(&mut app, false);
    assert_eq!(closed.len(), first.len());
    assert_eq!(regions(&closed), all);
}

/// A window shorter than the inspector's content, or a panel with a long
/// list, never grows the shell past the window (DECISIONS.md D-69: Bevy
/// puts UI roots in a grid whose items are at least as tall as their
/// content).
#[test]
fn the_shell_never_grows_past_a_short_window() {
    let mut app = shell_app((1440, 520), 1.0);
    frames(&mut app, 3);
    assert!(layout(&app).inspector_open);
    let root = region(&mut app, Region::Root);
    assert_eq!(rect(&app, root).1, Vec2::new(1440.0, 520.0));
    let status = region(&mut app, Region::StatusBar);
    let (min, size) = rect(&app, status);
    assert_eq!(min.y + size.y, 520.0);
}
