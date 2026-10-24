//! sRGB colors, WCAG contrast and hue-keeping lightness fixes.

/// An 8-bit sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    /// Parses `#rrggbb`, `0xrrggbb`, `rrggbb`, `#rgb` or `rgb`.
    pub fn hex(s: &str) -> Option<Rgb> {
        let s = s.trim();
        let s = s.strip_prefix('#').or_else(|| s.strip_prefix("0x")).unwrap_or(s);
        if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let d = |i: usize, n: usize| u8::from_str_radix(&s[i..i + n], 16).ok();
        match s.len() {
            6 => Some(Rgb(d(0, 2)?, d(2, 2)?, d(4, 2)?)),
            3 => Some(Rgb(d(0, 1)? * 17, d(1, 1)? * 17, d(2, 1)? * 17)),
            _ => None,
        }
    }

    /// `#rrggbb`, lower case.
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// WCAG relative luminance, 0.0 (black) to 1.0 (white).
    pub fn luminance(&self) -> f64 {
        fn lin(c: u8) -> f64 {
            let c = c as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// Blends toward `other`: `t = 0` is `self`, `t = 1` is `other`
    /// (like painting `other` at opacity `t` over `self`).
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    /// Hue in degrees, saturation and lightness in 0..=1.
    pub(crate) fn to_hsl(self) -> (f64, f64, f64) {
        let (r, g, b) = (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0.0, 0.0, l);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    pub(crate) fn from_hsl(h: f64, s: f64, l: f64) -> Rgb {
        let l = l.clamp(0.0, 1.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s.clamp(0.0, 1.0);
        let hp = h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        let f = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb(f(r), f(g), f(b))
    }
}

/// `#rrggbb` at compile time, for the built-in palette tables.
pub(crate) const fn h(s: &str) -> Rgb {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("bad hex digit"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() == 7 && b[0] == b'#', "expected #rrggbb");
    Rgb(nib(b[1]) * 16 + nib(b[2]), nib(b[3]) * 16 + nib(b[4]), nib(b[5]) * 16 + nib(b[6]))
}

/// WCAG 2.x contrast ratio, 1.0 to 21.0. Order does not matter.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Returns `fg` unchanged if `contrast(fg, bg) >= min`. Otherwise moves its
/// lightness (same hue and saturation) just far enough to reach `min`.
pub fn legible(fg: Rgb, bg: Rgb, min: f64) -> Rgb {
    legible_on(fg, &[bg], min)
}

/// Like [`legible`], but `fg` must reach `min` against every color in `bgs`.
/// If no lightness works, returns the best one found.
pub(crate) fn legible_on(fg: Rgb, bgs: &[Rgb], min: f64) -> Rgb {
    let worst = |c: Rgb| bgs.iter().map(|&b| contrast(c, b)).fold(f64::INFINITY, f64::min);
    let start = worst(fg);
    if start >= min {
        return fg;
    }
    let (h, s, l) = fg.to_hsl();
    let avg = bgs.iter().map(Rgb::luminance).sum::<f64>() / bgs.len() as f64;
    let lighter_first = fg.luminance() >= avg;
    let mut best = (fg, start);
    for lighter in [lighter_first, !lighter_first] {
        const STEPS: u32 = 512;
        for i in 1..=STEPS {
            let t = i as f64 / STEPS as f64;
            let l2 = if lighter { l + (1.0 - l) * t } else { l * (1.0 - t) };
            let c = Rgb::from_hsl(h, s, l2);
            let w = worst(c);
            if w >= min {
                return c;
            }
            if w > best.1 {
                best = (c, w);
            }
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hue_gap(a: f64, b: f64) -> f64 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(Rgb::hex("#1e1e2e"), Some(Rgb(0x1e, 0x1e, 0x2e)));
        assert_eq!(Rgb::hex("89B4FA"), Some(Rgb(0x89, 0xb4, 0xfa)));
        assert_eq!(Rgb::hex("#fff"), Some(Rgb::WHITE));
        assert_eq!(Rgb::hex("#12345"), None);
        assert_eq!(Rgb::hex("#+12345"), None);
        assert_eq!(Rgb::hex("#gg0000"), None);
        assert_eq!(Rgb(0xcd, 0xd6, 0xf4).to_hex(), "#cdd6f4");
        assert_eq!(h("#CDD6F4"), Rgb(0xcd, 0xd6, 0xf4));
    }

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(Rgb::BLACK, Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(Rgb::WHITE, Rgb::WHITE) - 1.0).abs() < 1e-9);
        // #777777 on white is the classic "just under 4.5" grey.
        let c = contrast(Rgb(0x77, 0x77, 0x77), Rgb::WHITE);
        assert!(c > 4.4 && c < 4.5, "{c}");
    }

    #[test]
    fn mix_ends() {
        let a = Rgb(10, 20, 30);
        let b = Rgb(200, 100, 0);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert_eq!(a.mix(b, 0.5), Rgb(105, 60, 15));
    }

    #[test]
    fn hsl_round_trip() {
        for c in [Rgb(0x89, 0xb4, 0xfa), Rgb(0xf3, 0x8b, 0xa8), Rgb(0x40, 0xa0, 0x2b), Rgb(7, 7, 7)] {
            let (h, s, l) = c.to_hsl();
            assert_eq!(Rgb::from_hsl(h, s, l), c);
        }
    }

    #[test]
    fn legible_unchanged_when_ok() {
        let fg = Rgb(0xcd, 0xd6, 0xf4);
        let bg = Rgb(0x1e, 0x1e, 0x2e);
        assert_eq!(legible(fg, bg, 4.5), fg);
    }

    #[test]
    fn legible_reaches_target_and_keeps_hue() {
        let cases = [
            (h("#7f849c"), h("#1e1e2e"), 4.5),
            (h("#565f89"), h("#1a1b26"), 4.5),
            (h("#93a1a1"), h("#fdf6e3"), 4.5),
            (h("#45475a"), h("#1e1e2e"), 3.0),
            (h("#ea76cb"), h("#eff1f5"), 4.5),
            (h("#e82424"), h("#1f1f28"), 4.5),
            (h("#df8e1d"), h("#eff1f5"), 4.5),
        ];
        for (fg, bg, min) in cases {
            let out = legible(fg, bg, min);
            let got = contrast(out, bg);
            assert!(got >= min, "{} on {}: {got:.2} < {min}", out.to_hex(), bg.to_hex());
            // Only just enough: not far past the target.
            assert!(got < min + 0.4, "{} on {}: overshot to {got:.2}", out.to_hex(), bg.to_hex());
            let (h0, s0, _) = fg.to_hsl();
            let (h1, _, _) = out.to_hsl();
            if s0 > 0.05 {
                assert!(hue_gap(h0, h1) < 6.0, "{} -> {}: hue moved", fg.to_hex(), out.to_hex());
            }
        }
    }

    #[test]
    fn legible_falls_back_to_other_direction() {
        // Mid grey on mid grey: lighter cannot reach 4.5, darker can.
        let bg = Rgb(0x90, 0x90, 0x90);
        let out = legible(Rgb(0x98, 0x98, 0x98), bg, 4.5);
        assert!(contrast(out, bg) >= 4.5);
        assert!(out.luminance() < bg.luminance());
    }
}
