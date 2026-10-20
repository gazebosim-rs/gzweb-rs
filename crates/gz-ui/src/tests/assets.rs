//! The icon atlas and the fonts.

use bevy::prelude::*;
use bevy::text::{FontSource, FontWeight, LetterSpacing};

use super::app::{frames, ui_app, ui_app_at};
use crate::brand::{logo, Logo, Logos, LOGO_DARK, LOGO_LIGHT};
use crate::fonts::{span, text, Type, UiFonts};
use crate::icons::{
    atlas_for_scale, atlas_layout, icon, layout, size_index, Icon, IconAtlas, IconId, ATLAS_1X,
    ATLAS_2X,
};
use crate::theme::{ColorRole, FontFamily, Theme, ThemeMode, TypeStyle};

fn decode(png: &[u8]) -> Image {
    crate::brand::decode_png(png)
}

#[test]
fn atlas_cells_are_disjoint_and_inside() {
    let n = IconId::ALL.len() as u32;
    for scale in [1, 2] {
        let (w, h) = layout::atlas_size(scale, n);
        let mut rects = Vec::new();
        for s in 0..layout::SIZES.len() {
            for i in 0..n {
                let (x, y, side) = layout::cell(scale, s, i, n);
                assert_eq!(side, layout::SIZES[s] * scale);
                assert!(x >= layout::PAD && y >= layout::PAD);
                assert!(x + side + layout::PAD <= w && y + side + layout::PAD <= h);
                rects.push(URect::new(x, y, x + side, y + side));
            }
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
        assert_eq!(atlas_layout(scale).textures.len(), rects.len());
        assert_eq!(atlas_layout(scale).size, UVec2::new(w, h));
    }
    assert_eq!(
        layout::atlas_size(1, 1).0,
        layout::side(24, 1) + 2 * layout::PAD
    );
}

#[test]
fn every_icon_is_drawn_white_at_every_size() {
    let n = IconId::ALL.len() as u32;
    for (scale, png) in [(1, ATLAS_1X), (2, ATLAS_2X)] {
        let image = decode(png);
        let (w, h) = layout::atlas_size(scale, n);
        assert_eq!(image.size(), UVec2::new(w, h));
        let data = image.data.as_ref().unwrap();
        let px = |x: u32, y: u32| {
            let o = ((y * w + x) * 4) as usize;
            [data[o], data[o + 1], data[o + 2], data[o + 3]]
        };
        for s in 0..layout::SIZES.len() {
            for id in IconId::ALL {
                let (x0, y0, side) = layout::cell(scale, s, id.index() as u32, n);
                let mut ink = 0;
                // Every texel is white, transparent ones too (so edges don't
                // darken under filtering).
                for y in y0 - layout::PAD..y0 + side + layout::PAD {
                    for x in x0 - layout::PAD..x0 + side + layout::PAD {
                        let p = px(x, y);
                        assert_eq!(&p[..3], &[255, 255, 255], "{} is white", id.name());
                        if p[3] > 0 {
                            ink += 1;
                        }
                    }
                }
                assert!(
                    ink > 4,
                    "{} at {}px@{scale}x has no ink",
                    id.name(),
                    layout::SIZES[s]
                );
                // Nothing in the gutter.
                assert_eq!(px(x0 - 1, y0 + side / 2)[3], 0, "{} gutter", id.name());
            }
        }
    }
}

#[test]
fn sizes_and_scale_pick_the_cell() {
    // Each size the spec uses has its own cell.
    for (i, &s) in layout::SIZES.iter().enumerate() {
        assert_eq!(size_index(s as f32), i);
    }
    assert_eq!(layout::SIZES, [10, 12, 13, 14, 15, 17, 18, 20, 24]);
    // Between sizes: the next one up; past the largest: the largest.
    assert_eq!(size_index(16.0), 5);
    assert_eq!(size_index(32.0), 8);
    assert_eq!(atlas_for_scale(1.0), 0);
    assert_eq!(atlas_for_scale(1.5), 1);
    assert_eq!(atlas_for_scale(2.0), 1);
}

fn icon_image(scale: f32) -> (ImageNode, IconAtlas) {
    let mut app = ui_app_at(ThemeMode::Light, scale);
    let e = app
        .world_mut()
        .spawn(icon(IconId::Rotate, 17.0, ColorRole::Icon))
        .id();
    frames(&mut app, 3);
    (
        app.world().get::<ImageNode>(e).unwrap().clone(),
        app.world().resource::<IconAtlas>().clone(),
    )
}

#[test]
fn icons_use_the_2x_atlas_on_hidpi() {
    let n = IconId::ALL.len();
    let (one, atlas) = icon_image(1.0);
    assert_eq!(one.image, atlas.images[0]);
    let a = one.texture_atlas.unwrap();
    assert_eq!(a.layout, atlas.layouts[0]);
    assert_eq!(a.index, size_index(17.0) * n + IconId::Rotate.index());
    assert_eq!(one.color, Theme::default().c.icon);
    let (two, atlas) = icon_image(2.0);
    assert_eq!(two.image, atlas.images[1]);
    assert_eq!(two.texture_atlas.unwrap().layout, atlas.layouts[1]);
}

#[test]
fn a_changed_icon_changes_cell_and_size() {
    let n = IconId::ALL.len();
    let mut app = ui_app(ThemeMode::Light);
    let e = app
        .world_mut()
        .spawn(icon(IconId::Rotate, 17.0, ColorRole::Icon))
        .id();
    frames(&mut app, 2);
    app.world_mut().get_mut::<Icon>(e).unwrap().px = 13.0;
    frames(&mut app, 2);
    let w = app.world();
    let index = w
        .get::<ImageNode>(e)
        .unwrap()
        .texture_atlas
        .as_ref()
        .unwrap()
        .index;
    assert_eq!(index, size_index(13.0) * n + IconId::Rotate.index());
    assert_eq!(w.get::<Node>(e).unwrap().width, Val::Px(13.0));
    assert_eq!(w.get::<Node>(e).unwrap().height, Val::Px(13.0));
}

#[test]
fn fonts_map_weights_to_the_listed_files() {
    let mut app = ui_app(ThemeMode::Light);
    frames(&mut app, 1);
    let fonts = app.world().resource::<UiFonts>().clone();
    assert_eq!(
        fonts.face(FontFamily::Ui, 400),
        (fonts.ui_regular.clone(), 400)
    );
    assert_eq!(
        fonts.face(FontFamily::Ui, 500),
        (fonts.ui_medium.clone(), 500)
    );
    assert_eq!(
        fonts.face(FontFamily::Ui, 600),
        (fonts.ui_semibold.clone(), 600)
    );
    assert_eq!(
        fonts.face(FontFamily::Mono, 400),
        (fonts.mono_regular.clone(), 400)
    );
    assert_eq!(
        fonts.face(FontFamily::Mono, 500),
        (fonts.mono_medium.clone(), 500)
    );
    // Geist Mono has no 600 (micro badges): Medium.
    assert_eq!(
        fonts.face(FontFamily::Mono, 600),
        (fonts.mono_medium.clone(), 500)
    );
    // Ours, plus Bevy's default font.
    assert!(app.world().resource::<Assets<Font>>().len() >= 5);

    let caps = app
        .world_mut()
        .spawn(text(TypeStyle::CAPS, ColorRole::Text3, "scene"))
        .id();
    let parent = app
        .world_mut()
        .spawn(text(TypeStyle::BODY, ColorRole::Text, "a"))
        .id();
    let child = app
        .world_mut()
        .spawn((
            span(TypeStyle::LABEL, ColorRole::Text2, "b"),
            ChildOf(parent),
        ))
        .id();
    frames(&mut app, 1);
    let w = app.world();
    assert_eq!(w.get::<Text>(caps).unwrap().0, "SCENE");
    let font = w.get::<TextFont>(caps).unwrap();
    assert_eq!(font.font, FontSource::Handle(fonts.mono_medium.clone()));
    assert_eq!(font.weight, FontWeight(500));
    assert_eq!(
        *w.get::<LetterSpacing>(caps).unwrap(),
        LetterSpacing::Px(TypeStyle::CAPS.tracking_px())
    );
    assert_eq!(w.get::<Type>(child).unwrap().0, TypeStyle::LABEL);
    assert_eq!(
        w.get::<TextFont>(child).unwrap().font,
        FontSource::Handle(fonts.ui_medium)
    );
}

#[test]
fn the_logo_follows_the_theme() {
    assert!(!LOGO_LIGHT.is_empty() && !LOGO_DARK.is_empty());
    let mut app = ui_app(ThemeMode::Light);
    let e = app.world_mut().spawn(logo(28.0)).id();
    frames(&mut app, 1);
    let logos = app.world().resource::<Logos>().clone();
    let image = |app: &App| app.world().get::<ImageNode>(e).unwrap().image.clone();
    assert_eq!(image(&app), logos.light);
    app.world_mut().resource_mut::<Theme>().toggle();
    frames(&mut app, 1);
    assert_eq!(image(&app), logos.dark);
    assert!(app.world().get::<Logo>(e).is_some());
    assert_eq!(decode(LOGO_LIGHT).size(), UVec2::splat(128));
}
