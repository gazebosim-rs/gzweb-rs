//! The HUD pills: the scale bar's maths and text, and the scale label in
//! an orthographic view (the target chip's tests are the rail's, beside
//! the Camera mode button it belongs to).

use gz_bevy::view::{ortho_height, SnapView, ViewOp};

use super::*;
use crate::shell::viewport::view::ViewNode;
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};

#[test]
fn the_scale_bar_is_a_one_two_five_length_that_fits() {
    for (m_per_px, metres) in [
        (0.01, 0.5),
        (0.0125, 1.0),
        (0.02, 1.0),
        (0.03, 2.0),
        (0.07, 5.0),
        (0.1, 5.0),
        (1.0, 50.0),
        (1e-5, 5e-4),
    ] {
        let (m, px) = scale_bar(m_per_px).unwrap();
        assert!((m - metres).abs() < metres * 1e-4, "{m_per_px}: {m}");
        assert!((MIN_BAR..=MAX_BAR + 1e-3).contains(&px), "{m_per_px}: {px}");
    }
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(scale_bar(bad), None);
    }
}

#[test]
fn lengths_read_in_the_unit_that_suits_them() {
    for (m, s) in [
        (1.0, "1 m"),
        (50.0, "50 m"),
        (0.5, "50 cm"),
        (0.02, "2 cm"),
        (0.005, "5 mm"),
        (2000.0, "2 km"),
        (0.0005, "0.5 mm"),
    ] {
        assert_eq!(scale_text(m), s);
    }
    // The bar's own lengths under a millimetre, as the closest orthographic
    // zoom in a tall view gives them, without f32's noise.
    for (m_per_px, s) in [(1e-6, "0.05 mm"), (2.2e-6, "0.1 mm"), (4.6e-6, "0.2 mm")] {
        let (m, _) = scale_bar(m_per_px).unwrap();
        assert_eq!(scale_text(m), s, "{m}");
    }
}

#[test]
fn the_scale_label_shows_in_orthographic_views_only() {
    let mut app = live_view(900);
    let label = find::<ScaleLabel>(&mut app);
    assert!(!displayed(&app, label));
    send(&mut app, ViewOp::Snap(SnapView::Top));
    // Land the animation, orthographic.
    send(&mut app, ViewOp::Pan(Vec2::ZERO));
    frames(&mut app, 2);
    assert!(displayed(&app, label));
    // Bottom right, inset 10.
    let (v, vs) = {
        let e = find::<ViewNode>(&mut app);
        rect(&app, e)
    };
    let (l, ls) = rect(&app, label);
    assert_eq!(v + vs - (l + ls), Vec2::splat(10.0));
    // The bar is as long as its text says, at the view's scale.
    let world = app.world_mut();
    let h = world
        .query_filtered::<&Projection, With<gz_bevy::camera::MainCamera>>()
        .single(world)
        .map(|p| ortho_height(p).unwrap())
        .unwrap();
    let (metres, px) = scale_bar(h / vs.y).unwrap();
    let bar = find::<ScaleBar>(&mut app);
    assert_eq!(rect(&app, bar).1.x, px.round());
    let text = descendants(app.world(), label)
        .into_iter()
        .find_map(|c| app.world().get::<Text>(c).map(|t| t.0.clone()))
        .unwrap();
    assert_eq!(text, scale_text(metres));
    // Back to perspective: gone.
    send(&mut app, ViewOp::ToggleProjection);
    assert!(!displayed(&app, label));
}
