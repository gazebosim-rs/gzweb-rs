//! The Gazebo logo (`design/ui/assets/ui/brand/`), embedded: light strokes
//! in dark mode (spec §4.2).

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

use crate::theme::Theme;

/// `gazebo-icon.png` (light theme).
pub(crate) const LOGO_LIGHT: &[u8] =
    include_bytes!("../../../../design/ui/assets/ui/brand/gazebo-icon.png");
/// `gazebo-icon-dark.png` (dark theme).
pub(crate) const LOGO_DARK: &[u8] =
    include_bytes!("../../../../design/ui/assets/ui/brand/gazebo-icon-dark.png");

/// The two logo images.
#[derive(Resource, Clone, Debug)]
pub(crate) struct Logos {
    pub light: Handle<Image>,
    pub dark: Handle<Image>,
}

impl Logos {
    pub(crate) fn for_theme(&self, theme: &Theme) -> Handle<Image> {
        if theme.is_dark {
            self.dark.clone()
        } else {
            self.light.clone()
        }
    }
}

pub(crate) fn decode_png(png: &[u8]) -> Image {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded PNGs decode")
}

impl FromWorld for Logos {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        Self {
            light: images.add(decode_png(LOGO_LIGHT)),
            dark: images.add(decode_png(LOGO_DARK)),
        }
    }
}

/// Marks a logo image; `logo_system` swaps it with the theme.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(ImageNode)]
pub(crate) struct Logo;

/// The logo, `size` px square.
pub(crate) fn logo(size: f32) -> impl Bundle {
    (
        Logo,
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            ..default()
        },
    )
}

/// Points logos at the image for the current theme.
pub(crate) fn logo_system(
    theme: Res<Theme>,
    logos: Res<Logos>,
    mut images: Query<&mut ImageNode, With<Logo>>,
) {
    let want = logos.for_theme(&theme);
    for mut image in &mut images {
        if image.image != want {
            image.image = want.clone();
        }
    }
}
