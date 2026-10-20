//! The top bar: the transport's sim commands and states, and its buttons
//! on `surface` in every theme.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};

use super::tools::ToolChoice;
use super::transport::StepsPerClick;
use super::transport::{steps_label, StepsOption, Transport};
use crate::shell::regions::Region;
use crate::shell::{format, Tool, UiCommand};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
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
