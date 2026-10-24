//! Procedural album covers: a hue gradient with concentric rings.

use crate::rng::Rng;
use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, RgbImage};
use std::io::Cursor;

fn hsv(h: f64, s: f64, v: f64) -> [f64; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Encoded cover image (JPEG or PNG), unique for each seed.
pub fn make(seed: u64, size: u32, png: bool) -> Result<Vec<u8>> {
    let mut rng = Rng::new(seed);
    let hue = rng.f64();
    let shift = rng.range_f(0.08, 0.35) * if rng.chance(0.5) { 1.0 } else { -1.0 };
    let top = hsv(hue, rng.range_f(0.5, 0.9), rng.range_f(0.55, 0.95));
    let bottom = hsv(hue + shift, rng.range_f(0.6, 1.0), rng.range_f(0.15, 0.45));
    let ring = hsv(hue + 0.5 + rng.range_f(-0.1, 0.1), rng.range_f(0.2, 0.7), rng.range_f(0.8, 1.0));
    let (cx, cy) = (rng.range_f(0.25, 0.75), rng.range_f(0.25, 0.75));
    let rings = rng.range_f(4.0, 16.0);
    let sharp = rng.range_f(2.0, 10.0);
    let radius = rng.range_f(0.25, 0.6);
    let angle = rng.range_f(0.0, std::f64::consts::TAU);
    let (ax, ay) = (angle.cos(), angle.sin());

    let img = RgbImage::from_fn(size, size, |x, y| {
        let (u, v) = (x as f64 / size as f64, y as f64 / size as f64);
        let g = ((u - 0.5) * ax + (v - 0.5) * ay + 0.5).clamp(0.0, 1.0);
        let mut c = mix(top, bottom, g);
        let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
        if d < radius {
            let wave = 0.5 + 0.5 * (d * rings * std::f64::consts::TAU).cos();
            let fade = 1.0 - d / radius;
            c = mix(c, ring, wave.powf(sharp) * fade * 0.85);
        }
        let vignette = 1.0 - 0.35 * ((u - 0.5).powi(2) + (v - 0.5).powi(2));
        image::Rgb(c.map(|ch| (ch * vignette * 255.0).clamp(0.0, 255.0) as u8))
    });

    let mut out = Vec::new();
    if png {
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    } else {
        JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img)?;
    }
    Ok(out)
}
