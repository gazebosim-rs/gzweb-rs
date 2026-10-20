//! What the UI embeds (spec §4.4, §4.5): the Geist fonts and their type
//! styles, the icon atlas generated from `icons.json`, and the Gazebo logo.

pub(crate) mod brand;
pub mod fonts;
pub mod icons;

#[cfg(test)]
mod tests;
