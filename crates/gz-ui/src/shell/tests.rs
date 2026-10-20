//! The shell (M2): region rects at 1440×900 and the collapse rules at
//! narrower widths, splitters, keys, the camera viewport at several scale
//! factors, pointer routing, the activity bar, the transport, formatting
//! and the disconnected banner, in a headless app.

use std::time::Duration;

use bevy::camera::visibility::VisibilitySystems;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::ButtonState;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{ComputedNode, InteractionDisabled, UiGlobalTransform};
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::{GzCommand, GzReconnect, GzStatus, SimStats};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::controls::{ActiveScopes, ViewPlacementSystems, ViewRouting};
use gz_bevy::view::input::{InputMap, Platform, Scope};
use gz_bevy::view::{apply_view_placement, ViewOrigin, ViewPlacement};
use gz_core::transport::Status;

use crate::shell::activity_bar::{ActivityBadges, ActivityFor, SettingsButton};
use crate::shell::connection::{banner, dot_role, ConnectionView, Phase, LOST_LONG_SECS};
use crate::shell::layout::MIN_SLOT_H;
use crate::shell::sidebar::{panel_count, panel_view, PanelAction, PanelBody, PanelView, SlotView};
use crate::shell::status_bar::StatusItem;
use crate::shell::top_bar::tools::ToolChoice;
use crate::shell::top_bar::transport::{steps_label, StepsOption, Transport};
use crate::shell::viewport::banner::DisconnectBanner;
use crate::shell::viewport::connect_card::{
    card_status, normalize_address, AddressField, ConnectButton, ConnectCard,
};
use crate::shell::viewport::view::{DimOverlay, ViewNode};
use crate::shell::{
    format, Panel, Region, ShellLayout, ShellPlugin, ShellSplitter, Side, Slot, StepsPerClick,
    Tool, UiCommand,
};
use crate::test_support::{descendants, frames, ui_app_with};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Fill, Theme, ThemeMode};

/// GzCommands and reconnects sent so far.
#[derive(Resource, Default)]
struct Sent(Vec<GzCommand>, Vec<GzReconnect>);

fn collect(
    mut sim: MessageReader<GzCommand>,
    mut again: MessageReader<GzReconnect>,
    mut sent: ResMut<Sent>,
) {
    sent.0.extend(sim.read().cloned());
    sent.1.extend(again.read().cloned());
}

/// The full UI headless, `size` logical pixels at `scale`, with a 3D main
/// camera and the gz-bevy resources the shell reads.
fn shell_app(size: (u32, u32), scale: f32) -> App {
    let mut app = ui_app_with(ThemeMode::Light, size, scale, |app| {
        app.add_plugins((
            crate::GzUiPlugin {
                theme: ThemeMode::Light,
            },
            ShellPlugin { camera: false },
        ));
    });
    let mut map = InputMap::builtin();
    map.platform = Platform::Linux;
    app.insert_resource(map)
        .insert_resource(ActiveScopes(vec![
            Scope::Global,
            Scope::Viewport,
            Scope::Minimal,
        ]))
        .insert_resource(GzStatus::new("ws://localhost:9004"))
        .init_resource::<GzIndex>()
        .init_resource::<Sent>()
        .add_systems(Last, collect)
        // gz-bevy's half of the view placement (ViewerControls adds it in
        // the app).
        .configure_sets(
            PostUpdate,
            ViewPlacementSystems
                .after(bevy::ui::UiSystems::Layout)
                .before(VisibilitySystems::UpdateFrusta),
        )
        .add_systems(
            PostUpdate,
            apply_view_placement.in_set(ViewPlacementSystems),
        );
    let physical = UVec2::new(
        (size.0 as f32 * scale) as u32,
        (size.1 as f32 * scale) as u32,
    );
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: physical,
                    scale_factor: scale,
                }),
                ..default()
            },
            ..default()
        },
        Projection::Perspective(PerspectiveProjection::default()),
        MainCamera,
    ));
    app
}

fn find<C: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .single(world)
        .expect("one")
}

fn region(app: &mut App, r: Region) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Region)>()
        .iter(world)
        .find(|(_, x)| **x == r)
        .map(|(e, _)| e)
        .expect("region")
}

/// A node's rect in UI logical pixels (`min`, `size`).
fn rect(app: &App, e: Entity) -> (Vec2, Vec2) {
    let n = app.world().get::<ComputedNode>(e).expect("laid out");
    let t = app.world().get::<UiGlobalTransform>(e).expect("placed");
    let k = n.inverse_scale_factor();
    let size = n.size() * k;
    (t.translation * k - size / 2.0, size)
}

fn displayed(app: &App, e: Entity) -> bool {
    app.world().get::<Node>(e).unwrap().display != Display::None
}

fn layout(app: &App) -> ShellLayout {
    app.world().resource::<ShellLayout>().clone()
}

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
    use crate::shell::ExpandButton;
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

fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
    let window = find::<Window>(app);
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window,
    });
}

fn chord(app: &mut App, keys: &[KeyCode]) {
    for k in keys {
        key(app, *k, ButtonState::Pressed);
    }
    app.update();
    for k in keys.iter().rev() {
        key(app, *k, ButtonState::Released);
    }
    app.update();
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

fn mouse(app: &mut App, state: ButtonState) {
    let window = find::<Window>(app);
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
}

/// The placed view: its window rect (logical), texture size, and whether
/// the main camera renders into that texture.
fn placed(app: &mut App) -> (Rect, UVec2, bool) {
    let placement = app.world().resource::<ViewPlacement>().clone();
    let handle = placement.image.clone().expect("a texture");
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(&handle)
        .unwrap()
        .size();
    let world = app.world_mut();
    let target = world
        .query_filtered::<&bevy::camera::RenderTarget, With<MainCamera>>()
        .single(world)
        .unwrap();
    let on = matches!(target, bevy::camera::RenderTarget::Image(t) if t.handle == handle);
    (placement.rect.unwrap(), size, on)
}

#[test]
fn the_3d_view_follows_the_view_node_at_any_scale() {
    for scale in [1.0, 1.5, 2.0] {
        let mut app = shell_app((1440, 900), scale);
        frames(&mut app, 3);
        let rect = Rect::new(356.0, 44.0, 1140.0, 872.0);
        let (r, size, on) = placed(&mut app);
        assert_eq!(r, rect, "{scale}");
        assert_eq!(size, (rect.size() * scale).round().as_uvec2(), "{scale}");
        assert!(on);
        // The node shows the texture.
        let view = find::<ViewNode>(&mut app);
        let handle = app.world().resource::<ViewPlacement>().image.clone();
        assert_eq!(
            app.world().get::<ImageNode>(view).map(|i| i.image.clone()),
            handle
        );
        // The camera's size and projection change in the same frame.
        let world = app.world_mut();
        let (camera, origin, p) = world
            .query_filtered::<(&Camera, &ViewOrigin, &Projection), With<MainCamera>>()
            .single(world)
            .unwrap();
        assert_eq!(camera.logical_viewport_size(), Some(rect.size()));
        assert_eq!(origin.0, rect.min);
        let Projection::Perspective(p) = p else {
            panic!()
        };
        assert!((p.aspect_ratio - 784.0 / 828.0).abs() < 1e-3, "{scale}");
        // Collapsing the inspector widens it at once (one update: the
        // layout and the texture follow in the same frame), less the
        // collapsed inspector's bar.
        app.world_mut().write_message(UiCommand::ToggleInspector);
        app.update();
        let (r, size, _) = placed(&mut app);
        assert_eq!(r.width(), 1060.0, "{scale}");
        assert_eq!(size.x, (1060.0 * scale).round() as u32, "{scale}");
    }
    // UI scale 1.25: everything is a quarter bigger, and the layout fits
    // the 1152 logical pixels that leaves (so the inspector starts
    // collapsed).
    let mut app = shell_app((1440, 900), 1.0);
    app.world_mut().resource_mut::<Theme>().scale = 1.25;
    frames(&mut app, 4);
    assert_eq!(app.world().resource::<UiScale>().0, 1.25);
    let l = layout(&app);
    assert!(l.sidebar_open && !l.inspector_open);
    let (r, size, _) = placed(&mut app);
    // The collapsed inspector's 24 px bar is at the right edge.
    let x = (56.0 + 300.0) * 1.25;
    let right = 1440.0 - 24.0 * 1.25;
    assert_eq!(r, Rect::new(x, 44.0 * 1.25, right, 900.0 - 28.0 * 1.25));
    assert_eq!(size, UVec2::new(1410 - 445, 900 - 55 - 35));
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
fn activity_buttons_open_collapse_and_use_the_second_slot() {
    let mut l = ShellLayout::for_width(1440.0);
    assert!(l.is_active(Panel::Scene));
    // The active one collapses the sidebar and stays selected; another
    // click brings it back.
    l.activity_click(Panel::Scene, false);
    assert!(!l.sidebar_open && l.is_active(Panel::Scene));
    l.activity_click(Panel::Scene, false);
    assert!(l.sidebar_open && l.is_active(Panel::Scene));
    // Collapsed, a different panel expands the sidebar with that panel.
    l.activity_click(Panel::Scene, false);
    l.activity_click(Panel::Assets, false);
    assert!(l.sidebar_open && l.primary == Some(Panel::Assets));
    l.activity_click(Panel::Scene, false);
    // An inactive one replaces the primary.
    l.activity_click(Panel::Topics, false);
    assert_eq!((l.primary, l.secondary), (Some(Panel::Topics), None));
    // Shift: the secondary.
    l.activity_click(Panel::Plot, true);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Plot))
    );
    assert!(l.is_active(Panel::Topics) && l.is_active(Panel::Plot));
    // A panel already in the other slot swaps there.
    l.activity_click(Panel::Topics, true);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Topics))
    );
    l.open_in(Panel::Topics, Slot::Primary);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Plot))
    );
    l.swap_slots();
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Topics))
    );
    l.close_secondary();
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    // Shift on the only panel keeps it in the primary.
    l.activity_click(Panel::Plot, true);
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    l.swap_slots();
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    // A collapsed sidebar reopens with a click on a panel it holds.
    l.show(Side::Sidebar, false);
    l.activity_click(Panel::Plot, false);
    assert!(l.sidebar_open);

    // Through the buttons.
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let world = app.world_mut();
    let buttons: Vec<(Entity, Panel)> = world
        .query::<(Entity, &ActivityFor)>()
        .iter(world)
        .map(|(e, a)| (e, a.0))
        .collect();
    let button = |p: Panel| buttons.iter().find(|b| b.1 == p).unwrap().0;
    let active = |app: &App, p: Panel| app.world().get::<Active>(button(p)).is_some();
    assert!(active(&app, Panel::Scene) && !active(&app, Panel::Topics));
    app.world_mut().trigger(Activate {
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(active(&app, Panel::Topics) && !active(&app, Panel::Scene));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    app.world_mut().trigger(Activate {
        entity: button(Panel::Console),
    });
    frames(&mut app, 2);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    let l = layout(&app);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Console))
    );
    assert!(active(&app, Panel::Topics) && active(&app, Panel::Console));
    // Both slots are laid out, 55/45, with the row splitter between.
    let world = app.world_mut();
    let slots: Vec<(Slot, Entity)> = world
        .query::<(&SlotView, Entity)>()
        .iter(world)
        .map(|(s, e)| (s.0, e))
        .collect();
    let h = |s: Slot| rect(&app, slots.iter().find(|x| x.0 == s).unwrap().1).1.y;
    let (p, s) = (h(Slot::Primary), h(Slot::Secondary));
    assert!((p + s + 6.0 - 828.0).abs() < 0.5, "{p} {s}");
    assert!((p / (p + s) - 0.55).abs() < 0.01, "{p} {s}");
    // Clicking the active one collapses the sidebar; both stay selected,
    // and pressed to AccessKit.
    app.world_mut().trigger(Activate {
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
    assert!(active(&app, Panel::Topics) && active(&app, Panel::Console));
    let toggled = |app: &App, p: Panel| {
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(button(p))
            .unwrap()
            .toggled()
    };
    assert_eq!(toggled(&app, Panel::Topics), Some(accesskit::Toggled::True));
    assert_eq!(toggled(&app, Panel::Scene), Some(accesskit::Toggled::False));
    // Clicked again, it expands.
    app.world_mut().trigger(Activate {
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(layout(&app).sidebar_open);
    // Badges show their counts and hide at zero.
    app.world_mut().resource_mut::<ActivityBadges>().plot = 2;
    frames(&mut app, 1);
    let plot = button(Panel::Plot);
    let badge = descendants(app.world(), plot)
        .into_iter()
        .find(|e| {
            app.world()
                .get::<crate::widgets::badge::Badge>(*e)
                .is_some()
        })
        .unwrap();
    assert!(displayed(&app, badge));
    let b = ActivityBadges {
        plot: 0,
        console_warnings: 2,
        console_errors: 1,
    };
    assert_eq!(b.badge(Panel::Plot), None);
    assert_eq!(
        b.badge(Panel::Console),
        Some((crate::widgets::BadgeKind::Error, 3))
    );
    assert_eq!(
        ActivityBadges {
            console_warnings: 2,
            ..default()
        }
        .badge(Panel::Console),
        Some((crate::widgets::BadgeKind::Warning, 2))
    );
    // Settings switches the theme (until the Settings dialog exists).
    let settings = find::<SettingsButton>(&mut app);
    app.world_mut().trigger(Activate { entity: settings });
    frames(&mut app, 1);
    assert!(app.world().resource::<Theme>().is_dark);
}

fn stats(paused: bool) -> SimStats {
    SimStats {
        paused,
        sim_time: 134.38,
        real_time: 140.0,
        real_time_factor: 0.98,
        iterations: 134_380,
        step_size: 0.001,
    }
}

fn go_live(app: &mut App, paused: bool) {
    let world = app.world_mut();
    let mut s = world.resource_mut::<GzStatus>();
    s.status = Status::Ready;
    s.world = Some("warehouse_demo".into());
    s.sim = Some(stats(paused));
    world.resource_mut::<GzIndex>().scene = Some(gz_core::scene::SceneDesc::from_msg(
        &Default::default(),
        Default::default(),
    ));
    frames(app, 2);
}

fn sent(app: &mut App) -> Vec<GzCommand> {
    std::mem::take(&mut app.world_mut().resource_mut::<Sent>().0)
}

#[test]
fn transport_buttons_send_sim_commands() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let world = app.world_mut();
    let buttons: Vec<(Entity, Transport)> = world
        .query::<(Entity, &Transport)>()
        .iter(world)
        .map(|(e, t)| (e, *t))
        .collect();
    let button = |t: Transport| buttons.iter().find(|b| b.1 == t).unwrap().0;
    let press = |app: &mut App, t: Transport| {
        app.world_mut().trigger(Activate { entity: button(t) });
        frames(app, 1);
        sent(app)
    };
    // Not connected: disabled, nothing sent.
    assert!(app
        .world()
        .get::<InteractionDisabled>(button(Transport::PlayPause))
        .is_some());
    assert!(press(&mut app, Transport::PlayPause).is_empty());
    go_live(&mut app, true);
    assert_eq!(press(&mut app, Transport::PlayPause), [GzCommand::Play]);
    assert_eq!(press(&mut app, Transport::Step), [GzCommand::Step(1)]);
    app.world_mut().resource_mut::<StepsPerClick>().0 = 10;
    assert_eq!(press(&mut app, Transport::Step), [GzCommand::Step(10)]);
    assert_eq!(press(&mut app, Transport::Reset), [GzCommand::Reset]);
    // Running: Pause, and Step is disabled.
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(false));
    frames(&mut app, 1);
    assert_eq!(press(&mut app, Transport::PlayPause), [GzCommand::Pause]);
    assert!(press(&mut app, Transport::Step).is_empty());
    // The play button shows pause while running.
    let icon = descendants(app.world(), button(Transport::PlayPause))
        .into_iter()
        .find_map(|e| app.world().get::<crate::assets::icons::Icon>(e).copied())
        .unwrap();
    assert_eq!(icon.id, crate::assets::icons::IconId::Pause);
    // The steps menu.
    let world = app.world_mut();
    let option = world
        .query::<(Entity, &StepsOption)>()
        .iter(world)
        .find(|(_, o)| o.0 == 100)
        .unwrap()
        .0;
    world.trigger(Activate { entity: option });
    frames(&mut app, 1);
    assert_eq!(app.world().resource::<StepsPerClick>().0, 100);
    assert_eq!(steps_label(1), "1 step");
    assert_eq!(steps_label(100), "100 steps");
}

/// Every text in the shell, joined.
fn all_text(app: &mut App) -> String {
    let world = app.world_mut();
    let mut out: Vec<String> = world
        .query::<&Text>()
        .iter(world)
        .map(|t| t.0.clone())
        .collect();
    out.extend(world.query::<&TextSpan>().iter(world).map(|t| t.0.clone()));
    out.join("\n")
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

#[test]
fn panel_states_follow_the_connection() {
    use Phase::*;
    let lost = Lost { since: 0.0 };
    assert_eq!(panel_view(Panel::Scene, Live), PanelView::Ready);
    assert!(matches!(
        panel_view(Panel::Scene, Connecting),
        PanelView::Empty {
            message: "No world loaded",
            ..
        }
    ));
    assert_eq!(panel_view(Panel::Scene, Loading), PanelView::Loading);
    assert_eq!(
        panel_view(Panel::Scene, lost),
        PanelView::Error {
            message: "Lost connection to server",
            actions: vec![PanelAction::Reconnect]
        }
    );
    assert!(matches!(
        panel_view(Panel::Scene, Idle),
        PanelView::Empty {
            message: "No world loaded",
            ..
        }
    ));
    assert!(matches!(
        panel_view(Panel::Topics, Live),
        PanelView::Empty {
            message: "No topics yet",
            ..
        }
    ));
    assert!(matches!(
        panel_view(Panel::Topics, lost),
        PanelView::Empty {
            message: "Not connected",
            ..
        }
    ));
    assert_eq!(panel_view(Panel::Topics, Loading), PanelView::Loading);
    for p in Panel::ALL {
        let _ = panel_view(p, Live);
        assert!(!p.title().is_empty());
    }
    assert_eq!(panel_count(Panel::Scene, Idle, None), None);
    assert_eq!(panel_count(Panel::Plot, Live, None), None);
    for a in [
        PanelAction::Reconnect,
        PanelAction::OpenWorld,
        PanelAction::NewWorld,
        PanelAction::AddField,
    ] {
        assert_eq!(a.available(), a == PanelAction::Reconnect);
        assert!(!a.label().is_empty());
    }

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

    // The card's line.
    let mut s = GzStatus::new("ws://h:1");
    assert_eq!(card_status(Idle, None), "Not connected");
    assert_eq!(card_status(Idle, Some(&s)), "Not connected");
    s.attempting = true;
    assert_eq!(card_status(Connecting, Some(&s)), "Connecting to h:1…");
    s.attempting = false;
    s.status = Status::Error("refused".into());
    assert_eq!(
        card_status(Connecting, Some(&s)),
        "Couldn't connect: refused · retrying (attempt 1)"
    );
    assert_eq!(card_status(Idle, Some(&s)), "Couldn't connect: refused");
    assert_eq!(
        card_status(Loading, Some(&s)),
        "Connected to h:1 · loading the world…"
    );
    assert_eq!(card_status(Live, Some(&s)), "Connected to h:1");
    assert_eq!(card_status(lost, Some(&s)), "Lost connection to h:1");
}

#[test]
fn the_banner_turns_red_after_a_minute() {
    let mut s = GzStatus::new("ws://localhost:11345");
    s.reconnect_at = Some(1.0);
    s.attempts = 3;
    let lost = Phase::Lost { since: 10.0 };
    let b = banner(lost, Some(&s), 10.0).unwrap();
    assert_eq!(
        b.text,
        "Lost connection to localhost:11345 · Reconnecting… (3)"
    );
    assert!(!b.long);
    assert_eq!(b.roles(), (ColorRole::WarnSoft, ColorRole::WarnText));
    let b = banner(lost, Some(&s), 10.0 + LOST_LONG_SECS).unwrap();
    assert!(b.long);
    assert_eq!(b.roles(), (ColorRole::DangerSoft, ColorRole::Danger));
    s.reconnect_at = None;
    assert_eq!(
        banner(lost, Some(&s), 10.0).unwrap().text,
        "Lost connection to localhost:11345"
    );
    assert_eq!(banner(Phase::Live, Some(&s), 10.0), None);
    assert_eq!(banner(lost, None, 10.0), None);

    // In the app, with time faked a second a frame.
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
    frames(&mut app, 2);
    // No world yet: the card, no banner.
    let card = find::<ConnectCard>(&mut app);
    let bar = find::<DisconnectBanner>(&mut app);
    let dim = find::<DimOverlay>(&mut app);
    assert!(displayed(&app, card) && !displayed(&app, bar) && !displayed(&app, dim));
    let view = find::<ViewNode>(&mut app);
    assert_eq!(
        app.world().get::<Visibility>(view),
        Some(&Visibility::Hidden)
    );
    go_live(&mut app, true);
    assert_eq!(
        app.world().get::<Visibility>(view),
        Some(&Visibility::Inherited)
    );
    frames(&mut app, 2);
    assert!(!displayed(&app, card), "the card fades out");
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Error("closed".into());
        s.reconnect_at = Some(1e9);
        s.attempts = 1;
        s.sim = None;
    }
    frames(&mut app, 2);
    assert!(displayed(&app, bar) && displayed(&app, dim) && !displayed(&app, card));
    assert_eq!(app.world().get::<Fill>(bar).unwrap().0, ColorRole::WarnSoft);
    assert!(all_text(&mut app).contains("Lost connection to localhost:9004 · Reconnecting… (1)"));
    let world = app.world_mut();
    let buttons: Vec<(Entity, ConnectButton)> = world
        .query::<(Entity, &ConnectButton)>()
        .iter(world)
        .map(|(e, b)| (e, *b))
        .collect();
    let button = |b: ConnectButton| buttons.iter().find(|x| x.1 == b).unwrap().0;
    assert!(!displayed(&app, button(ConnectButton::Elsewhere)));
    frames(&mut app, LOST_LONG_SECS as usize);
    assert_eq!(
        app.world().get::<Fill>(bar).unwrap().0,
        ColorRole::DangerSoft
    );
    assert!(displayed(&app, button(ConnectButton::Elsewhere)));
    // Retry now asks for a reconnect; "another server" shows the card.
    app.world_mut().trigger(Activate {
        entity: button(ConnectButton::Retry),
    });
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
    app.world_mut().trigger(Activate {
        entity: button(ConnectButton::Elsewhere),
    });
    frames(&mut app, 1);
    assert!(displayed(&app, card));
    // The card's Connect: its address (prefilled), to the server.
    app.world_mut().trigger(Activate {
        entity: button(ConnectButton::Connect),
    });
    frames(&mut app, 1);
    let again = app.world().resource::<Sent>().1.clone();
    assert_eq!(
        again.last(),
        Some(&GzReconnect {
            url: Some("ws://localhost:9004".into())
        })
    );
    // The Scene panel shows the error with Reconnect.
    assert!(all_text(&mut app).contains("Lost connection to server"));
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

/// The top bar is `surface` (white in light) in every theme and after
/// every switch, with its tool and transport buttons transparent on it at
/// rest (no group containers; DECISIONS.md D-48) and the current tool in
/// the activity button's active look.
#[test]
fn the_top_bar_buttons_sit_on_surface() {
    use crate::theme::visuals::Visuals;
    use crate::widgets::segmented::SegmentedGroup;
    use bevy::ui::Checked;
    use bevy::window::{PrimaryWindow, WindowTheme, WindowThemeChanged};
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let check = |app: &mut App, dark: bool| {
        let theme = app.world().resource::<Theme>().clone();
        assert_eq!(theme.is_dark, dark);
        let top = region(app, Region::TopBar);
        let w = app.world();
        let surface = theme.color(ColorRole::Surface);
        assert_eq!(w.get::<BackgroundColor>(top).unwrap().0, surface);
        if !dark {
            assert_eq!(crate::gallery::hex(surface), format!("{}FFFFFF", '#'));
        }
        let mut buttons = 0;
        for e in descendants(w, top) {
            let tool = w.get::<ToolChoice>(e).is_some();
            let transport = w.get::<Transport>(e).is_some();
            let group = w.get::<SegmentedGroup>(e).is_some();
            if !(tool || transport || group) {
                continue;
            }
            let bg = w.get::<BackgroundColor>(e).unwrap().0;
            let border = w.get::<BorderColor>(e).map_or(Color::NONE, |b| b.top);
            if w.get::<Checked>(e).is_some() {
                // The current tool: accent_soft with the accent_ring.
                assert_eq!(bg, theme.color(ColorRole::AccentSoft));
                assert_eq!(border, theme.color(ColorRole::AccentRing));
            } else {
                assert_eq!(bg.alpha(), 0.0, "{e} rests transparent");
                assert_eq!(border.alpha(), 0.0, "{e} has no border");
            }
            if tool || transport {
                assert_eq!(
                    *w.get::<Visuals>(e).unwrap(),
                    crate::widgets::activity::activity_visuals()
                );
                buttons += 1;
            }
        }
        assert_eq!(buttons, Tool::ALL.len() + 3);
    };
    check(&mut app, false);
    // The Settings command, both ways.
    app.world_mut()
        .write_message(UiCommand::SetTheme(ThemeMode::Dark));
    frames(&mut app, 2);
    check(&mut app, true);
    app.world_mut()
        .write_message(UiCommand::SetTheme(ThemeMode::Light));
    frames(&mut app, 2);
    check(&mut app, false);
    // System, following the OS.
    let window = {
        let w = app.world_mut();
        w.query_filtered::<Entity, With<PrimaryWindow>>()
            .single(w)
            .unwrap()
    };
    app.world_mut()
        .write_message(UiCommand::SetTheme(ThemeMode::System));
    for (os, dark) in [(WindowTheme::Dark, true), (WindowTheme::Light, false)] {
        app.world_mut()
            .write_message(WindowThemeChanged { window, theme: os });
        frames(&mut app, 3);
        check(&mut app, dark);
    }
}

#[test]
fn slot_header_buttons_swap_and_close() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: Panel::Plot,
        slot: Slot::Secondary,
    });
    frames(&mut app, 2);
    let world = app.world_mut();
    let actions: Vec<(Entity, crate::shell::sidebar::SlotAction)> = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .map(|(e, a)| (e, *a))
        .collect();
    use crate::shell::sidebar::SlotAction::*;
    assert_eq!(actions.iter().filter(|a| a.1 == Swap).count(), 2);
    let first = |a| actions.iter().find(|x| x.1 == a).unwrap().0;
    app.world_mut().trigger(Activate {
        entity: first(Swap),
    });
    frames(&mut app, 2);
    let l = layout(&app);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Scene))
    );
    let world = app.world_mut();
    let close = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Close)
        .unwrap()
        .0;
    world.trigger(Activate { entity: close });
    frames(&mut app, 2);
    assert_eq!(layout(&app).secondary, None);
    let world = app.world_mut();
    let collapse = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Collapse)
        .unwrap()
        .0;
    world.trigger(Activate { entity: collapse });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
}

#[test]
fn the_row_splitter_resizes_the_slots_within_their_minimum() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: Panel::Plot,
        slot: Slot::Secondary,
    });
    frames(&mut app, 3);
    let bar = find::<crate::shell::sidebar::SlotSplitter>(&mut app);
    assert!(displayed(&app, bar));
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
    let world = app.world_mut();
    world.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::new(0.0, -1000.0),
        delta: Vec2::new(0.0, -1000.0),
    });
    world.trigger(PointerDragEnd {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    frames(&mut app, 2);
    let split = layout(&app).split;
    assert!((split - MIN_SLOT_H / (828.0 - 6.0)).abs() < 1e-4, "{split}");
    assert!(app
        .world()
        .get::<crate::shell::sidebar::SlotDrag>(bar)
        .is_none());
}

#[test]
fn the_address_field_connects_on_enter_and_the_cursor_is_read_off_the_ground() {
    use crate::widgets::text_field::TextFieldInput;
    use bevy::input_focus::InputFocus;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let field = find::<AddressField>(&mut app);
    let input = find::<TextFieldInput>(&mut app);
    // Prefilled with the server's address.
    let text = app
        .world()
        .get::<bevy::text::EditableText>(input)
        .unwrap()
        .value()
        .to_string();
    assert_eq!(text, "ws://localhost:9004");
    // Focus shows the accent border.
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, bevy::input_focus::FocusCause::Navigated);
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<crate::theme::Stroke>(field).unwrap().0,
        ColorRole::Accent
    );
    // Enter submits what is typed.
    let window = find::<Window>(&mut app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1.last(),
        Some(&GzReconnect {
            url: Some("ws://localhost:9004".into())
        })
    );
    app.world_mut().trigger(crate::widgets::TextSubmit {
        entity: field,
        value: " gz:9002 ".into(),
    });
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1.last(),
        Some(&GzReconnect {
            url: Some("ws://gz:9002".into())
        })
    );
    // The cursor over the view's centre, from gzweb's home view, is on
    // the ground at the origin.
    let world = app.world_mut();
    let camera = world
        .query_filtered::<Entity, With<MainCamera>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(camera)
        .insert(gz_bevy::view::home_transform());
    world
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(356.0 + 392.0, 44.0 + 414.0)));
    frames(&mut app, 3);
    let at = app
        .world()
        .resource::<crate::shell::status_bar::CursorReadout>()
        .0
        .expect("on the ground");
    assert!(Vec2::from(at).length() < 0.05, "{at:?}");
    assert!(all_text(&mut app).contains("cursor 0.0"));
}
