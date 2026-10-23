//! The gallery: it builds headlessly, and a runtime theme switch
//! re-colours every role-carrying entity with no restart.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use bevy::ui_widgets::Activate;
use bevy::window::PrimaryWindow;

use crate::test_support::{frames, ui_app};
use bevy::input_focus::{FocusCause, InputFocus};

use super::components::FocusExample;
use super::tokens::{hex, TokenHex, TOKENS};
use super::{GalleryCapture, GalleryPlugin, GalleryRoot, ThemeText, ThemeToggle};
use crate::theme::{Fill, Ink, Opacity, Ring, Shadow, Stroke, Theme, ThemeMode};
use crate::widgets::number_field::NumberInput;

fn gallery(theme: ThemeMode) -> App {
    let mut app = ui_app(theme);
    app.add_plugins(GalleryPlugin);
    frames(&mut app, 3);
    app
}

/// Every `Fill`, `Stroke` and text `Ink` shows its role's colour in the
/// current theme.
fn assert_all_resolved(app: &mut App) {
    let theme = app.world().resource::<Theme>().clone();
    let w = app.world_mut();
    let alpha = |o: Option<&Opacity>| o.map_or(1.0, |o| o.0);
    let mut n = 0;
    let mut fills = w.query::<(&Fill, &BackgroundColor, Option<&Opacity>)>();
    for (fill, bg, o) in fills.iter(w) {
        let c = theme.color(fill.0);
        assert_eq!(bg.0, c.with_alpha(c.alpha() * alpha(o)), "{fill:?}");
        n += 1;
    }
    let mut strokes = w.query::<(&Stroke, &BorderColor, Option<&Opacity>)>();
    for (stroke, border, o) in strokes.iter(w) {
        let c = theme.color(stroke.0);
        assert_eq!(border.top, c.with_alpha(c.alpha() * alpha(o)), "{stroke:?}");
        n += 1;
    }
    let mut inks = w.query::<(&Ink, &TextColor, Option<&Opacity>)>();
    for (ink, color, o) in inks.iter(w) {
        let c = theme.color(ink.0);
        assert_eq!(color.0, c.with_alpha(c.alpha() * alpha(o)), "{ink:?}");
        n += 1;
    }
    let mut icons = w.query::<(&Ink, &ImageNode, Option<&Opacity>)>();
    let mut tints = 0;
    for (ink, image, o) in icons.iter(w) {
        let c = theme.color(ink.0);
        assert_eq!(image.color, c.with_alpha(c.alpha() * alpha(o)), "{ink:?}");
        tints += 1;
    }
    let mut rings = w.query::<(&Ring, &Outline, Option<&Opacity>)>();
    let mut outlines = 0;
    for (ring, outline, o) in rings.iter(w) {
        let c = theme.color(ring.0);
        assert_eq!(
            outline.color,
            c.with_alpha(c.alpha() * alpha(o)),
            "{ring:?}"
        );
        outlines += 1;
    }
    let mut shadows = w.query::<(&Shadow, &BoxShadow, Option<&Opacity>)>();
    let mut layers = 0;
    for (shadow, box_shadow, o) in shadows.iter(w) {
        let c = theme.color(shadow.0);
        for layer in &box_shadow.0 {
            assert_eq!(
                layer.color,
                c.with_alpha(c.alpha() * alpha(o)),
                "{shadow:?}"
            );
            layers += 1;
        }
    }
    assert!(n > 400, "{n} roles");
    assert!(tints > 40, "{tints} icons");
    assert!(outlines > 40, "{outlines} outlines");
    assert!(layers > 0, "{layers} shadows");
}

#[test]
fn the_gallery_builds_and_switches_theme_at_runtime() {
    let mut app = gallery(ThemeMode::Light);
    assert_eq!(
        app.world_mut()
            .query_filtered::<(), With<GalleryRoot>>()
            .iter(app.world())
            .count(),
        1
    );
    assert_all_resolved(&mut app);
    let texts = |app: &mut App| {
        let w = app.world_mut();
        let mut q = w.query::<(&ThemeText, &Text)>();
        q.iter(w).map(|(_, t)| t.0.clone()).collect::<Vec<_>>()
    };
    assert_eq!(texts(&mut app), ["Light", "DEFAULT · FOLLOWS OS"]);
    // The T key.
    let window = {
        let w = app.world_mut();
        w.query_filtered::<Entity, With<PrimaryWindow>>()
            .single(w)
            .unwrap()
    };
    let press_t = |app: &mut App| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyT,
                logical_key: Key::Character("t".into()),
                state,
                text: Some("t".into()),
                repeat: false,
                window,
            });
            frames(app, 1);
        }
        frames(app, 1);
    };
    // No field has focus at launch, so T switches at once.
    assert!(!app.world().resource::<crate::widgets::focus::TextFocus>().0);
    press_t(&mut app);
    assert!(app.world().resource::<Theme>().is_dark);
    // Typing in a field doesn't switch.
    let input = {
        let w = app.world_mut();
        let example = w
            .query_filtered::<&Children, With<FocusExample>>()
            .single(w)
            .unwrap()
            .to_vec();
        example
            .into_iter()
            .find(|c| w.get::<NumberInput>(*c).is_some())
            .unwrap()
    };
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Navigated);
    frames(&mut app, 1);
    press_t(&mut app);
    assert!(app.world().resource::<Theme>().is_dark);
    app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut app, 1);
    press_t(&mut app);
    assert!(!app.world().resource::<Theme>().is_dark);
    press_t(&mut app);
    assert!(app.world().resource::<Theme>().is_dark);
    assert_all_resolved(&mut app);
    assert_eq!(texts(&mut app), ["Dark", "SAME TOKENS · OWN VALUES"]);
    let theme = app.world().resource::<Theme>().clone();
    let w = app.world_mut();
    let mut hexes = w.query::<(&TokenHex, &Text)>();
    assert_eq!(hexes.iter(w).count(), TOKENS.len());
    for (token, text) in hexes.iter(w) {
        assert_eq!(text.0, hex(theme.color(token.0)));
    }
    // The button switches back.
    let toggle = w
        .query_filtered::<Entity, With<ThemeToggle>>()
        .single(w)
        .unwrap();
    w.trigger(Activate { entity: toggle });
    frames(&mut app, 2);
    assert!(!app.world().resource::<Theme>().is_dark);
    assert_all_resolved(&mut app);
}

/// The gallery owns its splitter: it keeps the scene panel's width and
/// range in it, and applies what it asks for.
#[test]
fn the_gallery_splitter_sizes_the_scene_panel() {
    use super::systems::{SizesPane, TREE_W};
    use crate::widgets::{Splitter, SplitterMoved, SplitterReset};
    let mut app = gallery(ThemeMode::Light);
    let (bar, pane) = {
        let w = app.world_mut();
        let mut q = w.query::<(Entity, &SizesPane)>();
        let (e, p) = q.single(w).unwrap();
        (e, p.0)
    };
    let s = *app.world().get::<Splitter>(bar).unwrap();
    assert_eq!((s.size, s.min, s.max), (TREE_W, 240.0, 420.0));
    app.world_mut().trigger(SplitterMoved {
        entity: bar,
        size: 400.0,
    });
    frames(&mut app, 2);
    assert_eq!(app.world().get::<Node>(pane).unwrap().width, px(400));
    assert_eq!(app.world().get::<Splitter>(bar).unwrap().size, 400.0);
    app.world_mut().trigger(SplitterReset { entity: bar });
    frames(&mut app, 1);
    assert_eq!(app.world().get::<Node>(pane).unwrap().width, px(TREE_W));
}

#[test]
fn hex_formats_tokens() {
    let t = Theme::default();
    assert_eq!(hex(t.c.accent), format!("{}1C64F2", '#'));
    assert_eq!(hex(t.c.shadow), format!("{}12151A1F", '#'));
}

#[test]
fn capture_waits_then_screenshots_once() {
    let mut app = gallery(ThemeMode::Dark);
    app.insert_resource(GalleryCapture {
        path: "unused.png".into(),
        frames: 2,
    });
    let shots = |app: &mut App| {
        let w = app.world_mut();
        w.query::<&Screenshot>().iter(w).count()
    };
    frames(&mut app, 1);
    assert_eq!(shots(&mut app), 0);
    // A capture shows the example field focused.
    let focused = app.world().resource::<InputFocus>().get();
    assert!(focused.is_some_and(|e| app.world().get::<NumberInput>(e).is_some()));
    frames(&mut app, 1);
    assert_eq!(shots(&mut app), 1);
    frames(&mut app, 2);
    assert_eq!(shots(&mut app), 1);
    assert_eq!(GalleryCapture::new("x.png").frames, 30);
}
