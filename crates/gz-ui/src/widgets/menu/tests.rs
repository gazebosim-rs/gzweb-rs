//! Menus: open and close, keyboard navigation, type-to-search, checkbox
//! items, selects, and shortcut text from the input map.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::NavAction;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, MenuAction, MenuEvent};
use bevy::window::PrimaryWindow;
use gz_bevy::view::input::{Chord, InputMap, Mods, Platform};

use crate::test_support::{frames, ui_app};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Fill, Theme, ThemeMode};
use crate::widgets::button::{text_button_parts, TextButton};
use crate::widgets::menu::{
    menu, type_to_search, CheckItem, Menu, MenuCheckBox, MenuLabel, MenuPopupOf, MenuSide,
    MenuTrigger,
};
use crate::widgets::select::{Select, SelectChanged, SelectLabel, SelectOption};
use crate::widgets::shortcut::{chord_label, key_label, shortcut_label};
use crate::widgets::{menu_checkbox, menu_group_label, menu_item, menu_separator, select};

struct M {
    app: App,
    root: Entity,
    trigger: Entity,
    popup: Entity,
    window: Entity,
}

fn child_with<C: Component>(app: &App, parent: Entity) -> Entity {
    app.world()
        .get::<Children>(parent)
        .unwrap()
        .iter()
        .find(|c| app.world().get::<C>(*c).is_some())
        .unwrap()
}

fn items(app: &App, popup: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(popup)
        .unwrap()
        .iter()
        .filter(|c| app.world().get::<MenuLabel>(*c).is_some())
        .collect()
}

fn spawn(app: &mut App, bundle: impl Bundle) -> M {
    let root = app.world_mut().spawn(bundle).id();
    frames(app, 2);
    let trigger = child_with::<MenuTrigger>(app, root);
    let popup = child_with::<MenuPopupOf>(app, root);
    let w = app.world_mut();
    let window = w
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap();
    M {
        app: std::mem::take(app),
        root,
        trigger,
        popup,
        window,
    }
}

fn camera_menu() -> M {
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let t2 = t.clone();
    spawn(
        &mut app,
        menu(
            &t,
            MenuSide::Below,
            text_button_parts(&t, TextButton::new("Camera").chevron()),
            move |p: &mut ChildSpawner| {
                p.spawn(menu_group_label("View"));
                p.spawn(menu_item(&t2, "Focus", None, Some("F".into())));
                p.spawn(menu_item(&t2, "Home", None, None));
                p.spawn(menu_separator());
                p.spawn(menu_item(&t2, "Follow", None, None));
                p.spawn(menu_checkbox(&t2, "Grid", false, Some("G".into())));
            },
        ),
    )
}

impl M {
    fn visible(&self) -> bool {
        *self.app.world().get::<Visibility>(self.popup).unwrap() == Visibility::Visible
    }

    fn focused(&self) -> Option<Entity> {
        self.app.world().resource::<InputFocus>().get()
    }

    fn open(&mut self) {
        let trigger = self.trigger;
        self.app.world_mut().trigger(MenuEvent {
            source: trigger,
            action: MenuAction::Open(NavAction::First),
        });
        frames(&mut self.app, 2);
    }

    fn key(&mut self, code: KeyCode, logical: Key, text: Option<&str>) {
        let window = self.window;
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: logical.clone(),
                state,
                text: text.map(Into::into),
                repeat: false,
                window,
            });
            frames(&mut self.app, 1);
        }
        frames(&mut self.app, 1);
    }
}

#[test]
fn opens_navigates_and_closes_on_escape() {
    let mut m = camera_menu();
    assert!(!m.visible());
    m.open();
    assert!(m.visible());
    let list = items(&m.app, m.popup);
    assert_eq!(list.len(), 4);
    assert_eq!(m.focused(), Some(list[0]));
    // The focused item looks hovered.
    assert_eq!(
        m.app.world().get::<Fill>(list[0]).unwrap().0,
        ColorRole::Hover
    );
    m.key(KeyCode::ArrowDown, Key::ArrowDown, None);
    assert_eq!(m.focused(), Some(list[1]));
    m.key(KeyCode::ArrowDown, Key::ArrowDown, None);
    assert_eq!(m.focused(), Some(list[2]));
    m.key(KeyCode::ArrowUp, Key::ArrowUp, None);
    assert_eq!(m.focused(), Some(list[1]));
    m.key(KeyCode::End, Key::End, None);
    assert_eq!(m.focused(), Some(list[3]));
    m.key(KeyCode::Home, Key::Home, None);
    assert_eq!(m.focused(), Some(list[0]));
    // Type-to-search: "f" from Focus goes to Follow.
    m.key(KeyCode::KeyF, Key::Character("f".into()), Some("f"));
    assert_eq!(m.focused(), Some(list[2]));
    // A chord isn't typing: Ctrl+F leaves focus where it is.
    m.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    m.key(KeyCode::KeyF, Key::Character("f".into()), Some("f"));
    assert_eq!(m.focused(), Some(list[2]));
    m.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    m.key(KeyCode::Escape, Key::Escape, None);
    assert!(!m.visible());
    // Focus returns to the trigger.
    assert_eq!(m.focused(), Some(m.trigger));
}

#[test]
fn selection_and_outside_click_close_it() {
    let mut m = camera_menu();
    m.open();
    let list = items(&m.app, m.popup);
    // Enter activates and closes.
    #[derive(Resource, Default)]
    struct Hits(Vec<Entity>);
    m.app
        .init_resource::<Hits>()
        .add_observer(|a: On<Activate>, mut h: ResMut<Hits>| h.0.push(a.entity));
    m.key(KeyCode::Enter, Key::Enter, None);
    assert_eq!(m.app.world().resource::<Hits>().0, vec![list[0]]);
    assert!(!m.visible());
    // Focus moving elsewhere (a click outside) closes it.
    m.open();
    assert!(m.visible());
    let outside = m.app.world_mut().spawn(Node::default()).id();
    m.app
        .world_mut()
        .resource_mut::<InputFocus>()
        .set(outside, FocusCause::Navigated);
    frames(&mut m.app, 2);
    assert!(!m.visible());
    // Toggle opens and closes.
    let trigger = m.trigger;
    for want in [true, false] {
        m.app.world_mut().trigger(MenuEvent {
            source: trigger,
            action: MenuAction::Toggle,
        });
        frames(&mut m.app, 2);
        assert_eq!(m.visible(), want);
    }
    assert!(m.app.world().get::<Menu>(m.root).is_some());
}

#[test]
fn checkbox_items_toggle() {
    let mut m = camera_menu();
    m.open();
    let list = items(&m.app, m.popup);
    let grid = list[3];
    assert!(m.app.world().get::<CheckItem>(grid).is_some());
    let boxed = child_with::<MenuCheckBox>(&m.app, grid);
    let fill = |m: &M| m.app.world().get::<Fill>(boxed).unwrap().0;
    assert_eq!(fill(&m), ColorRole::Clear);
    m.app.world_mut().trigger(Activate { entity: grid });
    frames(&mut m.app, 1);
    assert!(m.app.world().get::<Checked>(grid).is_some());
    assert_eq!(fill(&m), ColorRole::Accent);
    // A checked item keeps the plain look.
    assert_ne!(
        m.app.world().get::<Fill>(grid).unwrap().0,
        ColorRole::AccentSoft
    );
    m.app.world_mut().trigger(Activate { entity: grid });
    frames(&mut m.app, 1);
    assert!(m.app.world().get::<Checked>(grid).is_none());
}

#[test]
fn select_picks_an_option() {
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let mut m = spawn(
        &mut app,
        select(
            &t,
            "Mode",
            vec!["Free".into(), "Follow".into(), "Track".into()],
            0,
            120.0,
        ),
    );
    #[derive(Resource, Default)]
    struct Changes(Vec<usize>);
    m.app
        .init_resource::<Changes>()
        .add_observer(|c: On<SelectChanged>, mut r: ResMut<Changes>| r.0.push(c.index));
    let options = items(&m.app, m.popup);
    assert!(m.app.world().get::<Active>(options[0]).is_some());
    m.app.world_mut().trigger(Activate { entity: options[2] });
    frames(&mut m.app, 1);
    let w = m.app.world();
    assert_eq!(w.get::<Select>(m.root).unwrap().selected, 2);
    assert!(w.get::<Active>(options[2]).is_some());
    assert!(w.get::<Active>(options[0]).is_none());
    assert_eq!(w.get::<SelectOption>(options[2]), Some(&SelectOption(2)));
    let label = child_with::<SelectLabel>(&m.app, m.trigger);
    assert_eq!(m.app.world().get::<Text>(label).unwrap().0, "Track");
    assert_eq!(m.app.world().resource::<Changes>().0, vec![2]);
    // Activating something else does nothing here.
    let e = m.app.world_mut().spawn_empty().id();
    m.app.world_mut().trigger(Activate { entity: e });
    frames(&mut m.app, 1);
    assert_eq!(m.app.world().resource::<Changes>().0, vec![2]);
}

#[test]
fn type_to_search_wraps() {
    let labels = ["Focus", "Home", "Follow", "Grid"];
    assert_eq!(type_to_search(&labels, None, 'h'), Some(1));
    assert_eq!(type_to_search(&labels, Some(0), 'F'), Some(2));
    assert_eq!(type_to_search(&labels, Some(2), 'f'), Some(0));
    assert_eq!(type_to_search(&labels, Some(1), 'x'), None);
}

#[test]
fn shortcut_text() {
    let chord = |key, cmd, ctrl, shift, alt| Chord {
        key,
        mods: Mods {
            cmd,
            ctrl,
            shift,
            alt,
        },
    };
    let l = Platform::Linux;
    assert_eq!(
        chord_label(&chord(KeyCode::KeyF, false, false, true, false), l),
        "⇧F"
    );
    assert_eq!(
        chord_label(&chord(KeyCode::Digit5, false, false, false, false), l),
        "5"
    );
    assert_eq!(
        chord_label(&chord(KeyCode::KeyE, true, false, true, false), l),
        "Ctrl+Shift+E"
    );
    assert_eq!(
        chord_label(
            &chord(KeyCode::KeyE, true, false, true, false),
            Platform::Mac
        ),
        "Cmd+Shift+E"
    );
    assert_eq!(
        chord_label(
            &chord(KeyCode::Digit1, false, false, false, true),
            Platform::Mac
        ),
        "Opt+1"
    );
    assert_eq!(
        chord_label(&chord(KeyCode::KeyK, false, true, false, true), l),
        "Ctrl+Alt+K"
    );
    for (key, want) in [
        (KeyCode::Escape, "Esc"),
        (KeyCode::Space, "Space"),
        (KeyCode::Enter, "⏎"),
        (KeyCode::Backspace, "⌫"),
        (KeyCode::Delete, "Del"),
        (KeyCode::Tab, "Tab"),
        (KeyCode::ArrowLeft, "←"),
        (KeyCode::ArrowRight, "→"),
        (KeyCode::ArrowUp, "↑"),
        (KeyCode::ArrowDown, "↓"),
        (KeyCode::Comma, ","),
        (KeyCode::Period, "."),
        (KeyCode::Slash, "/"),
        (KeyCode::Minus, "-"),
        (KeyCode::Equal, "="),
        (KeyCode::BracketLeft, "["),
        (KeyCode::BracketRight, "]"),
        (KeyCode::Numpad7, "7"),
        (KeyCode::F2, "F2"),
    ] {
        assert_eq!(key_label(key), want);
    }
    let map = InputMap::parse(gz_bevy::view::input::SHORTCUTS_JSON, Platform::Linux).unwrap();
    assert_eq!(
        shortcut_label(&map, "view.projection").as_deref(),
        Some("5")
    );
    assert_eq!(shortcut_label(&map, "view.follow").as_deref(), Some("⇧F"));
    assert_eq!(shortcut_label(&map, "overlay.grid").as_deref(), Some("G"));
    assert_eq!(shortcut_label(&map, "no.such.binding"), None);
    let mac = InputMap::parse(gz_bevy::view::input::SHORTCUTS_JSON, Platform::Mac).unwrap();
    assert!(shortcut_label(&mac, "panel.scene")
        .unwrap()
        .starts_with("Cmd+"));
}

/// A disabled item dims everything in it, its shortcut, icon and checkbox
/// too, each keeping its own colour.
#[test]
fn disabled_items_dim_their_shortcut_icon_and_box() {
    use crate::theme::visuals::DISABLED_OPACITY;
    use crate::theme::{Ink, Opacity};
    use bevy::ui::InteractionDisabled;
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let item = app
        .world_mut()
        .spawn((
            menu_item(
                &t,
                "Grid",
                Some(crate::assets::icons::IconId::Home),
                Some("G".into()),
            ),
            InteractionDisabled,
        ))
        .id();
    let check = app
        .world_mut()
        .spawn((
            menu_checkbox(&t, "Snap", true, Some("S".into())),
            InteractionDisabled,
        ))
        .id();
    frames(&mut app, 3);
    let w = app.world();
    let opacity = |e| w.get::<Opacity>(e).map(|o| o.0);
    let parts: Vec<Entity> = w.get::<Children>(item).unwrap().to_vec();
    // Icon, label, spacer, shortcut.
    assert_eq!(opacity(parts[0]), Some(DISABLED_OPACITY));
    assert_eq!(w.get::<Ink>(parts[0]).unwrap().0, ColorRole::Text2);
    assert_eq!(opacity(parts[1]), Some(DISABLED_OPACITY));
    assert_eq!(opacity(parts[3]), Some(DISABLED_OPACITY));
    assert_eq!(w.get::<Ink>(parts[3]).unwrap().0, ColorRole::Text3);
    let parts: Vec<Entity> = w.get::<Children>(check).unwrap().to_vec();
    assert!(w.get::<MenuCheckBox>(parts[0]).is_some());
    assert_eq!(opacity(parts[0]), Some(DISABLED_OPACITY));
    assert_eq!(w.get::<Fill>(parts[0]).unwrap().0, ColorRole::Accent);
    assert_eq!(opacity(parts[3]), Some(DISABLED_OPACITY));
    // The resolved colours carry the opacity.
    let bg = w.get::<BackgroundColor>(parts[0]).unwrap().0;
    assert!((bg.alpha() - DISABLED_OPACITY).abs() < 1e-6);
}

#[test]
fn menus_fall_back_to_the_mirrored_end_placement() {
    use bevy::ui_widgets::popover::{PopoverAlign, PopoverSide};
    let p = MenuSide::Below.popover();
    let sides: Vec<(PopoverSide, PopoverAlign)> =
        p.positions.iter().map(|p| (p.side, p.align)).collect();
    assert_eq!(
        sides,
        [
            (PopoverSide::Bottom, PopoverAlign::Start),
            (PopoverSide::Top, PopoverAlign::Start),
            (PopoverSide::Bottom, PopoverAlign::End),
            (PopoverSide::Top, PopoverAlign::End),
        ]
    );
}
