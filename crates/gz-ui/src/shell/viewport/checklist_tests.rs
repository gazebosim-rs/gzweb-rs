//! The spec's §13 checklist items for the view that the other tests leave
//! out (M10e): the camera's region matches the view's node after a window
//! resize and a splitter drag, and Esc never changes the camera's drag
//! action.

use bevy::camera::RenderTargetInfo;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::WindowResized;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::view::{DragAction, ViewId, ViewOp, ViewPlacement};

use super::view::ViewNode;
use crate::shell::splitter::ShellSplitter;
use crate::shell::{Side, Tool, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

/// The view's node, where its camera is placed, and the camera's viewport
/// size, in logical pixels.
fn regions(app: &mut App) -> (Rect, Rect, Option<Vec2>) {
    let node = find::<ViewNode>(app);
    let (min, size) = rect(app, node);
    let world = app.world_mut();
    let (placement, camera, _) = world
        .query_filtered::<(&ViewPlacement, &Camera, &ViewId), With<MainCamera>>()
        .iter(world)
        .find(|(.., id)| id.0 == 0)
        .expect("the first view");
    let placed = placement.rect.expect("placed");
    (
        Rect::from_corners(min, min + size),
        placed,
        camera.logical_viewport_size(),
    )
}

fn matches_its_node(app: &mut App, when: &str) -> Rect {
    let (node, placed, viewport) = regions(app);
    let near = |a: Vec2, b: Vec2| (a - b).abs().max_element() < 0.5;
    assert!(
        near(node.min, placed.min) && near(node.max, placed.max),
        "{when}: node {node:?}, camera {placed:?}"
    );
    assert_eq!(viewport, Some(placed.size()), "{when}");
    placed
}

#[test]
fn the_camera_matches_its_node_after_a_resize_and_a_splitter_drag() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let start = matches_its_node(&mut app, "at start");
    // The window shrinks, as winit reports it.
    let window = find::<Window>(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .expect("the window")
        .resolution
        .set(1300.0, 800.0);
    app.world_mut().write_message(WindowResized {
        window,
        width: 1300.0,
        height: 800.0,
    });
    // Without a renderer nothing sizes the UI camera's target (the harness
    // gave it the first size), so the test does.
    let world = app.world_mut();
    let mut ui = world.query_filtered::<&mut Camera, With<IsDefaultUiCamera>>();
    for mut camera in ui.iter_mut(world) {
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new(1300, 800),
            scale_factor: 1.0,
        });
    }
    frames(&mut app, 3);
    let resized = matches_its_node(&mut app, "resized");
    assert!(resized.width() < start.width() && resized.height() < start.height());
    // The sidebar's splitter moves right, by its keys.
    let bar = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ShellSplitter)>()
            .iter(world)
            .find(|(_, s)| s.0 == Side::Sidebar)
            .expect("the sidebar's splitter")
            .0
    };
    for _ in 0..3 {
        focused_key(&mut app, bar, KeyCode::ArrowRight);
    }
    frames(&mut app, 2);
    let dragged = matches_its_node(&mut app, "after the splitter");
    let step = 3.0 * crate::widgets::splitter::KEY_STEP;
    assert_eq!(dragged.min.x, resized.min.x + step);
    assert_eq!(dragged.max.x, resized.max.x);
}

/// Esc closes, cancels or returns to Navigate (spec §6.3); the camera's
/// drag action (the rail's Orbit, Pan or Zoom) stays as the user set it.
#[test]
fn esc_never_changes_the_drag_action() {
    let mut app = live_view(900);
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    send(&mut app, ViewOp::SetDragAction(DragAction::Pan));
    let window = find::<Window>(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .expect("the window")
        .set_cursor_position(Some(Vec2::new(700.0, 400.0)));
    app.world_mut()
        .write_message(UiCommand::SetTool(Tool::Translate));
    frames(&mut app, 2);
    for _ in 0..2 {
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
    }
    assert_eq!(*app.world().resource::<Tool>(), Tool::Navigate);
    assert_eq!(view(&mut app).drag_action, DragAction::Pan);
}
