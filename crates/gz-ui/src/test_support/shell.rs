//! The full UI headless: the shell on the [`ui_app_with`] harness with a
//! 3D main camera and the gz-bevy resources the shell reads, and helpers
//! to find regions, read rects, press keys and go live.

use bevy::app::PluginGroupBuilder;
use bevy::camera::visibility::VisibilitySystems;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::ButtonState;
use std::time::Duration;

use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerPress, PointerRelease};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::{GzCommand, GzReconnect, GzStatus, SimStats};
use gz_bevy::overlays::{apply_overlay_commands, OverlayCommand, Overlays};
use gz_bevy::render::GzGrid;
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::controls::{ActiveScopes, ViewPlacementSystems};
use gz_bevy::view::input::{InputMap, Platform, Scope};
use gz_bevy::view::{
    apply_view_commands, apply_view_placement, ViewCamera, ViewCommand, ViewId, ViewSlots,
};
use gz_core::transport::Status;

use super::{frames, ui_app_with};
use crate::shell::regions::Region;
use crate::shell::viewport::connect_card::ConnectButton;
use crate::shell::{ShellLayout, ShellPlugin, ShellPlugins};
use crate::theme::ThemeMode;

/// GzCommands and reconnects sent so far.
#[derive(Resource, Default)]
pub(crate) struct Sent(pub Vec<GzCommand>, pub Vec<GzReconnect>);

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
pub(crate) fn shell_app(size: (u32, u32), scale: f32) -> App {
    shell_app_from(size, scale, ShellPlugins.build())
}

/// [`shell_app`] from `plugins` (`ShellPlugins.build()` with a panel left
/// out, say); the UI camera is the harness's.
pub(crate) fn shell_app_from(size: (u32, u32), scale: f32, plugins: PluginGroupBuilder) -> App {
    let mut app = ui_app_with(ThemeMode::Light, size, scale, |app| {
        app.add_plugins((
            crate::GzUiPlugin {
                theme: ThemeMode::Light,
            },
            plugins.set(ShellPlugin { camera: false }),
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
        // The view's and the overlays' state, as ViewerControls keeps it.
        .init_asset::<StandardMaterial>()
        .add_message::<ViewCommand>()
        .add_message::<OverlayCommand>()
        .init_resource::<Overlays>()
        .init_resource::<GzGrid>()
        .init_resource::<gz_bevy::view::Selection>()
        .init_resource::<gz_bevy::view::BoxSelectTool>()
        .init_resource::<gz_bevy::view::Marquee>()
        .init_resource::<gz_bevy::camera::GzCameraTarget>()
        .init_resource::<gz_bevy::view::LinkedViews>()
        .add_systems(
            Update,
            (
                gz_bevy::view::select::prune_selection,
                apply_view_commands,
                apply_overlay_commands,
            )
                .chain()
                .before(crate::GzUiSystems::Behaviour),
        )
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
    app.world_mut()
        .spawn(view_camera(physical, scale, ViewId(0)));
    app.add_systems(Update, spawn_test_views);
    app
}

/// A 3D view camera as `gz-bevy` spawns one, without the render stack.
fn view_camera(physical: UVec2, scale: f32, id: ViewId) -> impl Bundle {
    (
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
        Projection::Perspective(gz_bevy::camera::perspective()),
        gz_bevy::view::home_transform(),
        ViewCamera::new(Vec3::ZERO, gz_bevy::camera::perspective()),
        MainCamera,
        id,
        id.layers(),
        gz_bevy::overlays::OverlaySet::default(),
    )
}

/// `gz-bevy`'s `render::spawn_views` without the environment: a camera
/// for each view the layout shows.
fn spawn_test_views(
    slots: Res<ViewSlots>,
    cameras: Query<(&ViewId, &Camera), With<MainCamera>>,
    mut commands: Commands,
) {
    let Some(info) = cameras
        .iter()
        .find_map(|(_, c)| c.computed.target_info.clone())
    else {
        return;
    };
    for id in (0..slots.0).map(ViewId) {
        if !cameras.iter().any(|(v, _)| *v == id) {
            commands.spawn(view_camera(info.physical_size, info.scale_factor, id));
        }
    }
}

pub(crate) fn find<C: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .single(world)
        .expect("one")
}

pub(crate) fn region(app: &mut App, r: Region) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Region)>()
        .iter(world)
        .find(|(_, x)| **x == r)
        .map(|(e, _)| e)
        .expect("region")
}

/// A node's rect in UI logical pixels (`min`, `size`).
pub(crate) fn rect(app: &App, e: Entity) -> (Vec2, Vec2) {
    let n = app.world().get::<ComputedNode>(e).expect("laid out");
    let t = app.world().get::<UiGlobalTransform>(e).expect("placed");
    let k = n.inverse_scale_factor();
    let size = n.size() * k;
    (t.translation * k - size / 2.0, size)
}

pub(crate) fn displayed(app: &App, e: Entity) -> bool {
    app.world().get::<Node>(e).unwrap().display != Display::None
}

pub(crate) fn layout(app: &App) -> ShellLayout {
    app.world().resource::<ShellLayout>().clone()
}

/// Adds the viewer's own keys (`gz_bevy`'s `key_input`: Space taps
/// play/pause, ⌘→ steps), which the app runs beside the shell's.
pub(crate) fn with_viewer_keys(app: &mut App) {
    app.init_resource::<gz_bevy::view::controls::ControlState>()
        .init_resource::<GzGrid>()
        .add_message::<ViewCommand>()
        .add_systems(Update, gz_bevy::view::controls::key_input);
}

pub(crate) fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
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

/// Focuses `e` (as Tab would) and presses `key_code` in it.
pub(crate) fn focused_key(app: &mut App, e: Entity, key_code: KeyCode) {
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(e, bevy::input_focus::FocusCause::Navigated);
    chord(app, &[key_code]);
    app.update();
}

pub(crate) fn chord(app: &mut App, keys: &[KeyCode]) {
    for k in keys {
        key(app, *k, ButtonState::Pressed);
    }
    app.update();
    for k in keys.iter().rev() {
        key(app, *k, ButtonState::Released);
    }
    app.update();
}

pub(crate) fn mouse(app: &mut App, state: ButtonState) {
    let window = find::<Window>(app);
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
}

pub(crate) fn stats(paused: bool) -> SimStats {
    SimStats {
        paused,
        sim_time: 134.38,
        real_time: 140.0,
        real_time_factor: 0.98,
        iterations: 134_380,
        step_size: 0.001,
    }
}

pub(crate) fn go_live(app: &mut App, paused: bool) {
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

pub(crate) fn sent(app: &mut App) -> Vec<GzCommand> {
    std::mem::take(&mut app.world_mut().resource_mut::<Sent>().0)
}

/// Every text in the shell, joined.
pub(crate) fn all_text(app: &mut App) -> String {
    let world = app.world_mut();
    let mut out: Vec<String> = world
        .query::<&Text>()
        .iter(world)
        .map(|t| t.0.clone())
        .collect();
    out.extend(world.query::<&TextSpan>().iter(world).map(|t| t.0.clone()));
    out.join("\n")
}

// Pointer clicks and keyboard focus (DECISIONS.md D-52).

pub(crate) fn pointer() -> Pointer {
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    )
}

pub(crate) fn hit() -> HitData {
    HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
}

/// A left click on `e` as bevy_picking delivers it: the mouse button goes
/// down with a pointer press (which focuses a tab stop), then the click
/// (which activates a button) and the release.
pub(crate) fn click(app: &mut App, e: Entity) {
    mouse(app, ButtonState::Pressed);
    app.world_mut().trigger(PointerPress {
        entity: e,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
        count: 1,
    });
    app.update();
    app.world_mut().trigger(PointerClick {
        entity: e,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
        duration: Duration::from_millis(80),
        count: 1,
    });
    app.world_mut().trigger(PointerRelease {
        entity: e,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
    });
    mouse(app, ButtonState::Released);
    app.update();
}

pub(crate) fn focused(app: &App) -> Option<Entity> {
    app.world()
        .resource::<bevy::input_focus::InputFocus>()
        .get()
}

/// Focus `e` as Tab would: no pointer press involved.
pub(crate) fn tab_to(app: &mut App, e: Entity) {
    let world = app.world_mut();
    world.resource_mut::<crate::widgets::focus::PointerLed>().0 = false;
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(e, bevy::input_focus::FocusCause::Navigated);
}

pub(crate) fn connect_button(app: &mut App, which: ConnectButton) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ConnectButton)>()
        .iter(world)
        .find(|(_, b)| **b == which)
        .unwrap()
        .0
}

// The view (M3): live with a world, its camera's state, commands to it.

/// The full UI `h` tall with a live world, so the view and its controls
/// show.
pub(crate) fn live_view(h: u32) -> App {
    let mut app = shell_app((1440, h), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, true);
    frames(&mut app, 2);
    app
}

/// The main view's camera state.
pub(crate) fn view(app: &mut App) -> ViewCamera {
    let world = app.world_mut();
    world
        .query_filtered::<&ViewCamera, With<MainCamera>>()
        .single(world)
        .unwrap()
        .clone()
}

/// Sends `op` to the main view and runs two frames.
pub(crate) fn send(app: &mut App, op: gz_bevy::view::ViewOp) {
    let world = app.world_mut();
    let view = world
        .query_filtered::<Entity, With<MainCamera>>()
        .single(world)
        .unwrap();
    world.write_message(ViewCommand { view, op });
    frames(app, 2);
}
