//! Engine tests with a capture output (no sound card needed).
//! Needs `ffmpeg` with flac, alac, libmp3lame, aac, libvorbis and libopus.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel as cb;
use parking_lot::Mutex;

use super::*;
use crate::decode::FileStream;
use crate::model::{SourceId, TrackRef};
use crate::source::{Caps, Item, Lyrics, Page, SearchHits, Source};

struct Files(SourceId);
impl Source for Files {
    fn id(&self) -> &SourceId {
        &self.0
    }
    fn caps(&self) -> Caps {
        Caps::SEEK
    }
    fn children(&self, _: &str, _: Page) -> Result<Vec<Item>> {
        Ok(vec![])
    }
    fn search(&self, _: &str, _: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }
    fn track(&self, r: &TrackRef) -> Result<Track> {
        Ok(Track { r: Some(r.clone()), ..Default::default() })
    }
    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        Ok(Box::new(FileStream::open(Path::new(&r.id))?))
    }
    fn art(&self, _: &crate::model::ArtRef) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn lyrics(&self, _: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}

enum Ctl {
    Fmt(OutputFormat, cb::Sender<()>),
    Active(bool),
}

/// Pulls real samples only (never pads with silence), as fast as they come.
struct Capture {
    tx: cb::Sender<Ctl>,
}

impl Capture {
    fn start(mut r: Renderer, out: Arc<Mutex<Vec<f32>>>) -> Self {
        let (tx, rx) = cb::unbounded::<Ctl>();
        std::thread::spawn(move || {
            let mut fmt: Option<OutputFormat> = None;
            let mut active = false;
            let mut buf = vec![0f32; 4096];
            loop {
                loop {
                    match rx.try_recv() {
                        Ok(Ctl::Fmt(f, ack)) => {
                            fmt = Some(f);
                            let _ = ack.send(());
                        }
                        Ok(Ctl::Active(a)) => active = a,
                        Err(cb::TryRecvError::Empty) => break,
                        Err(cb::TryRecvError::Disconnected) => return,
                    }
                }
                match fmt {
                    Some(f) if active => {
                        let ch = f.channels as usize;
                        let n = (r.available() / ch * ch).min(buf.len() / ch * ch);
                        if n == 0 {
                            std::thread::sleep(Duration::from_micros(200));
                            continue;
                        }
                        r.render(&mut buf[..n], ch, f.rate);
                        out.lock().extend_from_slice(&buf[..n]);
                    }
                    _ => std::thread::sleep(Duration::from_micros(200)),
                }
            }
        });
        Self { tx }
    }
}

impl Output for Capture {
    fn configure(&self, fmt: OutputFormat) -> Result<()> {
        let (a, b) = cb::bounded(1);
        self.tx.send(Ctl::Fmt(fmt, a))?;
        b.recv()?;
        Ok(())
    }
    fn set_active(&self, on: bool) {
        let _ = self.tx.send(Ctl::Active(on));
    }
}

const FREQ: f64 = 441.0;
const AMP: f64 = 0.25;

/// Write one continuous stereo sine split into pieces, encode each piece separately.
fn make_album(dir: &Path, ext: &str, codec: &[&str], rate: u32, pieces: &[usize]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut n0 = 0usize;
    for (i, &len) in pieces.iter().enumerate() {
        let mut raw = Vec::with_capacity(len * 8);
        for n in n0..n0 + len {
            let v = (AMP * (2.0 * std::f64::consts::PI * FREQ * n as f64 / rate as f64).sin()) as f32;
            raw.extend_from_slice(&v.to_le_bytes());
            raw.extend_from_slice(&v.to_le_bytes());
        }
        n0 += len;
        let rawp = dir.join(format!("{ext}-{i}.raw"));
        std::fs::write(&rawp, &raw).unwrap();
        let dst = dir.join(format!("{ext}-{i}.{}", codec[0]));
        let st = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "f32le", "-ar", &rate.to_string(), "-ac", "2", "-i"])
            .arg(&rawp)
            .args(&codec[1..])
            .arg(&dst)
            .status()
            .expect("ffmpeg is needed for this test");
        assert!(st.success(), "ffmpeg failed for {ext}");
        out.push(dst);
    }
    out
}

/// Worst per-file SNR (dB) of the left channel against the ideal continuous sine,
/// measured `edge` samples away from file edges (lossy encoders smear the edges of
/// each separately encoded file). Any gap or extra sample shifts the phase of every
/// later file and ruins its SNR.
fn worst_snr(s: &[f32], rate: u32, pieces: &[usize], edge: usize) -> f64 {
    let l: Vec<f64> = s.iter().step_by(2).map(|v| *v as f64).collect();
    let mut start = 0usize;
    let mut worst = f64::INFINITY;
    for (i, &len) in pieces.iter().enumerate() {
        // Skip the 10 ms fade-in at the very start.
        let from = start + edge.max(if i == 0 { (rate / 50) as usize } else { 0 });
        let to = (start + len - edge).min(l.len());
        let mut noise = 0.0;
        for (n, x) in l.iter().enumerate().take(to).skip(from) {
            let ideal = AMP * (2.0 * std::f64::consts::PI * FREQ * n as f64 / rate as f64).sin();
            noise += (x - ideal).powi(2);
        }
        let noise = noise / (to.saturating_sub(from)).max(1) as f64;
        let snr = 10.0 * ((AMP * AMP / 2.0) / noise.max(1e-20)).log10();
        worst = worst.min(snr);
        start += len;
    }
    worst
}

fn play_all(files: &[PathBuf]) -> Vec<f32> {
    play_with(files, GainCfg { mode: RgMode::Off, ..Default::default() }, crate::model::Gain::default())
}

fn play_with(files: &[PathBuf], cfg: GainCfg, gain: crate::model::Gain) -> Vec<f32> {
    let src: Arc<dyn Source> = Arc::new(Files(Arc::from("files")));
    let sources = Sources::default();
    sources.add(src.clone());
    let cap = Arc::new(Mutex::new(Vec::new()));
    let cap2 = cap.clone();
    let engine = Engine::start_with(Arc::new(sources), 1.0, cfg, move |r, _| Ok(Box::new(Capture::start(r, cap2)))).unwrap();
    let tracks: Vec<Arc<Track>> = files
        .iter()
        .map(|p| Arc::new(Track { r: Some(TrackRef::new(src.id(), p.to_string_lossy())), gain, ..Default::default() }))
        .collect();
    let first = engine.queue.lock().replace(tracks, None, 0).unwrap();
    engine.send(Cmd::PlayItem(first));
    let t0 = Instant::now();
    let mut started = false;
    loop {
        match engine.events.recv_timeout(Duration::from_secs(20)) {
            Ok(Event::State(PlayState::Playing)) => started = true,
            Ok(Event::State(PlayState::Stopped)) if started => break,
            Ok(Event::Error(e)) => panic!("engine error: {e}"),
            Ok(_) => {}
            Err(_) => panic!("timeout"),
        }
        assert!(t0.elapsed() < Duration::from_secs(60));
    }
    drop(engine);

    cap.lock().clone()
}

#[test]
fn gapless_every_format() {
    let dir = tempfile::tempdir().unwrap();
    // Odd lengths so boundaries never line up with codec frames.
    let p44 = [44_100 + 17, 30_011, 52_919, 40_000];
    let p48 = [48_000 + 23, 31_337, 50_003, 44_444];
    // Minimum per-file SNR in dB: 16-bit lossless is ~86 dB at -12 dBFS.
    type Case<'a> = (&'a str, &'a [&'a str], u32, &'a [usize], f64);
    let cases: &[Case] = &[
        ("wav", &["wav", "-c:a", "pcm_s16le"], 44_100, &p44, 75.0),
        ("flac", &["flac", "-c:a", "flac"], 44_100, &p44, 75.0),
        ("alac", &["m4a", "-c:a", "alac"], 44_100, &p44, 75.0),
        ("mp3", &["mp3", "-c:a", "libmp3lame", "-b:a", "256k"], 44_100, &p44, 30.0),
        // Sample-exact edit list on every ffmpeg version (old ones default to ms).
        ("aac", &["m4a", "-c:a", "aac", "-b:a", "256k", "-movie_timescale", "44100"], 44_100, &p44, 30.0),
        ("vorbis", &["ogg", "-c:a", "libvorbis", "-q:a", "6"], 44_100, &p44, 30.0),
        ("opus", &["opus", "-c:a", "libopus", "-b:a", "192k"], 48_000, &p48, 30.0),
    ];
    let mut report = Vec::new();
    let mut failed = false;
    for (name, codec, rate, pieces, min_snr) in cases {
        let files = make_album(dir.path(), name, codec, *rate, pieces);
        let s = play_all(&files);
        let expected: usize = pieces.iter().sum();
        let frames = s.len() / 2;
        let edge = if *min_snr > 60.0 { 0 } else { 2048 };
        let snr = worst_snr(&s, *rate, pieces, edge);
        let ok = frames == expected && snr >= *min_snr;
        failed |= !ok;
        report.push(format!(
            "{name:7} {rate} Hz  frames {frames} (expected {expected}, diff {:+})  worst file SNR {snr:.1} dB (min {min_snr})  {}",
            frames as i64 - expected as i64,
            if ok { "OK" } else { "FAIL" }
        ));
    }
    println!("{}", report.join("\n"));
    assert!(!failed, "\n{}", report.join("\n"));
}

#[test]
fn seek_lands_on_target() {
    let dir = tempfile::tempdir().unwrap();
    let files = make_album(dir.path(), "seek", &["flac", "-c:a", "flac"], 44_100, &[44_100 * 6]);
    let mut s = FileStream::open(&files[0]).unwrap();
    let pos = s.seek(Duration::from_millis(2500)).unwrap();
    assert_eq!(pos, 110_250);
    let mut buf = Vec::new();
    s.read(&mut buf).unwrap();
    // First sample after the seek continues the sine at frame 110250.
    let want = (AMP * (2.0 * std::f64::consts::PI * FREQ * 110_250.0 / 44_100.0).sin()) as f32;
    assert!((buf[0] - want).abs() < 0.001, "{} vs {}", buf[0], want);
}

#[test]
fn replaygain_scales_the_samples() {
    let dir = tempfile::tempdir().unwrap();
    let files = make_album(dir.path(), "rg", &["flac", "-c:a", "flac"], 44_100, &[44_100]);
    let peak = |v: &[f32]| v[4410..].iter().fold(0f32, |a, b| a.max(b.abs()));
    let plain = peak(&play_all(&files));
    let tagged = crate::model::Gain { track_db: Some(-6.0), track_peak: Some(0.25), ..Default::default() };
    let cfg = GainCfg { mode: RgMode::Track, ..Default::default() };
    let quiet = peak(&play_with(&files, cfg, tagged));
    let ratio_db = 20.0 * (quiet / plain).log10();
    assert!((ratio_db + 6.0).abs() < 0.05, "applied {ratio_db:.2} dB");
    // Preamp +3 dB on top.
    let loud = peak(&play_with(&files, GainCfg { preamp_db: 3.0, ..cfg }, tagged));
    let ratio_db = 20.0 * (loud / plain).log10();
    assert!((ratio_db + 3.0).abs() < 0.05, "applied {ratio_db:.2} dB");
}

/// Engine with a capture output and a queue of `files`, album names per file.
fn engine_with(files: &[PathBuf], albums: &[&str]) -> (Engine, Arc<Mutex<Vec<f32>>>, u64) {
    let src: Arc<dyn Source> = Arc::new(Files(Arc::from("files")));
    let sources = Sources::default();
    sources.add(src.clone());
    let cap = Arc::new(Mutex::new(Vec::new()));
    let cap2 = cap.clone();
    let cfg = GainCfg { mode: RgMode::Off, ..Default::default() };
    let engine = Engine::start_with(Arc::new(sources), 1.0, cfg, move |r, _| Ok(Box::new(Capture::start(r, cap2)))).unwrap();
    let tracks: Vec<Arc<Track>> = files
        .iter()
        .zip(albums)
        .map(|(p, a)| Arc::new(Track { r: Some(TrackRef::new(src.id(), p.to_string_lossy())), album: a.to_string(), ..Default::default() }))
        .collect();
    let first = engine.queue.lock().replace(tracks, None, 0).unwrap();
    (engine, cap, first)
}

fn wait_stopped(engine: &Engine) {
    let t0 = Instant::now();
    let mut started = false;
    loop {
        match engine.events.recv_timeout(Duration::from_secs(20)) {
            Ok(Event::State(PlayState::Playing)) => started = true,
            Ok(Event::State(PlayState::Stopped)) if started => return,
            Ok(Event::Error(e)) => panic!("engine error: {e}"),
            Ok(_) => {}
            Err(_) => panic!("timeout"),
        }
        assert!(t0.elapsed() < Duration::from_secs(60));
    }
}

#[test]
fn sleep_timer_stops_after_song_or_album() {
    let dir = tempfile::tempdir().unwrap();
    let pieces = [20_000, 21_000, 22_000];
    let files = make_album(dir.path(), "st", &["flac", "-c:a", "flac"], 44_100, &pieces);

    let (engine, cap, first) = engine_with(&files, &["A", "A", "A"]);
    engine.send(Cmd::StopAfter(Some(StopAfter::Track)));
    engine.send(Cmd::PlayItem(first));
    wait_stopped(&engine);
    drop(engine);
    assert_eq!(cap.lock().len() / 2, 20_000, "end of song: only the first song plays");

    let (engine, cap, first) = engine_with(&files, &["A", "A", "B"]);
    engine.send(Cmd::StopAfter(Some(StopAfter::Album)));
    engine.send(Cmd::PlayItem(first));
    wait_stopped(&engine);
    drop(engine);
    assert_eq!(cap.lock().len() / 2, 41_000, "end of album: both songs of album A, not B");
}

#[test]
fn cue_waits_paused_at_the_position() {
    let dir = tempfile::tempdir().unwrap();
    let files = make_album(dir.path(), "cue", &["flac", "-c:a", "flac"], 44_100, &[44_100 * 4]);
    let (engine, cap, first) = engine_with(&files, &["A"]);
    engine.send(Cmd::Cue(first, Duration::from_millis(2500)));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(engine.snapshot().state, PlayState::Paused);
    assert!(cap.lock().iter().all(|s| *s == 0.0), "nothing may be heard while cued");
    let pos = engine.position().as_secs_f64();
    assert!((pos - 2.5).abs() < 0.01, "position {pos}");
}
