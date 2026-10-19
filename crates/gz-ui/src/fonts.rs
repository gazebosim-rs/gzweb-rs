//! Geist and Geist Mono (OFL-1.1, `assets/fonts/OFL.txt`), embedded so the
//! browser build needs no asset server path (spec §9.6). Only the files
//! `tokens.json` lists: Geist 400/500/600 and Geist Mono 400/500.
//!
//! Text entities carry a [`Type`] (a `tokens.json` type style) and an
//! [`Ink`] role; [`apply_type_system`] turns the style
//! into a `TextFont` and letter spacing.

use bevy::prelude::*;
use bevy::text::{FontHinting, FontWeight, LetterSpacing};

use crate::theme::{ColorRole, FontFamily, Ink, TypeStyle};

/// The embedded files, as `tokens.json` `font` lists them.
pub const FONT_FILES: [(&str, &[u8]); 5] = [
    (
        "Geist-Regular.ttf",
        include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    ),
    (
        "Geist-Medium.ttf",
        include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    ),
    (
        "Geist-SemiBold.ttf",
        include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    ),
    (
        "GeistMono-Regular.ttf",
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    ),
    (
        "GeistMono-Medium.ttf",
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    ),
];

/// Handles to the embedded fonts.
#[derive(Resource, Clone, Debug)]
pub struct UiFonts {
    pub ui_regular: Handle<Font>,
    pub ui_medium: Handle<Font>,
    pub ui_semibold: Handle<Font>,
    pub mono_regular: Handle<Font>,
    pub mono_medium: Handle<Font>,
}

impl FromWorld for UiFonts {
    fn from_world(world: &mut World) -> Self {
        let mut fonts = world.resource_mut::<Assets<Font>>();
        let mut add = |i: usize| fonts.add(Font::from_bytes(FONT_FILES[i].1.to_vec()));
        Self {
            ui_regular: add(0),
            ui_medium: add(1),
            ui_semibold: add(2),
            mono_regular: add(3),
            mono_medium: add(4),
        }
    }
}

impl UiFonts {
    /// The file for `family` at `weight`, and the weight it really has.
    /// Geist Mono has no 600: the `micro` badge style uses Medium
    /// (DECISIONS.md D-25).
    pub fn face(&self, family: FontFamily, weight: u16) -> (Handle<Font>, u16) {
        match (family, weight) {
            (FontFamily::Ui, 0..=449) => (self.ui_regular.clone(), 400),
            (FontFamily::Ui, 450..=549) => (self.ui_medium.clone(), 500),
            (FontFamily::Ui, _) => (self.ui_semibold.clone(), 600),
            (FontFamily::Mono, 0..=449) => (self.mono_regular.clone(), 400),
            (FontFamily::Mono, _) => (self.mono_medium.clone(), 500),
        }
    }

    /// The `TextFont` for a style.
    pub fn text_font(&self, style: &TypeStyle) -> TextFont {
        let (handle, weight) = self.face(style.font, style.weight);
        TextFont::from_font_size(style.size)
            .with_font(handle)
            .with_font_weight(FontWeight(weight))
    }
}

/// A text entity's type style. [`apply_type_system`] sets its `TextFont`
/// and `LetterSpacing`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(LetterSpacing, FontHinting::Enabled)]
pub struct Type(pub TypeStyle);

/// Resolves [`Type`] into fonts when it is added or changed.
pub fn apply_type_system(
    fonts: Res<UiFonts>,
    mut q: Query<(&Type, &mut TextFont, &mut LetterSpacing), Changed<Type>>,
) {
    for (ty, mut font, mut spacing) in &mut q {
        *font = fonts.text_font(&ty.0);
        *spacing = LetterSpacing::Px(ty.0.tracking_px());
    }
}

/// A text node in `style` and `role`. Uppercase styles are capitalized here.
pub fn text(style: TypeStyle, role: ColorRole, value: impl Into<String>) -> impl Bundle {
    let value = value.into();
    let value = if style.uppercase {
        value.to_uppercase()
    } else {
        value
    };
    (
        Text::new(value),
        TextFont::from_font_size(style.size),
        Type(style),
        Ink(role),
    )
}

/// A text span (a child of a [`text`] node) in `style` and `role`.
pub fn span(style: TypeStyle, role: ColorRole, value: impl Into<String>) -> impl Bundle {
    (
        TextSpan::new(value),
        TextFont::from_font_size(style.size),
        Type(style),
        Ink(role),
    )
}
