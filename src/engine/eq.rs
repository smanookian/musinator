//! 10-band graphic equalizer: peaking filters (RBJ cookbook), ±12 dB.
//! Runs in the real-time renderer: fixed arrays, no locks, no allocation.
//! Boosts lower the overall level by the biggest boost, so nothing clips.

pub const BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const LABELS: [&str; 10] = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
pub const MAX_DB: f32 = 12.0;
const MAX_CH: usize = 8;
/// Bandwidth of each band (octave spacing).
const Q: f32 = 1.41;

/// (name, gains in dB). "Custom" is whatever the user set.
pub const PRESETS: &[(&str, [f32; 10])] = &[
    ("Flat", [0.0; 10]),
    ("Rock", [5.0, 4.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 4.0, 5.0]),
    ("Pop", [-1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0, -1.0, -1.0]),
    ("Jazz", [3.0, 2.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0]),
    ("Classical", [4.0, 3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0]),
    ("Electronic", [5.0, 4.0, 1.0, 0.0, -2.0, 1.0, 0.0, 1.0, 4.0, 5.0]),
    ("Vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("Bass boost", [7.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Treble boost", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 6.0, 7.0]),
    ("Loudness", [6.0, 4.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 4.0, 5.0]),
];

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    fn peak(f: f32, db: f32, rate: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * std::f32::consts::PI * f / rate;
        let alpha = w.sin() / (2.0 * Q);
        let c = w.cos();
        let a0 = 1.0 + alpha / a;
        Self { b0: (1.0 + alpha * a) / a0, b1: -2.0 * c / a0, b2: (1.0 - alpha * a) / a0, a1: -2.0 * c / a0, a2: (1.0 - alpha / a) / a0 }
    }
}

pub struct Eq {
    filters: [Biquad; 10],
    active: [bool; 10],
    /// Transposed direct form II state: [band][channel][2].
    state: [[[f32; 2]; MAX_CH]; 10],
    pre: f32,
    on: bool,
}

impl Default for Eq {
    fn default() -> Self {
        Self { filters: [Biquad::default(); 10], active: [false; 10], state: [[[0.0; 2]; MAX_CH]; 10], pre: 1.0, on: false }
    }
}

/// Overall level change that keeps boosts from clipping (dB, ≤ 0).
pub fn preamp_db(gains: &[f32; 10]) -> f32 {
    -gains.iter().cloned().fold(0.0, f32::max)
}

impl Eq {
    /// New settings (or a new sample rate). Off or all-flat = bypass.
    pub fn set(&mut self, on: bool, gains: &[f32; 10], rate: u32) {
        let rate = rate.max(8000) as f32;
        self.on = on && gains.iter().any(|g| g.abs() > 0.01);
        for (i, &f) in BANDS.iter().enumerate() {
            let g = gains[i].clamp(-MAX_DB, MAX_DB);
            // Skip flat bands and bands too close to the top of the sample rate.
            self.active[i] = g.abs() > 0.01 && f < rate * 0.45;
            if self.active[i] {
                self.filters[i] = Biquad::peak(f, g, rate);
            }
        }
        self.pre = 10f32.powf(preamp_db(gains) / 20.0);
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn reset(&mut self) {
        self.state = [[[0.0; 2]; MAX_CH]; 10];
    }

    /// Process one sample of channel `ch`.
    #[inline]
    pub fn process(&mut self, x: f32, ch: usize) -> f32 {
        if !self.on {
            return x;
        }
        let ch = ch.min(MAX_CH - 1);
        let mut y = x * self.pre;
        for i in 0..10 {
            if !self.active[i] {
                continue;
            }
            let f = self.filters[i];
            let s = &mut self.state[i][ch];
            let out = f.b0 * y + s[0];
            s[0] = f.b1 * y - f.a1 * out + s[1];
            s[1] = f.b2 * y - f.a2 * out;
            y = out;
        }
        y
    }

    /// Response in dB at `f` Hz, for drawing the curve (includes the preamp).
    pub fn response_db(gains: &[f32; 10], f: f32, rate: u32) -> f32 {
        let rate = rate as f32;
        let w = 2.0 * std::f32::consts::PI * f / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let mut db = preamp_db(gains);
        for (i, &bf) in BANDS.iter().enumerate() {
            if gains[i].abs() < 0.01 || bf >= rate * 0.45 {
                continue;
            }
            let b = Biquad::peak(bf, gains[i], rate);
            let (nr, ni) = (b.b0 + b.b1 * c1 + b.b2 * c2, -(b.b1 * s1 + b.b2 * s2));
            let (dr, di) = (1.0 + b.a1 * c1 + b.a2 * c2, -(b.a1 * s1 + b.a2 * s2));
            db += 10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10();
        }
        db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Peak level (dB) of a sine at `f` after the EQ settles.
    fn level(gains: &[f32; 10], f: f32) -> f32 {
        let mut eq = Eq::default();
        eq.set(true, gains, 48_000);
        let mut peak = 0.0f32;
        for n in 0..48_000 {
            let x = (2.0 * std::f32::consts::PI * f * n as f32 / 48_000.0).sin();
            let y = eq.process(x, 0);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        20.0 * peak.log10()
    }

    #[test]
    fn boost_and_cut_hit_their_band_and_never_clip() {
        let mut g = [0.0; 10];
        g[5] = 6.0; // +6 dB at 1 kHz; the preamp takes 6 dB off everything.
        assert!(level(&g, 1000.0).abs() < 0.3, "{}", level(&g, 1000.0));
        assert!((level(&g, 62.0) + 6.0).abs() < 0.5);
        let mut g = [0.0; 10];
        g[1] = -9.0;
        assert!((level(&g, 62.0) + 9.0).abs() < 0.5);
        assert!(level(&g, 4000.0).abs() < 0.3);
        // The drawn curve matches what is heard.
        assert!((Eq::response_db(&g, 62.0, 48_000) - level(&g, 62.0)).abs() < 0.3);
    }

    #[test]
    fn flat_or_off_is_bit_exact() {
        let mut eq = Eq::default();
        eq.set(true, &[0.0; 10], 44_100);
        assert_eq!(eq.process(0.123, 0), 0.123);
        eq.set(false, &PRESETS[1].1, 44_100);
        assert_eq!(eq.process(0.123, 1), 0.123);
    }

    #[test]
    fn presets_stay_in_range() {
        for (name, g) in PRESETS {
            assert!(g.iter().all(|x| x.abs() <= MAX_DB), "{name}");
        }
    }
}
