use super::*;

/// A camera 10 m back on Bevy +Z looking at the origin, 800×600.
fn view() -> (Transform, Projection, Vec2) {
    (
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Projection::Perspective(PerspectiveProjection::default()),
        Vec2::new(800.0, 600.0),
    )
}

fn drag(handle: Handle) -> GizmoDrag {
    GizmoDrag {
        handle,
        view: None,
        press: Vec2::ZERO,
        center: Vec3::ZERO,
        axes: [Vec3::X, Vec3::Y, Vec3::Z],
        targets: Vec::new(),
        sent: Vec::new(),
        sent_at: 0.0,
    }
}

fn ray(t: &Transform, p: &Projection, size: Vec2, at: Vec2) -> Ray3d {
    viewport_ray(p, t, size, at).unwrap()
}

#[test]
fn the_handle_under_the_pointer_is_picked() {
    let (t, p, size) = view();
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let len = GIZMO_PX * metres_per_px(&t, &p, size, Vec3::ZERO);
    let tip_x = screen(&t, &p, size, Vec3::X * len * 0.9).unwrap();
    let pick_at = |tool, at| pick(tool, at, (&t, &p, size), Vec3::ZERO, &axes);
    assert_eq!(
        pick_at(TransformTool::Translate, tip_x),
        Some(Handle::Axis(0))
    );
    // Inside the XY square (normal Z), off the arrows.
    let xy = screen(&t, &p, size, (Vec3::X + Vec3::Y) * len * 0.35).unwrap();
    assert_eq!(
        pick_at(TransformTool::Translate, xy),
        Some(Handle::Plane(2))
    );
    // The ring about Z is a circle facing the camera.
    let on_ring = screen(&t, &p, size, Vec3::Y * len * RING).unwrap();
    assert_eq!(
        pick_at(TransformTool::Rotate, on_ring),
        Some(Handle::Ring(2))
    );
    // Far from everything; no tool.
    assert_eq!(pick_at(TransformTool::Translate, Vec2::new(5.0, 5.0)), None);
    assert_eq!(pick_at(TransformTool::None, tip_x), None);
}

#[test]
fn an_axis_drag_follows_the_pointer_along_the_axis_and_snaps() {
    let (t, p, size) = view();
    let press = screen(&t, &p, size, Vec3::ZERO).unwrap();
    let now = screen(&t, &p, size, Vec3::new(0.6, 0.3, 0.0)).unwrap();
    let (a, b) = (ray(&t, &p, size, press), ray(&t, &p, size, now));
    let d = drag(Handle::Axis(0));
    let (m, _) = drag_move(&d, a, b, Quat::IDENTITY, None).unwrap();
    let Move::Translate(v) = m else { panic!() };
    assert!((v - Vec3::new(0.6, 0.0, 0.0)).length() < 1e-3, "{v}");
    let (m, text) = drag_move(&d, a, b, Quat::IDENTITY, Some(Snap::default())).unwrap();
    assert_eq!(m, Move::Translate(Vec3::new(0.5, 0.0, 0.0)));
    assert_eq!(text, "0.50 m");
    // In Gazebo's frame: with the world root's rotation undone (Gazebo Z
    // is Bevy up).
    let root = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mut d = drag(Handle::Axis(2));
    d.axes = [root * Vec3::X, root * Vec3::Y, root * Vec3::Z];
    let up = screen(&t, &p, size, d.axes[2]).unwrap();
    let (m, _) = drag_move(&d, a, ray(&t, &p, size, up), root, None).unwrap();
    let Move::Translate(v) = m else { panic!() };
    assert!((v - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-2, "{v}");
}

#[test]
fn a_plane_drag_moves_in_the_plane_and_a_ring_turns_about_its_axis() {
    let (t, p, size) = view();
    let press = screen(&t, &p, size, Vec3::ZERO).unwrap();
    let now = screen(&t, &p, size, Vec3::new(0.3, 0.7, 0.0)).unwrap();
    let (a, b) = (ray(&t, &p, size, press), ray(&t, &p, size, now));
    let (m, _) = drag_move(&drag(Handle::Plane(2)), a, b, Quat::IDENTITY, None).unwrap();
    let Move::Translate(v) = m else { panic!() };
    assert!((v - Vec3::new(0.3, 0.7, 0.0)).length() < 1e-3, "{v}");
    // A quarter turn about Z: from +X to +Y on the ring.
    let from = screen(&t, &p, size, Vec3::X).unwrap();
    let to = screen(&t, &p, size, Vec3::Y).unwrap();
    let (a, b) = (ray(&t, &p, size, from), ray(&t, &p, size, to));
    let (m, text) = drag_move(&drag(Handle::Ring(2)), a, b, Quat::IDENTITY, None).unwrap();
    let Move::Rotate(q) = m else { panic!() };
    let (axis, angle) = q.to_axis_angle();
    assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3 && axis.z > 0.99);
    assert_eq!(text, "90°");
    // Snapped to 15°: 50° is 45°.
    let to = screen(
        &t,
        &p,
        size,
        Vec3::new(50f32.to_radians().cos(), 50f32.to_radians().sin(), 0.0),
    )
    .unwrap();
    let b = ray(&t, &p, size, to);
    let (_, text) = drag_move(
        &drag(Handle::Ring(2)),
        a,
        b,
        Quat::IDENTITY,
        Some(Snap::default()),
    )
    .unwrap();
    assert_eq!(text, "45°");
}

#[test]
fn geometry_helpers() {
    assert_eq!(
        to_segment(Vec2::new(5.0, 3.0), Vec2::ZERO, Vec2::new(10.0, 0.0)),
        3.0
    );
    assert_eq!(
        to_segment(Vec2::new(-4.0, 3.0), Vec2::ZERO, Vec2::new(10.0, 0.0)),
        5.0
    );
    let square = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
    assert!(inside(Vec2::splat(0.5), &square));
    assert_eq!(area(&square), 1.0);
    assert!(!inside(Vec2::new(1.5, 0.5), &square));
    assert_eq!(snap_to(0.37, 0.25), 0.25);
    assert_eq!(snap_to(0.37, 0.0), 0.37);
}

#[test]
fn a_turn_is_about_the_world_axis_whatever_the_start() {
    use std::f32::consts::FRAC_PI_2;
    // On its side (about X); a quarter turn about Z then takes its X to Y.
    let start = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_x(FRAC_PI_2));
    let quarter = Move::Rotate(Quat::from_rotation_z(FRAC_PI_2));
    let turned = apply_move(quarter, &start, start.translation);
    assert!((turned.rotation * Vec3::X - Vec3::Y).length() < 1e-5);
    assert_eq!(turned.translation, start.translation);
    // Another in the selection swings round the gizmo's centre.
    let other = apply_move(quarter, &start, Vec3::new(0.0, 2.0, 3.0));
    assert!((other.translation - Vec3::new(0.0, 3.0, 3.0)).length() < 1e-5);
}

#[test]
fn an_arrow_or_ring_seen_end_on_is_not_offered_and_does_not_throw() {
    // Looking down Bevy -Z: the Z arrow points at the camera, the X and Y
    // rings are edge-on.
    let (t, p, size) = view();
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let centre = screen(&t, &p, size, Vec3::ZERO).unwrap();
    let near = centre + Vec2::new(1.0, 1.0);
    let pick_at = |tool, at| pick(tool, at, (&t, &p, size), Vec3::ZERO, &axes);
    assert_ne!(
        pick_at(TransformTool::Translate, near),
        Some(Handle::Axis(2))
    );
    let len = GIZMO_PX * metres_per_px(&t, &p, size, Vec3::ZERO);
    let on_edge = screen(&t, &p, size, Vec3::Y * len * RING).unwrap();
    assert_eq!(
        pick_at(TransformTool::Rotate, on_edge),
        Some(Handle::Ring(2))
    );
    let on_x_edge = screen(&t, &p, size, Vec3::Y * len * RING * 0.5).unwrap();
    assert_eq!(pick_at(TransformTool::Rotate, on_x_edge), None);
    // A drag switched (Z) to the arrow at the camera: two pixels don't move
    // it at all, let alone metres.
    let (a, b) = (
        ray(&t, &p, size, centre),
        ray(&t, &p, size, centre + Vec2::new(2.0, 0.0)),
    );
    assert_eq!(
        drag_move(&drag(Handle::Axis(2)), a, b, Quat::IDENTITY, None),
        None
    );
    assert_eq!(
        drag_move(&drag(Handle::Ring(0)), a, b, Quat::IDENTITY, None),
        None
    );
    // Nearly end-on but not quite: bounded.
    let tilted = Transform::from_xyz(0.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
    let from = screen(&tilted, &p, size, Vec3::ZERO).unwrap();
    let (a, b) = (
        ray(&tilted, &p, size, from),
        ray(&tilted, &p, size, from + Vec2::new(0.0, -2.0)),
    );
    let (m, _) = drag_move(&drag(Handle::Axis(2)), a, b, Quat::IDENTITY, None).unwrap();
    let Move::Translate(v) = m else { panic!() };
    assert!(v.length() < 0.2, "{v}");
}
