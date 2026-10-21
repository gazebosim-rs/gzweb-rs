//! The viewport area: the 3D view on its node at any scale, the
//! disconnected banner, the connect card and its address field.

use std::time::Duration;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::controls::ActiveScopes;
use gz_bevy::view::{ViewOrigin, ViewPlacement};
use gz_core::transport::Status;

use super::banner::DisconnectBanner;
use super::connect_card::{
    card_status, normalize_address, AddressField, ConnectButton, ConnectCard,
};
use super::view::{DimOverlay, ViewNode};
use crate::shell::connection::{banner, ConnectionView, Phase, LOST_LONG_SECS};
use crate::shell::UiCommand;
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::{ColorRole, Fill, Theme};

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

/// The card's status line in each phase (§7.10).
#[test]
fn the_card_says_what_the_connection_is_doing() {
    use Phase::*;
    let lost = Lost { since: 0.0 };
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

// ---- PR #29 review fixes (DECISIONS.md D-52, D-56) ----

/// The review's scenario: click Connect, the world loads and the card
/// fades, Space (to pause). The clicked button didn't keep focus, so Space
/// doesn't press it again (which would drop the live connection).
#[test]
fn space_after_clicking_connect_doesnt_reconnect() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
    frames(&mut app, 2);
    let connect = connect_button(&mut app, ConnectButton::Connect);
    click(&mut app, connect);
    assert_eq!(app.world().resource::<Sent>().1.len(), 1, "clicked");
    assert_eq!(
        focused(&app),
        None,
        "a click doesn't leave focus on a button"
    );
    go_live(&mut app, false);
    frames(&mut app, 3);
    let card = find::<ConnectCard>(&mut app);
    assert!(!displayed(&app, card));
    chord(&mut app, &[KeyCode::Space]);
    chord(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.world().resource::<Sent>().1.len(), 1, "no reconnect");
}

/// "Retry now", then the connection comes back and the banner hides:
/// Space doesn't retry again.
#[test]
fn space_after_retry_now_doesnt_retry_again() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, false);
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Error("closed".into());
        s.sim = None;
    }
    frames(&mut app, 2);
    let banner = find::<DisconnectBanner>(&mut app);
    assert!(displayed(&app, banner));
    let retry = connect_button(&mut app, ConnectButton::Retry);
    click(&mut app, retry);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
    assert_eq!(focused(&app), None);
    // Even focused from the keyboard, it loses focus when the banner hides.
    tab_to(&mut app, retry);
    go_live(&mut app, false);
    assert!(!displayed(&app, banner));
    assert_eq!(focused(&app), None, "hidden: focus released");
    chord(&mut app, &[KeyCode::Space]);
    assert_eq!(app.world().resource::<Sent>().1.len(), 1);
}

/// Enter in the address field connects; the world loads and the card
/// fades with the field still focused. The field lets go as the fade
/// starts, so the shortcuts come back instead of going into an invisible
/// field.
#[test]
fn a_world_loading_takes_focus_from_the_hidden_address_field() {
    use crate::widgets::text_field::TextFieldInput;
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    frames(&mut app, 2);
    let input = find::<TextFieldInput>(&mut app);
    tab_to(&mut app, input);
    frames(&mut app, 2);
    assert!(
        app.world().resource::<ActiveScopes>().0.is_empty(),
        "typing"
    );
    let window = find::<Window>(&mut app);
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }
    assert_eq!(app.world().resource::<Sent>().1.len(), 1);
    assert_eq!(focused(&app), Some(input), "Enter doesn't blur the field");
    go_live(&mut app, false);
    let card = find::<ConnectCard>(&mut app);
    assert!(displayed(&app, card), "still fading");
    assert_eq!(focused(&app), None, "the fade releases focus at once");
    frames(&mut app, 2);
    assert!(!app.world().resource::<ActiveScopes>().0.is_empty());
    // The shortcuts work again.
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyB]);
    assert!(!layout(&app).sidebar_open);
}

/// A card fade interrupted by the world dropping again puts every
/// opacity back; a finished fade too.
#[test]
fn an_interrupted_card_fade_restores_its_opacities() {
    use super::connect_card::{Fading, PreFade};
    use crate::theme::Opacity;
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        40,
    )));
    frames(&mut app, 2);
    let card = find::<ConnectCard>(&mut app);
    let opacities = |app: &mut App| {
        descendants(app.world(), card)
            .into_iter()
            .chain([card])
            .filter_map(|e| app.world().get::<Opacity>(e).map(|o| o.0))
            .collect::<Vec<f32>>()
    };
    go_live(&mut app, false);
    assert!(app.world().get::<Fading>(card).is_some());
    assert!(opacities(&mut app).iter().any(|o| *o < 1.0), "fading");
    // The world drops before the fade ends: the card comes back whole.
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Connected;
    }
    app.world_mut().resource_mut::<GzIndex>().scene = None;
    app.world_mut().insert_resource(ConnectionView::default());
    frames(&mut app, 3);
    assert!(displayed(&app, card));
    assert!(app.world().get::<Fading>(card).is_none());
    assert!(app.world().get::<PreFade>(card).is_none());
    assert!(
        opacities(&mut app).iter().all(|o| *o == 1.0),
        "{:?}",
        opacities(&mut app)
    );
}

/// What the connect card makes of an address (DECISIONS.md D-55).
#[test]
fn addresses_normalize() {
    let port = gz_bevy::DEFAULT_PORT;
    assert_eq!(normalize_address(" gz:9002 ", false), "ws://gz:9002");
    assert_eq!(normalize_address("wss://gz", false), "wss://gz");
    // The page's http scheme names the websocket's.
    assert_eq!(normalize_address("http://gz:9002", false), "ws://gz:9002");
    assert_eq!(normalize_address("HTTPS://gz/ws", false), "wss://gz/ws");
    // A bare host gets the default port; from an https page, wss://.
    assert_eq!(normalize_address("gz", false), format!("ws://gz:{port}"));
    assert_eq!(normalize_address("gz", true), format!("wss://gz:{port}"));
    assert_eq!(
        normalize_address("gz/sim", false),
        format!("ws://gz:{port}/sim")
    );
    assert_eq!(normalize_address("[::1]:9004", true), "wss://[::1]:9004");
    assert_eq!(
        normalize_address("[::1]", false),
        format!("ws://[::1]:{port}")
    );
}
