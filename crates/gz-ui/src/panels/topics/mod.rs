//! The Topics panel (spec §3.3; M7): the server's topics, to echo, plot
//! and publish. Until then its body follows the connection.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Topics panel.
pub struct TopicsPanelPlugin;

impl Plugin for TopicsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Topics, "Topics", IconId::ActivityTopics, 20)
                .binding("panel.topics")
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
