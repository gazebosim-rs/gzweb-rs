//! Tree rows: selection, visibility, recycling and the controlled caret
//! and eye.

use bevy::a11y::AccessibilityNode;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{name_type, TreeRowPart};
use crate::assets::fonts::Type;
use crate::assets::icons::{Icon, IconId};
use crate::test_support::{descendants, frames, ui_app};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Fill, Ink, Opacity, Theme, ThemeMode};
use crate::widgets::*;

fn part(app: &mut App, row: Entity, which: TreeRowPart) -> Entity {
    let w = app.world_mut();
    let kids = descendants(w, row);
    kids.into_iter()
        .find(|e| w.get::<TreeRowPart>(*e) == Some(&which))
        .unwrap()
}

fn spec() -> TreeRowSpec {
    TreeRowSpec {
        depth: 2,
        kind: EntityKind::Model,
        name: "amr_01".into(),
        expanded: Some(false),
        hidden: false,
        hideable: true,
        dimmed: false,
    }
}

/// What the row's owner saw: (expanded asks, hidden asks).
#[derive(Resource, Default)]
struct Asked(Vec<bool>, Vec<bool>);

fn app_with_row() -> (App, Entity) {
    let t = Theme::default();
    let mut app = ui_app(ThemeMode::Light);
    app.init_resource::<Asked>()
        .add_observer(|ev: On<TreeRowExpand>, mut a: ResMut<Asked>| a.0.push(ev.expanded))
        .add_observer(|ev: On<TreeRowVisibility>, mut a: ResMut<Asked>| a.1.push(ev.hidden));
    let row = app.world_mut().spawn(tree_row(&t, spec())).id();
    frames(&mut app, 2);
    (app, row)
}

#[test]
fn tree_rows_show_selection_and_visibility() {
    let (mut app, row) = app_with_row();
    let badge = part(&mut app, row, TreeRowPart::Badge);
    let letter = part(&mut app, row, TreeRowPart::Letter);
    let name = part(&mut app, row, TreeRowPart::Name);
    let glyph = part(&mut app, row, TreeRowPart::EyeGlyph);
    let w = app.world();
    assert_eq!(w.get::<Text>(letter).unwrap().0, "M");
    assert_eq!(w.get::<Ink>(letter).unwrap().0, ColorRole::EntityModel);
    assert_eq!(w.get::<Fill>(badge).unwrap().0, ColorRole::EntityBadgeBg);
    // Visible and not hovered: no eye.
    assert_eq!(w.get::<Ink>(glyph).unwrap().0, ColorRole::Clear);
    // Selected: inverted badge, weight 500, accent_soft fill.
    app.world_mut()
        .entity_mut(row)
        .insert((Active, Hovered(true)));
    frames(&mut app, 2);
    let w = app.world();
    assert_eq!(w.get::<Fill>(badge).unwrap().0, ColorRole::Accent);
    assert_eq!(w.get::<Ink>(letter).unwrap().0, ColorRole::Surface);
    assert_eq!(w.get::<Type>(name).unwrap().0, name_type(true));
    assert_eq!(w.get::<Fill>(row).unwrap().0, ColorRole::AccentSoft);
    assert_eq!(w.get::<Ink>(glyph).unwrap().0, ColorRole::Text3);
    // Hidden (its owner wrote it): eye_off, 50% opacity.
    app.world_mut().get_mut::<TreeRowSpec>(row).unwrap().hidden = true;
    frames(&mut app, 2);
    let w = app.world();
    assert_eq!(w.get::<Icon>(glyph).unwrap().id, IconId::EyeOff);
    assert_eq!(w.get::<Opacity>(name).unwrap().0, 0.5);
    assert_eq!(name_type(false).weight, 400);
    for (kind, letter, role) in [
        (EntityKind::World, "W", ColorRole::EntityWorld),
        (EntityKind::Light, "L", ColorRole::EntityLight),
        (EntityKind::Link, "K", ColorRole::EntityLink),
        (EntityKind::Joint, "J", ColorRole::EntityJoint),
        (EntityKind::Sensor, "S", ColorRole::EntitySensor),
    ] {
        assert_eq!((kind.letter(), kind.role()), (letter, role));
    }
}

/// The caret and the eye ask; the spec changes only when the owner writes
/// it (the feathers controlled pattern).
#[test]
fn the_caret_and_the_eye_ask_their_owner() {
    let (mut app, row) = app_with_row();
    let caret = part(&mut app, row, TreeRowPart::Caret);
    let eye = part(&mut app, row, TreeRowPart::Eye);
    app.world_mut().trigger(Activate { entity: caret });
    app.world_mut().trigger(Activate { entity: eye });
    frames(&mut app, 1);
    let asked = app.world().resource::<Asked>();
    assert_eq!((asked.0.clone(), asked.1.clone()), (vec![true], vec![true]));
    assert_eq!(app.world().get::<TreeRowSpec>(row), Some(&spec()));
    // A leaf's caret and a row without a toggle ask nothing.
    {
        let mut s = app.world_mut().get_mut::<TreeRowSpec>(row).unwrap();
        s.expanded = None;
        s.hideable = false;
    }
    frames(&mut app, 1);
    app.world_mut().trigger(Activate { entity: caret });
    app.world_mut().trigger(Activate { entity: eye });
    frames(&mut app, 1);
    let asked = app.world().resource::<Asked>();
    assert_eq!((asked.0.len(), asked.1.len()), (1, 1));
    assert_eq!(app.world().get::<Node>(eye).unwrap().display, Display::None);
}

/// A row reused for another item shows it all: name, letter, indent,
/// caret and accessible labels.
#[test]
fn a_spec_change_repaints_every_part() {
    let (mut app, row) = app_with_row();
    let label = |app: &App, e: Entity| {
        app.world()
            .get::<AccessibilityNode>(e)
            .and_then(|a| a.0.label().map(str::to_string))
    };
    let caret = part(&mut app, row, TreeRowPart::Caret);
    let glyph = part(&mut app, row, TreeRowPart::CaretGlyph);
    let indent = part(&mut app, row, TreeRowPart::Indent);
    let eye = part(&mut app, row, TreeRowPart::Eye);
    let name = part(&mut app, row, TreeRowPart::Name);
    let letter = part(&mut app, row, TreeRowPart::Letter);
    let unit = Theme::default().s.tree_indent;
    assert_eq!(
        app.world().get::<Node>(indent).unwrap().width,
        px(2.0 * unit)
    );
    assert_eq!(
        app.world().get::<Icon>(glyph).unwrap().id,
        IconId::ChevronRight
    );
    assert_eq!(label(&app, caret).as_deref(), Some("Expand"));
    assert_eq!(label(&app, eye).as_deref(), Some("Hide amr_01"));
    let a = app.world().get::<AccessibilityNode>(row).unwrap();
    assert_eq!(
        (a.0.label(), a.0.is_expanded(), a.0.level()),
        (Some("amr_01"), Some(false), Some(3))
    );

    *app.world_mut().get_mut::<TreeRowSpec>(row).unwrap() = TreeRowSpec {
        depth: 0,
        kind: EntityKind::Sensor,
        name: "front_camera".into(),
        expanded: Some(true),
        hidden: true,
        hideable: true,
        dimmed: false,
    };
    frames(&mut app, 2);
    let w = app.world();
    assert_eq!(w.get::<Text>(name).unwrap().0, "front_camera");
    assert_eq!(w.get::<Text>(letter).unwrap().0, "S");
    assert_eq!(w.get::<Ink>(letter).unwrap().0, ColorRole::EntitySensor);
    assert_eq!(w.get::<Node>(indent).unwrap().width, px(0.0));
    assert_eq!(w.get::<Icon>(glyph).unwrap().id, IconId::ChevronDown);
    assert_eq!(w.get::<Ink>(glyph).unwrap().0, ColorRole::Text3);
    assert_eq!(label(&app, caret).as_deref(), Some("Collapse"));
    assert_eq!(label(&app, eye).as_deref(), Some("Show front_camera"));
    let a = app.world().get::<AccessibilityNode>(row).unwrap();
    assert_eq!(
        (a.0.label(), a.0.is_expanded(), a.0.level()),
        (Some("front_camera"), Some(true), Some(1))
    );
    // A leaf: no caret drawn, no expanded state.
    app.world_mut()
        .get_mut::<TreeRowSpec>(row)
        .unwrap()
        .expanded = None;
    frames(&mut app, 2);
    assert_eq!(app.world().get::<Ink>(glyph).unwrap().0, ColorRole::Clear);
    let a = app.world().get::<AccessibilityNode>(row).unwrap();
    assert_eq!(a.0.is_expanded(), None);
}
