//! Tiny synth: renders short songs (pads, bass, drums, arpeggios, a lead line),
//! plus test signals (sine, pink noise). Everything is deterministic.

use crate::rng::Rng;
use std::f64::consts::{PI, TAU};
use std::sync::LazyLock;

pub struct Stereo {
    pub sr: u32,
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Stereo {
    pub fn silent(sr: u32, frames: usize) -> Self {
        Stereo { sr, l: vec![0.0; frames], r: vec![0.0; frames] }
    }

    pub fn frames(&self) -> usize {
        self.l.len()
    }

    pub fn peak(&self) -> f32 {
        self.l.iter().chain(&self.r).fold(0.0f32, |m, &v| m.max(v.abs()))
    }

    pub fn gain(&mut self, g: f32) {
        for v in self.l.iter_mut().chain(self.r.iter_mut()) {
            *v *= g;
        }
    }

    /// Scale so the sample peak sits at `peak_db` dBFS.
    pub fn normalize_peak(&mut self, peak_db: f64) {
        let p = self.peak();
        if p > 0.0 {
            self.gain((10f64.powf(peak_db / 20.0) / p as f64) as f32);
        }
    }

    /// Round to the 16-bit grid. 24-bit files then carry 16-bit content (low bits zero),
    /// which keeps hi-res lossless files small; the container is still real 24-bit.
    pub fn quantize_16bit(&mut self) {
        for v in self.l.iter_mut().chain(self.r.iter_mut()) {
            *v = (*v * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0;
        }
    }

    /// Raised-cosine fades at both ends (seconds).
    pub fn fade(&mut self, fade_in: f64, fade_out: f64) {
        let n = self.frames();
        let fi = ((fade_in * self.sr as f64) as usize).min(n);
        let fo = ((fade_out * self.sr as f64) as usize).min(n);
        for i in 0..fi {
            let g = (0.5 - 0.5 * (PI * i as f64 / fi as f64).cos()) as f32;
            self.l[i] *= g;
            self.r[i] *= g;
        }
        for i in 0..fo {
            let g = (0.5 - 0.5 * (PI * i as f64 / fo as f64).cos()) as f32;
            self.l[n - 1 - i] *= g;
            self.r[n - 1 - i] *= g;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inst {
    Pad,
    Pluck,
    Bass,
    Lead,
    Kick,
    Snare,
    Hat,
    Swell,
}

impl Inst {
    fn release(self) -> f64 {
        match self {
            Inst::Pad => 0.8,
            Inst::Pluck => 0.25,
            Inst::Bass => 0.06,
            Inst::Lead => 0.12,
            Inst::Swell => 0.3,
            Inst::Kick | Inst::Snare | Inst::Hat => 0.0,
        }
    }

    /// Plucks and leads go through the delay.
    fn wet(self) -> bool {
        matches!(self, Inst::Pluck | Inst::Lead)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub t: f64,
    pub dur: f64,
    pub freq: f64,
    pub amp: f32,
    pub pan: f32,
    pub inst: Inst,
}

const TABLE: usize = 4096;

/// Band-limited single-cycle tables: pad, pluck, bass, lead.
static TABLES: LazyLock<[Vec<f32>; 4]> = LazyLock::new(|| {
    let build = |harm: &[f32]| {
        let mut t: Vec<f32> = (0..=TABLE)
            .map(|i| {
                let x = TAU * i as f64 / TABLE as f64;
                harm.iter().enumerate().map(|(k, &a)| a as f64 * ((k + 1) as f64 * x).sin()).sum::<f64>() as f32
            })
            .collect();
        let p = t.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        t.iter_mut().for_each(|v| *v /= p);
        t
    };
    [
        build(&[1.0, 0.5, 0.33, 0.25, 0.2, 0.16, 0.12, 0.1]),
        build(&[1.0, 0.5, 0.33, 0.2, 0.12, 0.08]),
        build(&[1.0, 0.45, 0.15]),
        build(&[1.0, 0.1, -0.11, 0.05, 0.04, 0.0, -0.02]),
    ]
});

fn lookup(t: &[f32], phase: f64) -> f32 {
    let x = phase * TABLE as f64;
    let i = x as usize;
    let f = (x - i as f64) as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

fn tonal_env(inst: Inst, t: f64, dur: f64) -> f32 {
    let (attack, body) = match inst {
        Inst::Pad => (0.35, 1.0),
        Inst::Pluck => (0.004, (-t * 4.5).exp()),
        Inst::Bass => (0.006, 0.75 + 0.25 * (-t * 6.0).exp()),
        _ => (0.02, 0.85),
    };
    let att = if t < attack { t / attack } else { 1.0 };
    let rel = if t > dur { (1.0 - (t - dur) / inst.release()).max(0.0) } else { 1.0 };
    (att * body * rel) as f32
}

fn render_note(n: &Note, buf: &mut Stereo, seed: u64) {
    let buf_sr = buf.sr;
    let sr = buf_sr as f64;
    let s0 = (n.t * sr).round() as usize;
    let frames = buf.frames();
    if s0 >= frames {
        return;
    }
    let len = match n.inst {
        Inst::Kick => 0.45,
        Inst::Snare => 0.3,
        Inst::Hat => 0.12,
        _ => n.dur + n.inst.release(),
    };
    let count = ((len * sr) as usize).min(frames - s0);
    let angle = (n.pan as f64 + 1.0) * PI / 4.0;
    let gl = (angle.cos() * std::f64::consts::SQRT_2) as f32 * n.amp;
    let gr = (angle.sin() * std::f64::consts::SQRT_2) as f32 * n.amp;
    let (l, r) = (&mut buf.l[s0..s0 + count], &mut buf.r[s0..s0 + count]);
    let mut rng = Rng::new(seed);
    let mut put = |i: usize, v: f32| {
        l[i] += v * gl;
        r[i] += v * gr;
    };
    match n.inst {
        Inst::Pad | Inst::Pluck | Inst::Bass | Inst::Lead => {
            let table = &TABLES[match n.inst {
                Inst::Pad => 0,
                Inst::Pluck => 1,
                Inst::Bass => 2,
                _ => 3,
            }];
            let inc = n.freq / sr;
            let mut phase = rng.f64();
            for i in 0..count {
                let t = i as f64 / sr;
                let v = lookup(table, phase) * tonal_env(n.inst, t, n.dur);
                put(i, v);
                let step = if n.inst == Inst::Lead { inc * (1.0 + 0.004 * (TAU * 5.5 * t).sin() * (t / 0.4).min(1.0)) } else { inc };
                phase += step;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
            }
        }
        Inst::Kick => {
            let mut noise = Noise::new(seed, buf_sr, false);
            let mut phase = 0.0;
            for i in 0..count {
                let t = i as f64 / sr;
                let f = 48.0 + 120.0 * (-t * 28.0).exp();
                let click = if t < 0.002 { noise.next() * 0.3 } else { 0.0 };
                put(i, (TAU * phase).sin() as f32 * (-t * 9.0).exp() as f32 + click);
                phase += f / sr;
            }
        }
        Inst::Snare => {
            let mut noise = Noise::new(seed, buf_sr, false);
            for i in 0..count {
                let t = i as f64 / sr;
                let hiss = noise.next() * (-t * 18.0).exp() as f32 * 0.5;
                let body = (TAU * 190.0 * t).sin() as f32 * (-t * 28.0).exp() as f32 * 0.6;
                put(i, hiss + body);
            }
        }
        Inst::Hat => {
            let mut noise = Noise::new(seed, buf_sr, true);
            for i in 0..count {
                let t = i as f64 / sr;
                put(i, noise.next() * (-t * 55.0).exp() as f32);
            }
        }
        Inst::Swell => {
            let mut noise = Noise::new(seed, buf_sr, false);
            let a = 1.0 - (-TAU * 434.0 / sr).exp() as f32;
            let mut y = 0.0f32;
            for i in 0..count {
                let t = i as f64 / sr;
                y += a * (noise.next() - y);
                let env = (PI * (t / len).min(1.0)).sin() as f32;
                put(i, y * env * 3.0);
            }
        }
    }
}

/// White noise drawn at about 44.1 kHz and interpolated, so high sample rates get no
/// ultrasonic hiss (real recordings have little there, and it would bloat lossless files).
struct Noise {
    rng: Rng,
    step: f64,
    pos: f64,
    a: f32,
    b: f32,
    last: f32,
    /// First difference: a bright, hi-hat like noise.
    highpass: bool,
}

impl Noise {
    fn new(seed: u64, sr: u32, highpass: bool) -> Self {
        let step = (44_100.0 / sr as f64).min(1.0);
        let mut n = Noise { rng: Rng::new(seed ^ 0x5eed), step, pos: 0.0, a: 0.0, b: 0.0, last: 0.0, highpass };
        n.b = n.draw();
        n
    }

    fn draw(&mut self) -> f32 {
        let w = self.rng.bipolar();
        if self.highpass {
            let v = (w - self.last) * 0.5;
            self.last = w;
            v
        } else {
            w
        }
    }

    fn next(&mut self) -> f32 {
        self.pos += self.step;
        if self.pos >= 1.0 {
            self.pos -= 1.0;
            self.a = self.b;
            self.b = self.draw();
        }
        self.a + (self.b - self.a) * self.pos as f32
    }
}

/// Render notes, then run plucks and leads through a ping-pong delay.
pub fn render(notes: &[Note], sr: u32, frames: usize, bpm: f64, seed: u64) -> Stereo {
    let mut dry = Stereo::silent(sr, frames);
    let mut wet = Stereo::silent(sr, frames);
    for (i, n) in notes.iter().enumerate() {
        let target = if n.inst.wet() { &mut wet } else { &mut dry };
        render_note(n, target, seed ^ (i as u64).wrapping_mul(0x9e37_79b9));
    }
    let d = ((60.0 / bpm * 0.75) * sr as f64) as usize;
    let (fb, mix) = (0.35f32, 0.35f32);
    let mut dl = vec![0.0f32; frames];
    let mut dr = vec![0.0f32; frames];
    for i in 0..frames {
        let (pl, pr) = if i >= d { (dl[i - d], dr[i - d]) } else { (0.0, 0.0) };
        dl[i] = wet.l[i] + fb * pr;
        dr[i] = wet.r[i] + fb * pl;
        dry.l[i] += wet.l[i] + mix * pl;
        dry.r[i] += wet.r[i] + mix * pr;
    }
    dry
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Layers {
    pub pad: bool,
    pub bass: bool,
    pub drums: bool,
    pub arp: bool,
    pub lead: bool,
    pub swell: bool,
}

impl Layers {
    pub const fn new(pad: bool, bass: bool, drums: bool, arp: bool, lead: bool, swell: bool) -> Self {
        Layers { pad, bass, drums, arp, lead, swell }
    }
}

#[derive(Clone, Debug)]
pub struct Song {
    pub seed: u64,
    pub dur: f64,
    pub bpm: f64,
    /// MIDI note of the key root (around 45-57).
    pub root: i32,
    pub minor: bool,
    pub prog: usize,
    pub layers: Layers,
    /// Demo songs: sections that add and drop layers, with intro and outro.
    pub arranged: bool,
    /// Stereo width, 0 (mono) to 1. Narrow mixes keep lossless files small.
    pub width: f32,
}

const PROGS: [[i32; 4]; 6] = [[0, 4, 5, 3], [0, 5, 3, 4], [5, 3, 0, 4], [0, 3, 4, 3], [0, 2, 5, 4], [1, 4, 0, 0]];
const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];

fn degree_note(root: i32, scale: &[i32; 7], deg: i32) -> i32 {
    root + scale[deg.rem_euclid(7) as usize] + 12 * deg.div_euclid(7)
}

fn mfreq(m: i32) -> f64 {
    440.0 * 2f64.powf((m - 69) as f64 / 12.0)
}

fn section_layers(bar: usize, bars: usize) -> Layers {
    const SECTIONS: [Layers; 6] = [
        Layers::new(true, false, false, false, false, true),
        Layers::new(true, true, true, false, false, false),
        Layers::new(true, true, true, true, false, false),
        Layers::new(true, true, true, true, true, false),
        Layers::new(true, false, false, true, false, true),
        Layers::new(true, true, true, true, true, true),
    ];
    if bar + 4 >= bars {
        return Layers::new(true, false, false, true, false, false);
    }
    let sec = bar / 8;
    SECTIONS[if sec < SECTIONS.len() { sec } else { 2 + (sec - 2) % 4 }]
}

pub fn compose(s: &Song) -> Vec<Note> {
    let mut rng = Rng::new(s.seed);
    let beat = 60.0 / s.bpm;
    let bar = 4.0 * beat;
    let bars = (s.dur / bar).ceil() as usize;
    let scale = if s.minor { &MINOR } else { &MAJOR };
    let prog = PROGS[s.prog % PROGS.len()];
    let mut bass_pattern = [false; 8];
    for (i, b) in bass_pattern.iter_mut().enumerate() {
        *b = i == 0 || i == 4 || rng.chance(0.4);
    }
    let arp_rate = *rng.pick(&[2usize, 4]);
    let arp_down = rng.chance(0.3);
    let seventh = rng.chance(0.4);
    let extra_kick = rng.chance(0.5);
    let mut lead_deg: i32 = 7;
    let mut notes = Vec::new();

    for b in 0..bars {
        let t0 = b as f64 * bar;
        let layers = if s.arranged { section_layers(b, bars) } else { s.layers };
        let deg = prog[b % 4];
        let mut chord = vec![deg, deg + 2, deg + 4];
        if seventh {
            chord.push(deg + 6);
        }
        if layers.pad {
            for &c in &chord {
                let f = mfreq(degree_note(s.root + 12, scale, c));
                for (detune, pan) in [(0.9985, -0.6), (1.0015, 0.6)] {
                    notes.push(Note { t: t0, dur: bar + 0.05, freq: f * detune, amp: 0.10, pan, inst: Inst::Pad });
                }
            }
        }
        if layers.bass {
            let f = mfreq(degree_note(s.root - 12, scale, deg));
            for (step, &on) in bass_pattern.iter().enumerate() {
                if on {
                    let t = t0 + step as f64 * beat / 2.0;
                    notes.push(Note { t, dur: beat / 2.0 * 0.9, freq: f, amp: 0.32, pan: 0.0, inst: Inst::Bass });
                }
            }
        }
        if layers.drums {
            let mut hit = |t: f64, amp: f32, pan: f32, inst: Inst| {
                notes.push(Note { t, dur: 0.1, freq: 0.0, amp, pan, inst });
            };
            hit(t0, 0.9, 0.0, Inst::Kick);
            hit(t0 + 2.0 * beat, 0.9, 0.0, Inst::Kick);
            if extra_kick {
                hit(t0 + 3.5 * beat, 0.6, 0.0, Inst::Kick);
            }
            hit(t0 + beat, 0.45, 0.05, Inst::Snare);
            hit(t0 + 3.0 * beat, 0.45, 0.05, Inst::Snare);
            for e in 0..8 {
                let amp = if e % 2 == 0 { 0.13 } else { 0.07 };
                hit(t0 + e as f64 * beat / 2.0, amp, 0.3, Inst::Hat);
            }
        }
        if layers.arp {
            let steps = 4 * arp_rate;
            let step_len = bar / steps as f64;
            for i in 0..steps {
                let k = if arp_down { chord.len() - 1 - i % chord.len() } else { i % chord.len() };
                let octave = if (i / chord.len()) % 2 == 1 { 7 } else { 0 };
                let f = mfreq(degree_note(s.root + 24, scale, chord[k] + octave));
                let pan = if i % 2 == 0 { -0.35 } else { 0.35 };
                notes.push(Note { t: t0 + i as f64 * step_len, dur: step_len * 0.9, freq: f, amp: 0.09, pan, inst: Inst::Pluck });
            }
        }
        if layers.lead {
            let mut beat_i = 0;
            while beat_i < 4 {
                let len = if rng.chance(0.3) && beat_i < 3 { 2 } else { 1 };
                if rng.chance(0.75) {
                    lead_deg = if beat_i == 0 { deg + 7 } else { (lead_deg + *rng.pick(&[-2, -1, 1, 1, 2])).clamp(4, 14) };
                    let f = mfreq(degree_note(s.root + 12, scale, lead_deg));
                    notes.push(Note {
                        t: t0 + beat_i as f64 * beat,
                        dur: len as f64 * beat * 0.92,
                        freq: f,
                        amp: 0.13,
                        pan: 0.1,
                        inst: Inst::Lead,
                    });
                }
                beat_i += len;
            }
        }
        if layers.swell && b % 4 == 0 {
            notes.push(Note { t: t0, dur: 2.0 * bar, freq: 0.0, amp: 0.07, pan: 0.0, inst: Inst::Swell });
        }
    }
    notes.retain(|n| n.t < s.dur);
    for n in &mut notes {
        n.pan *= s.width;
    }
    notes
}

/// Render a song and set its sample peak.
pub fn render_song(s: &Song, sr: u32, peak_db: f64) -> Stereo {
    let notes = compose(s);
    let frames = (s.dur * sr as f64).round() as usize;
    let mut out = render(&notes, sr, frames, s.bpm, s.seed);
    if s.arranged {
        out.fade(0.05, 6.0);
    } else {
        out.fade(0.005, 0.04);
    }
    out.normalize_peak(peak_db);
    out.quantize_16bit();
    if s.width == 0.0 {
        // Exact dual mono: lossless codecs then store the side channel for almost nothing.
        out.r.copy_from_slice(&out.l);
    }
    out
}

/// Stereo pink noise (Paul Kellet's filter), uncorrelated channels.
pub fn pink_noise(sr: u32, frames: usize, seed: u64) -> Stereo {
    let mut out = Stereo::silent(sr, frames);
    for (ch, buf) in [&mut out.l, &mut out.r].into_iter().enumerate() {
        let mut rng = Rng::new(seed.wrapping_add(ch as u64 * 7919));
        let mut b = [0.0f32; 7];
        for v in buf.iter_mut() {
            let w = rng.bipolar();
            b[0] = 0.99886 * b[0] + w * 0.0555179;
            b[1] = 0.99332 * b[1] + w * 0.0750759;
            b[2] = 0.96900 * b[2] + w * 0.153_852;
            b[3] = 0.86650 * b[3] + w * 0.3104856;
            b[4] = 0.55000 * b[4] + w * 0.5329522;
            b[5] = -0.7616 * b[5] - w * 0.0168980;
            *v = (b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362) * 0.11;
            b[6] = w * 0.115926;
        }
    }
    out.fade(0.01, 0.01);
    out
}

/// Same sine on both channels.
pub fn sine(sr: u32, frames: usize, freq: f64, amp: f32) -> Stereo {
    let l: Vec<f32> = (0..frames).map(|n| amp * (TAU * freq * n as f64 / sr as f64).sin() as f32).collect();
    let mut s = Stereo { sr, r: l.clone(), l };
    s.fade(0.01, 0.01);
    s
}
