//! Theme resolution: modes, role components, the OS preference, UI scale
//! and the focus ring.

use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::text::TextCursorStyle;
use bevy::window::{PrimaryWindow, WindowTheme, WindowThemeChanged};

use super::app::{frames, ui_app};
use crate::theme::{
    elevation, CaretRoles, ColorRole, Fill, FocusRing, FontFamily, Ink, Opacity, OsTheme, Palette,
    Ring, Shadow, Stroke, Theme, ThemeMode, TypeStyle,
};

#[test]
fn modes_resolve_to_a_palette() {
    let light = Theme::new(ThemeMode::Light, true);
    assert!(!light.is_dark);
    assert_eq!(light.c, Palette::light());
    let dark = Theme::new(ThemeMode::Dark, false);
    assert!(dark.is_dark);
    assert_eq!(dark.c, Palette::dark());
    for os_dark in [false, true] {
        let system = Theme::new(ThemeMode::System, os_dark);
        assert_eq!(system.is_dark, os_dark);
        assert_eq!(system.mode, ThemeMode::System);
    }
    let mut t = Theme::default();
    assert_eq!(t.mode, ThemeMode::Light);
    assert_eq!(t.scale, 1.0);
    assert_eq!(t.logo_path(), "ui/brand/gazebo-icon.png");
    t.toggle();
    assert!(t.is_dark && t.mode == ThemeMode::Dark);
    assert_eq!(t.logo_path(), "ui/brand/gazebo-icon-dark.png");
    assert_eq!(t.color(ColorRole::Bg), Palette::dark().bg);
    t.toggle();
    assert!(!t.is_dark && t.mode == ThemeMode::Light);
    t.set_mode(ThemeMode::System, true);
    assert!(t.is_dark);
}

#[test]
fn theme_mode_parses() {
    assert_eq!("system".parse(), Ok(ThemeMode::System));
    assert_eq!("Light".parse(), Ok(ThemeMode::Light));
    assert_eq!("DARK".parse(), Ok(ThemeMode::Dark));
    assert_eq!(
        "sepia".parse::<ThemeMode>(),
        Err("unknown theme 'sepia' (system, light or dark)".into())
    );
    assert_eq!(ThemeMode::default(), ThemeMode::System);
}

#[test]
fn type_and_elevation_helpers() {
    assert_eq!(TypeStyle::CAPS.tracking_px(), 0.04 * 11.0);
    let plain = TypeStyle::new(FontFamily::Ui, 12.0, 500);
    assert!(!plain.uppercase && plain.tracking == 0.0);
    let shadow = elevation::MENU.box_shadow();
    assert_eq!(shadow.0.len(), 1);
    assert_eq!(shadow.0[0].y_offset, px(8));
    assert_eq!(shadow.0[0].blur_radius, px(24));
}

fn resolved(app: &App, e: Entity) -> (Color, Color, Color) {
    let w = app.world();
    (
        w.get::<BackgroundColor>(e).unwrap().0,
        w.get::<BorderColor>(e).unwrap().top,
        w.get::<TextColor>(e).unwrap().0,
    )
}

#[test]
fn roles_resolve_and_re_resolve_on_a_theme_switch() {
    let mut app = ui_app(ThemeMode::Light);
    let e = app
        .world_mut()
        .spawn((
            Node::default(),
            Text::new("x"),
            Fill(ColorRole::Surface),
            Stroke(ColorRole::Line),
            Ink(ColorRole::Text),
            Ring(ColorRole::Accent),
            Outline::default(),
            Shadow(ColorRole::Shadow),
            elevation::TOOLTIP.box_shadow(),
            CaretRoles,
        ))
        .id();
    let icon = app
        .world_mut()
        .spawn((ImageNode::default(), Ink(ColorRole::Icon), Opacity(0.4)))
        .id();
    frames(&mut app, 1);
    let l = Palette::light();
    assert_eq!(resolved(&app, e), (l.surface, l.line, l.text));
    let w = app.world();
    assert_eq!(w.get::<Outline>(e).unwrap().color, l.accent);
    assert_eq!(w.get::<BoxShadow>(e).unwrap().0[0].color, l.shadow);
    let cursor = w.get::<TextCursorStyle>(e).unwrap();
    assert_eq!(cursor.color, l.text);
    assert_eq!(cursor.selection_color, l.accent_soft);
    let tint = w.get::<ImageNode>(icon).unwrap().color;
    assert_eq!(tint, l.icon.with_alpha(0.4));

    app.world_mut().resource_mut::<Theme>().toggle();
    frames(&mut app, 1);
    let d = Palette::dark();
    assert_eq!(resolved(&app, e), (d.surface, d.line, d.text));
    assert_eq!(app.world().get::<Outline>(e).unwrap().color, d.accent);

    // Changing one role re-resolves just that entity.
    app.world_mut().entity_mut(e).insert(Fill(ColorRole::Clear));
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<BackgroundColor>(e).unwrap().0,
        Color::NONE
    );
    app.world_mut().entity_mut(icon).insert(Opacity(1.0));
    frames(&mut app, 1);
    assert_eq!(app.world().get::<ImageNode>(icon).unwrap().color, d.icon);
}

fn primary_window(app: &mut App) -> Entity {
    let w = app.world_mut();
    w.query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap()
}

#[test]
fn system_mode_follows_the_os() {
    let mut app = ui_app(ThemeMode::System);
    frames(&mut app, 1);
    assert!(!app.world().resource::<Theme>().is_dark);
    // winit fills the window's theme at creation.
    let window = primary_window(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .window_theme = Some(WindowTheme::Dark);
    frames(&mut app, 2);
    assert!(app.world().resource::<OsTheme>().dark);
    assert!(app.world().resource::<Theme>().is_dark);
    // A change event.
    app.world_mut().write_message(WindowThemeChanged {
        window,
        theme: WindowTheme::Light,
    });
    frames(&mut app, 2);
    assert!(!app.world().resource::<OsTheme>().dark);
    assert!(!app.world().resource::<Theme>().is_dark);
    // A forced mode ignores the OS.
    app.world_mut()
        .resource_mut::<Theme>()
        .set_mode(ThemeMode::Light, false);
    app.world_mut().write_message(WindowThemeChanged {
        window,
        theme: WindowTheme::Dark,
    });
    frames(&mut app, 2);
    assert!(app.world().resource::<OsTheme>().dark);
    assert!(!app.world().resource::<Theme>().is_dark);
}

#[test]
fn ui_scale_follows_the_theme() {
    let mut app = ui_app(ThemeMode::Light);
    frames(&mut app, 1);
    app.world_mut().resource_mut::<Theme>().scale = 1.5;
    frames(&mut app, 1);
    assert_eq!(app.world().resource::<UiScale>().0, 1.5);
}

#[test]
fn focus_ring_shows_only_for_visible_keyboard_focus() {
    let mut app = ui_app(ThemeMode::Light);
    let a = app.world_mut().spawn((Node::default(), FocusRing)).id();
    let b = app.world_mut().spawn((Node::default(), FocusRing)).id();
    frames(&mut app, 1);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(a, FocusCause::Navigated);
    app.world_mut().resource_mut::<InputFocusVisible>().0 = true;
    frames(&mut app, 2);
    let ring = |app: &App, e| app.world().get::<Ring>(e).copied();
    assert_eq!(ring(&app, a), Some(Ring(ColorRole::Accent)));
    assert_eq!(ring(&app, b), Some(Ring(ColorRole::Clear)));
    assert_eq!(
        app.world().get::<Outline>(a).unwrap().color,
        Palette::light().accent
    );
    app.world_mut().resource_mut::<InputFocusVisible>().0 = false;
    frames(&mut app, 2);
    assert_eq!(ring(&app, a), Some(Ring(ColorRole::Clear)));
}

#[test]
fn the_plugin_follows_the_system_by_default() {
    assert_eq!(crate::GzUiPlugin::default().theme, ThemeMode::System);
}

#[test]
fn text_fields_take_the_keys_from_the_viewer() {
    use crate::focus::TextFocus;
    use gz_bevy::view::controls::ActiveScopes;
    use gz_bevy::view::input::Scope;

    let mut app = ui_app(ThemeMode::Light);
    app.insert_resource(ActiveScopes(vec![Scope::Global, Scope::Viewport]));
    let field = app
        .world_mut()
        .spawn(crate::widgets::number_field(
            &Theme::default(),
            crate::widgets::AxisTag::X,
            crate::widgets::NumberField::new(0.0),
            false,
        ))
        .id();
    let other = app.world_mut().spawn(Node::default()).id();
    frames(&mut app, 2);
    let input = app.world().get::<Children>(field).unwrap()[1];
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Navigated);
    frames(&mut app, 1);
    assert!(app.world().resource::<TextFocus>().0);
    assert!(app.world().resource::<ActiveScopes>().0.is_empty());
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(other, FocusCause::Navigated);
    frames(&mut app, 1);
    assert!(!app.world().resource::<TextFocus>().0);
    // Not in the frame focus left: the key that blurred the field (Esc,
    // Tab) is still `just_pressed` then, and the viewer mustn't see it.
    assert!(app.world().resource::<ActiveScopes>().0.is_empty());
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<ActiveScopes>().0,
        vec![Scope::Global, Scope::Viewport]
    );
    // Field to field: the scopes stay with the fields.
    let focus_input = |app: &mut App| {
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Navigated);
        frames(app, 1);
    };
    focus_input(&mut app);
    app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut app, 1);
    focus_input(&mut app);
    assert!(app.world().resource::<ActiveScopes>().0.is_empty());
    // Scopes set while typing aren't overwritten by the saved ones.
    app.world_mut().resource_mut::<ActiveScopes>().0 = vec![Scope::Global];
    app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut app, 2);
    assert_eq!(
        app.world().resource::<ActiveScopes>().0,
        vec![Scope::Global]
    );
}
