//! Where each icon sits in the atlas. Shared by `build.rs`, which draws the
//! atlas, and the runtime, which cuts it into a `TextureAtlasLayout`, so the
//! two can't disagree.
//!
//! The atlas has one block of rows per size in [`SIZES`]; each block holds
//! every icon in `icons.json` order, [`COLUMNS`] to a row. Every cell has a
//! [`PAD`]-pixel transparent gutter on each side so linear filtering never
//! picks up a neighbour.

/// Icon sizes in logical pixels: the four icons.json `$note` lists (15,
/// 17, 18, 20) and the others the spec draws icons at (10 px tree carets,
/// 12 px status-bar glyphs, the 13 px warning chip icon, 14 px play/pause
/// and eye, 24 px empty states), each rasterized at its own size rather
/// than scaled from a neighbour (DECISIONS.md D-24).
pub(crate) const SIZES: [u32; 9] = [10, 12, 13, 14, 15, 17, 18, 20, 24];
/// Icons per atlas row.
pub(crate) const COLUMNS: u32 = 16;
/// Transparent gutter around each cell, in atlas pixels.
pub(crate) const PAD: u32 = 2;

/// Side of a `size`-px icon at `scale` (1 or 2), in atlas pixels.
pub(crate) const fn side(size: u32, scale: u32) -> u32 {
    size * scale
}

fn rows(count: u32) -> u32 {
    count.div_ceil(COLUMNS)
}

/// Top-left corner and side of icon `icon` (of `count`) at size index
/// `size_index` in the `scale` atlas.
pub(crate) fn cell(scale: u32, size_index: usize, icon: u32, count: u32) -> (u32, u32, u32) {
    let block_y: u32 = SIZES[..size_index]
        .iter()
        .map(|&s| rows(count) * (side(s, scale) + 2 * PAD))
        .sum();
    let side = side(SIZES[size_index], scale);
    let pitch = side + 2 * PAD;
    let x = (icon % COLUMNS) * pitch + PAD;
    let y = block_y + (icon / COLUMNS) * pitch + PAD;
    (x, y, side)
}

/// Width and height of the `scale` atlas holding `count` icons.
pub(crate) fn atlas_size(scale: u32, count: u32) -> (u32, u32) {
    let widest = side(SIZES[SIZES.len() - 1], scale) + 2 * PAD;
    let height = SIZES
        .iter()
        .map(|&s| rows(count) * (side(s, scale) + 2 * PAD))
        .sum();
    (COLUMNS.min(count.max(1)) * widest, height)
}
