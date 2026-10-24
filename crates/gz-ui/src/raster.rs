//! Lines drawn on the CPU (DECISIONS.md D-100): the Plot panel's charts
//! and the Sensors panel's lidar view, into an image at its physical
//! size, so a line is anti-aliased the same in every browser and
//! natively, with no shader or extra dependency. A line's coverage is its
//! distance from each pixel's centre (round caps and joins for free); a
//! line is drawn whole before it blends, so its own segments never darken
//! where they meet.

use std::collections::VecDeque;
use std::sync::OnceLock;

use bevy::prelude::*;

/// A colour in linear RGB with alpha (straight), from the theme
/// (`Color::to_linear`).
pub(crate) type Rgba = [f32; 4];

/// An image being drawn: linear, premultiplied colour.
pub(crate) struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<[f32; 4]>,
    /// The series being drawn's coverage.
    cov: Vec<f32>,
}

impl Canvas {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0; 4]; w * h],
            cov: vec![0.0; w * h],
        }
    }

    /// A horizontal line `thickness` pixels thick centred on `y`, across
    /// the whole width; a row it partly covers is partly coloured.
    pub(crate) fn hline(&mut self, y: f32, thickness: f32, color: Rgba) {
        let (top, bottom) = (y - thickness / 2.0, y + thickness / 2.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = (top.floor().max(0.0) as usize)..(bottom.ceil().max(0.0) as usize).min(self.h);
        for row in rows {
            #[allow(clippy::cast_precision_loss)]
            let r = row as f32;
            let c = (bottom.min(r + 1.0) - top.max(r)).clamp(0.0, 1.0);
            for x in 0..self.w {
                blend(&mut self.px[row * self.w + x], color, c);
            }
        }
    }

    /// A line `width` pixels wide through `points`, blended once.
    pub(crate) fn polyline(&mut self, points: &[Vec2], width: f32, color: Rgba) {
        let r = width / 2.0;
        let (mut x0, mut y0, mut x1, mut y1) = (self.w, self.h, 0, 0);
        let segments = points.windows(2).map(|p| (p[0], p[1]));
        let single = (points.len() == 1).then(|| (points[0], points[0]));
        for (a, b) in segments.chain(single) {
            let lo = a.min(b) - Vec2::splat(r + 1.0);
            let hi = a.max(b) + Vec2::splat(r + 1.0);
            let Some((xs, ys)) = self.clip(lo, hi) else {
                continue;
            };
            x0 = x0.min(xs.start);
            x1 = x1.max(xs.end);
            y0 = y0.min(ys.start);
            y1 = y1.max(ys.end);
            for y in ys {
                for x in xs.clone() {
                    #[allow(clippy::cast_precision_loss)]
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let c = (r + 0.5 - distance(p, a, b)).clamp(0.0, 1.0);
                    let cov = &mut self.cov[y * self.w + x];
                    *cov = cov.max(c);
                }
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * self.w + x;
                let c = std::mem::take(&mut self.cov[i]);
                if c > 0.0 {
                    blend(&mut self.px[i], color, c);
                }
            }
        }
    }

    /// The pixel ranges a box from `lo` to `hi` touches, if any.
    fn clip(&self, lo: Vec2, hi: Vec2) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (self.w as f32, self.h as f32);
        if hi.x < 0.0 || hi.y < 0.0 || lo.x > w || lo.y > h {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let range = |a: f32, b: f32, max: f32| (a.max(0.0) as usize)..(b.min(max).ceil() as usize);
        Some((range(lo.x, hi.x, w), range(lo.y, hi.y, h)))
    }

    /// A circle `width` wide about `center`, of radius `r`.
    pub(crate) fn circle(&mut self, center: Vec2, r: f32, width: f32, color: Rgba) {
        // A segment every few pixels of arc reads as round.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = ((std::f32::consts::TAU * r / 3.0).ceil() as usize).clamp(16, 720);
        #[allow(clippy::cast_precision_loss)]
        let points: Vec<Vec2> = (0..=n)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                center + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        self.polyline(&points, width, color);
    }

    /// A filled dot of radius `r` at `at`.
    pub(crate) fn dot(&mut self, at: Vec2, r: f32, color: Rgba) {
        self.polyline(&[at], r * 2.0, color);
    }

    /// The drawing as an sRGB image.
    pub(crate) fn image(&self) -> Image {
        #[allow(clippy::cast_possible_truncation)]
        let size = bevy::render::render_resource::Extent3d {
            width: self.w as u32,
            height: self.h as u32,
            depth_or_array_layers: 1,
        };
        let mut image = Image::new(
            size,
            bevy::render::render_resource::TextureDimension::D2,
            self.rgba8(),
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        image.sampler = bevy::image::ImageSampler::nearest();
        image
    }

    /// The image's bytes: sRGB, straight alpha.
    pub(crate) fn rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for &[r, g, b, a] in &self.px {
            if a <= 0.0 {
                out.extend_from_slice(&[0; 4]);
                continue;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let alpha = (a.min(1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[srgb(r / a), srgb(g / a), srgb(b / a), alpha]);
        }
        out
    }
}

/// `color` at `coverage` over `dst` (both premultiplied).
fn blend(dst: &mut [f32; 4], color: Rgba, coverage: f32) {
    let [r, g, b, alpha] = color;
    let a = alpha * coverage;
    let src = [r * a, g * a, b * a, a];
    for (d, s) in dst.iter_mut().zip(src) {
        *d = s + *d * (1.0 - a);
    }
}

/// How far `p` is from the segment `a`–`b`.
fn distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len = ab.length_squared();
    let t = if len > 0.0 {
        ((p - a).dot(ab) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// Linear to sRGB, through a table.
fn srgb(v: f32) -> u8 {
    const N: usize = 4096;
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    let lut = LUT.get_or_init(|| {
        (0..N)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let l = i as f32 / (N - 1) as f32;
                let s = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let b = (s * 255.0).round() as u8;
                b
            })
            .collect()
    });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (v.clamp(0.0, 1.0) * (N - 1) as f32).round() as usize;
    lut[i]
}

/// The points that draw `samples` between `t0` and `t1`, through `to_px`
/// (time and value to pixels): each sample while there are fewer than a
/// column's worth, else each column's first, lowest, highest and last, so
/// a dense series keeps its extremes. The sample before `t0` and the one
/// after `t1` are kept, for the line to enter and leave the window. A
/// non-finite value breaks the line.
pub(crate) fn trace(
    samples: &VecDeque<(f64, f64)>,
    t0: f64,
    t1: f64,
    to_px: impl Fn(f64, f64) -> Vec2,
) -> Vec<Vec<Vec2>> {
    let start = samples.partition_point(|s| s.0 < t0).saturating_sub(1);
    let mut lines: Vec<Vec<Vec2>> = vec![vec![]];
    // The column being gathered: its index and first, min, max, last.
    let mut col: Option<(i64, [Vec2; 4])> = None;
    let flush = |lines: &mut Vec<Vec<Vec2>>, col: &mut Option<(i64, [Vec2; 4])>| {
        if let Some((_, [first, lo, hi, last])) = col.take() {
            let line = lines.last_mut().expect("there is always a line");
            line.push(first);
            // In the order they came, so the line doesn't double back.
            let (a, b) = if lo.x <= hi.x { (lo, hi) } else { (hi, lo) };
            for p in [a, b, last] {
                if line.last() != Some(&p) {
                    line.push(p);
                }
            }
        }
    };
    for &(t, v) in samples.range(start..) {
        if !v.is_finite() {
            flush(&mut lines, &mut col);
            if !lines.last().is_some_and(Vec::is_empty) {
                lines.push(vec![]);
            }
            continue;
        }
        let p = to_px(t, v);
        #[allow(clippy::cast_possible_truncation)]
        let c = p.x.floor() as i64;
        match &mut col {
            Some((i, [_, lo, hi, last])) if *i == c => {
                if p.y > lo.y {
                    *lo = p;
                }
                if p.y < hi.y {
                    *hi = p;
                }
                *last = p;
            }
            _ => {
                flush(&mut lines, &mut col);
                col = Some((c, [p; 4]));
            }
        }
        if t > t1 {
            break;
        }
    }
    flush(&mut lines, &mut col);
    lines.retain(|l| !l.is_empty());
    lines
}
