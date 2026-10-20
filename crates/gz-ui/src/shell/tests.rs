//! The shell (M2) in a headless app: region rects at 1440×900 and the
//! collapse rules at narrower widths, the collapsed bars, the side
//! splitters, keys and commands, pointer routing, the readouts, the
//! connection phases and the text formats. Each region's own tests are in
//! its folder.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzStatus, SimStats};
use gz_bevy::view::controls::ViewRouting;
use gz_core::transport::Status;

use super::activity_bar::ActivityFor;
use super::collapsed_bar::ExpandButton;
use super::connection::{dot_role, ConnectionView, Phase};
use super::layout::MIN_SLOT_H;
use super::regions::Region;
use super::sidebar::body::PanelBody;
use super::splitter::ShellSplitter;
use super::status_bar::StatusItem;
use super::top_bar::tools::ToolChoice;
use super::viewport::connect_card::normalize_address;
use super::viewport::view::ViewNode;
use crate::shell::{format, Panel, ShellLayout, Side, Tool, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
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
    assert_eq!((l.primary, l.secondary), (Some(Panel::Scene), None));
    let world = app.world_mut();
    let bodies: Vec<Panel> = world
        .query::<&PanelBody>()
        .iter(world)
        .map(|b| b.0)
        .collect();
    assert_eq!(bodies, [Panel::Scene]);
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
        .find(|(_, a)| a.0 == Panel::Scene)
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
    let mut l = ShellLayout::for_width(1440.0);
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
        let mut l = ShellLayout::for_width(w);
        l.show(Side::Sidebar, true);
        l.show(Side::Inspector, true);
        assert!(l.viewport_w() >= 240.0, "{w}");
    }
    let mut tiny = ShellLayout::for_width(400.0);
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
    let l = ShellLayout::for_width(1440.0);
    // The sidebar widens dragging right, the inspector dragging left.
    assert_eq!(l.drag_width(Side::Sidebar, 300.0, 50.0), 350.0);
    assert_eq!(l.drag_width(Side::Inspector, 300.0, -50.0), 350.0);
    assert_eq!(l.drag_width(Side::Sidebar, 300.0, -500.0), 240.0);
    assert_eq!(l.drag_width(Side::Inspector, 300.0, 500.0), 260.0);
    assert_eq!(l.drag_width(Side::Sidebar, 300.0, 500.0), 480.0);
    // Short of the floor: 1440 − 56 − 300 − 240 = 844 is the most, so at
    // 1000 px only 1000 − 56 − 300 − 240 = 404.
    let narrow = ShellLayout {
        window_w: 1000.0,
        inspector_open: true,
        sidebar_open: true,
        ..l
    };
    assert_eq!(narrow.drag_width(Side::Sidebar, 300.0, 400.0), 404.0);
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
    };
    // Routing is rewritten each frame from the pointer: hold it off.
    app.world_mut()
        .resource_mut::<crate::shell::routing::ViewFocus>()
        .0 = false;
    chord(&mut app, &[KeyCode::KeyT]);
    assert_eq!(*app.world().resource::<Tool>(), Tool::BoxSelect);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
    // Panel shortcuts open their panel.
    chord(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyC],
    );
    let l = layout(&app);
    assert!(l.sidebar_open);
    assert_eq!(l.primary, Some(Panel::Console));
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
    app.world_mut()
        .write_message(UiCommand::SetTool(Tool::Translate));
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains(Tool::Translate.hint()));
}

#[test]
fn formatting() {
    assert_eq!(format::clock(134.38), "00:02:14.380");
    assert_eq!(format::clock(0.0), "00:00:00.000");
    assert_eq!(format::clock(-1.0), "00:00:00.000");
    assert_eq!(format::clock(3600.0 * 101.0 + 0.0005), "101:00:00.001");
    assert_eq!(format::clock(59.9996), "00:01:00.000");
    let rtf = |r: f64, paused| {
        format::rtf(Some(&SimStats {
            real_time_factor: r,
            ..stats(paused)
        }))
    };
    assert_eq!(rtf(0.98, false), ("RTF 98%".into(), ColorRole::Ok));
    assert_eq!(rtf(0.9, false), ("RTF 90%".into(), ColorRole::Ok));
    assert_eq!(rtf(0.899, false).1, ColorRole::Warn);
    assert_eq!(rtf(0.5, false).1, ColorRole::Warn);
    assert_eq!(rtf(0.49, false), ("RTF 49%".into(), ColorRole::Danger));
    assert_eq!(rtf(0.98, true), ("paused".into(), ColorRole::Disabled));
    assert_eq!(format::rtf(None), ("RTF —".into(), ColorRole::Disabled));
    assert_eq!(format::thousands(0), "0");
    assert_eq!(format::thousands(999), "999");
    assert_eq!(format::thousands(1000), "1,000");
    assert_eq!(format::thousands(1_234_567), "1,234,567");
    assert_eq!(format::iterations(None), "iter —");
    assert_eq!(format::step(0.001), "1 ms");
    assert_eq!(format::step(0.0005), "0.5 ms");
    assert_eq!(format::step(2.0), "2 s");
    assert_eq!(format::physics(None, None), "—");
    assert_eq!(format::physics(None, Some(0.0)), "—");
    assert_eq!(format::physics(Some("dart"), Some(0.001)), "dart · 1 ms");
    assert_eq!(format::physics(Some("dart"), None), "dart · —");
    assert_eq!(format::fps(Some(59.6)), "60 fps");
    assert_eq!(format::fps(None), "— fps");
    assert_eq!(format::address("ws://localhost:11345/"), "localhost:11345");
    assert_eq!(format::address("localhost:9002"), "localhost:9002");
    assert_eq!(
        format::cursor(Some([2.414, -0.731])),
        "cursor 2.41, \u{2212}0.73"
    );
    assert_eq!(format::cursor(Some([-0.001, 0.0])), "cursor 0.00, 0.00");
    assert_eq!(format::cursor(None), "cursor —");
    assert!(format::rtf_detail(Some(&stats(false))).contains("134,380 iterations · step 1 ms"));
    assert!(format::rtf_detail(Some(&SimStats {
        step_size: 0.0,
        ..stats(true)
    }))
    .ends_with("step —"));
    assert_eq!(format::rtf_detail(None), "No statistics yet");
    assert_eq!(normalize_address(" gz:9002 "), "ws://gz:9002");
    assert_eq!(normalize_address("wss://gz"), "wss://gz");
    for t in Tool::ALL {
        assert!(!t.hint().is_empty() && !t.name().is_empty());
    }
}

/// The connection phases and the status dot (the panels' states per
/// phase are tested in each panel, the card's line in the viewport).
#[test]
fn panel_states_follow_the_connection() {
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
