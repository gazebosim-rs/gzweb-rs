//! Sensor output for the Sensors panel (DECISIONS.md D-103): a lidar
//! scan's plane as points in the sensor's frame, and a camera frame's size
//! (the websocket server's `image` op sends PNG data).

use prost::Message as _;

use crate::msgs;

/// One horizontal ring of a lidar scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    /// The beams' angles and ranges, in order; a range outside
    /// `range_min..=range_max` (or not finite) is no return.
    pub beams: Vec<(f64, f64)>,
    pub range_min: f64,
    pub range_max: f64,
    /// Beams a ring, and rings.
    pub count: u32,
    pub vertical_count: u32,
}

impl Scan {
    /// A `gz.msgs.LaserScan`'s middle ring.
    pub fn decode(payload: &[u8]) -> Option<Scan> {
        let s = msgs::LaserScan::decode(payload).ok()?;
        let count = (s.count as usize).max(1);
        let rings = (s.ranges.len() / count).max(1);
        let ring = rings / 2;
        let ranges = s
            .ranges
            .get(ring * count..((ring + 1) * count).min(s.ranges.len()))
            .unwrap_or(&s.ranges);
        let step = if s.angle_step != 0.0 {
            s.angle_step
        } else if ranges.len() > 1 {
            #[allow(clippy::cast_precision_loss)]
            let n = (ranges.len() - 1) as f64;
            (s.angle_max - s.angle_min) / n
        } else {
            0.0
        };
        #[allow(clippy::cast_precision_loss)]
        let beams = ranges
            .iter()
            .enumerate()
            .map(|(i, r)| (s.angle_min + step * i as f64, *r))
            .collect();
        Some(Scan {
            beams,
            range_min: s.range_min,
            range_max: s.range_max,
            count: s.count,
            vertical_count: s.vertical_count.max(1),
        })
    }

    /// Whether `range` is a return.
    pub fn hit(&self, range: f64) -> bool {
        range.is_finite() && range >= self.range_min && range <= self.range_max
    }

    /// Its angular span (radians).
    pub fn span(&self) -> f64 {
        match (self.beams.first(), self.beams.last()) {
            (Some(a), Some(b)) => (b.0 - a.0).abs(),
            _ => 0.0,
        }
    }
}

/// A PNG's width and height, from its header.
pub fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if !data.starts_with(SIGNATURE) || data.len() < 24 || &data[12..16] != b"IHDR" {
        return None;
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    Some((be(&data[16..20]), be(&data[20..24])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scan's middle ring, with each beam's angle; returns are within
    /// range.
    #[test]
    fn a_scan_reads_its_middle_ring() {
        let scan = msgs::LaserScan {
            angle_min: -1.0,
            angle_max: 1.0,
            angle_step: 1.0,
            range_min: 0.1,
            range_max: 10.0,
            count: 3,
            vertical_count: 3,
            ranges: vec![1.0, 1.0, 1.0, 2.0, f64::INFINITY, 0.05, 3.0, 3.0, 3.0],
            ..Default::default()
        }
        .encode_to_vec();
        let s = Scan::decode(&scan).unwrap();
        assert_eq!(s.beams, [(-1.0, 2.0), (0.0, f64::INFINITY), (1.0, 0.05)]);
        assert!(s.hit(2.0));
        assert!(!s.hit(f64::INFINITY));
        assert!(!s.hit(0.05));
        assert_eq!(s.span(), 2.0);
        assert_eq!((s.count, s.vertical_count), (3, 3));
        // No step: spread over the span.
        let flat = msgs::LaserScan {
            angle_min: 0.0,
            angle_max: 1.0,
            count: 3,
            ranges: vec![1.0; 3],
            ..Default::default()
        }
        .encode_to_vec();
        let s = Scan::decode(&flat).unwrap();
        assert_eq!(s.beams[2].0, 1.0);
        assert_eq!(s.vertical_count, 1);
        assert_eq!(Scan::decode(&[0xff]), None);
    }

    #[test]
    fn a_png_says_its_size() {
        let mut png = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
        ];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(b"nope"), None);
    }
}
