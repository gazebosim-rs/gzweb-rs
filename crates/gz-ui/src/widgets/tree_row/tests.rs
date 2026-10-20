//! Tree rows: selection, visibility and the eye.

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

fn part(app: &mut App, row: Entity, which: fn(&TreeRowPart) -> bool) -> Entity {
    let w = app.world_mut();
    let kids = descendants(w, row);
    kids.into_iter()
        .find(|e| w.get::<TreeRowPart>(*e).is_some_and(which))
        .unwrap()
}

#[test]
fn tree_rows_show_selection_and_visibility() {
    let t = Theme::default();
    let mut app = ui_app(ThemeMode::Light);
    let spec = TreeRowSpec {
        depth: 2,
        kind: EntityKind::Model,
        name: "amr_01".into(),
        expanded: Some(false),
        hidden: false,
    };
    let row = app.world_mut().spawn(tree_row(&t, spec)).id();
    frames(&mut app, 2);
    let badge = part(&mut app, row, |p| matches!(p, TreeRowPart::Badge));
    let letter = part(&mut app, row, |p| matches!(p, TreeRowPart::Letter));
    let name = part(&mut app, row, |p| matches!(p, TreeRowPart::Name));
    let eye = part(&mut app, row, |p| matches!(p, TreeRowPart::Eye));
    let glyph = app.world().get::<Children>(eye).unwrap()[0];
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
    // Clicking the eye hides the row: eye_off, 50% opacity.
    app.world_mut().trigger(Activate { entity: eye });
    frames(&mut app, 2);
    let w = app.world();
    assert!(w.get::<TreeRowSpec>(row).unwrap().hidden);
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
