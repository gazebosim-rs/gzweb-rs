//! The widgets that are one file each (chips, badges, buttons, text
//! fields, shortcut text, the drag cursor), the layout and text helpers,
//! and every widget's height against the spec.

use bevy::prelude::*;
use bevy::ui::{Checked, ComputedNode};
use gz_bevy::view::input::{Chord, InputMap, Mods, Platform};

use super::{
    activity_button, badge, button, filter_chip, icon_button, menu_item, number_field,
    primary_button, section_header, segmented_group, splitter, status_chip, text_button,
    tool_button, tree_row, AxisTag, BadgeKind, ButtonVariant, EntityKind, IconButtonSize,
    MenuItemProps, NumberField, SplitAxis, Splitter, StatusKind, TextButtonProps, Tooltip,
    TreeRowSpec,
};
use crate::assets::icons::IconId;
use crate::test_support::{descendants, frames, ui_app};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Fill, Ink, Stroke, Theme, ThemeMode};
use crate::widgets::shortcut::{chord_label, key_label, shortcut_label};

fn size(app: &mut App, e: Entity) -> Vec2 {
    let n = app.world().get::<ComputedNode>(e).expect("laid out");
    n.size() * n.inverse_scale_factor()
}

#[test]
fn widget_heights_match_the_spec() {
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let w = app.world_mut();
    let root = w
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            width: px(600),
            ..default()
        })
        .id();
    let mut spawn = |b: Box<dyn FnOnce(&mut World) -> Entity>| {
        let e = b(w);
        w.entity_mut(root).add_child(e);
        e
    };
    let _ = &mut spawn;
    let header = w.spawn(section_header(&t, "Scene", Some("14".into()))).id();
    let item = w
        .spawn(menu_item(&t, MenuItemProps::new("Grid").shortcut("G")))
        .id();
    let row = w
        .spawn(tree_row(
            &t,
            TreeRowSpec {
                depth: 1,
                kind: EntityKind::Model,
                name: "amr_01".into(),
                expanded: None,
                hidden: false,
            },
        ))
        .id();
    let bordered = w.spawn(button(&t, "Duplicate", ButtonVariant::Normal)).id();
    let input = w
        .spawn((
            Node {
                width: px(200),
                ..default()
            },
            children![number_field(&t, AxisTag::X, NumberField::new(1.0), false)],
        ))
        .id();
    let icon32 = w
        .spawn(icon_button(
            &t,
            IconId::Settings,
            IconButtonSize::Regular,
            Tooltip::new("S"),
        ))
        .id();
    let icon28 = w
        .spawn(icon_button(
            &t,
            IconId::Filter,
            IconButtonSize::Small,
            Tooltip::new("F"),
        ))
        .id();
    let rail = w
        .spawn(icon_button(
            &t,
            IconId::CamPan,
            IconButtonSize::Rail,
            Tooltip::new("P"),
        ))
        .id();
    for e in [header, item, row, bordered, input, icon32, icon28, rail] {
        w.entity_mut(root).add_child(e);
    }
    frames(&mut app, 3);
    assert_eq!(size(&mut app, header).y, 36.0);
    assert_eq!(size(&mut app, item).y, 30.0);
    assert_eq!(size(&mut app, row).y, 28.0);
    assert_eq!(size(&mut app, bordered).y, 30.0);
    let field = app.world().get::<Children>(input).unwrap()[0];
    assert_eq!(size(&mut app, field).y, 32.0);
    assert_eq!(size(&mut app, icon32), Vec2::splat(32.0));
    assert_eq!(size(&mut app, icon28), Vec2::splat(28.0));
    assert_eq!(size(&mut app, rail), Vec2::splat(32.0));
}

#[test]
fn chips_badges_and_buttons() {
    assert_eq!(BadgeKind::Count.fill(), ColorRole::Text2);
    assert_eq!(BadgeKind::Warning.fill(), ColorRole::WarnBadge);
    assert_eq!(BadgeKind::Error.fill(), ColorRole::Danger);
    assert_eq!(
        StatusKind::Warning.roles(),
        (ColorRole::WarnSoft, ColorRole::WarnText)
    );
    assert_eq!(
        StatusKind::Error.roles(),
        (ColorRole::DangerSoft, ColorRole::Danger)
    );
    assert_eq!(
        StatusKind::Info.roles(),
        (ColorRole::Clear, ColorRole::Text2)
    );
    let t = Theme::default();
    assert_eq!(IconButtonSize::Regular.metrics(&t), (32.0, 17.0, 7.0));
    assert_eq!(IconButtonSize::Small.metrics(&t), (28.0, 15.0, 7.0));
    assert_eq!(IconButtonSize::Rail.metrics(&t), (32.0, 18.0, 8.0));
    assert_eq!(IconButtonSize::Bar.metrics(&t), (20.0, 15.0, 4.0));

    let mut app = ui_app(ThemeMode::Light);
    let w = app.world_mut();
    let chip = w.spawn(filter_chip("All", true)).id();
    let status = w
        .spawn(status_chip(
            &t,
            StatusKind::Warning,
            "Mesh not found",
            Some("2 warnings".into()),
        ))
        .id();
    let info = w
        .spawn(status_chip(&t, StatusKind::Info, "Saved", None))
        .id();
    let b = w.spawn(badge(&t, BadgeKind::Warning, 12)).id();
    let destructive = w
        .spawn(button(&t, "Delete", ButtonVariant::Destructive))
        .id();
    let play = w
        .spawn(primary_button(&t, IconId::Play, Tooltip::new("Play")))
        .id();
    let tb = w
        .spawn(text_button(
            &t,
            TextButtonProps::new("warehouse_demo")
                .leading(IconId::Folder)
                .dot(ColorRole::Warn)
                .chevron()
                .large(),
        ))
        .id();
    let group = w.spawn(segmented_group("Tools", true, true)).id();
    let tool = w
        .spawn((
            tool_button(
                &t,
                IconId::Navigate,
                IconButtonSize::Regular,
                Tooltip::new("Navigate"),
                true,
            ),
            ChildOf(group),
        ))
        .id();
    let plain = w.spawn(segmented_group("Transport", false, false)).id();
    let activity = w
        .spawn(activity_button(
            &t,
            IconId::ActivityConsole,
            Tooltip::new("Console"),
            Some((BadgeKind::Error, 3)),
        ))
        .id();
    frames(&mut app, 2);
    let w = app.world();
    // A selected filter chip: `text` fill, `surface` label.
    assert!(w.get::<Checked>(chip).is_some());
    assert_eq!(w.get::<Fill>(chip).unwrap().0, ColorRole::Text);
    // No `text` border on the `text` fill (it leaves a ragged seam); the
    // border keeps its width, clear.
    assert_eq!(w.get::<Stroke>(chip).unwrap().0, ColorRole::Clear);
    assert_eq!(w.get::<Node>(chip).unwrap().border, UiRect::all(px(1)));
    assert_eq!(w.get::<Fill>(status).unwrap().0, ColorRole::WarnSoft);
    assert_eq!(w.get::<Children>(status).unwrap().len(), 3);
    assert_eq!(w.get::<Children>(info).unwrap().len(), 1);
    assert_eq!(w.get::<Fill>(b).unwrap().0, ColorRole::WarnBadge);
    let label = w.get::<Children>(destructive).unwrap()[0];
    assert_eq!(w.get::<Ink>(label).unwrap().0, ColorRole::Danger);
    assert_eq!(w.get::<Fill>(play).unwrap().0, ColorRole::Primary);
    assert_eq!(w.get::<Children>(tb).unwrap().len(), 4);
    assert!(w.get::<bevy::ui_widgets::RadioGroup>(group).is_some());
    assert!(w.get::<bevy::ui_widgets::RadioGroup>(plain).is_none());
    assert!(w.get::<Checked>(tool).is_some());
    assert_eq!(w.get::<Fill>(tool).unwrap().0, ColorRole::AccentSoft);
    assert_eq!(w.get::<Children>(activity).unwrap().len(), 2);
    // Active activity button: inset accent_ring.
    app.world_mut().entity_mut(activity).insert(Active);
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<Stroke>(activity).unwrap().0,
        ColorRole::AccentRing
    );
}

/// Hover cursors are on (Bevy's `CursorIconPlugin`), and a drag holds the
/// cursor of what it started on until it ends.
#[test]
fn drags_hold_their_cursor() {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::cursor::{EntityCursor, OverrideCursor};
    use bevy::picking::events::{Pointer, PointerDragEnd, PointerDragStart};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
    let mut app = ui_app(ThemeMode::Light);
    let pane = app.world_mut().spawn(Node::default()).id();
    let s = Splitter {
        axis: SplitAxis::Rows,
        pane,
        sign: 1.0,
        min: 0.0,
        max: 100.0,
    };
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize"))
        .id();
    let plain = app.world_mut().spawn(Node::default()).id();
    let line = app.world().get::<Children>(bar).unwrap()[0];
    frames(&mut app, 2);
    // The plugin's system gives the window a cursor.
    let w = app.world_mut();
    let window = w
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap();
    assert!(w.get::<CursorIcon>(window).is_some());
    let pointer = |id| {
        Pointer::new(
            id,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        )
    };
    let start = |w: &mut World, entity, id| {
        w.trigger(PointerDragStart {
            entity,
            pointer: pointer(id),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        });
        w.flush();
    };
    let end = |w: &mut World, entity, id| {
        w.trigger(PointerDragEnd {
            entity,
            pointer: pointer(id),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
        });
        w.flush();
    };
    let held = |w: &World| w.resource::<OverrideCursor>().0.clone();
    // Started on the splitter's line: the splitter's cursor.
    start(w, line, PointerId::Mouse);
    assert_eq!(
        held(w),
        Some(EntityCursor::System(SystemCursorIcon::RowResize))
    );
    end(w, line, PointerId::Mouse);
    assert_eq!(held(w), None);
    // Started on something without a cursor: the default one.
    start(w, plain, PointerId::Mouse);
    assert_eq!(held(w), Some(EntityCursor::default()));
    end(w, plain, PointerId::Mouse);
    // Touch has no cursor to hold.
    start(w, bar, PointerId::Touch(1));
    assert_eq!(held(w), None);
}

/// Enter in a text field submits once, however deep the field sits (the
/// key bubbles through every ancestor).
#[test]
fn text_fields_submit_once() {
    use crate::widgets::{text_field, TextSubmit};
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::window::PrimaryWindow;
    #[derive(Resource, Default)]
    struct Submits(Vec<String>);
    let mut app = ui_app(ThemeMode::Light);
    app.init_resource::<Submits>()
        .add_observer(|ev: On<TextSubmit>, mut s: ResMut<Submits>| {
            s.0.push(ev.value.clone());
        });
    let outer = app.world_mut().spawn(Node::default()).id();
    let inner = app
        .world_mut()
        .spawn((Node::default(), ChildOf(outer)))
        .id();
    let field = app
        .world_mut()
        .spawn((
            text_field(&Theme::default(), "Address", "ws://localhost:9002"),
            ChildOf(inner),
        ))
        .id();
    frames(&mut app, 2);
    let input = app.world().get::<Children>(field).unwrap()[0];
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Navigated);
    frames(&mut app, 1);
    // The focus look.
    assert_eq!(
        app.world().get::<Stroke>(field).unwrap().0,
        ColorRole::Accent
    );
    let window = {
        let w = app.world_mut();
        w.query_filtered::<Entity, With<PrimaryWindow>>()
            .single(w)
            .unwrap()
    };
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 2);
    assert_eq!(
        app.world().resource::<Submits>().0,
        ["ws://localhost:9002".to_string()]
    );
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

#[test]
fn layout_primitives_say_what_they_are() {
    use super::layout::{
        clip_box, column, display, fill_parent, flex_spacer, group, group_header, overlay,
        property_row, row, separator, Orientation,
    };
    let r = row(6.0);
    assert_eq!((r.column_gap, r.align_items), (px(6), AlignItems::Center));
    let c = column(4.0);
    assert_eq!(
        (c.flex_direction, c.row_gap),
        (FlexDirection::Column, px(4))
    );
    assert_eq!(flex_spacer().flex_grow, 1.0);
    let f = fill_parent();
    assert_eq!(
        (f.width, f.height),
        (Val::Percent(100.0), Val::Percent(100.0))
    );
    let o = overlay();
    assert_eq!(o.position_type, PositionType::Absolute);
    assert_eq!([o.left, o.right, o.top, o.bottom], [px(0); 4]);
    let b = clip_box();
    assert_eq!((b.min_width, b.overflow), (px(0), Overflow::clip()));
    assert_eq!(
        (display(true), display(false)),
        (Display::Flex, Display::None)
    );
    // Separators run either way, 1px of `line`.
    let mut app = ui_app(ThemeMode::Light);
    let w = app.world_mut();
    let v = w.spawn(separator(Orientation::Vertical, 18.0, 4.0)).id();
    let h = w.spawn(separator(Orientation::Horizontal, 24.0, 6.0)).id();
    let node = |e| w.get::<Node>(e).unwrap().clone();
    let (vn, hn) = (node(v), node(h));
    assert_eq!((vn.width, vn.height), (px(1), px(18)));
    assert_eq!((vn.margin.left, vn.margin.top), (px(4), px(0)));
    assert_eq!((hn.width, hn.height), (px(24), px(1)));
    assert_eq!((hn.margin.left, hn.margin.top), (px(0), px(6)));
    assert_eq!(w.get::<Fill>(v).unwrap().0, ColorRole::Line);
    // A property group: header, then rows of label, spacer and value.
    let g = w
        .spawn(group())
        .with_children(|g| {
            g.spawn(group_header("Physics", Some("hint".into())));
            g.spawn(property_row("Gravity", Text::new("on")));
        })
        .id();
    frames(&mut app, 1);
    let mut texts: Vec<String> = descendants(app.world(), g)
        .into_iter()
        .filter_map(|e| app.world().get::<Text>(e).map(|t| t.0.clone()))
        .collect();
    texts.sort();
    assert_eq!(texts, ["Gravity", "Physics", "hint", "on"]);
}

#[test]
fn readout_setters_mark_changed_only_on_a_change() {
    use super::layout::set_shown;
    use super::text::set_text;
    let mut world = World::new();
    let e = world.spawn((Text::new("same"), Node::default())).id();
    world.clear_trackers();
    let mut t = world.get_mut::<Text>(e).unwrap();
    set_text(&mut t, "same");
    assert!(!t.is_changed());
    set_text(&mut t, "new");
    assert!(t.is_changed() && t.0 == "new");
    let mut n = world.get_mut::<Node>(e).unwrap();
    set_shown(&mut n, true);
    assert!(!n.is_changed());
    set_shown(&mut n, false);
    assert!(n.is_changed() && n.display == Display::None);
}
