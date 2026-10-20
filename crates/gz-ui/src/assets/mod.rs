//! What the UI embeds (spec §4.4, §4.5): the Geist fonts and their type
//! styles, the icon atlas generated from `icons.json`, and the Gazebo logo.

pub(crate) mod brand;
pub(crate) mod fonts;
pub(crate) mod icons;

pub use fonts::{span, text, Type};
pub use icons::{content_icon, icon, Icon, IconId};

#[cfg(test)]
mod tests;
