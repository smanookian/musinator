//! Spectrum from the real output audio (the engine tap): 4096-point FFT,
//! Hann window, log-spaced bands, fast attack and slow fall.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::engine::render::Tap;

const N: usize = 4096;
const FLOOR_DB: f32 = -72.0;
const TOP_DB: f32 = -9.0;

pub struct Viz {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mags: Vec<f32>,
    pub bars: Vec<f32>,
    last_written: usize,
}

impl Viz {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(N);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
        Self {
            fft,
            window,
            samples: vec![0.0; N],
            buf: vec![Complex::default(); N],
            scratch,
            mags: vec![0.0; N / 2],
            bars: Vec::new(),
            last_written: 0,
        }
    }

    /// Update `bars` (0..=1). Returns true while anything is still moving.
    pub fn update(&mut self, tap: &Tap, n_bars: usize, dt: f32) -> bool {
        if self.bars.len() != n_bars {
            self.bars = vec![0.0; n_bars];
        }
        if n_bars == 0 {
            return false;
        }
        let written = tap.written();
        let fresh = written != self.last_written;
        self.last_written = written;
        let fall = 1.4 * dt;
        if !fresh {
            // No new audio (paused): let the bars fall.
            for b in &mut self.bars {
                *b = (*b - fall).max(0.0);
            }
            return self.bars.iter().any(|b| *b > 0.0);
        }
        tap.snapshot(&mut self.samples);
        for i in 0..N {
            self.buf[i] = Complex::new(self.samples[i] * self.window[i], 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
        // Amplitude of a full-scale sine = 1.0 (Hann coherent gain 0.5).
        let norm = 4.0 / N as f32;
        for (m, c) in self.mags.iter_mut().zip(&self.buf) {
            *m = c.norm() * norm;
        }
        let rate = tap.rate.load(std::sync::atomic::Ordering::Relaxed).max(8000) as f32;
        let (lo, hi) = (40.0f32, (rate / 2.0).min(16_000.0));
        let bin_hz = rate / N as f32;
        for (i, bar) in self.bars.iter_mut().enumerate() {
            let f0 = lo * (hi / lo).powf(i as f32 / n_bars as f32);
            let f1 = lo * (hi / lo).powf((i + 1) as f32 / n_bars as f32);
            let b0 = (f0 / bin_hz) as usize;
            let b1 = ((f1 / bin_hz) as usize).max(b0 + 1).min(self.mags.len());
            let peak = self.mags[b0.min(self.mags.len() - 1)..b1].iter().fold(0.0f32, |a, b| a.max(*b));
            // Gentle tilt so highs stay visible (music falls off with frequency).
            let tilt = (1.5 * (f0 / 1000.0).log2()).clamp(-4.5, 6.0);
            let db = 20.0 * peak.max(1e-9).log10() + tilt;
            let v = ((db - FLOOR_DB) / (TOP_DB - FLOOR_DB)).clamp(0.0, 1.0);
            if v > *bar {
                *bar += (v - *bar) * 0.7;
            } else {
                *bar = (*bar - fall).max(v);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lights_its_band() {
        let tap = Tap::new();
        tap.rate.store(48_000, std::sync::atomic::Ordering::Relaxed);
        // Feed a 1 kHz sine through the public path: write via snapshot is read-only,
        // so build the tap through the renderer-like push.
        for n in 0..Tap::LEN {
            tap.push_for_test((2.0 * std::f32::consts::PI * 1000.0 * n as f32 / 48_000.0).sin() * 0.5);
        }
        let mut v = Viz::new();
        v.update(&tap, 32, 0.033);
        let max = v.bars.iter().enumerate().fold((0, 0.0f32), |a, (i, b)| if *b > a.1 { (i, *b) } else { a });
        // 1 kHz on a 40 Hz..16 kHz log scale with 32 bars lands in bar ~17.
        assert!((16..=18).contains(&max.0), "peak bar {}", max.0);
        assert!(v.bars[2] < max.1 * 0.5);
    }
}
