//! The Topics panel (spec §3.3, §3.3.1; DECISIONS.md D-99): the server's
//! topics, filtered by name and kind, each with its rate, and one echoed
//! as text.
//!
//! The list is [`GzTopics`](gz_bevy::topics::GzTopics), asked for again
//! every two seconds while the panel shows. Rates come from
//! [`GzTraffic`](gz_bevy::topics::GzTraffic), so only the topics the
//! client receives have one; the rest show "—". Clicking a row (or ↑ and
//! ↓ in the focused list) echoes its topic: the panel holds it in
//! [`GzSubscriptions`](gz_bevy::topics::GzSubscriptions) only while it
//! shows, and draws its newest message in protobuf text format at most
//! ten times a second. A topic that leaves the server's list says "Topic
//! ended" and keeps its last message.
//!
//! The footer copies the message, and Plot field opens the Plot panel's
//! picker on the echoed topic (D-100). Publish and Services are unavailable: the Gazebo websocket
//! server takes no publications and lists no services.
//!
//! The panel contract (DECISIONS.md D-58): what the user set is
//! [`model::TopicsState`], the echo [`model::Echo`]; the body is built
//! from them ([`view::build_topics`]) and can be respawned at any time.

mod model;
mod systems;
mod view;

/// A message cut to show (the Sensors panel's text preview too).
pub(crate) use model::clip_lines;

/// A [`TopicsStart`]'s system, for a panel that opens beside it.
pub(crate) use systems::apply_start as apply_topics_start;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, Phase, ShellSystems};

/// The Topics panel's id.
pub const TOPICS: PanelId = PanelId::new("topics");
/// Its header's Publish….
pub(crate) const PUBLISH: ActionId = ActionId::new("publish");
/// Its header's Services.
pub(crate) const SERVICES: ActionId = ActionId::new("services");
/// Its footer's Plot field: the Plot panel's picker on the echoed topic.
pub(crate) const PLOT_FIELD: ActionId = ActionId::new("plot_field");
/// Its footer's Copy: the echoed message.
pub(crate) const COPY: ActionId = ActionId::new("copy");
/// Its footer's Publish….
pub(crate) const PUBLISH_FOOTER: ActionId = ActionId::new("publish_footer");

/// Why Plot field is unavailable.
pub(crate) const NO_PLOT: &str = "Echo a topic to plot one of its fields";
/// Why Publish is unavailable.
pub(crate) const NO_PUBLISH: &str = "The Gazebo websocket server doesn't take publications";
/// Why Services is unavailable.
pub(crate) const NO_SERVICES: &str = "The Gazebo websocket server doesn't list services";

/// Why Copy is unavailable in a browser without the clipboard.
pub(crate) const NO_CLIPBOARD: &str = "Copy needs the page on https or localhost";

/// Copy: enabled once there is a message, where the clipboard can be
/// written.
fn copy_action() -> PanelAction {
    let copy = PanelAction::new(COPY, "Copy");
    if gz_bevy::clipboard::clipboard_available() {
        copy.disabled()
    } else {
        copy.unavailable(NO_CLIPBOARD)
    }
}

/// How the panel starts, for a capture (`gzweb --echo TOPIC`, D-99): the
/// panel opens in the primary slot and echoes `echo` once the server
/// lists it; then the resource is removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct TopicsStart {
    pub echo: String,
}

/// The Topics panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TopicsBody;

/// Registers the Topics panel: its view, count, list and echo.
pub struct TopicsPanelPlugin;

impl Plugin for TopicsPanelPlugin {
    fn build(&self, app: &mut App) {
        gz_bevy::topics::TopicsPlugin::ensure(app);
        app.register_panel(
            PanelDef::new(TOPICS, "Topics", IconId::ActivityTopics)
                .place(DATA_GROUP, 20)
                .binding("panel.topics")
                .view(systems::view(Phase::Idle, 0))
                .header_action(
                    HeaderAction::new(PUBLISH, IconId::Publish, "Publish…").unavailable(NO_PUBLISH),
                )
                .header_action(
                    HeaderAction::new(SERVICES, IconId::Services, "Services")
                        .unavailable(NO_SERVICES),
                )
                .content::<TopicsBody>()
                .footer_action(PanelAction::new(PLOT_FIELD, "Plot field").unavailable(NO_PLOT))
                .footer_action(copy_action())
                .footer_action(
                    PanelAction::new(PUBLISH_FOOTER, "Publish…").unavailable(NO_PUBLISH),
                ),
        )
        .add_message::<crate::panels::PlotPick>()
        .init_resource::<model::TopicsState>()
        .init_resource::<model::Echo>()
        .init_resource::<systems::TopicsScroll>()
        .init_resource::<systems::BuiltRows>()
        .add_systems(
            Update,
            (
                (
                    systems::apply_start,
                    systems::read_echo,
                    systems::topics_status,
                    systems::hold_echo.before(gz_bevy::topics::sync_subscriptions),
                )
                    .chain(),
                (
                    systems::refresh_topics,
                    systems::filter_text,
                    systems::keep_scroll,
                    systems::sync_rows,
                    systems::row_states,
                    systems::chip_states,
                    systems::echo_section,
                )
                    .chain()
                    .after(systems::read_echo)
                    .run_if(panel_shown(TOPICS)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_topics)
        .add_observer(systems::on_row)
        .add_observer(systems::on_chip)
        .add_observer(systems::on_echo_close)
        .add_observer(systems::on_list_key)
        .add_observer(systems::on_action);
    }
}

#[cfg(test)]
mod tests;
