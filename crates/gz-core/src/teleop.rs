//! What the Teleop panel publishes (DECISIONS.md D-104): a drive's
//! velocity command and a joint's position command, serialized.

use prost::Message as _;

use crate::msgs;

/// A `gz.msgs.Twist`: `linear` m/s forward and `angular` rad/s about up.
pub fn twist(linear: f64, angular: f64) -> Vec<u8> {
    msgs::Twist {
        linear: Some(msgs::Vector3d {
            x: linear,
            ..Default::default()
        }),
        angular: Some(msgs::Vector3d {
            z: angular,
            ..Default::default()
        }),
        ..Default::default()
    }
    .encode_to_vec()
}

/// A `gz.msgs.Double`.
pub fn double(value: f64) -> Vec<u8> {
    msgs::Double {
        data: value,
        ..Default::default()
    }
    .encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_serialize() {
        let t = msgs::Twist::decode(twist(0.35, -0.5).as_slice()).unwrap();
        assert_eq!(t.linear.unwrap().x, 0.35);
        assert_eq!(t.angular.unwrap().z, -0.5);
        assert_eq!(
            msgs::Double::decode(double(1.5).as_slice()).unwrap().data,
            1.5
        );
    }
}
