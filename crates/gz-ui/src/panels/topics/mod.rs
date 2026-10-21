//! The Topics panel (spec §3.3; M7): the server's topics, to echo, plot
//! and publish. Until then its body follows the connection ("No topics
//! yet" once live: the client doesn't keep the server's topic list yet),
//! and its header offers Publish and Services, disabled until M7.

mod systems;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{Phase, ShellSystems};

/// The Topics panel's id.
pub const TOPICS: PanelId = PanelId::new("topics");
/// Its header's Publish… (M7).
pub(crate) const PUBLISH: ActionId = ActionId::new("publish");
/// Its header's Services (M7).
pub(crate) const SERVICES: ActionId = ActionId::new("services");

/// Registers the Topics panel and its view.
pub struct TopicsPanelPlugin;

impl Plugin for TopicsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(TOPICS, "Topics", IconId::ActivityTopics)
                .place(DATA_GROUP, 20)
                .binding("panel.topics")
                .view(systems::view(Phase::Idle))
                .header_action(HeaderAction::new(PUBLISH, IconId::Publish, "Publish…").disabled())
                .header_action(
                    HeaderAction::new(SERVICES, IconId::Services, "Services").disabled(),
                ),
        )
        .add_systems(Update, systems::topics_view.in_set(ShellSystems::Panels));
    }
}

#[cfg(test)]
mod tests;
