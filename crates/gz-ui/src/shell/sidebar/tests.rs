//! The sidebar's slots: the header buttons and the row splitter.

use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::panels::{PLOT, SCENE};
use crate::shell::layout::MIN_SLOT_H;
use crate::shell::{Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

#[test]
fn slot_header_buttons_swap_and_close() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Secondary,
    });
    frames(&mut app, 2);
    let world = app.world_mut();
    let actions: Vec<(Entity, crate::shell::sidebar::SlotAction)> = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .map(|(e, a)| (e, *a))
        .collect();
    use crate::shell::sidebar::SlotAction::*;
    assert_eq!(actions.iter().filter(|a| a.1 == Swap).count(), 2);
    let first = |a| actions.iter().find(|x| x.1 == a).unwrap().0;
    app.world_mut().trigger(Activate {
        entity: first(Swap),
    });
    frames(&mut app, 2);
    let l = layout(&app);
    assert_eq!((l.primary, l.secondary), (Some(PLOT), Some(SCENE)));
    let world = app.world_mut();
    let close = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Close)
        .unwrap()
        .0;
    world.trigger(Activate { entity: close });
    frames(&mut app, 2);
    assert_eq!(layout(&app).secondary, None);
    let world = app.world_mut();
    let collapse = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Collapse)
        .unwrap()
        .0;
    world.trigger(Activate { entity: collapse });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
}

#[test]
fn the_row_splitter_resizes_the_slots_within_their_minimum() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Secondary,
    });
    frames(&mut app, 3);
    let bar = find::<crate::shell::sidebar::slot_splitter::SlotSplitter>(&mut app);
    assert!(displayed(&app, bar));
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    let world = app.world_mut();
    world.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::new(0.0, -1000.0),
        delta: Vec2::new(0.0, -1000.0),
    });
    world.trigger(PointerDragEnd {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    frames(&mut app, 2);
    let split = layout(&app).split;
    assert!((split - MIN_SLOT_H / (828.0 - 6.0)).abs() < 1e-4, "{split}");
    assert!(app
        .world()
        .get::<crate::shell::sidebar::slot_splitter::SlotDrag>(bar)
        .is_none());
}

/// A count change rewrites the header's count only: the body (M4's tree,
/// its scroll and expansion) stays the same entity.
#[test]
fn a_count_change_keeps_the_panel_body() {
    use crate::shell::sidebar::body::PanelBody;
    use crate::widgets::SectionCount;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, false);
    frames(&mut app, 1);
    let body = find::<PanelBody>(&mut app);
    let count = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<SectionCount>>()
            .iter(world)
            .map(|t| t.0.clone())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
    };
    assert_eq!(count(&mut app), ["0 entities"]);
    let entity = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<gz_bevy::spawn::GzIndex>()
        .order
        .push(entity);
    frames(&mut app, 2);
    assert_eq!(count(&mut app), ["1 entity"]);
    assert_eq!(find::<PanelBody>(&mut app), body, "the body wasn't rebuilt");
}

/// A panel's content fills its body once it's ready, and an error state's
/// Reconnect retries the server (Open world stays disabled).
#[test]
fn panel_content_and_actions() {
    use crate::assets::icons::IconId;
    use crate::panels::{PanelAction, PanelDef, PanelId, PanelView, RegisterPanel};
    use crate::shell::connection::Phase;
    use crate::shell::sidebar::body::PanelBody;
    use bevy::ui::InteractionDisabled;
    use gz_bevy::connection::GzReconnect;

    #[derive(Component)]
    struct Probe;
    const PROBE: PanelId = PanelId::new("probe");
    let mut app = shell_app((1440, 900), 1.0);
    app.register_panel(
        PanelDef::new(PROBE, "Probe", IconId::ActivityPlot, 9, 0)
            .view(|phase| match phase {
                Phase::Live => PanelView::Ready,
                _ => PanelView::Error {
                    message: "down",
                    actions: vec![PanelAction::Reconnect, PanelAction::OpenWorld],
                },
            })
            .content(|b, _| {
                b.spawn(Probe);
            }),
    );
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PROBE,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    let world = app.world_mut();
    let actions: Vec<(Entity, PanelAction, bool)> = world
        .query::<(Entity, &PanelAction, Has<InteractionDisabled>)>()
        .iter(world)
        .map(|(e, a, d)| (e, *a, d))
        .collect();
    assert_eq!(actions.len(), 2);
    for (e, action, disabled) in actions {
        assert_eq!(disabled, action == PanelAction::OpenWorld);
        app.world_mut().trigger(Activate { entity: e });
    }
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
    go_live(&mut app, false);
    let body = find::<PanelBody>(&mut app);
    let probe = find::<Probe>(&mut app);
    assert_eq!(
        app.world().get::<ChildOf>(probe).map(ChildOf::parent),
        Some(body)
    );
}
