//! The icon set (`design/ui/icons.json`), rasterized by `build.rs` into one
//! atlas at 1× and one at 2× (every size in [`layout::SIZES`]), embedded
//! with `include_bytes!`. Glyphs are white on transparent; an icon's [`Ink`]
//! role tints it at runtime. Icons are `ImageNode`s with a `TextureAtlas`;
//! the 2× atlas is used when the UI renders at more than one physical
//! pixel per logical pixel.

pub mod layout;

use bevy::prelude::*;
use bevy::ui::ComputedUiRenderTargetInfo;

use crate::brand::decode_png;
use crate::theme::{ColorRole, Ink};
use crate::visuals::Content;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// The 1× atlas PNG.
pub const ATLAS_1X: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icons@1x.png"));
/// The 2× atlas PNG.
pub const ATLAS_2X: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icons@2x.png"));

impl IconId {
    /// Position in `icons.json` (and the atlas).
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The atlas images and layouts, 1× and 2×.
#[derive(Resource, Clone, Debug)]
pub struct IconAtlas {
    pub images: [Handle<Image>; 2],
    pub layouts: [Handle<TextureAtlasLayout>; 2],
}

/// The atlas layout for `scale` (1 or 2): `size_index * N + icon`.
pub fn atlas_layout(scale: u32) -> TextureAtlasLayout {
    let count = IconId::ALL.len() as u32;
    let (w, h) = layout::atlas_size(scale, count);
    let mut atlas = TextureAtlasLayout::new_empty(UVec2::new(w, h));
    for size_index in 0..layout::SIZES.len() {
        for icon in 0..count {
            let (x, y, side) = layout::cell(scale, size_index, icon, count);
            atlas.add_texture(URect::new(x, y, x + side, y + side));
        }
    }
    atlas
}

impl FromWorld for IconAtlas {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        let images = [
            images.add(decode_png(ATLAS_1X)),
            images.add(decode_png(ATLAS_2X)),
        ];
        let mut layouts = world.resource_mut::<Assets<TextureAtlasLayout>>();
        let layouts = [layouts.add(atlas_layout(1)), layouts.add(atlas_layout(2))];
        Self { images, layouts }
    }
}

/// An icon `px` logical pixels square. `update_icons` picks the atlas
/// cell: the smallest rasterized size ≥ `px` (else the largest), at 1× or
/// 2× for the node's scale factor.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(ImageNode)]
pub struct Icon {
    pub id: IconId,
    pub px: f32,
}

/// The rasterized size index used for an icon drawn at `px`.
pub fn size_index(px: f32) -> usize {
    layout::SIZES
        .iter()
        .position(|&s| s as f32 >= px - 0.01)
        .unwrap_or(layout::SIZES.len() - 1)
}

/// Which atlas (0 = 1×, 1 = 2×) suits a physical-per-logical `scale`.
pub fn atlas_for_scale(scale: f32) -> usize {
    usize::from(scale > 1.01)
}

/// Points each [`Icon`]'s `ImageNode` at its atlas cell, and sizes its node
/// to `px`. Runs for icons that are new or changed, or whose render
/// target's scale factor changed.
#[allow(clippy::type_complexity)]
pub(crate) fn update_icons(
    atlas: Res<IconAtlas>,
    mut q: Query<
        (
            Ref<Icon>,
            Option<&ComputedUiRenderTargetInfo>,
            &mut ImageNode,
            Option<&mut Node>,
        ),
        Or<(Changed<Icon>, Changed<ComputedUiRenderTargetInfo>)>,
    >,
) {
    let count = IconId::ALL.len();
    for (icon, target, mut image, node) in &mut q {
        let scale = target.map_or(1.0, ComputedUiRenderTargetInfo::scale_factor);
        let which = atlas_for_scale(scale);
        let index = size_index(icon.px) * count + icon.id.index();
        let up_to_date = image.image == atlas.images[which]
            && image
                .texture_atlas
                .as_ref()
                .is_some_and(|a| a.index == index && a.layout == atlas.layouts[which]);
        if !up_to_date {
            image.image = atlas.images[which].clone();
            image.texture_atlas = Some(TextureAtlas {
                layout: atlas.layouts[which].clone(),
                index,
            });
        }
        if let (true, Some(mut node)) = (icon.is_changed(), node) {
            let side = Val::Px(icon.px);
            if node.width != side || node.height != side {
                node.width = side;
                node.height = side;
            }
        }
    }
}

/// An icon node: `id` at `px`, tinted by `role`.
pub fn icon(id: IconId, px: f32, role: ColorRole) -> impl Bundle {
    (
        Icon { id, px },
        Ink(role),
        Node {
            width: Val::Px(px),
            height: Val::Px(px),
            flex_shrink: 0.0,
            ..default()
        },
    )
}

/// An icon whose colour follows its widget's state ([`Content`]).
pub fn content_icon(id: IconId, px: f32, role: ColorRole) -> impl Bundle {
    (icon(id, px, role), Content)
}
