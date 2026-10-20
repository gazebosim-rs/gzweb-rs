//! The top bar: the transport's sim commands and states, and its buttons
//! on `surface` in every theme.

use std::time::Duration;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{ComputedNode, InteractionDisabled};
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};

use super::tools::ToolChoice;
use super::transport::StepsPerClick;
use super::transport::{steps_label, StepsOption, Transport};
use crate::shell::regions::Region;
use crate::shell::{Tool, UiCommand};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Theme, ThemeMode};

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

// ---- PR #29 review fixes (DECISIONS.md D-52, D-54, D-56) ----

/// Click Play, then Space (which gz-bevy turns into play/pause): Play
/// isn't pressed a second time. The same for Step.
#[test]
fn space_after_play_or_step_doesnt_press_them_again() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, true);
    let world = app.world_mut();
    let buttons: Vec<(Entity, Transport)> = world
        .query::<(Entity, &Transport)>()
        .iter(world)
        .map(|(e, t)| (e, *t))
        .collect();
    let button = |t: Transport| buttons.iter().find(|b| b.1 == t).unwrap().0;
    for (which, want) in [
        (Transport::Step, GzCommand::Step(1)),
        (Transport::PlayPause, GzCommand::Play),
    ] {
        click(&mut app, button(which));
        assert_eq!(sent(&mut app), [want], "{which:?}");
        assert_eq!(focused(&app), None);
        chord(&mut app, &[KeyCode::Space]);
        chord(&mut app, &[KeyCode::Enter]);
        assert!(sent(&mut app).is_empty(), "{which:?}");
    }
}

fn transport_button(app: &mut App, which: Transport) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Transport)>()
        .iter(world)
        .find(|(_, t)| **t == which)
        .unwrap()
        .0
}

fn label(app: &App, e: Entity) -> Option<String> {
    app.world()
        .get::<bevy::a11y::AccessibilityNode>(e)
        .and_then(|a| a.label().map(str::to_string))
}

/// Play/Pause's accessible name follows its icon; the steps trigger's its
/// label; the menu marks the current choice, and Custom sets any count.
#[test]
fn the_transport_names_and_the_steps_menu_follow_their_state() {
    use super::transport::{CustomSteps, StepsTrigger};
    use crate::widgets::NumberField;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let play = transport_button(&mut app, Transport::PlayPause);
    go_live(&mut app, true);
    assert_eq!(label(&app, play).as_deref(), Some("Play"));
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(false));
    frames(&mut app, 1);
    assert_eq!(label(&app, play).as_deref(), Some("Pause"));
    let trigger = find::<StepsTrigger>(&mut app);
    let current = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&StepsOption, With<Active>>()
            .iter(world)
            .map(|o| o.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(current(&mut app), [1]);
    app.world_mut().resource_mut::<StepsPerClick>().0 = 10;
    frames(&mut app, 2);
    assert_eq!(label(&app, trigger).as_deref(), Some("10 steps"));
    assert!(all_text(&mut app).contains("10 steps"));
    assert_eq!(current(&mut app), [10]);
    // Custom: open the menu, type 25, Enter.
    app.world_mut().trigger(bevy::ui_widgets::MenuEvent {
        source: trigger,
        action: bevy::ui_widgets::MenuAction::Open(
            bevy::input_focus::tab_navigation::NavAction::First,
        ),
    });
    frames(&mut app, 2);
    let field = find::<CustomSteps>(&mut app);
    assert_eq!(app.world().get::<NumberField>(field).unwrap().value, 10.0);
    let input = descendants(app.world(), field)
        .into_iter()
        .find(|e| app.world().get::<bevy::ui_widgets::TextInput>(*e).is_some())
        .unwrap();
    tab_to(&mut app, input);
    frames(&mut app, 1);
    app.world_mut()
        .get_mut::<bevy::text::EditableText>(input)
        .unwrap()
        .editor
        .set_text("25");
    let window = find::<Window>(&mut app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<StepsPerClick>().0, 25);
    assert_eq!(current(&mut app), Vec::<u32>::new());
    assert_eq!(label(&app, trigger).as_deref(), Some("25 steps"));
    let popup = descendants(
        app.world(),
        app.world().get::<ChildOf>(trigger).unwrap().parent(),
    )
    .into_iter()
    .find(|e| app.world().get::<bevy::ui_widgets::MenuPopup>(*e).is_some())
    .unwrap();
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Hidden),
        "Enter closes the menu"
    );
    // A commit while the menu is closed (leaving the field) changes nothing.
    app.world_mut().trigger(crate::widgets::NumberCommit {
        entity: field,
        value: 7.0,
        is_final: true,
    });
    frames(&mut app, 1);
    assert_eq!(app.world().resource::<StepsPerClick>().0, 25);
    // The Step button steps by it.
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(true));
    frames(&mut app, 1);
    sent(&mut app);
    let step = transport_button(&mut app, Transport::Step);
    app.world_mut().trigger(Activate { entity: step });
    frames(&mut app, 1);
    assert_eq!(sent(&mut app), [GzCommand::Step(25)]);
}

/// The RTF tooltip shows the stats as they change while it is open.
#[test]
fn the_rtf_tooltip_is_live() {
    use super::transport::RtfReadout;
    use crate::widgets::tooltip::{TooltipCard, TooltipLine};
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
    frames(&mut app, 2);
    go_live(&mut app, true);
    let rtf = find::<RtfReadout>(&mut app);
    app.world_mut()
        .entity_mut(rtf)
        .insert(bevy::picking::hover::Hovered(true));
    frames(&mut app, 3);
    let line = |app: &mut App| {
        let world = app.world_mut();
        let card = world
            .query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
            .iter(world)
            .find(|(_, p)| p.parent() == rtf)
            .expect("the tooltip is open")
            .0;
        descendants(world, card)
            .into_iter()
            .find_map(|e| {
                world
                    .get::<TooltipLine>(e)
                    .and(world.get::<Text>(e))
                    .map(|t| t.0.clone())
            })
            .unwrap()
    };
    assert!(line(&mut app).contains("134,380 iterations"));
    app.world_mut()
        .resource_mut::<GzStatus>()
        .sim
        .as_mut()
        .unwrap()
        .iterations = 200_000;
    frames(&mut app, 2);
    assert!(
        line(&mut app).contains("200,000 iterations"),
        "{}",
        line(&mut app)
    );
}

/// The search placeholder never cuts a word: the longest text that fits.
#[test]
fn the_search_placeholder_fits_its_room() {
    use super::search::{SearchPlaceholder, SEARCH_PLACEHOLDERS};
    let fit = SearchPlaceholder {
        widths: [Some(150.0), Some(50.0), Some(0.0)],
        shown: 0,
    };
    assert_eq!(fit.pick(160.0), 0);
    assert_eq!(fit.pick(120.0), 1);
    assert_eq!(fit.pick(20.0), 2);
    let unmeasured = SearchPlaceholder {
        widths: [Some(150.0), None, None],
        shown: 0,
    };
    assert_eq!(unmeasured.pick(120.0), 1, "tried, then measured");
    for (w, scale) in [(1440, 1.0), (1024, 1.0), (1440, 1.5)] {
        let mut app = shell_app((w, 700), scale);
        frames(&mut app, 8);
        let world = app.world_mut();
        let (text, fit, node, parent) = world
            .query::<(&Text, &SearchPlaceholder, &ComputedNode, &ChildOf)>()
            .single(world)
            .unwrap();
        let room = world.get::<ComputedNode>(parent.parent()).unwrap();
        let k = node.inverse_scale_factor();
        assert_eq!(text.0, SEARCH_PLACEHOLDERS[fit.shown], "{w}");
        assert!(
            node.size().x * k <= room.size().x * k + 0.5,
            "{w}: {} in {}",
            node.size().x * k,
            room.size().x * k
        );
    }
}

#[test]
fn every_tool_has_a_name_and_a_hint() {
    for t in Tool::ALL {
        assert!(!t.hint().is_empty() && !t.name().is_empty());
    }
}
