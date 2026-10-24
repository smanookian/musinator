//! The real-time side: pulls samples from the ring, applies volume with a short
//! ramp (no clicks), feeds the visualizer tap. No locks, no allocation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};

/// State shared between the engine thread and the renderer.
#[derive(Default)]
pub struct Shared {
    /// Samples taken out of the ring so far (played or discarded).
    pub consumed: AtomicU64,
    /// Discard samples until `consumed` reaches this value (flush on seek/skip).
    pub flush_to: AtomicU64,
    pub paused: AtomicBool,
    /// Linear output gain (user volume after the curve), f32 bits.
    volume: AtomicU32,
    /// True once the renderer has faded out and stopped taking samples.
    pub silent: AtomicBool,
    /// Rate of the PipeWire graph (what the device runs at). 0 = unknown.
    pub graph_rate: AtomicU32,
    /// Rate and channels the renderer currently expects.
    pub stream_rate: AtomicU32,
    pub channels: AtomicU32,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        let s = Self::default();
        s.volume.store(1f32.to_bits(), Relaxed);
        s.silent.store(true, Relaxed);
        s.paused.store(true, Relaxed);
        Arc::new(s)
    }
    pub fn set_volume(&self, amp: f32) {
        self.volume.store(amp.to_bits(), Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Relaxed))
    }
}

/// Mono copy of the newest output samples, for the visualizer. Overwrites; never blocks.
pub struct Tap {
    buf: Box<[AtomicU32]>,
    pos: AtomicUsize,
    pub rate: AtomicU32,
    pub enabled: AtomicBool,
}

impl Tap {
    pub const LEN: usize = 8192;
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buf: (0..Self::LEN).map(|_| AtomicU32::new(0)).collect(),
            pos: AtomicUsize::new(0),
            rate: AtomicU32::new(48_000),
            enabled: AtomicBool::new(false),
        })
    }
    fn push(&self, v: f32) {
        let p = self.pos.load(Relaxed);
        self.buf[p % Self::LEN].store(v.to_bits(), Relaxed);
        self.pos.store(p.wrapping_add(1), Release);
    }
    /// Copy the newest `out.len()` samples (oldest first).
    pub fn snapshot(&self, out: &mut [f32]) {
        let end = self.pos.load(Acquire);
        let n = out.len().min(Self::LEN);
        for (i, o) in out.iter_mut().enumerate().take(n) {
            let idx = end.wrapping_sub(n).wrapping_add(i) % Self::LEN;
            *o = f32::from_bits(self.buf[idx].load(Relaxed));
        }
    }
    /// Total samples written so far (lets readers see if anything new arrived).
    pub fn written(&self) -> usize {
        self.pos.load(Acquire)
    }
}

pub struct Renderer {
    cons: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    tap: Arc<Tap>,
    consumed: u64,
    gain: f32,
}

impl Renderer {
    pub fn new(cons: rtrb::Consumer<f32>, shared: Arc<Shared>, tap: Arc<Tap>) -> Self {
        Self { cons, shared, tap, consumed: 0, gain: 0.0 }
    }

    /// Samples waiting in the ring.
    pub fn available(&self) -> usize {
        self.cons.slots()
    }

    /// Fill `out` (interleaved, `channels` wide) at `rate`.
    pub fn render(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        let channels = channels.max(1);
        // Flush: drop samples queued before a seek/skip.
        let flush_to = self.shared.flush_to.load(Acquire);
        if self.consumed < flush_to {
            let n = ((flush_to - self.consumed) as usize).min(self.cons.slots());
            if let Ok(chunk) = self.cons.read_chunk(n) {
                chunk.commit_all();
                self.consumed += n as u64;
            }
            // Ramp in after a jump so it does not click.
            self.gain = 0.0;
            if self.consumed < flush_to {
                // Old samples not all here yet: play silence this cycle.
                out.fill(0.0);
                self.shared.consumed.store(self.consumed, Release);
                return;
            }
        }

        let paused = self.shared.paused.load(Acquire);
        let target = if paused { 0.0 } else { self.shared.volume() };
        if paused && self.gain == 0.0 {
            out.fill(0.0);
            self.shared.silent.store(true, Release);
            return;
        }
        self.shared.silent.store(false, Release);

        // 10 ms linear ramp toward the target gain.
        let step = 1.0 / (rate.max(8000) as f32 * 0.010);
        let frames_out = out.len() / channels;
        let avail_frames = self.cons.slots() / channels;
        let mut frames = frames_out.min(avail_frames);
        if paused {
            // Only take as many frames as the fade-out needs.
            let fade = (self.gain / step).ceil() as usize;
            frames = frames.min(fade.max(1));
        }
        let n = frames * channels;
        let tap_on = self.tap.enabled.load(Relaxed);
        if let Ok(chunk) = self.cons.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            let mut i = 0;
            let mut acc = 0.0f32;
            for &s in a.iter().chain(b.iter()) {
                if i % channels == 0 {
                    if self.gain < target {
                        self.gain = (self.gain + step).min(target);
                    } else if self.gain > target {
                        self.gain = (self.gain - step).max(target);
                    }
                }
                let v = s * self.gain;
                out[i] = v;
                if tap_on {
                    acc += v;
                    if i % channels == channels - 1 {
                        self.tap.push(acc / channels as f32);
                        acc = 0.0;
                    }
                }
                i += 1;
            }
            chunk.commit_all();
            self.consumed += n as u64;
        }
        out[n..].fill(0.0);
        if paused && frames < frames_out {
            self.gain = 0.0;
        }
        self.shared.consumed.store(self.consumed, Release);
    }
}
