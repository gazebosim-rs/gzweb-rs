use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzCommand;

use super::view::{WorldAction, WorldField};
use crate::test_support::frames;
use crate::test_support::shell::{go_live, sent, shell_app};
use crate::theme::{ColorRole, Ink};

/// Each field's text and ink.
fn fields(app: &mut App) -> Vec<(WorldField, String, ColorRole)> {
    let world = app.world_mut();
    let mut out: Vec<_> = world
        .query::<(&WorldField, &Text, &Ink)>()
        .iter(world)
        .map(|(f, t, i)| (*f, t.0.clone(), i.0))
        .collect();
    out.sort_by_key(|(f, _, _)| *f as u8);
    out
}

fn action(app: &mut App, which: WorldAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &WorldAction)>()
        .iter(world)
        .find(|(_, a)| **a == which)
        .expect("the action")
        .0
}

#[test]
fn the_world_inspector_follows_the_server_and_resets_a_live_world() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    assert_eq!(
        fields(&mut app),
        [
            (WorldField::Name, "No world".into(), ColorRole::Text3),
            (
                WorldField::Source,
                "localhost:9004".into(),
                ColorRole::Text3
            ),
            (WorldField::Step, "—".into(), ColorRole::Text3),
        ]
    );
    // Reset needs a live world; Save and Statistics aren't there yet.
    let reset = action(&mut app, WorldAction::Reset);
    let save = action(&mut app, WorldAction::Save);
    assert!(app.world().get::<InteractionDisabled>(reset).is_some());
    app.world_mut().trigger(Activate { entity: reset });
    frames(&mut app, 1);
    assert!(sent(&mut app).is_empty());
    go_live(&mut app, true);
    assert_eq!(
        fields(&mut app),
        [
            (WorldField::Name, "warehouse_demo".into(), ColorRole::Text),
            (
                WorldField::Source,
                "localhost:9004".into(),
                ColorRole::Text3
            ),
            (WorldField::Step, "1 ms".into(), ColorRole::Text),
        ]
    );
    assert!(app.world().get::<InteractionDisabled>(reset).is_none());
    assert!(app.world().get::<InteractionDisabled>(save).is_some());
    app.world_mut().trigger(Activate { entity: reset });
    frames(&mut app, 1);
    assert_eq!(sent(&mut app), [GzCommand::Reset]);
    app.world_mut().trigger(Activate { entity: save });
    frames(&mut app, 1);
    assert!(sent(&mut app).is_empty());
}

#[test]
fn a_rebuilt_world_inspector_reads_the_server_and_the_phase() {
    use gz_bevy::spawn::GzEntity;
    use gz_bevy::view::Selection;
    use gz_core::scene::NodeKind;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let model = app
        .world_mut()
        .spawn((
            GzEntity {
                name: "cart".into(),
                id: 0,
                kind: NodeKind::Model,
                path: "cart".into(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let round_trip = |app: &mut App| {
        app.world_mut().resource_mut::<Selection>().entities = vec![model];
        frames(app, 3);
        app.world_mut().resource_mut::<Selection>().entities.clear();
        frames(app, 3);
    };
    // Not live: a World body built again keeps Reset off.
    round_trip(&mut app);
    let reset = action(&mut app, WorldAction::Reset);
    assert!(app.world().get::<InteractionDisabled>(reset).is_some());
    // Live: it reads the world straight away, with no new stats.
    go_live(&mut app, true);
    round_trip(&mut app);
    let reset = action(&mut app, WorldAction::Reset);
    assert!(app.world().get::<InteractionDisabled>(reset).is_none());
    assert_eq!(
        fields(&mut app)[0],
        (WorldField::Name, "warehouse_demo".into(), ColorRole::Text)
    );
    assert_eq!(fields(&mut app)[2].1, "1 ms");
}
