//! The current lyric line as real big text: drawn with a font into a picture on a
//! background thread and shown with Kitty graphics (Kitty/Ghostty only).
//! Pictures live only in memory; the terminal keeps one, under a fixed id.

use std::collections::VecDeque;
use std::path::PathBuf;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use crossbeam_channel as cb;
use image::{DynamicImage, Rgba, RgbaImage};
use musinator_themes::Rgb;
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

use super::art::{ID_LYRIC, protocol};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub text: String,
    pub color: Rgb,
    /// Area for the picture, in cells.
    pub cells: Size,
}

pub struct Ready {
    pub key: Key,
    /// None = could not draw (no font, or text does not fit): use normal text.
    pub proto: Option<Protocol>,
}

pub struct BigLyrics {
    tx: cb::Sender<Key>,
}

impl BigLyrics {
    pub fn start(picker: Picker, out: cb::Sender<Ready>) -> Self {
        let (tx, rx) = cb::unbounded::<Key>();
        std::thread::Builder::new()
            .name("big-lyrics".into())
            .spawn(move || {
                let font = system_font().and_then(|p| std::fs::read(p).ok()).and_then(|b| FontVec::try_from_vec(b).ok());
                let mut done: VecDeque<Key> = VecDeque::new();
                while let Ok(first) = rx.recv() {
                    let mut keys = vec![first];
                    keys.extend(rx.try_iter());
                    for key in keys {
                        if done.contains(&key) {
                            continue;
                        }
                        let proto = font.as_ref().and_then(|f| draw(f, &picker, &key)).and_then(|img| protocol(&picker, img, ID_LYRIC));
                        done.push_back(key.clone());
                        if done.len() > 8 {
                            done.pop_front();
                        }
                        let _ = out.send(Ready { key, proto });
                    }
                }
            })
            .ok();
        Self { tx }
    }

    pub fn request(&self, key: Key) {
        let _ = self.tx.send(key);
    }
}

/// The system's bold monospace font (fontconfig), same family as most terminals.
fn system_font() -> Option<PathBuf> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "monospace:bold"]).output().ok()?;
    let p = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    p.exists().then_some(p)
}

/// Split words into at most `max_lines` lines no wider than `width` px.
fn wrap(font: &ab_glyph::PxScaleFont<&FontVec>, text: &str, width: f32, max_lines: usize) -> Option<Vec<String>> {
    let w = |s: &str| s.chars().map(|c| font.h_advance(font.glyph_id(c))).sum::<f32>();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let try_ = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if w(&try_) <= width {
            cur = try_;
        } else {
            if cur.is_empty() || w(word) > width {
                return None;
            }
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    (lines.len() <= max_lines && !lines.is_empty()).then_some(lines)
}

/// Draw `key.text` centered, as big as fits (2 lines max), on a transparent background.
fn draw(font: &FontVec, picker: &Picker, key: &Key) -> Option<DynamicImage> {
    let fs = picker.font_size();
    let (cw, ch) = (fs.width as u32, fs.height as u32);
    let (w, h) = (key.cells.width as u32 * cw, key.cells.height as u32 * ch);
    if w == 0 || h == 0 || key.text.trim().is_empty() {
        return None;
    }
    // Start at ~3x the terminal text, shrink until it fits in two lines.
    let mut px = ch as f32 * 2.6;
    let min = ch as f32 * 1.3;
    let (lines, scaled) = loop {
        let scaled = font.as_scaled(PxScale::from(px));
        let line_h = scaled.height() + scaled.line_gap();
        let max_lines = ((h as f32 / line_h) as usize).clamp(1, 2);
        if let Some(lines) = wrap(&scaled, key.text.trim(), w as f32 * 0.96, max_lines) {
            break (lines, scaled);
        }
        px *= 0.9;
        if px < min {
            return None;
        }
    };
    let mut img = RgbaImage::new(w, h);
    let line_h = scaled.height() + scaled.line_gap();
    let total = line_h * lines.len() as f32;
    let top = (h as f32 - total) / 2.0;
    let Rgb(r, g, b) = key.color;
    for (i, line) in lines.iter().enumerate() {
        let lw: f32 = line.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum();
        let mut x = (w as f32 - lw) / 2.0;
        let y = top + i as f32 * line_h + scaled.ascent();
        for c in line.chars() {
            let gid = scaled.glyph_id(c);
            let glyph = gid.with_scale_and_position(scaled.scale(), point(x, y));
            x += scaled.h_advance(gid);
            if let Some(o) = font.outline_glyph(glyph) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                        let a = (cov.clamp(0.0, 1.0) * 255.0) as u8;
                        let p = img.get_pixel_mut(px as u32, py as u32);
                        if a > p[3] {
                            *p = Rgba([r, g, b, a]);
                        }
                    }
                });
            }
        }
    }
    Some(DynamicImage::ImageRgba8(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_text_that_fits_and_is_fast() {
        let Some(font) = system_font().and_then(|p| std::fs::read(p).ok()).and_then(|b| FontVec::try_from_vec(b).ok()) else {
            eprintln!("no system font; skipped");
            return;
        };
        let picker = Picker::halfblocks();
        let key = Key { text: "You got the lips to make a strong man weak".into(), color: Rgb(232, 82, 55), cells: Size::new(90, 7) };
        let t = std::time::Instant::now();
        let img = draw(&font, &picker, &key).expect("drawn");
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let fs = picker.font_size();
        assert_eq!((img.width(), img.height()), (90 * fs.width as u32, 7 * fs.height as u32));
        let lit = img.to_rgba8().pixels().filter(|p| p[3] > 128).count();
        assert!(lit > 1000, "text pixels {lit}");
        println!("drew one line in {ms:.2} ms");
        assert!(ms < 50.0, "{ms} ms");
    }
}
