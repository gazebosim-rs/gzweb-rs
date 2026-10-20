//! Widget sizes against the spec heights, laid out headlessly.

use bevy::prelude::*;
use bevy::ui::ComputedNode;

use crate::assets::icons::IconId;
use crate::test_support::{frames, ui_app};
use crate::theme::{Theme, ThemeMode};
use crate::widgets::*;

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
