//! The sidebar's slots: the header and slot buttons, the row splitter,
//! and what a slot rebuilds (headers and bodies, DECISIONS.md D-58), on
//! the test probe panel.

use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use bevy::app::PluginGroupBuilder;

use crate::inspectors::InspectorPlugins;
use crate::panels::{PanelBody, PanelId, PanelView, ScenePanelPlugin, ASSETS, PLOT, SCENE};
use crate::shell::layout::MIN_SLOT_H;
use crate::shell::sidebar::{PanelHeader, SlotView};
use crate::shell::{ShellPlugin, ShellPlugins};
use crate::shell::{Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::probe::*;
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
        .get::<crate::widgets::splitter::SplitterDrag>(bar)
        .is_none());
    // A double-click puts the default split back.
    for count in [1, 2] {
        app.world_mut().trigger(PointerClick {
            entity: bar,
            pointer: Pointer::new(
                PointerId::Mouse,
                Location {
                    target: bevy::camera::NormalizedRenderTarget::None {
                        width: 1,
                        height: 1,
                    },
                    position: Vec2::ZERO,
                },
            ),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::from_millis(80),
            count,
        });
        frames(&mut app, 1);
    }
    assert_eq!(layout(&app).split, crate::shell::layout::DEFAULT_SPLIT);
    // Its range: both slots at least their minimum.
    let s = *app.world().get::<crate::widgets::Splitter>(bar).unwrap();
    assert_eq!((s.min, s.max), (MIN_SLOT_H, 828.0 - 6.0 - MIN_SLOT_H));
    assert!((s.size - 0.55 * (828.0 - 6.0)).abs() < 1e-3, "{}", s.size);
    // With one slot it is out of the tab order (hidden as undisplayed).
    let shown = |app: &App| app.world().get::<InheritedVisibility>(bar).unwrap().get();
    assert!(shown(&app));
    app.world_mut().write_message(UiCommand::CloseSecondary);
    frames(&mut app, 2);
    assert!(!shown(&app));
}

fn bodies(app: &mut App) -> Vec<(Entity, PanelId)> {
    let world = app.world_mut();
    world
        .query::<(Entity, &PanelBody)>()
        .iter(world)
        .map(|(e, b)| (e, b.0))
        .collect()
}

/// A command naming a panel nothing registered shows the first panel in
/// the activity bar instead of an empty slot.
#[test]
fn an_unknown_panel_id_opens_the_first_panel() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PanelId::new("nope"),
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    let l = layout(&app);
    assert_eq!((l.primary, l.secondary), (Some(SCENE), None));
    let shown: Vec<PanelId> = bodies(&mut app).into_iter().map(|b| b.1).collect();
    assert_eq!(shown, [SCENE]);
    // An activity click names its button's own panel: an unknown one is
    // ignored rather than taken for the (active) first panel, which would
    // collapse the sidebar.
    app.world_mut().write_message(UiCommand::Activity {
        panel: PanelId::new("nope"),
        shift: false,
    });
    frames(&mut app, 2);
    let l = layout(&app);
    assert!(l.sidebar_open);
    assert_eq!(l.primary, Some(SCENE));
}

/// Without its default panel (an app disabled Scene's plugin), the first
/// panel in the bar opens at start; with no panels, the sidebar starts
/// collapsed rather than empty.
#[test]
fn without_the_default_panel_the_first_one_opens() {
    let mut app = shell_app_from(
        (1440, 900),
        1.0,
        ShellPlugins.build().disable::<ScenePanelPlugin>(),
    );
    frames(&mut app, 2);
    assert_eq!(layout(&app).primary, Some(ASSETS));
    assert_eq!(bodies(&mut app).len(), 1);
    let no_panels = PluginGroupBuilder::start::<ShellPlugins>()
        .add(ShellPlugin::default())
        .add_group(InspectorPlugins);
    let mut app = shell_app_from((1440, 900), 1.0, no_panels);
    frames(&mut app, 2);
    let l = layout(&app);
    assert_eq!((l.primary, l.sidebar_open), (None, false));
    // Nor does it open when the window's real size arrives during
    // start-up, from ⌘B, from its collapsed bar or for an activity.
    let window = find::<Window>(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(1300.0, 900.0);
    frames(&mut app, 2);
    let l = layout(&app);
    assert!(l.startup && l.window_w == 1300.0 && !l.sidebar_open);
    for cmd in [
        UiCommand::ToggleSidebar,
        UiCommand::Show(crate::shell::Side::Sidebar),
        UiCommand::Activity {
            panel: PLOT,
            shift: false,
        },
    ] {
        app.world_mut().write_message(cmd.clone());
        frames(&mut app, 2);
        assert!(!layout(&app).sidebar_open, "{cmd:?}");
    }
    let sidebar = region(&mut app, crate::shell::regions::Region::Sidebar);
    assert!(!displayed(&app, sidebar));
}

/// The probe panel opened in `slot` (Scene is live in the primary).
fn probe_app(slot: Slot) -> App {
    let mut app = shell_app((1440, 900), 1.0);
    app.add_plugins(ProbePanelPlugin);
    frames(&mut app, 2);
    go_live(&mut app, false);
    app.world_mut()
        .write_message(UiCommand::OpenPanel { panel: PROBE, slot });
    frames(&mut app, 2);
    app
}

fn body_of(app: &mut App, panel: PanelId) -> Entity {
    bodies(app)
        .into_iter()
        .find(|b| b.1 == panel)
        .expect("a body")
        .0
}

fn slot_of(app: &App, e: Entity) -> Slot {
    let parent = app.world().get::<ChildOf>(e).unwrap().parent();
    app.world().get::<SlotView>(parent).unwrap().0
}

fn rows(app: &mut App) -> Vec<(u32, Entity)> {
    let world = app.world_mut();
    let mut rows: Vec<(u32, Entity)> = world
        .query::<(&ProbeRow, &ChildOf)>()
        .iter(world)
        .map(|(r, p)| (r.0, p.parent()))
        .collect();
    rows.sort();
    rows
}

/// Opening, swapping and closing the second slot keeps each body: the
/// first slot's isn't rebuilt when the second opens or closes, and a swap
/// moves both (the reviewer's XP1 and XP3 rebuilt them).
#[test]
fn bodies_survive_the_second_slot_opening_swapping_and_closing() {
    let mut app = shell_app((1440, 900), 1.0);
    app.add_plugins(ProbePanelPlugin);
    frames(&mut app, 2);
    go_live(&mut app, false);
    frames(&mut app, 1);
    let scene = body_of(&mut app, SCENE);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PROBE,
        slot: Slot::Secondary,
    });
    frames(&mut app, 2);
    assert_eq!(body_of(&mut app, SCENE), scene, "opening the second slot");
    let probe = body_of(&mut app, PROBE);
    assert_eq!(slot_of(&app, probe), Slot::Secondary);
    let content = rows(&mut app);
    app.world_mut().write_message(UiCommand::SwapSlots);
    frames(&mut app, 2);
    assert_eq!(body_of(&mut app, SCENE), scene, "a swap moves the body");
    assert_eq!(body_of(&mut app, PROBE), probe);
    assert_eq!(
        (slot_of(&app, scene), slot_of(&app, probe)),
        (Slot::Secondary, Slot::Primary)
    );
    assert_eq!(rows(&mut app), content, "and keeps its content");
    // Each slot is its header, then its body.
    for body in [scene, probe] {
        let slot = app.world().get::<ChildOf>(body).unwrap().parent();
        let kids: Vec<Entity> = app.world().get::<Children>(slot).unwrap().to_vec();
        assert_eq!(kids.len(), 2);
        assert!(app.world().get::<PanelHeader>(kids[0]).is_some());
        assert_eq!(kids[1], body);
    }
    app.world_mut().write_message(UiCommand::CloseSecondary);
    frames(&mut app, 2);
    assert_eq!(body_of(&mut app, PROBE), probe, "closing the second slot");
    assert_eq!(bodies(&mut app).len(), 1, "Scene's body went with its slot");
    // The header's count follows its panel after the swap.
    app.world_mut().resource_mut::<Probe>().count = Some("3 things".into());
    frames(&mut app, 1);
    assert!(all_text(&mut app).contains("3 things"));
}

/// A count or badge change rewrites the header or the activity button in
/// place; a change of the panel's own state doesn't touch its body; a
/// view change rebuilds that body alone, and its content is built again
/// from the panel's state, so nothing is lost (bodies are disposable
/// views). Content is full-bleed; the other states are padded.
#[test]
fn a_body_is_rebuilt_only_for_its_view_and_built_from_panel_state() {
    use crate::theme::SPACE;
    let mut app = probe_app(Slot::Secondary);
    let (scene, probe) = (body_of(&mut app, SCENE), body_of(&mut app, PROBE));
    assert_eq!(rows(&mut app), [(0, probe), (1, probe)]);
    let padding = |app: &App, e: Entity| app.world().get::<Node>(e).unwrap().padding;
    assert_eq!(padding(&app, probe), UiRect::ZERO, "content is full-bleed");
    let gap = |app: &App, e: Entity| app.world().get::<Node>(e).unwrap().row_gap;
    assert_eq!(gap(&app, probe), px(0), "and lays itself out");
    {
        let mut p = app.world_mut().resource_mut::<Probe>();
        p.count = Some("2 rows".into());
        p.rows = 3;
    }
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains("2 rows"));
    assert_eq!(
        body_of(&mut app, PROBE),
        probe,
        "a count change keeps the body"
    );
    assert_eq!(rows(&mut app).len(), 2, "the content isn't rebuilt either");
    app.world_mut().resource_mut::<Probe>().view = PanelView::Loading;
    frames(&mut app, 2);
    let loading = body_of(&mut app, PROBE);
    assert_ne!(loading, probe);
    assert!(app.world().get_entity(probe).is_err());
    assert_eq!(padding(&app, loading), UiRect::all(px(SPACE[4])));
    assert_eq!(gap(&app, loading), px(SPACE[1]));
    assert!(rows(&mut app).is_empty());
    assert_eq!(body_of(&mut app, SCENE), scene, "the other body stays");
    app.world_mut().resource_mut::<Probe>().view = PanelView::Ready;
    frames(&mut app, 2);
    let ready = body_of(&mut app, PROBE);
    assert_eq!(rows(&mut app), [(0, ready), (1, ready), (2, ready)]);
    // A message the panel computes (`Cow`), not only a literal.
    let n = 4;
    app.world_mut().resource_mut::<Probe>().view = PanelView::empty(format!("{n} hidden"));
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains("4 hidden"));
    let empty = body_of(&mut app, PROBE);
    assert_eq!(padding(&app, empty), UiRect::all(px(SPACE[4])));
}

/// Header actions and an empty or error state's actions trigger
/// `PanelActionActivated` for their panel; disabled ones don't, and say
/// they're not available yet. Reconnect is the shell's: it retries the
/// server from any panel.
#[test]
fn panel_actions_reach_their_panel_and_reconnect_retries() {
    use crate::panels::{ActionId, PanelAction};
    use crate::shell::sidebar::body::ActionButton;
    use bevy::ui::InteractionDisabled;
    use gz_bevy::connection::GzReconnect;

    let retry = ActionId::new("retry");
    let later = ActionId::new("later_too");
    let mut app = probe_app(Slot::Primary);
    app.world_mut().resource_mut::<Probe>().view = PanelView::error("down")
        .action(PanelAction::new(retry, "Retry"))
        .action(PanelAction::reconnect())
        .action(PanelAction::new(later, "Later").disabled());
    frames(&mut app, 2);
    let world = app.world_mut();
    let buttons: Vec<(Entity, ActionButton, bool)> = world
        .query::<(Entity, &ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .filter(|(_, b, _)| b.panel == PROBE)
        .map(|(e, b, d)| (e, *b, d))
        .collect();
    let mut ids: Vec<(ActionId, bool)> = buttons.iter().map(|b| (b.1.action, b.2)).collect();
    ids.sort();
    let mut want = [
        (POKE, false),
        (LATER, true),
        (retry, false),
        (crate::panels::RECONNECT, false),
        (later, true),
    ];
    want.sort();
    assert_eq!(ids, want);
    for (e, ..) in &buttons {
        app.world_mut().trigger(Activate { entity: *e });
    }
    frames(&mut app, 1);
    let mut seen = app.world().resource::<ProbeActions>().0.clone();
    seen.sort();
    let mut want: Vec<(ActionId, Entity)> = buttons
        .iter()
        .filter(|b| !b.2)
        .map(|b| (b.1.action, b.0))
        .collect();
    want.sort();
    // Each event names its button, to anchor a popover on.
    assert_eq!(seen, want);
    assert_eq!(seen.len(), 3);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
    let text = all_text(&mut app);
    assert!(text.contains("down") && text.contains("Retry"));
}

/// `panel_shown` is true while the panel is in a slot of the open
/// sidebar.
#[test]
fn panel_shown_follows_the_slots_and_the_sidebar() {
    use crate::shell::panel_shown;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = probe_app(Slot::Secondary);
    let shown =
        |app: &mut App, p: PanelId| app.world_mut().run_system_once(panel_shown(p)).unwrap();
    assert!(shown(&mut app, PROBE) && shown(&mut app, SCENE));
    assert!(!shown(&mut app, PLOT));
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 1);
    assert!(!shown(&mut app, PROBE));
}

/// The probe's header action button for `action`.
fn header_button(app: &mut App, action: crate::panels::ActionId) -> Entity {
    use crate::shell::sidebar::body::ActionButton;
    let world = app.world_mut();
    world
        .query::<(Entity, &ActionButton)>()
        .iter(world)
        .find(|(_, b)| b.panel == PROBE && b.action == action)
        .expect("the header action")
        .0
}

fn slot_actions(app: &mut App, header: Entity) -> Vec<crate::shell::sidebar::SlotAction> {
    let world = app.world_mut();
    world
        .query::<(&crate::shell::sidebar::SlotAction, &ChildOf)>()
        .iter(world)
        .filter(|(_, p)| p.parent() == header)
        .map(|(a, _)| *a)
        .collect()
}

/// A second slot opening or closing rebuilds the header's slot buttons
/// only: the header and a focused header action stay (a popover anchored
/// on it in M7 would too), and keyboard focus with it.
#[test]
fn a_focused_header_action_survives_the_second_slot() {
    use crate::shell::sidebar::SlotAction::*;
    let mut app = probe_app(Slot::Primary);
    let poke = header_button(&mut app, POKE);
    let header = app.world().get::<ChildOf>(poke).unwrap().parent();
    assert_eq!(slot_actions(&mut app, header), [Collapse]);
    tab_to(&mut app, poke);
    frames(&mut app, 1);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Secondary,
    });
    frames(&mut app, 2);
    assert_eq!(header_button(&mut app, POKE), poke);
    assert_eq!(app.world().get::<ChildOf>(poke).unwrap().parent(), header);
    assert_eq!(focused(&app), Some(poke));
    assert_eq!(slot_actions(&mut app, header), [Swap, Collapse]);
    // The slot buttons stay last, after the header actions.
    let kids = app.world().get::<Children>(header).unwrap().to_vec();
    let at = |e: Entity| kids.iter().position(|k| *k == e).unwrap();
    let swap = kids
        .iter()
        .copied()
        .find(|k| app.world().get::<crate::shell::sidebar::SlotAction>(*k) == Some(&Swap))
        .unwrap();
    assert!(at(poke) < at(swap));
    app.world_mut().write_message(UiCommand::CloseSecondary);
    frames(&mut app, 2);
    assert_eq!(header_button(&mut app, POKE), poke);
    assert_eq!(focused(&app), Some(poke));
    assert_eq!(slot_actions(&mut app, header), [Collapse]);
}

/// A body the slots don't host (M7's pop-out) isn't theirs to despawn.
#[test]
fn the_slots_leave_a_body_hosted_elsewhere() {
    let mut app = probe_app(Slot::Primary);
    let elsewhere = app
        .world_mut()
        .spawn((
            PanelBody(PLOT),
            crate::shell::sidebar::body::BodyView(PanelView::Ready),
            Node::default(),
        ))
        .id();
    let host = app
        .world_mut()
        .spawn(Node::default())
        .add_child(elsewhere)
        .id();
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: ASSETS,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    assert!(app.world().get_entity(elsewhere).is_ok());
    assert_eq!(
        app.world().get::<ChildOf>(elsewhere).unwrap().parent(),
        host
    );
}

/// A panel enables and disables its header actions at run time through
/// `PanelStatus`: the button changes in place (look, tooltip, whether it
/// reacts), and the status marks a change only on a real one.
#[test]
fn header_actions_are_enabled_at_run_time_in_place() {
    use crate::shell::sidebar::body::NOT_YET;
    use crate::shell::{PanelStatus, SetPanelStatus};
    use crate::widgets::Tooltip;
    use bevy::ui::InteractionDisabled;

    let mut app = probe_app(Slot::Primary);
    let later = header_button(&mut app, LATER);
    let state = |app: &App| {
        let w = app.world();
        (
            w.get::<InteractionDisabled>(later).is_none(),
            w.get::<Tooltip>(later).unwrap().detail.clone(),
        )
    };
    assert_eq!(state(&app), (false, Some(NOT_YET.into())));
    assert_eq!(
        app.world()
            .resource::<PanelStatus>()
            .action_enabled(PROBE, LATER),
        None,
        "as registered"
    );
    let set = |app: &mut App, enabled: bool| {
        app.world_mut()
            .resource_mut::<PanelStatus>()
            .set_action_enabled(PROBE, LATER, enabled);
        frames(app, 2);
    };
    set(&mut app, true);
    assert_eq!(header_button(&mut app, LATER), later, "not respawned");
    assert_eq!(state(&app), (true, None));
    app.world_mut().trigger(Activate { entity: later });
    frames(&mut app, 1);
    assert_eq!(app.world().resource::<ProbeActions>().0, [(LATER, later)]);
    // Only a real change marks the status changed.
    let tick = app.world().resource_ref::<PanelStatus>().last_changed();
    app.world_mut()
        .resource_mut::<PanelStatus>()
        .set_action_enabled(PROBE, LATER, true);
    assert_eq!(
        app.world().resource_ref::<PanelStatus>().last_changed(),
        tick
    );
    set(&mut app, false);
    assert_eq!(state(&app), (false, Some(NOT_YET.into())));
    app.world_mut().trigger(Activate { entity: later });
    frames(&mut app, 1);
    assert_eq!(app.world().resource::<ProbeActions>().0.len(), 1);
    // A header rebuilt later (the panel reopened) starts as the status says.
    app.world_mut()
        .resource_mut::<PanelStatus>()
        .set_action_enabled(PROBE, POKE, false);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PROBE,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    let poke = header_button(&mut app, POKE);
    assert!(app.world().get::<InteractionDisabled>(poke).is_some());
}

/// A header action's binding activates it, as its button would, while its
/// panel is shown and the action enabled.
#[test]
fn a_header_action_binding_activates_it_while_shown_and_enabled() {
    use crate::shell::{PanelStatus, SetPanelStatus};
    let mut app = probe_app(Slot::Secondary);
    let poke = header_button(&mut app, POKE);
    let press = |app: &mut App| {
        chord(app, &[KeyCode::ControlLeft, KeyCode::KeyI]);
        std::mem::take(&mut app.world_mut().resource_mut::<ProbeActions>().0)
    };
    assert_eq!(press(&mut app), [(POKE, poke)]);
    assert!(layout(&app).inspector_open, "⌘⌥I isn't ⌘I");
    let enable = |app: &mut App, on: bool| {
        app.world_mut()
            .resource_mut::<PanelStatus>()
            .set_action_enabled(PROBE, POKE, on);
        frames(app, 1);
    };
    enable(&mut app, false);
    assert_eq!(press(&mut app), []);
    enable(&mut app, true);
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert_eq!(press(&mut app), [], "the sidebar is collapsed");
    app.world_mut().write_message(UiCommand::CloseSecondary);
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert!(layout(&app).sidebar_open);
    assert_eq!(press(&mut app), [], "the panel isn't shown");
}
