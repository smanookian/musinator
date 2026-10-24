//! Audio masters: every distinct piece of audio is synthesized and encoded once, cached in
//! `<out>/.masters/`, and then copied into the library. Masters do not depend on the seed.

use crate::ffmpeg::{self, Loudness, Pcm};
use crate::formats::Fmt;
use crate::rng::Rng;
use crate::synth::{self, Layers, Song};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bump when synthesis or encoding changes, so old caches are not reused.
const VERSION: &str = "v1";

/// Length of each library master in seconds; the library uses these as track lengths.
/// Mostly short to keep the library small, a few up to 12 s.
const LIB_DURATIONS: [f64; 24] = [
    3.0, 3.2, 3.4, 3.6, 3.8, 4.0, 4.2, 4.4, 4.6, 4.8, 5.0, 3.1, 3.5, 3.9, 4.3, 4.7, 5.2, 5.6, 6.0,
    6.5, 7.0, 8.0, 10.0, 12.0,
];
const LIB_LAYERS: [Layers; 8] = [
    Layers::new(true, false, false, false, false, false),
    Layers::new(true, false, false, true, false, false),
    Layers::new(false, true, true, false, false, false),
    Layers::new(true, true, true, false, false, false),
    Layers::new(true, true, true, true, true, false),
    Layers::new(true, false, false, false, true, false),
    Layers::new(false, false, true, false, false, true),
    Layers::new(false, true, false, true, true, false),
];
/// Sample peaks (dBFS) so masters cover a wide loudness range.
const LIB_PEAKS: [f64; 8] = [-0.3, -1.0, -2.0, -3.5, -6.0, -9.0, -12.0, -16.0];

pub const DEMO_ALBUMS: [(&str, u32, Fmt, &str); 2] = [
    ("Night Signals", 2026, Fmt::Flac24_96, "Synthwave"),
    ("Neon Harbor", 2025, Fmt::Mp3Cbr320, "Electronic"),
];
pub const DEMO_TRACKS: usize = 8;

pub const GAPLESS_FMTS: [(Fmt, &str); 7] = [
    (Fmt::Flac16, "FLAC"),
    (Fmt::Mp3V0, "MP3"),
    (Fmt::Aac, "AAC"),
    (Fmt::Alac16, "ALAC"),
    (Fmt::Vorbis, "Vorbis"),
    (Fmt::Opus, "Opus"),
    (Fmt::Wav16, "WAV"),
];
pub const GAPLESS_FREQ: f64 = 440.0;
pub const GAPLESS_DBFS: f64 = -12.0;
pub const GAPLESS_SECONDS: usize = 20;
/// Where the sine is cut, as fractions of the whole; chosen so cuts fall mid-frame for every codec.
const GAPLESS_CUTS: [f64; 3] = [0.2173, 0.4951, 0.7389];

pub const LOUDNESS_FMT: Fmt = Fmt::Flac16;
pub const LOUDNESS_SECONDS: f64 = 20.0;
/// (signal, target LUFS)
pub const LOUDNESS_TRACKS: [(Signal, f64); 6] = [
    (Signal::Tone, -10.0),
    (Signal::Pink, -14.0),
    (Signal::Pink, -18.0),
    (Signal::Pink, -23.0),
    (Signal::Tone, -23.0),
    (Signal::Pink, -30.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Tone,
    Pink,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Signal::Tone => "1 kHz tone",
            Signal::Pink => "pink noise",
        }
    }
}

pub struct Master {
    pub dur: f64,
    pub loud: Loudness,
}

pub struct GaplessAlbum {
    pub fmt: Fmt,
    pub name: &'static str,
    pub rate: u32,
    pub total: usize,
    /// Sample count of each piece.
    pub pieces: Vec<usize>,
}

pub struct Masters {
    dir: PathBuf,
    pub lib: Vec<Master>,
    /// `demo[album][track]`
    pub demo: Vec<Vec<Master>>,
    pub gapless: Vec<GaplessAlbum>,
    /// Loudness of the encoded loudness test tracks.
    pub loudness: Vec<Loudness>,
}

impl Masters {
    pub fn lib_path(&self, fmt: Fmt, idx: usize) -> PathBuf {
        lib_path(&self.dir, fmt, idx)
    }
    pub fn demo_path(&self, album: usize, track: usize) -> PathBuf {
        demo_path(&self.dir, album, track)
    }
    pub fn gapless_path(&self, fmt: Fmt, piece: usize) -> PathBuf {
        gapless_path(&self.dir, fmt, piece)
    }
    pub fn loudness_path(&self, idx: usize) -> PathBuf {
        loudness_path(&self.dir, idx)
    }
}

fn lib_path(dir: &Path, fmt: Fmt, idx: usize) -> PathBuf {
    dir.join(format!("lib/{idx:02}.{}.{}", fmt.id(), fmt.ext()))
}
fn demo_path(dir: &Path, album: usize, track: usize) -> PathBuf {
    let fmt = DEMO_ALBUMS[album].2;
    dir.join(format!(
        "demo/{album}-{track:02}.{}.{}",
        fmt.id(),
        fmt.ext()
    ))
}
fn gapless_path(dir: &Path, fmt: Fmt, piece: usize) -> PathBuf {
    dir.join(format!("gapless/{}-{piece}.{}", fmt.id(), fmt.ext()))
}
fn loudness_path(dir: &Path, idx: usize) -> PathBuf {
    dir.join(format!(
        "loudness/{idx}.{}.{}",
        LOUDNESS_FMT.id(),
        LOUDNESS_FMT.ext()
    ))
}

fn lib_song(idx: usize) -> (Song, f64) {
    let mut rng = Rng::new(1000 + idx as u64);
    let song = Song {
        seed: rng.next_u64(),
        dur: LIB_DURATIONS[idx],
        bpm: rng.range_f(80.0, 140.0),
        root: rng.range(45, 57) as i32,
        minor: rng.chance(0.5),
        prog: rng.below(6),
        layers: LIB_LAYERS[idx % LIB_LAYERS.len()],
        arranged: false,
        // Most masters are exact dual mono to keep lossless files small; every sixth is stereo.
        width: if idx % 6 == 0 { 0.6 } else { 0.0 },
    };
    // Shift by idx / 8 so each layer mix gets several loudness levels.
    (song, LIB_PEAKS[(idx * 3 + 1 + idx / 8) % LIB_PEAKS.len()])
}

fn demo_song(album: usize, track: usize) -> Song {
    let mut rng = Rng::new(5000 + (album * 100 + track) as u64);
    Song {
        seed: rng.next_u64(),
        dur: rng.range_f(120.0, 180.0).round(),
        bpm: rng.range_f(84.0, 128.0).round(),
        root: rng.range(45, 55) as i32,
        minor: rng.chance(0.6),
        prog: rng.below(6),
        layers: Layers::default(),
        arranged: true,
        width: 1.0,
    }
}

pub fn gapless_album(fmt: Fmt, name: &'static str) -> GaplessAlbum {
    let rate = if fmt == Fmt::Opus { 48_000 } else { 44_100 };
    let total = GAPLESS_SECONDS * rate as usize;
    let mut cuts: Vec<usize> = GAPLESS_CUTS
        .iter()
        .map(|f| (f * total as f64) as usize)
        .collect();
    cuts.insert(0, 0);
    cuts.push(total);
    let pieces = cuts.windows(2).map(|w| w[1] - w[0]).collect();
    GaplessAlbum {
        fmt,
        name,
        rate,
        total,
        pieces,
    }
}

/// Sample `n` of the gapless sine, as 16-bit PCM.
pub fn gapless_sample(n: usize, rate: u32) -> i16 {
    let amp = 10f64.powf(GAPLESS_DBFS / 20.0);
    (amp * 32767.0 * (std::f64::consts::TAU * GAPLESS_FREQ * n as f64 / rate as f64).sin()).round()
        as i16
}

enum Job {
    Lib { idx: usize, rate: u32 },
    Demo { album: usize, track: usize },
    Gapless(usize),
    Loudness(usize),
}

impl Job {
    /// Rough cost, so long jobs start first.
    fn cost(&self) -> f64 {
        match self {
            Job::Lib { idx, rate } => LIB_DURATIONS[*idx] * *rate as f64 / 44_100.0,
            Job::Demo { album, track } => demo_song(*album, *track).dur * 6.0,
            Job::Gapless(_) => 80.0,
            Job::Loudness(_) => 40.0,
        }
    }
}

type Index = BTreeMap<String, Loudness>;

/// Build every master that is not cached yet, then return their info.
pub fn build(dir: &Path) -> Result<Masters> {
    let dir = dir.join(VERSION);
    for sub in ["lib", "demo", "gapless", "loudness"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    let index_path = dir.join("index.json");
    let index: Index = std::fs::read(&index_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let mut rates: Vec<u32> = Fmt::ALL.iter().map(|f| f.rate()).collect();
    rates.sort();
    rates.dedup();
    let mut jobs = Vec::new();
    for idx in 0..LIB_DURATIONS.len() {
        jobs.extend(rates.iter().map(|&rate| Job::Lib { idx, rate }));
    }
    for album in 0..DEMO_ALBUMS.len() {
        jobs.extend((0..DEMO_TRACKS).map(|track| Job::Demo { album, track }));
    }
    jobs.extend((0..GAPLESS_FMTS.len()).map(Job::Gapless));
    jobs.extend((0..LOUDNESS_TRACKS.len()).map(Job::Loudness));
    jobs.sort_by(|a, b| b.cost().total_cmp(&a.cost()));

    let results: Vec<Option<(String, Loudness)>> = jobs
        .par_iter()
        .map(|job| run_job(&dir, job, &index))
        .collect::<Result<_>>()?;
    let mut index = index;
    let built = results.iter().filter(|r| r.is_some()).count();
    index.extend(results.into_iter().flatten());
    if built > 0 {
        std::fs::write(&index_path, serde_json::to_vec_pretty(&index)?)?;
    }

    let get = |key: &str| {
        index
            .get(key)
            .copied()
            .with_context(|| format!("master {key} has no loudness"))
    };
    let lib = (0..LIB_DURATIONS.len())
        .map(|i| {
            Ok(Master {
                dur: LIB_DURATIONS[i],
                loud: get(&format!("lib-{i}"))?,
            })
        })
        .collect::<Result<_>>()?;
    let demo = (0..DEMO_ALBUMS.len())
        .map(|a| {
            (0..DEMO_TRACKS)
                .map(|t| {
                    Ok(Master {
                        dur: demo_song(a, t).dur,
                        loud: get(&format!("demo-{a}-{t}"))?,
                    })
                })
                .collect::<Result<_>>()
        })
        .collect::<Result<_>>()?;
    let gapless = GAPLESS_FMTS
        .iter()
        .map(|&(f, n)| gapless_album(f, n))
        .collect();
    let loudness = (0..LOUDNESS_TRACKS.len())
        .map(|i| get(&format!("loudness-{i}")))
        .collect::<Result<_>>()?;
    Ok(Masters {
        dir,
        lib,
        demo,
        gapless,
        loudness,
    })
}

/// Returns a new index entry when the job measured something.
fn run_job(dir: &Path, job: &Job, index: &Index) -> Result<Option<(String, Loudness)>> {
    match *job {
        Job::Lib { idx, rate } => {
            let fmts: Vec<Fmt> = Fmt::ALL.into_iter().filter(|f| f.rate() == rate).collect();
            let paths: Vec<PathBuf> = fmts.iter().map(|&f| lib_path(dir, f, idx)).collect();
            // The 44.1 kHz render is the one that gets measured.
            let key = format!("lib-{idx}");
            let measure = rate == 44_100;
            if paths.iter().all(|p| p.exists()) && (!measure || index.contains_key(&key)) {
                return Ok(None);
            }
            let (song, peak) = lib_song(idx);
            let audio = synth::render_song(&song, rate, peak);
            let outputs: Vec<(Fmt, &Path)> = fmts
                .iter()
                .copied()
                .zip(paths.iter().map(PathBuf::as_path))
                .collect();
            let loud = ffmpeg::encode(&Pcm::F32(&audio), &outputs, measure)?;
            Ok(loud.map(|l| (key, l)))
        }
        Job::Demo { album, track } => {
            let key = format!("demo-{album}-{track}");
            let path = demo_path(dir, album, track);
            if path.exists() && index.contains_key(&key) {
                return Ok(None);
            }
            let fmt = DEMO_ALBUMS[album].2;
            let audio = synth::render_song(&demo_song(album, track), fmt.rate(), -1.0);
            let loud = ffmpeg::encode(&Pcm::F32(&audio), &[(fmt, &path)], true)?;
            Ok(loud.map(|l| (key, l)))
        }
        Job::Gapless(i) => {
            let (fmt, name) = GAPLESS_FMTS[i];
            let album = gapless_album(fmt, name);
            let paths: Vec<PathBuf> = (0..album.pieces.len())
                .map(|p| gapless_path(dir, fmt, p))
                .collect();
            if paths.iter().all(|p| p.exists()) {
                return Ok(None);
            }
            // Render the whole sine once, then encode each piece on its own, like a real album.
            let full: Vec<i16> = (0..album.total)
                .map(|n| gapless_sample(n, album.rate))
                .collect();
            let mut start = 0;
            for (len, path) in album.pieces.iter().zip(&paths) {
                let pcm = Pcm::S16Mono {
                    sr: album.rate,
                    samples: &full[start..start + len],
                };
                ffmpeg::encode(&pcm, &[(fmt, path)], false)?;
                start += len;
            }
            Ok(None)
        }
        Job::Loudness(i) => {
            let key = format!("loudness-{i}");
            let path = loudness_path(dir, i);
            if path.exists() && index.contains_key(&key) {
                return Ok(None);
            }
            let (signal, target) = LOUDNESS_TRACKS[i];
            let sr = LOUDNESS_FMT.rate();
            let frames = (LOUDNESS_SECONDS * sr as f64) as usize;
            let mut audio = match signal {
                Signal::Tone => synth::sine(sr, frames, 1000.0, 0.1),
                Signal::Pink => synth::pink_noise(sr, frames, 77 + i as u64),
            };
            let raw = ffmpeg::encode(&Pcm::F32(&audio), &[], true)?.context("no loudness")?;
            audio.gain(10f64.powf((target - raw.lufs) / 20.0) as f32);
            anyhow::ensure!(
                audio.peak() < 1.0,
                "loudness track {i} would clip at {target} LUFS"
            );
            ffmpeg::encode(&Pcm::F32(&audio), &[(LOUDNESS_FMT, &path)], false)?;
            Ok(Some((key, ffmpeg::measure_file(&path)?)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gapless_pieces_cover_the_whole_sine() {
        for (fmt, name) in GAPLESS_FMTS {
            let g = gapless_album(fmt, name);
            assert_eq!(g.pieces.iter().sum::<usize>(), g.total);
            assert_eq!(g.total, GAPLESS_SECONDS * g.rate as usize);
        }
    }
}
