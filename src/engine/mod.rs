//! The player engine.
//!
//! One thread decodes ahead into a lock-free ring (~0.5 s). The output (PipeWire)
//! pulls from the ring in its real-time callback. Track changes inside the ring
//! are tracked by sample index, so "now playing" switches when you *hear* the
//! boundary. Same-format tracks flow straight into each other (gapless).

pub mod gain;
pub mod pw;
pub mod queue;
pub mod render;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering::*;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel as cb;
use parking_lot::Mutex;

use crate::model::{AudioFormat, GainOrigin, Track};
use crate::source::{Sources, Stream};
pub use gain::{GainCfg, RgMode};
use queue::Queue;
use render::{Renderer, Shared, Tap};

/// Ring size in samples: > 1 s of 192 kHz stereo.
const RING: usize = 1 << 19;
/// How far ahead to decode.
const AHEAD: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    pub rate: u32,
    pub channels: u16,
}

/// An audio output that pulls from the shared [`Renderer`].
pub trait Output: Send {
    /// (Re)open the device at this format. The ring is flushed or drained first.
    fn configure(&self, fmt: OutputFormat) -> Result<()>;
    fn set_active(&self, on: bool);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl PlayState {
    pub fn label(self) -> &'static str {
        match self {
            PlayState::Stopped => "stopped",
            PlayState::Playing => "playing",
            PlayState::Paused => "paused",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopAfter {
    Track,
    Album,
}

pub enum Cmd {
    /// Play this queue item from the start.
    PlayItem(u64),
    Toggle,
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    SeekTo(Duration),
    SeekBy(i64),
    /// 0..=1
    Volume(f32),
    Gain(GainCfg),
    /// Reconnect the audio output (e.g. after PipeWire's allowed rates changed).
    ReopenOutput,
    /// Load this item paused at a position (resume after restart).
    Cue(u64, Duration),
    /// Stop when the current track or album ends (sleep timer). None cancels.
    StopAfter(Option<StopAfter>),
    Quit,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// The audible track changed (None = nothing playing).
    Track(Option<Arc<Track>>),
    State(PlayState),
    Seeked(Duration),
    Volume,
    Error(String),
    /// A track stopped being current; `listened` is real listening time.
    Finished {
        track: Arc<Track>,
        listened: Duration,
    },
}

/// What is audible right now. Cheap to clone.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub state: PlayState,
    pub track: Option<Arc<Track>>,
    pub item_id: Option<u64>,
    /// Real decoded format of the audible track.
    pub format: Option<AudioFormat>,
    /// ReplayGain applied to it, dB.
    pub gain_db: Option<f32>,
    pub gain_origin: GainOrigin,
    /// User volume 0..=1.
    pub volume: f32,
}

#[derive(Clone, Copy, Default)]
struct Audible {
    at: u64,
    pos_frames: u64,
    channels: u16,
    rate: u32,
}

pub struct Engine {
    tx: cb::Sender<Cmd>,
    pub events: cb::Receiver<Event>,
    pub queue: Arc<Mutex<Queue>>,
    pub tap: Arc<Tap>,
    shared: Arc<Shared>,
    snap: Arc<Mutex<Snapshot>>,
    audible: Arc<Mutex<Audible>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start with the PipeWire output.
    pub fn start(sources: Arc<Sources>, volume: f32, gain: GainCfg) -> Result<Engine> {
        Self::start_with(sources, volume, gain, |r, s| Ok(Box::new(pw::PwOutput::start(r, s)?)))
    }

    /// Start with any output (tests use a capture output).
    pub fn start_with(
        sources: Arc<Sources>,
        volume: f32,
        gain: GainCfg,
        make_output: impl FnOnce(Renderer, Arc<Shared>) -> Result<Box<dyn Output>>,
    ) -> Result<Engine> {
        let (prod, cons) = rtrb::RingBuffer::<f32>::new(RING);
        let shared = Shared::new();
        let tap = Tap::new();
        shared.set_volume(gain::volume_curve(volume));
        let output = make_output(Renderer::new(cons, shared.clone(), tap.clone()), shared.clone())?;
        let (tx, rx) = cb::unbounded();
        let (ev_tx, events) = cb::unbounded();
        let queue = Arc::new(Mutex::new(Queue::new()));
        let snap = Arc::new(Mutex::new(Snapshot { volume, ..Default::default() }));
        let audible = Arc::new(Mutex::new(Audible::default()));
        let mut w = Worker {
            rx,
            ev: ev_tx,
            sources,
            queue: queue.clone(),
            shared: shared.clone(),
            tap: tap.clone(),
            snap: snap.clone(),
            audible: audible.clone(),
            output,
            out_fmt: None,
            active: false,
            prod,
            pushed: 0,
            dec: None,
            waiting: None,
            markers: VecDeque::new(),
            pending: Vec::new(),
            pending_off: 0,
            scratch: Vec::with_capacity(16384),
            state: PlayState::Stopped,
            gain,
            listen_start: None,
            listened: Duration::ZERO,
            stop_after: None,
        };
        let thread = std::thread::Builder::new().name("engine".into()).spawn(move || w.run())?;
        Ok(Engine { tx, events, queue, tap, shared, snap, audible, thread: Some(thread) })
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().clone()
    }

    /// Position in the audible track.
    pub fn position(&self) -> Duration {
        self.view().position()
    }

    /// Rate the PipeWire graph runs at (0 = not known yet).
    pub fn output_rate(&self) -> u32 {
        self.shared.graph_rate.load(Relaxed)
    }

    /// A read-only handle for other threads (MPRIS, IPC).
    pub fn view(&self) -> EngineView {
        EngineView { snap: self.snap.clone(), audible: self.audible.clone(), shared: self.shared.clone(), pub_queue: self.queue.clone() }
    }
}

#[derive(Clone)]
pub struct EngineView {
    snap: Arc<Mutex<Snapshot>>,
    audible: Arc<Mutex<Audible>>,
    shared: Arc<Shared>,
    pub_queue: Arc<Mutex<Queue>>,
}

impl EngineView {
    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().clone()
    }
    pub fn queue(&self) -> &Arc<Mutex<Queue>> {
        &self.pub_queue
    }
    pub fn output_rate(&self) -> u32 {
        self.shared.graph_rate.load(Relaxed)
    }
    pub fn position(&self) -> Duration {
        let a = *self.audible.lock();
        if a.rate == 0 || a.channels == 0 {
            return Duration::ZERO;
        }
        let consumed = self.shared.consumed.load(Acquire);
        let played = consumed.saturating_sub(a.at) / a.channels as u64;
        // What you hear lags what we handed to PipeWire by its output delay.
        let lag = self.shared.latency_us.load(Relaxed) as u64 * a.rate as u64 / 1_000_000;
        Duration::from_secs_f64((a.pos_frames + played.saturating_sub(lag)) as f64 / a.rate as f64)
    }

    /// PipeWire output delay right now.
    pub fn latency(&self) -> Duration {
        Duration::from_micros(self.shared.latency_us.load(Relaxed) as u64)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Decoding {
    item_id: u64,
    track: Arc<Track>,
    stream: Box<dyn Stream>,
    fmt: OutputFormat,
    amp: f32,
    gain_db: Option<f32>,
}

struct Marker {
    at: u64,
    pos_frames: u64,
    item_id: u64,
    track: Arc<Track>,
    fmt: OutputFormat,
    format: AudioFormat,
    gain_db: Option<f32>,
}

struct Worker {
    rx: cb::Receiver<Cmd>,
    ev: cb::Sender<Event>,
    sources: Arc<Sources>,
    queue: Arc<Mutex<Queue>>,
    shared: Arc<Shared>,
    tap: Arc<Tap>,
    snap: Arc<Mutex<Snapshot>>,
    audible: Arc<Mutex<Audible>>,
    output: Box<dyn Output>,
    out_fmt: Option<OutputFormat>,
    active: bool,
    prod: rtrb::Producer<f32>,
    /// Samples pushed into the ring so far.
    pushed: u64,
    /// The decode head (may already be in the next track).
    dec: Option<Decoding>,
    /// Next track needs another output format; waits until the ring drains.
    waiting: Option<Decoding>,
    /// Gapless boundaries inside the ring, not yet heard.
    markers: VecDeque<Marker>,
    pending: Vec<f32>,
    pending_off: usize,
    scratch: Vec<f32>,
    state: PlayState,
    gain: GainCfg,
    listen_start: Option<Instant>,
    listened: Duration,
    stop_after: Option<StopAfter>,
}

impl Worker {
    fn run(&mut self) {
        loop {
            // Sleep while nothing needs doing; wake often only while audio flows.
            let busy = self.state == PlayState::Playing || self.active;
            let first = if busy {
                match self.rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(c) => Some(c),
                    Err(cb::RecvTimeoutError::Timeout) => None,
                    Err(cb::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match self.rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            let mut cmds: Vec<Cmd> = first.into_iter().collect();
            cmds.extend(self.rx.try_iter());
            for c in cmds {
                if matches!(c, Cmd::Quit) {
                    self.output.set_active(false);
                    return;
                }
                self.handle(c);
            }
            self.update_audible();
            if self.state == PlayState::Playing {
                self.fill();
                self.check_end();
            } else if self.active && self.shared.silent.load(Acquire) {
                // Faded out: release the device.
                self.output.set_active(false);
                self.active = false;
            }
        }
    }

    fn emit(&self, e: Event) {
        let _ = self.ev.send(e);
    }

    fn set_state(&mut self, s: PlayState) {
        if self.state == s {
            return;
        }
        match s {
            PlayState::Playing => self.listen_start = Some(Instant::now()),
            _ => {
                if let Some(t) = self.listen_start.take() {
                    self.listened += t.elapsed();
                }
            }
        }
        self.state = s;
        self.snap.lock().state = s;
        self.emit(Event::State(s));
    }

    fn position_frames(&self) -> u64 {
        let a = *self.audible.lock();
        a.pos_frames + self.shared.consumed.load(Acquire).saturating_sub(a.at) / a.channels.max(1) as u64
    }

    fn handle(&mut self, c: Cmd) {
        match c {
            Cmd::PlayItem(id) => self.start_item(id, Duration::ZERO),
            Cmd::Toggle => match self.state {
                PlayState::Playing => self.pause(),
                _ => self.play(),
            },
            Cmd::Play => self.play(),
            Cmd::Pause => self.pause(),
            Cmd::Stop => self.stop(),
            Cmd::Next => {
                let cur = self.snap.lock().item_id;
                let next = self.queue.lock().next_after(cur, false).map(|i| i.id);
                match next {
                    Some(id) => self.start_item(id, Duration::ZERO),
                    None => self.stop(),
                }
            }
            Cmd::Prev => {
                let cur = self.snap.lock().item_id;
                let rate = self.audible.lock().rate.max(1) as u64;
                if self.position_frames() > 3 * rate {
                    self.seek(Duration::ZERO);
                } else {
                    let prev = self.queue.lock().prev_before(cur).map(|i| i.id);
                    match prev {
                        Some(id) => self.start_item(id, Duration::ZERO),
                        None => self.seek(Duration::ZERO),
                    }
                }
            }
            Cmd::SeekTo(t) => self.seek(t),
            Cmd::SeekBy(ms) => {
                let rate = self.audible.lock().rate.max(1) as f64;
                let now = self.position_frames() as f64 / rate;
                self.seek(Duration::from_secs_f64((now + ms as f64 / 1000.0).max(0.0)));
            }
            Cmd::Volume(v) => {
                let v = v.clamp(0.0, 1.0);
                self.shared.set_volume(gain::volume_curve(v));
                self.snap.lock().volume = v;
                self.emit(Event::Volume);
            }
            Cmd::Gain(g) => {
                self.gain = g;
                if let Some(d) = self.dec.as_mut() {
                    let album = self.queue.lock().album_context(d.item_id);
                    d.gain_db = gain::gain_db(&g, &d.track.gain, album);
                    d.amp = d.gain_db.map(gain::db_to_amp).unwrap_or(1.0);
                    self.snap.lock().gain_db = d.gain_db;
                }
            }
            Cmd::ReopenOutput => {
                self.out_fmt = None;
                let cur = self.snap.lock().item_id;
                if let Some(cur) = cur.filter(|_| self.state != PlayState::Stopped) {
                    let state = self.state;
                    let pos = Duration::from_secs_f64(self.position_frames() as f64 / self.audible.lock().rate.max(1) as f64);
                    self.start_item(cur, pos);
                    if state == PlayState::Paused {
                        self.pause();
                    }
                }
            }
            Cmd::Cue(id, at) => {
                self.start_item(id, at);
                if self.state == PlayState::Playing {
                    // Nothing has been heard yet: go silent at once.
                    self.shared.paused.store(true, Release);
                    self.output.set_active(false);
                    self.active = false;
                    self.set_state(PlayState::Paused);
                }
            }
            Cmd::StopAfter(s) => self.stop_after = s,
            Cmd::Quit => {}
        }
    }

    fn play(&mut self) {
        match self.state {
            PlayState::Playing => {}
            PlayState::Paused => {
                self.shared.paused.store(false, Release);
                self.output.set_active(true);
                self.active = true;
                self.set_state(PlayState::Playing);
            }
            PlayState::Stopped => {
                let id = {
                    let q = self.queue.lock();
                    q.current_id().filter(|id| q.get(*id).is_some()).or_else(|| q.items().first().map(|i| i.id))
                };
                if let Some(id) = id {
                    self.start_item(id, Duration::ZERO);
                }
            }
        }
    }

    fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.shared.paused.store(true, Release);
            self.set_state(PlayState::Paused);
        }
    }

    fn stop(&mut self) {
        self.shared.paused.store(true, Release);
        self.flush();
        self.dec = None;
        self.finish_current();
        self.snap.lock().state = PlayState::Stopped;
        self.set_state(PlayState::Stopped);
    }

    /// Drop everything queued in the ring.
    fn flush(&mut self) {
        self.shared.flush_to.store(self.pushed, Release);
        self.markers.clear();
        self.pending.clear();
        self.pending_off = 0;
        self.waiting = None;
    }

    fn open(&mut self, item_id: u64) -> Result<Decoding> {
        let (track, album) = {
            let q = self.queue.lock();
            let it = q.get(item_id).ok_or_else(|| anyhow::anyhow!("item gone"))?;
            (it.track.clone(), q.album_context(item_id))
        };
        let stream = self.sources.open(track.tref())?;
        let f = &stream.info().format;
        let fmt = OutputFormat { rate: f.sample_rate, channels: f.channels };
        let gain_db = gain::gain_db(&self.gain, &track.gain, album);
        let amp = gain_db.map(gain::db_to_amp).unwrap_or(1.0);
        Ok(Decoding { item_id, track, stream, fmt, amp, gain_db })
    }

    fn ensure_format(&mut self, fmt: OutputFormat) -> Result<()> {
        if self.out_fmt != Some(fmt) {
            self.output.configure(fmt)?;
            self.out_fmt = Some(fmt);
            self.tap.rate.store(fmt.rate, Relaxed);
        }
        Ok(())
    }

    /// Hard switch: flush and start `id` at `at`.
    fn start_item(&mut self, id: u64, at: Duration) {
        self.flush();
        let wanted = id;
        let mut id = id;
        let mut tries = self.queue.lock().len().max(1);
        let mut d = loop {
            match self.open(id) {
                Ok(d) => break d,
                Err(e) => {
                    self.emit(Event::Error(format!("Can't play: {e:#}")));
                    tries -= 1;
                    // Skip to the next playable item.
                    let next = self.queue.lock().next_after(Some(id), false).map(|i| i.id);
                    match next {
                        Some(n) if tries > 0 && n != id => id = n,
                        _ => {
                            self.stop();
                            return;
                        }
                    }
                }
            }
        };
        let at = if d.item_id == wanted { at } else { Duration::ZERO };
        let pos = if at.is_zero() { 0 } else { d.stream.seek(at).unwrap_or(0) };
        if let Err(e) = self.ensure_format(d.fmt) {
            self.emit(Event::Error(format!("Audio output: {e:#}")));
            self.stop();
            return;
        }
        let m = self.marker(&d, pos);
        self.dec = Some(d);
        self.make_audible(m);
        self.shared.paused.store(false, Release);
        self.output.set_active(true);
        self.active = true;
        self.set_state(PlayState::Playing);
    }

    fn seek(&mut self, to: Duration) {
        let cur = self.snap.lock().item_id;
        let Some(cur) = cur else { return };
        let same = self.dec.as_ref().is_some_and(|d| d.item_id == cur);
        if !same {
            // Decode head already moved into the next track: reopen.
            let state = self.state;
            self.start_item(cur, to);
            if state == PlayState::Paused {
                self.pause();
            }
            return;
        }
        self.flush();
        let d = self.dec.as_mut().unwrap();
        if let Some(len) = d.stream.info().duration()
            && to >= len
        {
            // Seeking past the end = next track.
            self.handle(Cmd::Next);
            return;
        }
        let pos = match d.stream.seek(to) {
            Ok(p) => p,
            Err(e) => {
                self.emit(Event::Error(format!("Seek failed: {e:#}")));
                return;
            }
        };
        let d = self.dec.as_ref().unwrap();
        let m = self.marker(d, pos);
        let rate = m.fmt.rate;
        self.make_audible_quiet(&m);
        self.emit(Event::Seeked(Duration::from_secs_f64(pos as f64 / rate as f64)));
    }

    fn marker(&self, d: &Decoding, pos_frames: u64) -> Marker {
        Marker {
            at: self.pushed,
            pos_frames,
            item_id: d.item_id,
            track: d.track.clone(),
            fmt: d.fmt,
            format: d.stream.info().format.clone(),
            gain_db: d.gain_db,
        }
    }

    /// Update position info only (seek within the same track).
    fn make_audible_quiet(&mut self, m: &Marker) {
        *self.audible.lock() = Audible { at: m.at, pos_frames: m.pos_frames, channels: m.fmt.channels, rate: m.fmt.rate };
    }

    /// A new track is now heard.
    fn make_audible(&mut self, m: Marker) {
        self.make_audible_quiet(&m);
        self.finish_current();
        {
            let mut s = self.snap.lock();
            s.track = Some(m.track.clone());
            s.item_id = Some(m.item_id);
            s.format = Some(m.format);
            s.gain_db = m.gain_db;
            s.gain_origin = if m.gain_db.is_some() { m.track.gain.origin } else { GainOrigin::None };
        }
        self.queue.lock().set_current(Some(m.item_id));
        self.listened = Duration::ZERO;
        if self.state == PlayState::Playing {
            self.listen_start = Some(Instant::now());
        }
        self.emit(Event::Track(Some(m.track)));
    }

    /// Report how long the old track was listened to.
    fn finish_current(&mut self) {
        let track = self.snap.lock().track.clone();
        if let Some(track) = track {
            let mut listened = self.listened;
            if let Some(t) = self.listen_start.take() {
                listened += t.elapsed();
            }
            if self.state == PlayState::Playing {
                self.listen_start = Some(Instant::now());
            }
            self.listened = Duration::ZERO;
            if !listened.is_zero() {
                self.emit(Event::Finished { track, listened });
            }
        }
    }

    fn update_audible(&mut self) {
        let consumed = self.shared.consumed.load(Acquire);
        while self.markers.front().is_some_and(|m| m.at <= consumed) {
            let m = self.markers.pop_front().unwrap();
            self.make_audible(m);
        }
    }

    /// Keep about `AHEAD` of audio in the ring.
    fn fill(&mut self) {
        let Some(fmt) = self.out_fmt else { return };
        let target = (fmt.rate as f64 * fmt.channels as f64 * AHEAD.as_secs_f64()) as u64;
        loop {
            let consumed = self.shared.consumed.load(Acquire);
            let queued = self.pushed.saturating_sub(consumed);
            if self.pending_off < self.pending.len() {
                let free = self.prod.slots();
                let n = free.min(self.pending.len() - self.pending_off);
                if n == 0 {
                    return;
                }
                if let Ok(mut chunk) = self.prod.write_chunk_uninit(n) {
                    let src = &self.pending[self.pending_off..self.pending_off + n];
                    let (a, b) = chunk.as_mut_slices();
                    for (dst, s) in a.iter_mut().chain(b.iter_mut()).zip(src) {
                        dst.write(*s);
                    }
                    unsafe { chunk.commit_all() };
                }
                self.pending_off += n;
                self.pushed += n as u64;
                continue;
            }
            if queued >= target {
                return;
            }
            if let Some(next) = self.waiting.as_ref() {
                // Wait until everything of the old format has been heard.
                if consumed < self.pushed {
                    return;
                }
                let fmt = next.fmt;
                if let Err(e) = self.ensure_format(fmt) {
                    self.emit(Event::Error(format!("Audio output: {e:#}")));
                    self.stop();
                    return;
                }
                let d = self.waiting.take().unwrap();
                let m = self.marker(&d, 0);
                self.dec = Some(d);
                self.make_audible(m);
                self.output.set_active(true);
                continue;
            }
            let Some(d) = self.dec.as_mut() else { return };
            self.scratch.clear();
            let res = d.stream.read(&mut self.scratch);
            // Live streams (radio, v2) report song changes inside the stream.
            if let Some(mut t) = d.stream.meta_update() {
                t.r = d.track.r.clone();
                let t = Arc::new(t);
                d.track = t.clone();
                self.snap.lock().track = Some(t.clone());
                self.emit(Event::Track(Some(t)));
            }
            let Some(d) = self.dec.as_mut() else { return };
            match res {
                Ok(true) => {
                    if d.amp != 1.0 {
                        let a = d.amp;
                        self.scratch.iter_mut().for_each(|s| *s *= a);
                    }
                    std::mem::swap(&mut self.pending, &mut self.scratch);
                    self.pending_off = 0;
                }
                Ok(false) => self.advance(),
                Err(e) => {
                    self.emit(Event::Error(format!("Decode error: {e:#}")));
                    self.advance();
                }
            }
        }
    }

    /// The decode head reached the end of a track: chain the next one.
    fn advance(&mut self) {
        let Some(done) = self.dec.take() else { return };
        match self.stop_after {
            Some(StopAfter::Track) => {
                self.stop_after = None;
                return;
            }
            Some(StopAfter::Album) => {
                let key = |t: &Track| (if t.album_artist.is_empty() { t.artist.clone() } else { t.album_artist.clone() }, t.album.clone());
                let next = self.queue.lock().next_after(Some(done.item_id), true).map(|i| key(&i.track));
                if next != Some(key(&done.track)) {
                    self.stop_after = None;
                    return;
                }
            }
            None => {}
        }
        let mut after = Some(done.item_id);
        let mut tries = self.queue.lock().len();
        let mut auto = true;
        while tries > 0 {
            tries -= 1;
            let next = self.queue.lock().next_after(after, auto).map(|i| i.id);
            let Some(id) = next else { return };
            match self.open(id) {
                Ok(d) => {
                    if Some(d.fmt) == self.out_fmt {
                        let m = self.marker(&d, 0);
                        self.markers.push_back(m);
                        self.dec = Some(d);
                    } else {
                        self.waiting = Some(d);
                    }
                    return;
                }
                Err(e) => {
                    self.emit(Event::Error(format!("Can't play: {e:#}")));
                    if Some(id) == after {
                        return;
                    }
                    after = Some(id);
                    auto = false;
                }
            }
        }
    }

    fn check_end(&mut self) {
        if self.dec.is_none() && self.waiting.is_none() && self.markers.is_empty() && self.pending_off >= self.pending.len() {
            let consumed = self.shared.consumed.load(Acquire);
            if consumed >= self.pushed {
                self.shared.paused.store(true, Release);
                self.finish_current();
                self.set_state(PlayState::Stopped);
                self.emit(Event::Track(self.snap.lock().track.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests;
