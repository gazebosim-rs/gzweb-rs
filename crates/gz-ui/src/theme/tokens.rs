//! The token tables, generated from `design/ui/tokens.json` by `build.rs`
//! (DECISIONS.md D-21): `Palette` (light and dark), [`ColorRole`],
//! [`Sizes`], [`SPACE`], [`radius`], [`border`], [`motion`], [`elevation`]
//! and the [`TypeStyle`] constants, with the types they are made of.
//!
//! This is the one file the colour-literal check exempts, for the palette
//! it includes. The values are all generated; the types they are made of
//! ([`FontFamily`], [`TypeStyle`], [`Elevation`]) and their methods are
//! written here.

use bevy::prelude::*;

use super::roles::Shadow;
// The generated docs link to the role components.
#[cfg(doc)]
use super::roles::{Fill, Ink, Stroke};

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

/// Which of the two embedded families a [`TypeStyle`] uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum FontFamily {
    /// Geist.
    Ui,
    /// Geist Mono.
    Mono,
}

/// A `tokens.json` `type` entry. The constants (`TypeStyle::LABEL`, …) are
/// generated; widgets that need an off-scale style (bordered buttons use
/// Geist 12/500) build one with [`TypeStyle::new`].
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct TypeStyle {
    pub font: FontFamily,
    /// Logical pixels.
    pub size: f32,
    /// 400, 500 or 600.
    pub weight: u16,
    /// Render the text in capitals.
    pub uppercase: bool,
    /// Letter spacing, in ems.
    pub tracking: f32,
}

impl TypeStyle {
    /// A plain style: no capitals, no tracking.
    pub const fn new(font: FontFamily, size: f32, weight: u16) -> Self {
        Self {
            font,
            size,
            weight,
            uppercase: false,
            tracking: 0.0,
        }
    }

    /// Letter spacing in logical pixels.
    pub fn tracking_px(&self) -> f32 {
        self.tracking * self.size
    }
}

/// A `tokens.json` `elevation` entry: a drop shadow `y` below, blurred by
/// `blur`, in the `color` token (`None`: no shadow).
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Elevation {
    pub blur: f32,
    pub y: f32,
    pub color: Option<ColorRole>,
}

impl Elevation {
    /// The Bevy shadow for this elevation (colour set by [`Shadow`]).
    pub fn box_shadow(self) -> BoxShadow {
        BoxShadow::new(Color::NONE, px(0), px(self.y), px(0), px(self.blur))
    }

    /// The shadow and its colour role.
    pub fn shadow(self) -> (BoxShadow, Shadow) {
        (
            self.box_shadow(),
            Shadow(self.color.unwrap_or(ColorRole::Clear)),
        )
    }
}
