//! The Topics panel (spec §3.3; M7): the server's topics, to echo, plot
//! and publish. Until then its body follows the connection.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;

/// The Topics panel's id.
pub const TOPICS: PanelId = PanelId::new("topics");

/// Registers the Topics panel.
pub struct TopicsPanelPlugin;

impl Plugin for TopicsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(TOPICS, "Topics", IconId::ActivityTopics)
                .place(DATA_GROUP, 20)
                .binding("panel.topics")
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
