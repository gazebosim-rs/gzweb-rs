//! Saving a world (spec §7.1; DECISIONS.md D-108): the SDFormat the
//! server generates (`generate_world_sdf`), with its top-level models put
//! back at their authored poses (where they were loaded or last placed),
//! not where physics has taken them; and one model's SDFormat, to copy it
//! (D-109).

use crate::scene::Pose;

/// A number as SDFormat writes it: up to six decimals, no trailing zeros,
/// never "-0".
pub(crate) fn number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "-0" | "" => "0".to_string(),
        s => s.to_string(),
    }
}

/// How close to ±1 the pitch's sine is taken as ±90° (gimbal lock): roll
/// and yaw turn about the same axis there, so the roll goes into the yaw.
/// A pose's quaternion comes from `f32`s, about 1e-7 apart.
const GIMBAL: f64 = 1.0 - 1e-6;

/// `pose` as an SDFormat `<pose>`'s text: `x y z roll pitch yaw` (radians;
/// the rotation is yaw about Z, then pitch about Y, then roll about X, as
/// gz-math reads it).
pub fn sdf_pose(pose: &Pose) -> String {
    let [x, y, z, w] = pose.quat;
    let len = (x * x + y * y + z * z + w * w).sqrt();
    let [x, y, z, w] = if len > 1e-12 {
        [x / len, y / len, z / len, w / len]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let sinp = 2.0 * (w * y - z * x);
    let (roll, pitch, yaw) = if sinp >= GIMBAL {
        (0.0, std::f64::consts::FRAC_PI_2, -2.0 * x.atan2(w))
    } else if sinp <= -GIMBAL {
        (0.0, -std::f64::consts::FRAC_PI_2, 2.0 * x.atan2(w))
    } else {
        (
            (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y)),
            sinp.asin(),
            (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z)),
        )
    };
    // Into (-π, π].
    let wrap = |a: f64| {
        let pi = std::f64::consts::PI;
        let a = (a + pi).rem_euclid(2.0 * pi) - pi;
        if a <= -pi + 1e-12 {
            pi
        } else {
            a
        }
    };
    let (roll, yaw) = (wrap(roll), wrap(yaw));
    let [px, py, pz] = pose.pos;
    [px, py, pz, roll, pitch, yaw]
        .iter()
        .map(|v| number(*v))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `sdf` (a world's SDFormat) with each named top-level model's own
/// `<pose>` set to its pose here, in the world frame: replaced where it
/// has one (its `relative_to` dropped), added where it hasn't. Models not
/// named keep theirs; the rest of the text is left exactly as it was.
pub fn with_model_poses(sdf: &str, poses: &[(&str, Pose)]) -> Result<String, String> {
    let doc = roxmltree::Document::parse(sdf).map_err(|e| e.to_string())?;
    let world = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("world"))
        .ok_or("no <world> in the SDFormat")?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = vec![];
    for model in world.children().filter(|n| n.has_tag_name("model")) {
        let Some(name) = model.attribute("name") else {
            continue;
        };
        let Some((_, pose)) = poses.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let element = format!("<pose>{}</pose>", sdf_pose(pose));
        if let Some(old) = model.children().find(|n| n.has_tag_name("pose")) {
            edits.push((old.range(), element));
            continue;
        }
        // No pose: one goes first inside the model.
        let range = model.range();
        let tag = &sdf[range.clone()];
        let open_end = tag.find('>').ok_or("a model's tag doesn't end")?;
        if tag[..open_end].ends_with('/') {
            // `<model name="m"/>`: opened out to hold it.
            let at = range.start + open_end - 1;
            edits.push((
                at..range.start + open_end + 1,
                format!(">{element}</model>"),
            ));
        } else {
            let at = range.start + open_end + 1;
            edits.push((at..at, element));
        }
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = sdf.to_string();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    Ok(out)
}

/// The top-level model `name` of `sdf` (a world's SDFormat) as an
/// SDFormat document of its own, to `create` a copy of it from (DECISIONS.md
/// D-109): its element as written, in an `<sdf>` of the world's version.
pub fn model_sdf(sdf: &str, name: &str) -> Result<String, String> {
    let doc = roxmltree::Document::parse(sdf).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let world = root
        .children()
        .find(|n| n.has_tag_name("world"))
        .ok_or("no <world> in the SDFormat")?;
    let model = world
        .children()
        .find(|n| n.has_tag_name("model") && n.attribute("name") == Some(name))
        .ok_or_else(|| format!("no model {name} in the world"))?;
    let version = root.attribute("version").unwrap_or("1.9");
    Ok(format!(
        "<?xml version=\"1.0\"?><sdf version=\"{version}\">{}</sdf>",
        &sdf[model.range()]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(pos: [f64; 3], rpy: [f64; 3]) -> Pose {
        let [r, p, y] = rpy;
        let (cr, sr) = ((r / 2.0).cos(), (r / 2.0).sin());
        let (cp, sp) = ((p / 2.0).cos(), (p / 2.0).sin());
        let (cy, sy) = ((y / 2.0).cos(), (y / 2.0).sin());
        Pose {
            pos,
            quat: [
                sr * cp * cy - cr * sp * sy,
                cr * sp * cy + sr * cp * sy,
                cr * cp * sy - sr * sp * cy,
                cr * cp * cy + sr * sp * sy,
            ],
        }
    }

    /// A pose reads back as its position and roll, pitch and yaw, with
    /// tidy numbers.
    #[test]
    fn poses_write_as_sdformat() {
        assert_eq!(sdf_pose(&Pose::IDENTITY), "0 0 0 0 0 0");
        let p = pose([1.5, -2.0, 0.25], [0.1, -0.2, 1.0]);
        assert_eq!(sdf_pose(&p), "1.5 -2 0.25 0.1 -0.2 1");
        assert_eq!(number(-0.000_000_1), "0");
        assert_eq!(number(3.0), "3");
    }

    /// The rotation `x y z roll pitch yaw` means, as a quaternion.
    fn quat_of(text: &str) -> [f64; 4] {
        let v: Vec<f64> = text.split(' ').map(|s| s.parse().unwrap()).collect();
        pose([0.0; 3], [v[3], v[4], v[5]]).quat
    }

    /// Whether two quaternions are the same rotation, within `tol` (rad).
    fn same_rotation(a: [f64; 4], b: [f64; 4], tol: f64) -> bool {
        let dot: f64 = (0..4).map(|i| a[i] * b[i]).sum::<f64>().abs().min(1.0);
        2.0 * dot.acos() < tol
    }

    /// Any pose reads back as the same rotation, at ±90° pitch too (gimbal
    /// lock), from `f32`-rounded and unnormalized quaternions.
    // The near-π/2 pitches are as a world file writes them.
    #[allow(clippy::approx_constant)]
    #[test]
    fn poses_keep_their_rotation_at_gimbal_lock() {
        let half_pi = std::f64::consts::FRAC_PI_2;
        let cases = [
            [0.0, -1.570_796_3, 0.7],
            [0.0, 1.570_796, 1.0],
            [0.3, half_pi, 0.5],
            [-0.4, -half_pi, -2.0],
            [0.1, 1.5, -3.0],
            [3.0, -0.2, 2.5],
            [-3.1, 0.0, 3.1],
        ];
        for rpy in cases {
            let mut p = pose([0.0; 3], rpy);
            // As a `Transform` holds it.
            p.quat = p.quat.map(|v| f64::from(v as f32));
            let want = p.quat;
            assert!(
                same_rotation(quat_of(&sdf_pose(&p)), want, 2e-3),
                "{rpy:?} → {}",
                sdf_pose(&p)
            );
            p.quat = p.quat.map(|v| v * 2.0);
            assert!(same_rotation(quat_of(&sdf_pose(&p)), want, 2e-3));
        }
    }

    /// Named models get their pose, replaced or added (a self-closed one
    /// opened out); others and the rest of the text are untouched; a
    /// document without a world says so.
    #[test]
    fn model_poses_are_set_in_place() {
        let sdf = r#"<?xml version="1.0"?>
<sdf version="1.9"><world name="w">
  <model name="cart"><pose relative_to="world">5 5 0 0 0 1</pose><link name="l"><pose>1 0 0 0 0 0</pose></link></model>
  <model name="box"><static>true</static></model>
  <model name="flat"/>
  <model name="kept"><pose>9 9 9 0 0 0</pose></model>
</world></sdf>"#;
        let at = pose([1.0, 2.0, 0.0], [0.0, 0.0, 0.5]);
        let out =
            with_model_poses(sdf, &[("cart", at), ("box", Pose::IDENTITY), ("flat", at)]).unwrap();
        assert!(out.contains(r#"<model name="cart"><pose>1 2 0 0 0 0.5</pose><link name="l"><pose>1 0 0 0 0 0</pose>"#), "{out}");
        assert!(
            out.contains(r#"<model name="box"><pose>0 0 0 0 0 0</pose><static>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<model name="flat"><pose>1 2 0 0 0 0.5</pose></model>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<model name="kept"><pose>9 9 9 0 0 0</pose></model>"#),
            "{out}"
        );
        assert!(out.starts_with(r#"<?xml version="1.0"?>"#));
        assert!(roxmltree::Document::parse(&out).is_ok());
        assert!(with_model_poses("<sdf/>", &[]).is_err());
        assert!(with_model_poses("not xml", &[]).is_err());
    }

    /// A model comes out whole, in an `<sdf>` of the world's version; a
    /// nested model of the same name or a missing one doesn't.
    #[test]
    fn a_model_comes_out_as_its_own_document() {
        let sdf = r#"<sdf version="1.10"><world name="w">
  <model name="outer"><model name="cart"><link name="n"/></model></model>
  <model name="cart"><static>true</static><link name="l"/></model>
</world></sdf>"#;
        assert_eq!(
            model_sdf(sdf, "cart").unwrap(),
            r#"<?xml version="1.0"?><sdf version="1.10"><model name="cart"><static>true</static><link name="l"/></model></sdf>"#
        );
        assert!(model_sdf(sdf, "gone")
            .unwrap_err()
            .contains("no model gone"));
        assert!(model_sdf("<sdf/>", "cart").is_err());
    }
}
