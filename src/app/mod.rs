//! The TUI: state, the event loop and actions. Drawing is in `draw.rs`,
//! floating dialogs in `dialogs.rs`.
//!
//! The loop sleeps on channels. It only ticks while something moves
//! (progress while playing, visualizer frames while visible), so idle CPU is ~0.

pub mod art;
pub mod biglyric;
mod clicks;
pub mod dialogs;
pub mod draw;
pub mod session;
pub mod viz;
pub mod worker;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel as cb;
use crossterm::event::{Event as TermEvent, KeyEventKind, MouseButton, MouseEventKind};
use musinator_themes::Theme;
use ratatui::layout::{Rect, Size};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

use crate::config::Config;
use crate::engine::{Cmd, Engine, Event as EngineEvent, PlayState};
use crate::ipc::{self, Request};
use crate::keys::{Action, Keymap};
use crate::library::{LibEvent, Library, db, lrc};
use crate::model::{ArtRef, Track, TrackRef};
use crate::source::local::LocalSource;
use crate::source::{Item, Lyrics, NodeKind, SearchHits, Sources};
use art::{ArtReady, ArtWorker, Slot};
use dialogs::Dialog;
use worker::{Job, Msg, Then, Worker};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Library,
    NowPlaying,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Side,
    List,
    Panel,
}

#[derive(Clone, Debug)]
pub enum Side {
    NowPlaying,
    Queue,
    Header(&'static str),
    Blank,
    Node { label: String, node: String },
    Playlist { id: i64, name: String, count: u64 },
    Empty(&'static str),
}

impl Side {
    pub fn selectable(&self) -> bool {
        !matches!(self, Side::Header(_) | Side::Blank | Side::Empty(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Node,
    Queue,
}

pub struct ListView {
    pub title: String,
    pub node: String,
    pub kind: ListKind,
    pub items: Vec<Item>,
    pub cursor: usize,
    pub offset: usize,
    pub loading: bool,
    pub seq: u64,
    pub error: Option<String>,
    /// Typed filter (`ctrl+f`); `all` holds the unfiltered items while it is set.
    pub filter: String,
    pub all: Option<Vec<Item>>,
    /// The filter box has the keyboard.
    pub filtering: bool,
}

impl ListView {
    fn new(title: String, node: String, kind: ListKind, seq: u64) -> Self {
        Self {
            title,
            node,
            kind,
            items: Vec::new(),
            cursor: 0,
            offset: 0,
            loading: kind == ListKind::Node,
            seq,
            error: None,
            filter: String::new(),
            all: None,
            filtering: false,
        }
    }
    /// Show only items matching the filter (all words, any order, ignoring case).
    pub fn apply_filter(&mut self) {
        let all = self.all.get_or_insert_with(|| std::mem::take(&mut self.items));
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(str::to_string).collect();
        self.items = all
            .iter()
            .filter(|it| {
                let text = match it {
                    Item::Track(t) => format!("{} {} {} {}", t.display_title(), t.artist, t.album, t.album_artist),
                    Item::Node(n) => format!("{} {}", n.title, n.subtitle),
                }
                .to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .cloned()
            .collect();
        self.cursor = 0;
        self.offset = 0;
    }

    pub fn clear_filter(&mut self) {
        if let Some(all) = self.all.take() {
            // Keep the selected item selected.
            let sel = self.items.get(self.cursor).map(item_key);
            self.items = all;
            self.cursor = sel.and_then(|k| self.items.iter().position(|i| item_key(i) == k)).unwrap_or(0);
        }
        self.filter.clear();
        self.filtering = false;
    }

    pub fn is_albums(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| matches!(i, Item::Node(n) if n.kind == NodeKind::Album))
    }
    pub fn playlist_id(&self) -> Option<i64> {
        self.node.strip_prefix("playlist:").and_then(|s| s.parse().ok())
    }
}

/// An image ready to draw, with the request it answers.
pub struct Pic {
    pub art: ArtRef,
    pub cells: Size,
    pub proto: Option<Protocol>,
}

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    pub keys: Keymap,
    pub screen: Screen,
    pub focus: Focus,
    pub side: Vec<Side>,
    pub side_cursor: usize,
    pub side_offset: usize,
    pub lists: Vec<ListView>,
    pub upnext_cursor: usize,
    pub upnext_offset: usize,
    pub dialog: Option<Dialog>,
    pub playlists: Vec<db::Playlist>,
    pub engine: Engine,
    pub lib: Arc<Library>,
    pub sources: Arc<Sources>,
    worker: Worker,
    art: Option<ArtWorker>,
    pub picker: Picker,
    pub images_on: bool,
    pub pics: Vec<(Slot, Pic)>,
    pub grid_pics: std::collections::HashMap<ArtRef, Pic>,
    pub scrobbler: crate::lastfm::Scrobbler,
    /// Logged-in Last.fm user (cached; the file is read on login/logout only).
    pub lastfm_user: Option<String>,
    /// The radio song on air now and when it started (scrobbled when the next one starts).
    on_air: Option<(Arc<Track>, Instant)>,
    /// The song `L` was pressed on (to love it on Last.fm when the like is saved).
    last_liked: Option<Arc<Track>>,
    pub podcasts: Arc<crate::source::podcast::PodcastSource>,
    /// Total size of podcast downloads ("1.2 GB"), updated when the Downloads list loads.
    pub downloads_label: String,
    pub server: Arc<crate::source::subsonic::SubsonicSource>,
    pub jellyfin: Arc<crate::source::jellyfin::JellyfinSource>,
    /// Music server results for the main search.
    pub server_hits: Vec<Item>,
    /// When the episode position was last saved.
    podcast_saved: Instant,
    /// Songs whose tags were saved; refreshed in the queue after the next scan.
    edited: Vec<crate::model::TrackRef>,
    pub art_file: Option<std::path::PathBuf>,
    pub lyrics: Option<(TrackRef, Lyrics)>,
    pub viz: viz::Viz,
    pub status: Option<(String, bool, Instant)>,
    /// Per-song lyrics offset for the playing song (ms, positive = later).
    pub lyrics_offset: i64,
    pub sleep: Option<Sleep>,
    /// When an online lyrics lookup started (shows a spinner).
    pub lyrics_lookup: Option<Instant>,
    /// Full-screen lyrics view is open.
    pub lyrics_full: bool,
    /// Mini mode chosen with the key (it also turns on by itself in small windows).
    pub mini: bool,
    big: Option<biglyric::BigLyrics>,
    /// Big lyric pictures asked for / ready (small, newest last).
    pub big_pics: Vec<(biglyric::Key, Option<Protocol>)>,
    big_asked: Vec<biglyric::Key>,
    /// Ratings changed this session (lists may still hold old track copies).
    pub ratings: std::collections::HashMap<TrackRef, Option<u8>>,
    /// (bg, fg) last sent to the terminal with OSC 11/10.
    term_colors: Option<(musinator_themes::Rgb, musinator_themes::Rgb)>,
    pub scan: Option<(usize, usize)>,
    pub loudness: Option<(usize, usize)>,
    pub library_size: usize,
    seq: u64,
    pub dirty: bool,
    pub quit: bool,
    next_tick: Option<Instant>,
    last_viz: Instant,
    pub viz_area: Rect,
    /// Areas from the last draw, for mouse clicks.
    pub hit: draw::Hits,
    pub search_hits: SearchHits,
    pub search_seq: u64,
    /// Station hits for the search, and whether they are still coming.
    pub search_stations: Vec<Arc<Track>>,
    pub stations_busy: bool,
    /// The search was opened from "Find a station…": show stations only.
    pub stations_only: bool,
    pub mpris: Option<crate::mpris::Mpris>,
    preview_theme: Option<Theme>,
    kitty_ok: bool,
    art_tx: cb::Sender<ArtReady>,
}

impl App {
    pub fn selected_side(&self) -> Option<&Side> {
        self.side.get(self.side_cursor)
    }

    pub fn list(&self) -> Option<&ListView> {
        self.lists.last()
    }

    fn list_mut(&mut self) -> Option<&mut ListView> {
        self.lists.last_mut()
    }

    /// Open the selected (or playing) video file in mpv at the right spot and pause here.
    fn open_video(&mut self) {
        const VIDEO: &[&str] = &["mp4", "m4v", "mkv", "webm", "mov", "avi"];
        let is_video = |t: &Track| {
            t.r.as_ref().is_some_and(|r| {
                &*r.source == crate::source::local::ID
                    && std::path::Path::new(&r.id)
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| VIDEO.contains(&e.to_ascii_lowercase().as_str()))
            })
        };
        let now = self.now();
        let selected = self.targets().and_then(|(_, t, _)| t.into_iter().next()).filter(|t| is_video(t));
        let Some(t) = selected.or(now.track.clone().filter(|t| is_video(t))) else {
            return self.info("Not a video file");
        };
        let playing_it = now.track.as_ref().is_some_and(|c| c.r == t.r);
        let start = if playing_it { self.engine.position().as_secs_f64() } else { 0.0 };
        let path = t.tref().id.clone();
        let spawned = std::process::Command::new("mpv")
            .arg("--force-window=yes")
            .arg(format!("--start={start:.1}"))
            .arg("--")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(mut child) => {
                // Reap it in the background so it never lingers as a zombie.
                std::thread::spawn(move || child.wait());
                if now.state == PlayState::Playing {
                    self.engine.send(Cmd::Toggle);
                }
                self.info(format!("Playing video in mpv: {}", t.display_title()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.error("mpv is not installed (sudo pacman -S mpv)"),
            Err(e) => self.error(format!("Can't start mpv: {e}")),
        }
    }

    /// Last.fm "now playing", and scrobbles for radio songs when the next one starts.
    /// An episode started: jump to where you stopped last time.
    fn podcast_resume(&mut self, t: &Track) {
        if !is_podcast(t) {
            return;
        }
        let (pos, played) = self.podcasts.position(&t.tref().id);
        if !played && pos > 10_000 && self.engine.position() < Duration::from_secs(5) {
            self.engine.send(Cmd::SeekTo(Duration::from_millis(pos)));
            self.info(format!("Continuing at {}", crate::model::fmt_time(pos)));
        }
        self.podcast_saved = Instant::now();
    }

    /// Save where you are in an episode (every 10 s while playing, and on pause/quit).
    fn podcast_save(&mut self, force: bool) {
        let snap = self.now();
        let Some(t) = snap.track.filter(|t| is_podcast(t)) else { return };
        if !force && (snap.state != PlayState::Playing || self.podcast_saved.elapsed() < Duration::from_secs(10)) {
            return;
        }
        self.podcast_saved = Instant::now();
        let ms = self.engine.position().as_millis() as u64;
        // A tiny write; done here so it also happens right before quitting.
        self.podcasts.save_position(&t.tref().id, ms, t.duration_ms.unwrap_or(0));
    }

    fn lastfm_track(&mut self, t: Option<&Arc<Track>>) {
        if let Some((prev, since)) = self.on_air.take()
            && t.is_some_and(|t| t.r == prev.r && (t.title != prev.title || t.artist != prev.artist))
            && self.cfg.lastfm_scrobble
            && self.cfg.lastfm_radio
            && crate::lastfm::counts(None, since.elapsed())
            && let Some(p) = lastfm_play(&prev, crate::lastfm::now() - since.elapsed().as_secs() as i64)
        {
            self.scrobbler.scrobble(p);
        }
        let Some(t) = t else { return };
        if draw::is_live(t) {
            if !t.on_air {
                return;
            }
            self.on_air = Some((t.clone(), Instant::now()));
            if !self.cfg.lastfm_radio {
                return;
            }
        }
        if self.cfg.lastfm_scrobble
            && let Some(p) = lastfm_play(t, crate::lastfm::now())
        {
            self.scrobbler.now_playing(p);
        }
    }

    /// The music server lists for the sidebar (Navidrome/Subsonic or Jellyfin), if one is set up.
    pub fn server_nodes(&self) -> Option<[(&'static str, &'static str); 4]> {
        use crate::source::{jellyfin as jf, subsonic as ss};
        if self.server.login.lock().is_some() {
            Some([("Artists", ss::ARTISTS), ("Albums", ss::ALBUMS), ("Recently added", ss::NEWEST), ("Playlists", ss::PLAYLISTS)])
        } else if self.jellyfin.login.lock().is_some() {
            Some([("Artists", jf::ARTISTS), ("Albums", jf::ALBUMS), ("Recently added", jf::NEWEST), ("Playlists", jf::PLAYLISTS)])
        } else {
            None
        }
    }

    /// Last.fm part of the info line: "last.fm", "last.fm ✓" once the song counts,
    /// plus waiting scrobbles. None when not logged in or off.
    pub fn lastfm_status(&self) -> Option<(String, bool)> {
        if self.lastfm_user.is_none() || !self.cfg.lastfm_scrobble {
            return None;
        }
        let t = self.now().track?;
        if is_podcast(&t) {
            return None;
        }
        let live = draw::is_live(&t);
        let counted = if live {
            if !t.on_air || !self.cfg.lastfm_radio {
                return None;
            }
            self.on_air.as_ref().is_some_and(|(_, since)| crate::lastfm::counts(None, since.elapsed()))
        } else {
            crate::lastfm::counts(t.duration_ms, self.engine.position())
        };
        let waiting = self.scrobbler.waiting.load(std::sync::atomic::Ordering::Relaxed);
        let mut s = if counted { "last.fm ✓".to_string() } else { "last.fm".to_string() };
        if waiting > 0 {
            s.push_str(&format!(" · {waiting} waiting"));
        }
        Some((s, counted))
    }

    /// The equalizer changes the sound (on and not flat).
    pub fn eq_active(&self) -> bool {
        self.cfg.eq_on && self.cfg.eq_bands.iter().any(|g| g.abs() > 0.01)
    }

    /// Apply and save equalizer settings.
    pub fn apply_eq(&mut self) {
        self.engine.set_eq(self.cfg.eq_on, &self.cfg.eq_bands);
        self.save_cfg();
    }

    pub fn now(&self) -> crate::engine::Snapshot {
        self.engine.snapshot()
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), false, Instant::now()));
        self.dirty = true;
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), true, Instant::now()));
        self.dirty = true;
    }

    pub fn save_cfg(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.error(format!("Can't save settings: {e:#}"));
        }
    }

    // ---------- theme ----------

    pub fn load_theme(id: &str) -> Theme {
        if id == musinator_themes::omarchy::ID
            && let Some(t) = musinator_themes::omarchy::current()
        {
            return t;
        }
        musinator_themes::by_id(id).unwrap_or_else(|| musinator_themes::by_id(musinator_themes::DEFAULT_ID).expect("default theme"))
    }

    pub fn preview_theme(&mut self, t: Theme) {
        if self.preview_theme.is_none() {
            self.preview_theme = Some(self.theme.clone());
        }
        self.theme = t;
        self.dirty = true;
    }

    pub fn end_preview(&mut self, keep: bool) {
        if let Some(old) = self.preview_theme.take() {
            if keep {
                self.cfg.theme = self.theme.id.clone();
                self.save_cfg();
            } else {
                self.theme = old;
            }
        }
        self.dirty = true;
    }

    fn reload_omarchy(&mut self) {
        if self.cfg.theme == musinator_themes::omarchy::ID
            && self.preview_theme.is_none()
            && let Some(t) = musinator_themes::omarchy::current()
            && (t.label != self.theme.label || t.accent != self.theme.accent || t.bg != self.theme.bg)
        {
            self.theme = t;
            self.dirty = true;
        }
    }

    // ---------- sidebar & lists ----------

    pub fn rebuild_side(&mut self) {
        let keep = self.selected_side().map(side_key);
        let mut s = vec![Side::NowPlaying, Side::Queue, Side::Blank, Side::Header("Library")];
        for (l, n) in [("Artists", "artists"), ("Albums", "albums"), ("Tracks", "tracks"), ("Genres", "genres"), ("Folders", "folders")] {
            s.push(Side::Node { label: l.into(), node: n.into() });
        }
        s.push(Side::Blank);
        s.push(Side::Header("Playlists"));
        if self.playlists.is_empty() {
            s.push(Side::Empty("none yet · N to add"));
        }
        for p in &self.playlists {
            s.push(Side::Playlist { id: p.id, name: p.name.clone(), count: p.count });
        }
        s.push(Side::Blank);
        s.push(Side::Header("Radio"));
        for (l, n) in [
            ("Favorites", crate::source::radio::FAVORITES),
            ("Popular", crate::source::radio::POPULAR),
            ("Genres", crate::source::radio::GENRES),
            ("Countries", crate::source::radio::COUNTRIES),
            ("Find a station…", RADIO_FIND),
            ("Add stream address…", RADIO_ADD),
        ] {
            s.push(Side::Node { label: l.into(), node: n.into() });
        }
        if let Some(nodes) = self.server_nodes() {
            s.push(Side::Blank);
            s.push(Side::Header("Music server"));
            for (l, n) in nodes {
                s.push(Side::Node { label: l.into(), node: n.into() });
            }
        }
        s.push(Side::Blank);
        s.push(Side::Header("Podcasts"));
        for (l, n) in [
            ("New episodes", crate::source::podcast::NEW),
            ("Shows", crate::source::podcast::SHOWS),
            ("Downloads", crate::source::podcast::DOWNLOADS),
            ("Find a podcast…", PODCAST_FIND),
        ] {
            s.push(Side::Node { label: l.into(), node: n.into() });
        }
        if self.lastfm_user.is_some() {
            s.push(Side::Blank);
            s.push(Side::Header("Last.fm"));
            for (l, n) in [
                ("Recent", "lastfm:recent"),
                ("Loved", "lastfm:loved"),
                ("Top artists", "lastfm:top-artists"),
                ("Top songs", "lastfm:top-tracks"),
                ("Open profile", LASTFM_PROFILE),
            ] {
                s.push(Side::Node { label: l.into(), node: n.into() });
            }
        }
        s.push(Side::Blank);
        s.push(Side::Header("Smart"));
        for (l, n) in [
            ("Most played", "smart:most"),
            ("Never played", "smart:never"),
            ("Added this week", "smart:week"),
            ("Liked", "smart:liked"),
            ("Top rated", "smart:rated"),
        ] {
            s.push(Side::Node { label: l.into(), node: n.into() });
        }
        self.side = s;
        if let Some(k) = keep
            && let Some(i) = self.side.iter().position(|x| side_key(x) == k)
        {
            self.side_cursor = i;
        }
        self.side_cursor = self.side_cursor.min(self.side.len() - 1);
        if !self.side[self.side_cursor].selectable() {
            self.move_side(1);
        }
        self.dirty = true;
    }

    /// Open what the sidebar points at in the list pane.
    pub(crate) fn open_side(&mut self) {
        let Some(s) = self.selected_side().cloned() else { return };
        let seq = self.next_seq();
        let view = match s {
            Side::NowPlaying => {
                let t = self.now().track;
                match t {
                    Some(t) if !t.album.is_empty() => {
                        let node = format!("album:{}", album_key(&t));
                        let title = album_title(&t);
                        ListView::new(title, node, ListKind::Node, seq)
                    }
                    _ => {
                        let mut v = ListView::new("Now Playing".into(), String::new(), ListKind::Queue, seq);
                        v.loading = false;
                        v
                    }
                }
            }
            Side::Queue => {
                let mut v = ListView::new("Queue".into(), "queue".into(), ListKind::Queue, seq);
                v.loading = false;
                v
            }
            // Searching needs typing: show a hint until enter is pressed.
            Side::Node { node, label } if node == RADIO_FIND || node == RADIO_ADD || node == LASTFM_PROFILE || node == PODCAST_FIND => {
                let mut v = ListView::new(label.trim_end_matches('…').into(), node, ListKind::Node, seq);
                v.loading = false;
                v
            }
            Side::Node { label, node } => ListView::new(label, node, ListKind::Node, seq),
            Side::Playlist { id, name, .. } => ListView::new(name, format!("playlist:{id}"), ListKind::Node, seq),
            _ => return,
        };
        let node = view.node.clone();
        let kind = view.kind;
        self.lists = vec![view];
        if kind == ListKind::Node
            && !node.is_empty()
            && node != RADIO_FIND
            && node != RADIO_ADD
            && node != LASTFM_PROFILE
            && node != PODCAST_FIND
        {
            self.worker.send(Job::Children { seq, node });
        }
        if kind == ListKind::Queue {
            self.fill_queue_view();
        }
        self.dirty = true;
    }

    fn fill_queue_view(&mut self) {
        let q = self.engine.queue.lock();
        let items: Vec<Item> = q.up_next().iter().map(|i| Item::Track(i.track.clone())).collect();
        drop(q);
        if let Some(v) = self.lists.last_mut().filter(|v| v.kind == ListKind::Queue) {
            v.items = items;
            v.cursor = v.cursor.min(v.items.len().saturating_sub(1));
        }
    }

    pub(crate) fn drill(&mut self, title: String, node: String) {
        let seq = self.next_seq();
        self.lists.push(ListView::new(title, node.clone(), ListKind::Node, seq));
        self.worker.send(Job::Children { seq, node });
        self.focus = Focus::List;
        self.dirty = true;
    }

    fn reload_lists_matching(&mut self, node: &str) {
        let mut jobs = Vec::new();
        for v in &mut self.lists {
            if v.node == node {
                self.seq += 1;
                v.seq = self.seq;
                jobs.push(Job::Children { seq: v.seq, node: v.node.clone() });
            }
        }
        for j in jobs {
            self.worker.send(j);
        }
    }

    fn move_side(&mut self, delta: isize) {
        let n = self.side.len() as isize;
        let mut i = self.side_cursor as isize;
        loop {
            i += delta.signum();
            if i < 0 || i >= n {
                return;
            }
            if self.side[i as usize].selectable() {
                break;
            }
        }
        let steps = delta.unsigned_abs();
        self.side_cursor = i as usize;
        if steps > 1 {
            for _ in 1..steps {
                let before = self.side_cursor;
                self.move_side(delta.signum());
                if self.side_cursor == before {
                    break;
                }
            }
            return;
        }
        self.open_side();
    }

    fn move_cursor(&mut self, delta: isize, page: usize) {
        match (self.screen, self.focus) {
            (Screen::Library, Focus::Side) => {
                if delta == isize::MIN {
                    self.side_cursor = 0;
                    self.open_side();
                } else if delta == isize::MAX {
                    self.side_cursor = self.side.len() - 1;
                    while !self.side[self.side_cursor].selectable() {
                        self.side_cursor -= 1;
                    }
                    self.open_side();
                } else {
                    self.move_side(delta);
                }
            }
            (Screen::Library, Focus::List) => {
                let grid = self.grid_cols();
                if let Some(v) = self.list_mut() {
                    let n = v.items.len();
                    if n == 0 {
                        return;
                    }
                    let d = match delta {
                        isize::MIN => -(n as isize),
                        isize::MAX => n as isize,
                        d => d * grid.unwrap_or(1) as isize * if page > 0 { page as isize } else { 1 },
                    };
                    v.cursor = (v.cursor as isize + d).clamp(0, n as isize - 1) as usize;
                }
            }
            (Screen::NowPlaying, Focus::List) => {
                let n = self.engine.queue.lock().up_next().len();
                if n == 0 {
                    return;
                }
                let d = match delta {
                    isize::MIN => -(n as isize),
                    isize::MAX => n as isize,
                    d => d * if page > 0 { page as isize } else { 1 },
                };
                self.upnext_cursor = (self.upnext_cursor as isize + d).clamp(0, n as isize - 1) as usize;
            }
            _ => {}
        }
        self.dirty = true;
    }

    /// Columns in the album grid, when it is showing.
    pub fn grid_cols(&self) -> Option<usize> {
        let v = self.list()?;
        if self.cfg.album_grid && v.is_albums() { Some(self.hit.grid_cols.max(1)) } else { None }
    }

    fn selected_item(&self) -> Option<Item> {
        let v = self.list()?;
        v.items.get(v.cursor).cloned()
    }

    fn list_tracks(&self) -> Vec<Arc<Track>> {
        self.list()
            .map(|v| v.items.iter().filter_map(|i| if let Item::Track(t) = i { Some(t.clone()) } else { None }).collect())
            .unwrap_or_default()
    }

    // ---------- playback ----------

    fn play_tracks(&mut self, tracks: Vec<Arc<Track>>, start: usize, from: String) {
        if tracks.is_empty() {
            return;
        }
        let id = self.engine.queue.lock().replace(tracks, Some(from), start);
        if let Some(id) = id {
            self.engine.send(Cmd::PlayItem(id));
        }
        self.upnext_cursor = 0;
    }

    fn on_tracks(&mut self, tracks: Vec<Arc<Track>>, then: Then) {
        let n = tracks.len();
        match then {
            Then::Play { from } => self.play_tracks(tracks, 0, from),
            Then::Queue { from } => {
                let start = self.engine.queue.lock().is_empty();
                self.engine.queue.lock().append(tracks, Some(from));
                if start && self.now().state == PlayState::Stopped {
                    self.engine.send(Cmd::Play);
                }
                self.info(format!("Added {n} to queue"));
            }
            Then::PlayNext { from } => {
                self.engine.queue.lock().insert_next(tracks, Some(from));
                self.info(format!("{n} will play next"));
            }
            Then::AddToPlaylist(_) | Then::EditTags => {}
        }
        self.fill_queue_view();
        self.rebuild_side();
    }

    /// The tracks an action applies to: the selected list item, or what the sidebar points at.
    fn targets(&self) -> Option<(Vec<String>, Vec<Arc<Track>>, String)> {
        match (self.screen, self.focus) {
            (Screen::Library, Focus::List) => {
                let from = self.list()?.title.clone();
                match self.selected_item()? {
                    Item::Track(t) => Some((vec![], vec![t], from)),
                    Item::Node(n) => Some((vec![n.id.clone()], vec![], n.title)),
                }
            }
            (Screen::Library, Focus::Side) => match self.selected_side()? {
                Side::Node { label, node } if node.starts_with("smart:") => Some((vec![node.clone()], vec![], label.clone())),
                Side::Playlist { id, name, .. } => Some((vec![format!("playlist:{id}")], vec![], name.clone())),
                _ => None,
            },
            (Screen::NowPlaying, _) | (_, Focus::Panel) => {
                let t = self.now().track?;
                Some((vec![], vec![t], String::new()))
            }
        }
    }

    fn enter(&mut self) {
        match (self.screen, self.focus) {
            (Screen::Library, Focus::Side) => match self.selected_side() {
                Some(Side::NowPlaying) if self.now().track.is_some() => self.screen = Screen::NowPlaying,
                Some(Side::Node { node, .. }) if node == RADIO_FIND => {
                    self.open_search(true);
                }
                Some(Side::Node { node, .. }) if node == PODCAST_FIND => {
                    self.dialog = Some(Dialog::input("Find a podcast", "Name or feed address", String::new(), dialogs::InputFor::Podcast));
                }
                Some(Side::Node { node, .. }) if node == LASTFM_PROFILE => {
                    if let Some(u) = crate::lastfm::profile_url() {
                        let _ = std::process::Command::new("xdg-open")
                            .arg(u)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                            .map(|mut c| std::thread::spawn(move || c.wait()));
                        self.info("Opening your Last.fm profile");
                    }
                }
                Some(Side::Node { node, .. }) if node == RADIO_ADD => {
                    self.dialog = Some(Dialog::input("Add stream address", "Address", String::new(), dialogs::InputFor::RadioUrl));
                }
                Some(_) => self.focus = Focus::List,
                None => {}
            },
            (Screen::Library, Focus::List) => {
                let Some(v) = self.list() else { return };
                if v.kind == ListKind::Queue {
                    let idx = v.cursor;
                    let id = self.engine.queue.lock().up_next().get(idx).map(|i| i.id);
                    if let Some(id) = id {
                        self.engine.send(Cmd::PlayItem(id));
                    }
                    return;
                }
                match self.selected_item() {
                    Some(Item::Node(n)) => {
                        let title = if n.kind == NodeKind::Album { format!("{} · {}", n.title, n.subtitle) } else { n.title.clone() };
                        self.drill(title, n.id);
                    }
                    Some(Item::Track(t)) => {
                        let tracks = self.list_tracks();
                        let start = tracks.iter().position(|x| Arc::ptr_eq(x, &t)).unwrap_or(0);
                        let from = self.list().map(|v| v.title.clone()).unwrap_or_default();
                        self.play_tracks(tracks, start, from);
                    }
                    None => {}
                }
            }
            (Screen::NowPlaying, Focus::List) => {
                let id = self.engine.queue.lock().up_next().get(self.upnext_cursor).map(|i| i.id);
                if let Some(id) = id {
                    self.engine.send(Cmd::PlayItem(id));
                    self.upnext_cursor = 0;
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    /// Typing into the list filter. True when the key was used.
    fn filter_key(&mut self, k: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};
        if self.screen != Screen::Library || self.focus != Focus::List {
            return false;
        }
        let Some(v) = self.list_mut().filter(|v| v.filtering) else { return false };
        match k.code {
            KeyCode::Esc => v.clear_filter(),
            KeyCode::Enter => {
                v.filtering = false;
                if v.filter.is_empty() {
                    v.clear_filter();
                }
                self.enter();
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                v.filtering = false;
                if v.filter.is_empty() {
                    v.clear_filter();
                }
                return false;
            }
            KeyCode::Backspace => {
                v.filter.pop();
                v.apply_filter();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                v.filter.clear();
                v.apply_filter();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                v.filter.push(c);
                v.apply_filter();
            }
            _ => {}
        }
        true
    }

    fn back(&mut self) {
        // A filtered list: esc first shows the whole list again.
        if self.screen == Screen::Library
            && let Some(v) = self.list_mut()
            && v.all.is_some()
        {
            v.clear_filter();
            self.dirty = true;
            return;
        }
        if self.lyrics_full {
            self.lyrics_full = false;
            self.dirty = true;
            return;
        }
        match self.screen {
            Screen::NowPlaying => self.screen = Screen::Library,
            Screen::Library => {
                if self.focus == Focus::List && self.lists.len() > 1 {
                    self.lists.pop();
                } else if self.focus != Focus::Side {
                    self.focus = Focus::Side;
                }
            }
        }
        self.dirty = true;
    }

    fn queue_view_active(&self) -> bool {
        (self.screen == Screen::NowPlaying && self.focus == Focus::List)
            || (self.screen == Screen::Library && self.focus == Focus::List && self.list().is_some_and(|v| v.kind == ListKind::Queue))
    }

    fn queue_cursor_id(&self) -> Option<u64> {
        let idx = if self.screen == Screen::NowPlaying { self.upnext_cursor } else { self.list()?.cursor };
        self.engine.queue.lock().up_next().get(idx).map(|i| i.id)
    }

    fn after_queue_edit(&mut self) {
        self.fill_queue_view();
        let n = self.engine.queue.lock().up_next().len();
        self.upnext_cursor = self.upnext_cursor.min(n.saturating_sub(1));
        self.rebuild_side();
    }

    pub fn act(&mut self, a: Action) {
        let small = self.cfg.seek_small as i64 * 1000;
        let big = self.cfg.seek_big as i64 * 1000;
        match a {
            Action::PlayPause => {
                if self.now().state == PlayState::Stopped && self.engine.queue.lock().is_empty() {
                    if let Some((nodes, tracks, from)) = self.targets() {
                        self.worker.send(Job::Tracks { nodes, tracks, then: Then::Play { from } });
                    }
                } else {
                    self.engine.send(Cmd::Toggle);
                }
            }
            Action::PlaySelected => self.enter(),
            Action::Next => self.engine.send(Cmd::Next),
            Action::Prev => self.engine.send(Cmd::Prev),
            Action::SeekBack | Action::SeekFwd | Action::SeekBackBig | Action::SeekFwdBig => {
                if !self.can_seek() {
                    self.info("This stream can't seek");
                    return;
                }
                let ms = match a {
                    Action::SeekBack => -small,
                    Action::SeekFwd => small,
                    Action::SeekBackBig => -big,
                    _ => big,
                };
                self.engine.send(Cmd::SeekBy(ms));
            }
            Action::VolUp | Action::VolDown => {
                let d = if a == Action::VolUp { 0.05 } else { -0.05 };
                let v = ((self.cfg.volume + d) * 20.0).round() / 20.0;
                self.set_volume(v);
            }
            Action::Shuffle => {
                let mut q = self.engine.queue.lock();
                let on = !q.shuffle;
                q.set_shuffle(on);
                drop(q);
                self.after_queue_edit();
                self.info(if on { "Shuffle on" } else { "Shuffle off" });
            }
            Action::Repeat => {
                let r = {
                    let mut q = self.engine.queue.lock();
                    q.repeat = q.repeat.next();
                    q.repeat
                };
                self.info(format!("Repeat {r}"));
                self.mpris_update();
            }
            Action::AddQueue | Action::PlayNext => {
                if let Some((nodes, tracks, from)) = self.targets() {
                    let then = if a == Action::AddQueue { Then::Queue { from } } else { Then::PlayNext { from } };
                    self.worker.send(Job::Tracks { nodes, tracks, then });
                }
            }
            Action::Search => self.open_search(false),
            Action::ToggleScreen => {
                self.screen = match self.screen {
                    Screen::Library => Screen::NowPlaying,
                    Screen::NowPlaying => Screen::Library,
                };
                self.focus = if self.screen == Screen::NowPlaying { Focus::Panel } else { Focus::List };
            }
            Action::Lyrics => {
                self.cfg.lyrics = !self.cfg.lyrics;
                self.save_cfg();
                self.info(if self.cfg.lyrics { "Lyrics on" } else { "Lyrics off" });
            }
            Action::Visualizer => {
                self.cfg.visualizer = !self.cfg.visualizer;
                self.save_cfg();
                self.info(if self.cfg.visualizer { "Visualizer on" } else { "Visualizer off" });
            }
            Action::Queue => match self.screen {
                Screen::Library => {
                    self.side_cursor = 1;
                    self.open_side();
                    self.focus = Focus::List;
                }
                Screen::NowPlaying => self.focus = Focus::List,
            },
            Action::Themes => self.dialog = Some(Dialog::themes(self)),
            Action::Settings => self.dialog = Some(Dialog::settings()),
            Action::Help => self.dialog = Some(Dialog::help()),
            Action::NextPane | Action::PrevPane => {
                let order: &[Focus] = match self.screen {
                    Screen::Library => &[Focus::Side, Focus::List, Focus::Panel],
                    Screen::NowPlaying => &[Focus::Panel, Focus::List],
                };
                let i = order.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
                let d = if a == Action::NextPane { 1 } else { -1 };
                self.focus = order[(i + d).rem_euclid(order.len() as isize) as usize];
            }
            Action::Down => self.move_cursor(1, 0),
            Action::Up => self.move_cursor(-1, 0),
            Action::Top => self.move_cursor(isize::MIN, 0),
            Action::Bottom => self.move_cursor(isize::MAX, 0),
            Action::PageDown => self.move_cursor(1, self.hit.page.max(1)),
            Action::PageUp => self.move_cursor(-1, self.hit.page.max(1)),
            Action::Back => self.back(),
            Action::MoveDown | Action::MoveUp => {
                let d: isize = if a == Action::MoveDown { 1 } else { -1 };
                if self.queue_view_active() {
                    if let Some(id) = self.queue_cursor_id()
                        && self.engine.queue.lock().move_by(id, d)
                    {
                        if self.screen == Screen::NowPlaying {
                            self.upnext_cursor = (self.upnext_cursor as isize + d).max(0) as usize;
                        } else if let Some(v) = self.list_mut() {
                            v.cursor = (v.cursor as isize + d).max(0) as usize;
                        }
                        self.after_queue_edit();
                    }
                } else if let Some((id, cur, n)) = self.list().and_then(|v| Some((v.playlist_id()?, v.cursor, v.items.len()))) {
                    let to = cur as isize + d;
                    if to >= 0 && (to as usize) < n {
                        self.worker.send(Job::MoveInPlaylist { id, a: cur as i64, b: to as i64 });
                        if let Some(v) = self.list_mut() {
                            v.items.swap(cur, to as usize);
                            v.cursor = to as usize;
                        }
                    }
                }
            }
            Action::Remove => {
                if self.queue_view_active() {
                    if let Some(id) = self.queue_cursor_id() {
                        self.engine.queue.lock().remove(id);
                        self.after_queue_edit();
                    }
                } else if self.screen == Screen::Library && self.focus == Focus::List {
                    if let Some((id, cur)) = self.list().and_then(|v| Some((v.playlist_id()?, v.cursor)))
                        && self.list().is_some_and(|v| cur < v.items.len())
                    {
                        self.worker.send(Job::RemoveFromPlaylist { id, pos: cur as i64 });
                        if let Some(v) = self.list_mut() {
                            v.items.remove(cur);
                            v.cursor = v.cursor.min(v.items.len().saturating_sub(1));
                        }
                    }
                } else if let Some(Side::Playlist { id, name, .. }) = self.selected_side().cloned() {
                    self.dialog = Some(Dialog::confirm(format!("Delete playlist \"{name}\"?"), dialogs::Confirm::DeletePlaylist(id)));
                }
            }
            Action::FetchLyrics => {
                let Some(t) = self.now().track else {
                    self.info("Nothing playing");
                    return;
                };
                if self.current_lyric().is_some_and(|(l, _)| !l.synced.is_empty()) {
                    self.info("This song already has synced lyrics");
                    return;
                }
                if self.lyrics_lookup.is_some() {
                    return;
                }
                self.lyrics_lookup = Some(Instant::now());
                self.worker.send(Job::FetchLyrics { track: t, next_to_song: self.cfg.save_lyrics_next });
            }
            Action::MiniMode => self.mini = !self.mini,
            Action::FullLyrics => self.lyrics_full = !self.lyrics_full,
            Action::SleepTimer => self.dialog = Some(Dialog::sleep(self)),
            Action::OpenVideo => self.open_video(),
            Action::Download => match self.targets().and_then(|(_, t, _)| t.into_iter().next()) {
                Some(t) if t.r.as_ref().is_some_and(|r| &*r.source == crate::source::podcast::ID) => {
                    self.send_job(Job::Download(t.tref().id.clone()))
                }
                _ => self.info("Select a podcast episode to download"),
            },
            Action::Filter => {
                // Not in the queue or playlists: moving and removing there needs the full list.
                let ok =
                    self.screen == Screen::Library && self.list().is_some_and(|v| v.kind == ListKind::Node && v.playlist_id().is_none());
                if ok && let Some(v) = self.list_mut() {
                    v.filtering = true;
                    self.focus = Focus::List;
                } else {
                    self.info("This list can't be filtered");
                }
            }
            Action::Equalizer => self.dialog = Some(Dialog::Eq { band: 0 }),
            Action::LyricsEarlier | Action::LyricsLater => {
                let Some(r) = self.lyrics.as_ref().map(|(r, _)| r.clone()) else {
                    self.info("No lyrics for this song");
                    return;
                };
                self.lyrics_offset += if a == Action::LyricsLater { 500 } else { -500 };
                self.worker.send(Job::LyricsOffset { track: r, ms: self.lyrics_offset });
                self.info(format!("Lyrics offset {:+.1} s", self.lyrics_offset as f64 / 1000.0));
            }
            Action::Rate1 | Action::Rate2 | Action::Rate3 | Action::Rate4 | Action::Rate5 | Action::RateClear => {
                let rating = match a {
                    Action::Rate1 => Some(1),
                    Action::Rate2 => Some(2),
                    Action::Rate3 => Some(3),
                    Action::Rate4 => Some(4),
                    Action::Rate5 => Some(5),
                    _ => None,
                };
                let track = match self.targets() {
                    Some((_, tracks, _)) if tracks.len() == 1 => tracks[0].clone(),
                    _ => {
                        self.info("Select a song to rate");
                        return;
                    }
                };
                let r = track.tref().clone();
                self.ratings.insert(r.clone(), rating);
                self.worker.send(Job::Rate { track: r, rating });
                self.info(match rating {
                    Some(n) => format!("{} {}", stars(n), track.display_title()),
                    None => "Rating cleared".into(),
                });
            }
            Action::Like if matches!(self.selected_item(), Some(Item::Node(n)) if n.id.starts_with(crate::source::podcast::FEED)) => {
                if let Some(Item::Node(n)) = self.selected_item() {
                    self.info("Checking the feed…");
                    self.send_job(Job::Subscribe(n.id[crate::source::podcast::FEED.len()..].to_string()));
                }
            }
            Action::Like => {
                if let Some((_, tracks, _)) = self.targets()
                    && let Some(t) = tracks.first()
                {
                    self.worker.send(Job::Like(t.tref().clone()));
                    self.last_liked = Some(t.clone());
                }
            }
            Action::EditTags => {
                if let Some((nodes, tracks, _)) = self.targets() {
                    self.send_job(Job::Tracks { nodes, tracks, then: Then::EditTags });
                }
            }
            Action::AddToPlaylist => {
                if let Some((nodes, tracks, _)) = self.targets() {
                    self.dialog = Some(Dialog::playlist_pick(nodes, tracks, &self.playlists));
                }
            }
            Action::NewPlaylist => self.dialog = Some(Dialog::input("New playlist", "Name", String::new(), dialogs::InputFor::NewPlaylist)),
            Action::RenamePlaylist => {
                if let Some(Side::Playlist { id, name, .. }) = self.selected_side().cloned() {
                    self.dialog = Some(Dialog::input("Rename playlist", "Name", name, dialogs::InputFor::RenamePlaylist(id)));
                }
            }
            Action::ExportPlaylist => {
                let pl = match self.selected_side() {
                    Some(Side::Playlist { id, name, .. }) => Some((*id, name.clone())),
                    _ => self.list().and_then(|v| Some((v.playlist_id()?, v.title.clone()))),
                };
                if let Some((id, name)) = pl {
                    let path = format!("~/Music/Playlists/{name}.m3u8");
                    self.dialog = Some(Dialog::input("Export playlist", "Save as", path, dialogs::InputFor::Export(id)));
                }
            }
            Action::ClearQueue => {
                self.engine.queue.lock().clear();
                self.after_queue_edit();
                self.info("Queue cleared");
            }
            Action::GridView => {
                self.cfg.album_grid = !self.cfg.album_grid;
                self.save_cfg();
                self.grid_pics.clear();
            }
            Action::PaneShrink | Action::PaneGrow => {
                let d: i32 = if a == Action::PaneGrow { 2 } else { -2 };
                self.cfg.sidebar_width = (self.cfg.sidebar_width as i32 + d).clamp(18, 70) as u16;
                self.save_cfg();
            }
            Action::Rescan => {
                self.lib.scan();
                self.info("Scanning music folders");
            }
            Action::Quit => {
                if self.cfg.confirm_quit {
                    self.dialog = Some(Dialog::quit());
                } else {
                    self.quit = true;
                }
            }
        }
        self.dirty = true;
    }

    /// The playing track's source supports seeking (radio in v2 does not).
    fn can_seek(&self) -> bool {
        let Some(r) = self.now().track.and_then(|t| t.r.clone()) else { return false };
        self.sources.get(&r.source).is_ok_and(|s| s.caps().contains(crate::source::Caps::SEEK))
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.cfg.volume = v;
        self.engine.send(Cmd::Volume(v));
        self.info(format!("Volume {}%", (v * 100.0).round()));
        self.mpris_update();
    }

    // ---------- events ----------

    fn on_term(&mut self, e: TermEvent) {
        match e {
            TermEvent::Key(k) if k.kind != KeyEventKind::Release => {
                if let Some(mut d) = self.dialog.take() {
                    let keep = d.key(self, k);
                    if keep && self.dialog.is_none() {
                        self.dialog = Some(d);
                    }
                    self.dirty = true;
                    return;
                }
                if self.filter_key(&k) {
                    self.dirty = true;
                    return;
                }
                if let Some(a) = self.keys.action(&k) {
                    self.act(a);
                }
            }
            TermEvent::Mouse(m) => self.on_mouse(m),
            TermEvent::Resize(..) => {
                self.grid_pics.clear();
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn on_mouse(&mut self, m: crossterm::event::MouseEvent) {
        use clicks::Click;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let p = ratatui::layout::Position::new(m.column, m.row);
        let key = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        // Buttons and key hints drawn this frame (dialogs on top).
        if m.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(c) = clicks::at(p)
        {
            match c {
                Click::Act(a) if self.dialog.is_none() => self.act(a),
                Click::Act(_) => {}
                Click::Key(k) => self.on_term(TermEvent::Key(k)),
                Click::Row(i) => {
                    if let Some(mut d) = self.dialog.take() {
                        let keep = d.click_row(self, i);
                        if keep && self.dialog.is_none() {
                            self.dialog = Some(d);
                        }
                    }
                }
            }
            self.dirty = true;
            return;
        }
        if self.dialog.is_some() {
            // The wheel moves the dialog's selection, like the arrow keys.
            let k = match m.kind {
                MouseEventKind::ScrollDown => key(KeyCode::Down),
                MouseEventKind::ScrollUp => key(KeyCode::Up),
                _ => return,
            };
            self.on_term(TermEvent::Key(k));
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let d = if m.kind == MouseEventKind::ScrollDown { 3 } else { -3 };
                if self.hit.side.contains(p) {
                    self.focus = Focus::Side;
                } else if self.hit.list.contains(p) {
                    self.focus = Focus::List;
                }
                for _ in 0..3 {
                    self.move_cursor(d / 3, 0);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.hit.progress.contains(p) && self.hit.progress.width > 0 {
                    if let Some(len) = self.now().track.and_then(|t| t.duration_ms) {
                        let frac = (m.column - self.hit.progress.x) as f64 / self.hit.progress.width as f64;
                        self.engine.send(Cmd::SeekTo(Duration::from_millis((len as f64 * frac) as u64)));
                    }
                } else if self.hit.side.contains(p) && self.screen == Screen::Library {
                    self.focus = Focus::Side;
                    let i = (m.row - self.hit.side.y) as usize + self.side_offset;
                    if self.side.get(i).is_some_and(Side::selectable) {
                        if self.side_cursor == i {
                            self.enter();
                        } else {
                            self.side_cursor = i;
                            self.open_side();
                        }
                    }
                } else if self.hit.list.contains(p) {
                    self.focus = Focus::List;
                    if let Some(i) = self.hit.row_at(p) {
                        if self.screen == Screen::NowPlaying {
                            self.upnext_cursor = i;
                        } else if let Some(v) = self.list_mut()
                            && i < v.items.len()
                        {
                            if v.cursor == i {
                                self.enter();
                            } else {
                                v.cursor = i;
                            }
                        }
                    }
                } else if self.hit.panel.contains(p) {
                    self.focus = Focus::Panel;
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn on_engine(&mut self, e: EngineEvent) {
        match e {
            EngineEvent::Track(t) => {
                self.lastfm_track(t.as_ref());
                if let Some(t) = &t {
                    self.podcast_resume(t);
                }
                if let Some(t) = &t {
                    if let Some(r) = t.r.clone()
                        && self.lyrics.as_ref().is_none_or(|(lr, _)| *lr != r)
                    {
                        self.lyrics = None;
                        self.worker.send(Job::Lyrics { track: r });
                    }
                    self.art_file = t.art.as_ref().map(art::cache_file).filter(|p| p.exists());
                    if self.cfg.notifications {
                        crate::mpris::notify(t);
                    }
                }
                self.fill_queue_view();
                self.rebuild_side();
                if matches!(self.selected_side(), Some(Side::NowPlaying)) && self.lists.len() == 1 {
                    let node = t.as_ref().filter(|t| !t.album.is_empty()).map(|t| format!("album:{}", album_key(t)));
                    if node.as_deref() != self.list().map(|v| v.node.as_str()) {
                        self.open_side();
                    }
                }
                self.mpris_update();
            }
            EngineEvent::State(s) => {
                // Radio pause is a stop: the song on air was not heard to its end.
                if s != PlayState::Playing {
                    self.on_air = None;
                    self.podcast_save(true);
                }
                if s == PlayState::Stopped && matches!(self.sleep, Some(Sleep::Track | Sleep::Album)) {
                    self.sleep = None;
                    self.info("Sleep timer: stopped");
                }
                if s == PlayState::Stopped {
                    self.engine.tap.enabled.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                self.mpris_update();
            }
            EngineEvent::Volume => self.mpris_update(),
            EngineEvent::Seeked(p) => {
                if let Some(m) = &self.mpris {
                    m.seeked(p);
                }
            }
            EngineEvent::Error(e) => self.error(e),
            EngineEvent::Finished { track, listened } => {
                let len = track.duration_ms.unwrap_or(0);
                let need = Duration::from_millis((len / 2).clamp(1, 240_000));
                if listened >= need
                    && let Some(r) = track.r.clone()
                {
                    if &*r.source == crate::source::subsonic::ID || &*r.source == crate::source::jellyfin::ID {
                        self.worker.send(Job::ServerScrobble(r.clone()));
                    }
                    self.worker.send(Job::RecordPlay(r));
                }
                // Songs with a length (not radio, which is handled per song on air).
                if self.cfg.lastfm_scrobble
                    && track.duration_ms.is_some()
                    && !draw::is_live(&track)
                    && crate::lastfm::counts(track.duration_ms, listened)
                    && let Some(p) = lastfm_play(&track, crate::lastfm::now() - listened.as_secs() as i64)
                {
                    self.scrobbler.scrobble(p);
                }
            }
        }
        self.dirty = true;
    }

    fn on_lib(&mut self, e: LibEvent) {
        match e {
            LibEvent::Progress { done, total } => self.scan = if total > 0 { Some((done, total)) } else { None },
            LibEvent::Done { added, updated, removed, total } => {
                self.scan = None;
                self.library_size = total;
                if !self.edited.is_empty() {
                    // Songs whose tags we just saved: refresh them in the queue too.
                    let fresh: Vec<Arc<Track>> = std::mem::take(&mut self.edited)
                        .iter()
                        .filter_map(|r| self.sources.get(&r.source).ok()?.track(r).ok())
                        .map(Arc::new)
                        .collect();
                    self.engine.refresh_tracks(&fresh);
                    self.fill_queue_view();
                }
                if added > 0 || removed > 0 || updated > 0 {
                    for v in &mut self.lists {
                        v.loading = true;
                    }
                    let nodes: Vec<String> = self.lists.iter().map(|v| v.node.clone()).collect();
                    for n in nodes {
                        self.reload_lists_matching(&n);
                    }
                    if added > 0 || removed > 0 {
                        self.info(format!("Library: {total} songs"));
                    }
                }
            }
            LibEvent::Loudness { done, total } => self.loudness = if done < total { Some((done, total)) } else { None },
            LibEvent::Error(e) => self.error(e),
        }
        self.dirty = true;
    }

    fn on_worker(&mut self, m: Msg) {
        match m {
            Msg::Children { seq, items } => {
                if self.lists.iter().any(|v| v.seq == seq && v.node == crate::source::podcast::DOWNLOADS) {
                    self.downloads_label = crate::source::podcast::fmt_size(self.podcasts.downloads_size());
                }
                if let Some(v) = self.lists.iter_mut().find(|v| v.seq == seq) {
                    v.loading = false;
                    match items {
                        Ok(items) => {
                            if v.all.is_some() {
                                v.all = Some(items);
                                let c = v.cursor;
                                v.apply_filter();
                                v.cursor = c.min(v.items.len().saturating_sub(1));
                            } else {
                                v.items = items;
                            }
                            v.cursor = v.cursor.min(v.items.len().saturating_sub(1));
                            v.error = None;
                        }
                        Err(e) => v.error = Some(e),
                    }
                }
            }
            Msg::Stations { seq, list } => {
                if seq == self.search_seq {
                    self.search_stations = list;
                    self.stations_busy = false;
                }
            }
            Msg::Search { seq, hits } => {
                if seq == self.search_seq {
                    self.search_hits = hits;
                }
            }
            Msg::Lyrics { track, lyrics, offset } => {
                self.lyrics_offset = offset;
                self.lyrics = lyrics.map(|l| (track, l));
            }
            Msg::Restored { session, tracks } => self.restore(session, tracks),
            Msg::LookupDone => self.lyrics_lookup = None,
            Msg::Rated => {
                if let Some(node) = self.list().map(|v| v.node.clone()).filter(|n| n.starts_with("smart:")) {
                    self.reload_lists_matching(&node);
                }
            }
            Msg::Tracks { tracks, then } => self.on_tracks(tracks, then),
            Msg::ServerHits { seq, items } => {
                if seq == self.search_seq {
                    self.server_hits = items;
                }
            }
            Msg::ServerLogin(r) => match r {
                Ok((label, version)) => {
                    self.info(format!("Music server connected: {label} ({version})"));
                    self.rebuild_side();
                }
                Err(e) => self.error(e),
            },
            Msg::Subscribed { title, on } => {
                self.info(if on { format!("Subscribed to {title}") } else { format!("Unsubscribed from {title}") });
                self.reload_lists_matching(crate::source::podcast::SHOWS);
                self.reload_lists_matching(crate::source::podcast::NEW);
            }
            Msg::Downloaded(s) => {
                self.info(s);
                let nodes: Vec<String> = self.lists.iter().filter(|v| v.node.starts_with("podcast:")).map(|v| v.node.clone()).collect();
                for n in nodes {
                    self.reload_lists_matching(&n);
                }
            }
            Msg::LastfmLogin(r) => match r {
                Ok(u) => {
                    self.info(format!("Last.fm: logged in as {u}"));
                    self.lastfm_user = Some(u);
                    self.rebuild_side();
                }
                Err(e) => self.error(e),
            },
            Msg::StreamAdded(r) => match r {
                Ok(name) => {
                    self.info(format!("Added {name} to radio favorites"));
                    self.reload_lists_matching(crate::source::radio::FAVORITES);
                }
                Err(e) => self.error(e),
            },
            Msg::RadioFav(on) => {
                self.info(if on { "Added to radio favorites" } else { "Removed from radio favorites" });
                self.reload_lists_matching(crate::source::radio::FAVORITES);
            }
            Msg::MbResults(r) => {
                if let Some(d) = &mut self.dialog {
                    d.mb_results(r);
                }
            }
            Msg::MbFill(r) => {
                if let Some(d) = &mut self.dialog {
                    d.mb_fill(r);
                }
            }
            Msg::EditTags(info) => self.dialog = Some(Dialog::tags(*info)),
            Msg::TagsSaved { ok, failed, arts, songs } => {
                self.edited.extend(songs);
                if !arts.is_empty() {
                    self.grid_pics.retain(|a, _| !arts.contains(a));
                    self.pics.retain(|(_, p)| !arts.contains(&p.art));
                    if let Some(w) = &self.art {
                        w.forget(arts);
                    }
                }
                if failed.is_empty() {
                    self.info(format!("Saved tags for {ok} song{}", if ok == 1 { "" } else { "s" }));
                } else {
                    self.error(format!("Saved {ok}, failed {}: {}", failed.len(), failed[0]));
                }
                if ok > 0 {
                    self.lib.scan();
                }
            }
            Msg::Playlists(p) => {
                self.playlists = p;
                self.rebuild_side();
            }
            Msg::Liked(on) => {
                let mut loved = false;
                if self.cfg.lastfm_scrobble
                    && crate::lastfm::user().is_some()
                    && let Some(p) = self.last_liked.take().and_then(|t| lastfm_play(&t, 0))
                {
                    self.scrobbler.love(p, on);
                    loved = true;
                }
                let msg = match (on, loved) {
                    (true, true) => "Liked ♥ (and loved on Last.fm)",
                    (true, false) => "Liked ♥",
                    (false, true) => "Like removed (also on Last.fm)",
                    (false, false) => "Like removed",
                };
                self.info(msg);
            }
            Msg::Info(s) => self.info(s),
            Msg::Error(s) => self.error(s),
            Msg::PlaylistChanged(id) => self.reload_lists_matching(&format!("playlist:{id}")),
        }
        self.dirty = true;
    }

    fn on_art(&mut self, a: ArtReady) {
        let pic = Pic { art: a.art.clone(), cells: a.cells, proto: a.proto };
        if a.slot == Slot::Grid {
            self.grid_pics.insert(a.art.clone(), pic);
        } else {
            self.pics.retain(|(s, _)| *s != a.slot);
            self.pics.push((a.slot, pic));
        }
        if let Some(f) = a.file {
            let cur = self.now().track.and_then(|t| t.art.clone());
            if cur.as_ref() == Some(&a.art) && self.art_file.as_ref() != Some(&f) {
                self.art_file = Some(f);
                self.mpris_update();
            }
        }
        self.dirty = true;
    }

    /// Ask the art worker for an image if we do not have it at this size.
    pub fn want_art(&mut self, art: &ArtRef, cells: Size, slot: Slot) {
        let Some(w) = &self.art else { return };
        let have = match slot {
            Slot::Grid => self.grid_pics.get(art).map(|p| p.cells == cells),
            _ => self.pics.iter().find(|(s, _)| *s == slot).map(|(_, p)| p.art == *art && p.cells == cells),
        };
        if have != Some(true) {
            // Mark as requested so we do not ask again every frame.
            let pic = Pic { art: art.clone(), cells, proto: None };
            match slot {
                Slot::Grid => {
                    self.grid_pics.insert(art.clone(), pic);
                }
                _ => {
                    let old = self.pics.iter().position(|(s, _)| *s == slot);
                    // Keep showing the old picture until the new one is ready, if it is the same art.
                    if old.map(|i| self.pics[i].1.art != *art).unwrap_or(true) {
                        self.pics.retain(|(s, _)| *s != slot);
                        self.pics.push((slot, pic));
                    } else if let Some(i) = old {
                        self.pics[i].1.cells = cells;
                    }
                }
            }
            w.request(art.clone(), cells, slot);
        }
    }

    fn on_ipc(&mut self, req: Request) {
        let reply = match req.cmd.as_str() {
            "play" => {
                self.engine.send(Cmd::Play);
                None
            }
            "pause" => {
                self.engine.send(Cmd::Pause);
                None
            }
            "toggle" => {
                self.engine.send(Cmd::Toggle);
                None
            }
            "next" => {
                self.engine.send(Cmd::Next);
                None
            }
            "prev" => {
                self.engine.send(Cmd::Prev);
                None
            }
            "stop" => {
                self.engine.send(Cmd::Stop);
                None
            }
            "status" => None,
            other => Some(format!("unknown command {other}")),
        };
        // Give the engine a moment so status reflects the command.
        if req.cmd != "status" {
            std::thread::sleep(Duration::from_millis(30));
        }
        let status = ipc::status(&self.engine);
        let _ = req.reply.send(match reply {
            Some(err) => serde_json::json!({ "error": err }),
            None => status,
        });
        self.dirty = true;
    }

    pub fn mpris_update(&mut self) {
        if let Some(m) = &self.mpris {
            m.update(&self.engine, self.art_file.clone());
        }
    }

    /// When the loop should wake up next, if anything moves.
    fn tick_interval(&self) -> Option<Duration> {
        let snap = self.engine.snapshot();
        let mut iv: Option<Duration> = None;
        let mut want = |d: Duration| iv = Some(iv.map_or(d, |x: Duration| x.min(d)));
        if snap.state == PlayState::Playing {
            want(Duration::from_millis(250));
            if self.cfg.visualizer && self.viz_area.area() > 0 {
                want(Duration::from_secs_f64(1.0 / self.cfg.viz_fps.max(1) as f64));
            }
        } else if self.viz.bars.iter().any(|b| *b > 0.0) && self.viz_area.area() > 0 {
            want(Duration::from_millis(33));
        }
        if self.lyrics_lookup.is_some() {
            want(Duration::from_millis(100));
        }
        if snap.state == PlayState::Playing
            && let Some(d) = self.lyric_wait()
        {
            want(d);
        }
        if let Some(Sleep::At(t)) = self.sleep {
            want(t.saturating_duration_since(Instant::now()).max(Duration::from_millis(50)).min(Duration::from_secs(30)));
        }
        if let Some((_, _, t)) = &self.status {
            let left = Duration::from_secs(4).saturating_sub(t.elapsed());
            want(left.max(Duration::from_millis(50)));
        }
        if self.dialog.as_ref().is_some_and(Dialog::wants_tick) {
            want(Duration::from_millis(120));
        }
        iv
    }

    fn on_tick(&mut self) {
        self.podcast_save(false);
        if let Some(Sleep::At(t)) = self.sleep
            && Instant::now() >= t
        {
            self.sleep = None;
            self.engine.send(Cmd::Pause);
            self.info("Sleep timer: paused");
        }
        if self.status.as_ref().is_some_and(|(_, _, t)| t.elapsed() >= Duration::from_secs(4)) {
            self.status = None;
        }
        let tap_on = self.cfg.visualizer && self.viz_area.area() > 0;
        self.engine.tap.enabled.store(tap_on, std::sync::atomic::Ordering::Relaxed);
        if tap_on {
            let dt = self.last_viz.elapsed().as_secs_f32().min(0.2);
            self.last_viz = Instant::now();
            let bars = draw::viz_bars(self.viz_area);
            self.viz.update(&self.engine.tap, bars, dt);
        }
        if let Some(d) = self.dialog.as_mut()
            && d.wants_tick()
        {
            let mut d = self.dialog.take().unwrap();
            d.tick(self);
            if self.dialog.is_none() {
                self.dialog = Some(d);
            }
        }
        self.dirty = true;
    }

    /// Re-create the art worker after the image setting changed.
    pub fn restart_images(&mut self) {
        let mut p = self.picker.clone();
        let kitty = self.kitty_ok && self.cfg.images != "halfblocks";
        p.set_protocol_type(if kitty { ProtocolType::Kitty } else { ProtocolType::Halfblocks });
        self.picker = p.clone();
        self.images_on = self.cfg.images != "off";
        self.pics.clear();
        self.grid_pics.clear();
        self.art = self.images_on.then(|| ArtWorker::start(self.sources.clone(), p, self.art_tx.clone()));
    }

    pub fn restart_mpris(&mut self) {
        self.mpris = None;
        if self.cfg.mpris {
            match crate::mpris::Mpris::start(engine_handle(&self.engine)) {
                Ok(m) => {
                    self.mpris = Some(m);
                    self.mpris_update();
                }
                Err(e) => self.error(format!("{e:#}")),
            }
        }
    }

    pub fn current_lyric(&self) -> Option<(&Lyrics, Option<usize>)> {
        let (r, l) = self.lyrics.as_ref()?;
        let t = self.now().track?;
        if t.r.as_ref() != Some(r) {
            return None;
        }
        let ms = self.lyric_clock();
        Some((l, lrc::current(l, ms)))
    }

    pub fn send_job(&self, j: Job) {
        self.worker.send(j);
    }

    /// Open the search box; `stations_only` from "Find a station…".
    pub fn open_search(&mut self, stations_only: bool) {
        self.stations_only = stations_only;
        self.search_hits = Default::default();
        self.search_stations.clear();
        self.server_hits.clear();
        self.stations_busy = false;
        self.dialog = Some(Dialog::search());
    }

    pub fn search(&mut self, q: String) {
        self.search_seq += 1;
        let seq = self.search_seq;
        self.search_stations.clear();
        self.stations_busy = q.trim().chars().count() >= 3;
        if self.stations_busy {
            self.worker.send(Job::FindStations { seq, query: q.trim().to_string() });
        }
        self.server_hits.clear();
        if !self.stations_only && q.trim().chars().count() >= 2 && self.server_nodes().is_some() {
            self.worker.send(Job::FindServer { seq, query: q.trim().to_string() });
        }
        if !self.stations_only {
            self.worker.send(Job::Search { seq, query: q });
        }
    }
}

fn side_key(s: &Side) -> String {
    match s {
        Side::NowPlaying => "np".into(),
        Side::Queue => "queue".into(),
        Side::Node { node, .. } => node.clone(),
        Side::Playlist { id, .. } => format!("playlist:{id}"),
        _ => String::new(),
    }
}

pub fn album_key(t: &Track) -> String {
    let artist = if t.album_artist.is_empty() { &t.artist } else { &t.album_artist };
    format!("{}\u{1f}{}", artist.to_lowercase(), t.album.to_lowercase())
}

pub fn album_title(t: &Track) -> String {
    let artist = if t.album_artist.is_empty() { &t.artist } else { &t.album_artist };
    match t.year {
        Some(y) => format!("{} · {} · {}", t.album, artist, y),
        None => format!("{} · {}", t.album, artist),
    }
}

/// Start the TUI.
fn item_key(i: &Item) -> String {
    match i {
        Item::Track(t) => t.r.as_ref().map(|r| format!("{}:{}", r.source, r.id)).unwrap_or_default(),
        Item::Node(n) => n.id.clone(),
    }
}

fn is_podcast(t: &Track) -> bool {
    t.r.as_ref().is_some_and(|r| &*r.source == crate::source::podcast::ID)
}

/// What Last.fm gets for a song; None without artist or title.
fn lastfm_play(t: &Track, at: i64) -> Option<crate::lastfm::Play> {
    if t.r.as_ref().is_some_and(|r| &*r.source == crate::source::podcast::ID) {
        return None;
    }
    let live = draw::is_live(t);
    (!t.artist.is_empty() && !t.title.is_empty()).then(|| crate::lastfm::Play {
        artist: t.artist.clone(),
        title: t.title.clone(),
        // The station name is not an album.
        album: if live { String::new() } else { t.album.clone() },
        album_artist: if live { String::new() } else { t.album_artist.clone() },
        duration: t.duration_ms.unwrap_or(0) / 1000,
        at,
    })
}

/// Sidebar entry that asks for a radio search.
pub const RADIO_FIND: &str = "radio:find";
/// Sidebar entry that asks for a stream address.
pub const RADIO_ADD: &str = "radio:add";
/// Sidebar entry that asks for a podcast name or feed address.
pub const PODCAST_FIND: &str = "podcast:find";
/// Sidebar entry that opens the Last.fm profile in the browser.
pub const LASTFM_PROFILE: &str = "lastfm:profile";

/// A background part ended unexpectedly.
fn stopped(what: &str) -> anyhow::Error {
    anyhow::anyhow!("internal error: the {what} part stopped")
}

pub fn run(cfg: Config) -> Result<()> {
    let (lib_tx, lib_rx) = cb::unbounded();
    let lib = Library::new(crate::config::data_dir().join("library.db"), cfg.folders(), lib_tx);
    lib.measure_loudness.store(cfg.measure_loudness, std::sync::atomic::Ordering::Relaxed);
    let local = Arc::new(LocalSource::new(lib.clone())?);
    let mut sources = Sources::default();
    sources.add(local.clone());
    let radio = Arc::new(crate::source::radio::RadioSource::open(&crate::config::data_dir().join("radio.db"))?);
    sources.add(radio.clone());
    let podcasts = Arc::new(crate::source::podcast::PodcastSource::open(
        &crate::config::data_dir().join("podcasts.db"),
        crate::config::data_dir().join("podcasts"),
    )?);
    podcasts.auto_download.store(cfg.podcast_auto_download, std::sync::atomic::Ordering::Relaxed);
    podcasts.start_refresher();
    sources.add(podcasts.clone());
    let server = Arc::new(crate::source::subsonic::SubsonicSource::new());
    sources.add(server.clone());
    let jellyfin = Arc::new(crate::source::jellyfin::JellyfinSource::default());
    sources.add(jellyfin.clone());
    let sources = Arc::new(sources);
    let engine = Engine::start(sources.clone(), cfg.volume, cfg.gain())?;
    engine.set_eq(cfg.eq_on, &cfg.eq_bands);

    let ipc_rx = ipc::serve()?;

    // Terminal.
    let mut terminal = ratatui::init();
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;
    // Asking the terminal about pictures: tmux/screen and the Linux console don't
    // answer, and the waiting reader would then swallow the first key press.
    let term = std::env::var("TERM").unwrap_or_default();
    let no_answer =
        std::env::var_os("TMUX").is_some() || term.starts_with("screen") || term.starts_with("tmux") || term == "linux" || term == "dumb";
    let mut picker = if no_answer { Picker::halfblocks() } else { Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()) };
    let kitty_ok = picker.protocol_type() == ProtocolType::Kitty;
    // Real images on Kitty/Ghostty, half-blocks everywhere else.
    if !kitty_ok || cfg.images == "halfblocks" {
        picker.set_protocol_type(ProtocolType::Halfblocks);
    }
    let (in_tx, in_rx) = cb::unbounded();
    std::thread::Builder::new().name("input".into()).spawn(move || {
        loop {
            match crossterm::event::read() {
                Ok(e) => {
                    if in_tx.send(e).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("musinator: reading the keyboard failed: {e}");
                    break;
                }
            }
        }
    })?;
    let (w_tx, w_rx) = cb::unbounded();
    let worker =
        Worker::start(worker::Srcs { local, radio, podcasts: podcasts.clone(), server: server.clone(), jellyfin: jellyfin.clone() }, w_tx);
    let (a_tx, a_rx) = cb::unbounded();
    let images_on = cfg.images != "off";
    let art = images_on.then(|| ArtWorker::start(sources.clone(), picker.clone(), a_tx.clone()));
    let (big_tx, big_rx) = cb::unbounded();
    let big = kitty_ok.then(|| biglyric::BigLyrics::start(picker.clone(), big_tx));
    // Parts that are not running (no Kitty pictures, not on Omarchy) must never wake the
    // loop: their closed channels would otherwise look ready all the time.
    let big_rx = if big.is_some() { big_rx } else { cb::never() };
    let (theme_tx, theme_rx) = cb::unbounded::<()>();
    let _theme_watch = watch_omarchy(theme_tx);
    let theme_rx = if _theme_watch.is_some() { theme_rx } else { cb::never() };
    let mpris = if cfg.mpris { crate::mpris::Mpris::start(engine_handle(&engine)).ok() } else { None };

    let mut app = App {
        theme: App::load_theme(&cfg.theme),
        keys: Keymap::new(&cfg.keys),
        cfg,
        screen: Screen::Library,
        focus: Focus::Side,
        side: Vec::new(),
        side_cursor: 3,
        side_offset: 0,
        lists: Vec::new(),
        upnext_cursor: 0,
        upnext_offset: 0,
        dialog: None,
        playlists: Vec::new(),
        engine,
        lib: lib.clone(),
        edited: Vec::new(),
        scrobbler: crate::lastfm::Scrobbler::start(),
        lastfm_user: crate::lastfm::user(),
        on_air: None,
        last_liked: None,
        podcasts,
        downloads_label: String::new(),
        server,
        jellyfin,
        server_hits: Vec::new(),
        podcast_saved: Instant::now(),
        sources,
        worker,
        art,
        picker,
        images_on,
        pics: Vec::new(),
        grid_pics: Default::default(),
        art_file: None,
        lyrics: None,
        viz: viz::Viz::new(),
        status: None,
        scan: None,
        loudness: None,
        library_size: 0,
        seq: 0,
        dirty: true,
        quit: false,
        next_tick: None,
        last_viz: Instant::now(),
        viz_area: Rect::default(),
        hit: Default::default(),
        search_hits: Default::default(),
        search_seq: 0,
        search_stations: Vec::new(),
        stations_busy: false,
        stations_only: false,
        mpris,
        preview_theme: None,
        lyrics_offset: 0,
        sleep: None,
        lyrics_lookup: None,
        lyrics_full: false,
        mini: false,
        big,
        big_pics: Vec::new(),
        big_asked: Vec::new(),
        ratings: Default::default(),
        term_colors: None,
        kitty_ok,
        art_tx: a_tx,
    };
    // Start on "Albums".
    app.rebuild_side();
    app.side_cursor = app.side.iter().position(|s| matches!(s, Side::Node { node, .. } if node == "albums")).unwrap_or(0);
    app.open_side();
    app.worker.send(Job::Playlists);
    lib.scan();
    if app.cfg.watch_folders {
        lib.watch();
    }
    crate::bar::sync(app.cfg.omarchy_bar);
    if app.cfg.resume
        && let Some(s) = session::load()
    {
        app.worker.send(Job::Restore(s));
    }

    let res = (|| -> Result<()> {
        loop {
            if app.dirty {
                app.sync_term_colors();
                terminal.draw(|f| draw::draw(f, &mut app))?;
                app.dirty = false;
            }
            if app.quit {
                break;
            }
            let iv = app.tick_interval();
            app.next_tick = match (iv, app.next_tick) {
                (None, _) => None,
                (Some(d), Some(t)) if t <= Instant::now() + d => Some(t),
                (Some(d), _) => Some(Instant::now() + d),
            };
            let tick = app.next_tick.map(cb::at).unwrap_or_else(cb::never);
            let mpris_rx = app.mpris.as_ref().map(|m| m.commands.clone()).unwrap_or_else(cb::never);
            cb::select! {
                recv(in_rx) -> e => app.on_term(e.map_err(|_| stopped("keyboard input"))?),
                recv(app.engine.events) -> e => { let e = e.map_err(|_| stopped("audio engine"))?; app.on_engine(e) },
                recv(lib_rx) -> e => app.on_lib(e.map_err(|_| stopped("library"))?),
                recv(w_rx) -> m => app.on_worker(m.map_err(|_| stopped("worker"))?),
                recv(a_rx) -> a => app.on_art(a.map_err(|_| stopped("pictures"))?),
                recv(big_rx) -> b => {
                    let b = b.map_err(|_| stopped("big lyrics"))?;
                    app.big_pics.retain(|(k, _)| *k != b.key);
                    app.big_pics.push((b.key, b.proto));
                    if app.big_pics.len() > 40 {
                        app.big_pics.remove(0);
                    }
                    app.dirty = true;
                },
                recv(theme_rx) -> _ => { while theme_rx.try_recv().is_ok() {} app.reload_omarchy() },
                recv(ipc_rx) -> r => app.on_ipc(r.map_err(|_| stopped("command socket"))?),
                recv(mpris_rx) -> c => { if let Ok(c) = c { app.on_mpris(c) } },
                recv(tick) -> _ => { app.next_tick = None; app.on_tick() },
            }
        }
        Ok(())
    })();
    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture).ok();
    ratatui::restore();
    app.reset_term_colors();
    let _ = app.cfg.save();
    app.save_session();
    app.engine.send(Cmd::Stop);
    ipc::cleanup();
    res
}

fn engine_handle(e: &Engine) -> crate::mpris::EngineView {
    crate::mpris::EngineView::new(e)
}

impl App {
    fn on_mpris(&mut self, c: crate::mpris::MprisCmd) {
        use crate::mpris::MprisCmd as M;
        match c {
            M::Play => self.engine.send(Cmd::Play),
            M::Pause => self.engine.send(Cmd::Pause),
            M::Toggle => self.engine.send(Cmd::Toggle),
            M::Stop => self.engine.send(Cmd::Stop),
            M::Next => self.engine.send(Cmd::Next),
            M::Prev => self.engine.send(Cmd::Prev),
            M::SeekBy(us) => self.engine.send(Cmd::SeekBy(us / 1000)),
            M::SetPosition(us) => self.engine.send(Cmd::SeekTo(Duration::from_micros(us.max(0) as u64))),
            M::Volume(v) => self.set_volume(v as f32),
            M::Shuffle(on) => {
                self.engine.queue.lock().set_shuffle(on);
                self.after_queue_edit();
                self.mpris_update();
            }
            M::Loop(r) => {
                self.engine.queue.lock().repeat = r;
                self.mpris_update();
            }
        }
        self.dirty = true;
    }
}

/// Watch the Omarchy theme files; send a ping when they change.
fn watch_omarchy(tx: cb::Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let paths = musinator_themes::omarchy::watch_paths();
    if paths.is_empty() {
        return None;
    }
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
            let _ = tx.send(());
        }
    })
    .ok()?;
    for p in paths {
        let _ = w.watch(&p, RecursiveMode::NonRecursive);
    }
    Some(w)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sleep {
    At(Instant),
    Track,
    Album,
}

pub fn stars(n: u8) -> String {
    (0..5).map(|i| if i < n { '★' } else { '☆' }).collect()
}

impl App {
    /// Rating to show for a track (this session's changes win).
    pub fn rating_of(&self, t: &Track) -> Option<u8> {
        match t.r.as_ref().and_then(|r| self.ratings.get(r)) {
            Some(r) => *r,
            None => t.rating,
        }
    }

    pub fn set_sleep(&mut self, s: Option<Sleep>) {
        self.sleep = s;
        let stop = match s {
            Some(Sleep::Track) => Some(crate::engine::StopAfter::Track),
            Some(Sleep::Album) => Some(crate::engine::StopAfter::Album),
            _ => None,
        };
        self.engine.send(Cmd::StopAfter(stop));
        self.info(match s {
            None => "Sleep timer off".to_string(),
            Some(Sleep::At(t)) => format!("Sleep in {} min", t.saturating_duration_since(Instant::now()).as_secs().div_ceil(60)),
            Some(Sleep::Track) => "Stop at the end of this song".into(),
            Some(Sleep::Album) => "Stop at the end of this album".into(),
        });
    }

    /// Short text for the status bar.
    pub fn sleep_label(&self) -> Option<String> {
        Some(match self.sleep? {
            Sleep::At(t) => format!("sleep {}m", t.saturating_duration_since(Instant::now()).as_secs().div_ceil(60)),
            Sleep::Track => "sleep: end of song".into(),
            Sleep::Album => "sleep: end of album".into(),
        })
    }

    fn save_session(&mut self) {
        self.podcast_save(true);
        if !self.cfg.resume {
            session::clear();
            return;
        }
        let q = self.engine.queue.lock();
        let s = session::Session {
            items: q
                .items()
                .iter()
                .filter_map(|i| {
                    let r = i.track.r.as_ref()?;
                    let source = (&*r.source != crate::source::local::ID).then(|| r.source.to_string());
                    Some(session::Item { path: r.id.clone(), from: i.from.clone(), source })
                })
                .collect(),
            current: q.current().and_then(|i| i.track.r.as_ref().map(|r| r.id.clone())),
            position_ms: self.engine.position().as_millis() as u64,
            shuffle: q.shuffle,
            repeat: q.repeat.label().into(),
        };
        drop(q);
        let _ = session::save(&s);
    }

    fn restore(&mut self, s: session::Session, tracks: Vec<Arc<Track>>) {
        if tracks.is_empty() || !self.engine.queue.lock().is_empty() {
            return;
        }
        let from: std::collections::HashMap<&str, Option<String>> = s.items.iter().map(|i| (i.path.as_str(), i.from.clone())).collect();
        let cur = s.current.as_deref().and_then(|c| tracks.iter().position(|t| t.r.as_ref().is_some_and(|r| r.id == c)));
        let items = tracks.iter().map(|t| (t.clone(), t.r.as_ref().and_then(|r| from.get(r.id.as_str()).cloned().flatten()))).collect();
        let repeat = match s.repeat.as_str() {
            "all" => crate::model::Repeat::All,
            "one" => crate::model::Repeat::One,
            _ => crate::model::Repeat::Off,
        };
        let id = self.engine.queue.lock().restore(items, cur, s.shuffle, repeat);
        if let Some(id) = id {
            self.engine.send(Cmd::Cue(id, Duration::from_millis(s.position_ms)));
        }
        self.after_queue_edit();
        self.info(format!("Resumed: {} in queue", tracks.len()));
    }

    /// Give the terminal our background and text color (OSC 11 / OSC 10).
    fn sync_term_colors(&mut self) {
        use std::io::Write;
        let want = self.cfg.term_colors.then_some((self.theme.bg, self.theme.fg));
        if want == self.term_colors {
            return;
        }
        let mut out = std::io::stdout();
        match want {
            Some((bg, fg)) => {
                let _ = write!(out, "\x1b]11;{}\x07\x1b]10;{}\x07", bg.to_hex(), fg.to_hex());
            }
            None => {
                let _ = write!(out, "\x1b]111\x07\x1b]110\x07");
            }
        }
        let _ = out.flush();
        self.term_colors = want;
    }

    /// Put the terminal's own colors back.
    fn reset_term_colors(&mut self) {
        use std::io::Write;
        if self.term_colors.take().is_some() {
            let mut out = std::io::stdout();
            let _ = write!(out, "\x1b]111\x07\x1b]110\x07");
            let _ = out.flush();
        }
    }
}

impl App {
    /// "⠋ Looking up lyrics…" while an online lookup runs.
    pub fn lookup_label(&self) -> Option<String> {
        const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let t = self.lyrics_lookup?;
        Some(format!("{} Looking up lyrics", SPIN[(t.elapsed().as_millis() / 100) as usize % SPIN.len()]))
    }
}

impl App {
    /// Big lyric line picture for `key` if ready; asks for it otherwise.
    /// `Some(None)` means it can't be drawn big (use normal text).
    pub fn big_pic(&mut self, key: &biglyric::Key) -> Option<Option<&Protocol>> {
        let on = self.big_enabled();
        let big = self.big.as_ref().filter(|_| on)?;
        if let Some(i) = self.big_pics.iter().position(|(k, _)| k == key) {
            return Some(self.big_pics[i].1.as_ref());
        }
        if !self.big_asked.contains(key) {
            big.request(key.clone());
            self.big_asked.push(key.clone());
            if self.big_asked.len() > 80 {
                self.big_asked.remove(0);
            }
        }
        None
    }

    pub fn big_enabled(&self) -> bool {
        self.big.is_some() && self.cfg.big_lyrics && self.images_on && self.picker.protocol_type() == ProtocolType::Kitty
    }

    /// Prepare a line ahead of time (the next lyric line).
    pub fn big_prefetch(&mut self, key: biglyric::Key) {
        let _ = self.big_pic(&key);
    }
}

impl App {
    /// Song time used for lyrics: what you hear, minus your offset, plus the lead time.
    pub fn lyric_clock(&self) -> u64 {
        let lead = (self.cfg.lyrics_lead * 1000.0) as i64;
        (self.engine.position().as_millis() as i64 - self.lyrics_offset + lead).max(0) as u64
    }

    /// Time until the next lyric line change, when lyrics are on screen.
    fn lyric_wait(&self) -> Option<Duration> {
        let showing = self.lyrics_full || (self.screen == Screen::NowPlaying && self.cfg.lyrics);
        if !showing {
            return None;
        }
        let (l, cur) = self.current_lyric()?;
        let now = self.lyric_clock();
        let mut wait = l.synced.get(cur.map(|c| c + 1).unwrap_or(0)).map(|n| n.0.saturating_sub(now));
        // Next word in the current line (karaoke).
        if let Some(words) = cur.and_then(|c| l.words.get(c)) {
            let wc = self.word_clock();
            if let Some(w) = words.iter().find(|w| w.0 > wc) {
                let d = w.0 - wc;
                wait = Some(wait.map_or(d, |x| x.min(d)));
            }
        }
        Some(Duration::from_millis(wait?).max(Duration::from_millis(5)))
    }
}

impl App {
    /// Song time for word highlighting: what you hear minus your offset (no lead:
    /// karaoke fills a word when it is sung).
    pub fn word_clock(&self) -> u64 {
        (self.engine.position().as_millis() as i64 - self.lyrics_offset).max(0) as u64
    }

    /// Word-by-word state of the current line: (word texts, how many are sung).
    pub fn current_words(&self) -> Option<(Vec<String>, usize)> {
        let (l, cur) = self.current_lyric()?;
        let i = cur?;
        let sung = lrc::words_sung(l, i, self.word_clock())?;
        Some((l.words[i].iter().map(|w| w.1.clone()).collect(), sung))
    }
}
