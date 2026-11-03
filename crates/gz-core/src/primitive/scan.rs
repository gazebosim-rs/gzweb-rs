// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! A single measured planar scan, independent of scene geometry and public IDs.
use serde::Deserialize;

pub const MAX_SCAN_BEAMS: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScanFrame {
    pub version: u32,
    pub world_id: u32,
    pub epoch: u64,
    pub sim_time_ns: u64,
    #[serde(deserialize_with = "required_scan")]
    pub scan: Option<Scan>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    pub sensor_id: u32,
    pub acquired_ns: u64,
    pub sensor_position: [f64; 3],
    /// World orientation, xyzw; rays lie in the sensor's local XY plane.
    pub sensor_orientation: [f64; 4],
    pub angle_min: f64,
    pub angle_max: f64,
    pub angle_step: f64,
    pub range_min: f64,
    pub range_max: f64,
    /// A null is no return, never a zero-range hit or a maximum-range point.
    pub ranges: Vec<Option<f64>>,
}

fn required_scan<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Scan>, D::Error> {
    Option::<Scan>::deserialize(deserializer)
}

impl Scan {
    /// The producer includes the declared final angle exactly.
    pub fn angle(&self, index: usize) -> f64 {
        if index + 1 == self.ranges.len() {
            self.angle_max
        } else {
            self.angle_min + index as f64 * self.angle_step
        }
    }
}

impl ScanFrame {
    pub fn validate(&self) -> Result<(), String> {
        let invalid = || "Invalid bounded planar scan".to_string();
        if self.version != 1 || self.world_id == 0 {
            return Err(invalid());
        }
        let Some(s) = &self.scan else { return Ok(()) };
        let n = s.ranges.len();
        let norm = s.sensor_orientation.iter().map(|v| v * v).sum::<f64>();
        let span = s.angle_max - s.angle_min;
        if s.sensor_id == 0
            || s.sensor_id == self.world_id
            || s.acquired_ns > self.sim_time_ns
            || !(2..=MAX_SCAN_BEAMS).contains(&n)
            || ![
                s.angle_min,
                s.angle_max,
                s.angle_step,
                s.range_min,
                s.range_max,
            ]
            .iter()
            .all(|v| v.is_finite())
            || span <= 0.0
            || span > std::f64::consts::TAU
            || s.angle_step <= 0.0
            || s.range_min < 0.0
            || s.range_max <= s.range_min
            || s.sensor_orientation.iter().any(|v| !v.is_finite())
            || !norm.is_finite()
            || (norm - 1.0).abs() > 1e-6
            || s.sensor_position
                .iter()
                .any(|v| !v.is_finite() || v.abs() + s.range_max > f64::from(f32::MAX))
            || s.ranges
                .iter()
                .flatten()
                .any(|r| !r.is_finite() || *r < s.range_min || *r > s.range_max)
        {
            return Err(invalid());
        }
        // Compare the progression itself, never an absolute angle offset.
        let expected_step = span / (n - 1) as f64;
        let bound = 4.0 * f64::EPSILON * expected_step.abs();
        if !expected_step.is_finite()
            || expected_step <= 0.0
            || (s.angle_step - expected_step).abs() > bound
        {
            return Err(invalid());
        }
        let mut previous = None;
        for index in 0..n {
            let angle = s.angle(index);
            if !angle.is_finite() || previous.is_some_and(|last| angle <= last) {
                return Err(invalid());
            }
            previous = Some(angle);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
