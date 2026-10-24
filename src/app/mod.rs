//! The TUI: state, the event loop and actions. Drawing is in `draw.rs`,
//! floating dialogs in `dialogs.rs`.
//!
//! The loop sleeps on channels. It only ticks while something moves
//! (progress while playing, visualizer frames while visible), so idle CPU is ~0.

pub mod art;
pub mod dialogs;
pub mod draw;
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
}

impl ListView {
    fn new(title: String, node: String, kind: ListKind, seq: u64) -> Self {
        Self { title, node, kind, items: Vec::new(), cursor: 0, offset: 0, loading: kind == ListKind::Node, seq, error: None }
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
    pub art_file: Option<std::path::PathBuf>,
    pub lyrics: Option<(TrackRef, Lyrics)>,
    pub viz: viz::Viz,
    pub status: Option<(String, bool, Instant)>,
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
        s.push(Side::Header("Smart"));
        for (l, n) in
            [("Most played", "smart:most"), ("Never played", "smart:never"), ("Added this week", "smart:week"), ("Liked", "smart:liked")]
        {
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
            Side::Node { label, node } => ListView::new(label, node, ListKind::Node, seq),
            Side::Playlist { id, name, .. } => ListView::new(name, format!("playlist:{id}"), ListKind::Node, seq),
            _ => return,
        };
        let node = view.node.clone();
        let kind = view.kind;
        self.lists = vec![view];
        if kind == ListKind::Node && !node.is_empty() {
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
            Then::AddToPlaylist(_) => {}
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

    fn back(&mut self) {
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
            Action::Search => self.dialog = Some(Dialog::search()),
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
            Action::Like => {
                if let Some((_, tracks, _)) = self.targets()
                    && let Some(t) = tracks.first()
                {
                    self.worker.send(Job::Like(t.tref().clone()));
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
        if self.dialog.is_some() {
            return;
        }
        let p = ratatui::layout::Position::new(m.column, m.row);
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
                    self.worker.send(Job::RecordPlay(r));
                }
            }
        }
        self.dirty = true;
    }

    fn on_lib(&mut self, e: LibEvent) {
        match e {
            LibEvent::Progress { done, total } => self.scan = if total > 0 { Some((done, total)) } else { None },
            LibEvent::Done { added, removed, total } => {
                self.scan = None;
                self.library_size = total;
                if added > 0 || removed > 0 {
                    for v in &mut self.lists {
                        v.loading = true;
                    }
                    let nodes: Vec<String> = self.lists.iter().map(|v| v.node.clone()).collect();
                    for n in nodes {
                        self.reload_lists_matching(&n);
                    }
                    self.info(format!("Library: {total} songs"));
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
                if let Some(v) = self.lists.iter_mut().find(|v| v.seq == seq) {
                    v.loading = false;
                    match items {
                        Ok(items) => {
                            v.items = items;
                            v.cursor = v.cursor.min(v.items.len().saturating_sub(1));
                            v.error = None;
                        }
                        Err(e) => v.error = Some(e),
                    }
                }
            }
            Msg::Search { seq, hits } => {
                if seq == self.search_seq {
                    self.search_hits = hits;
                }
            }
            Msg::Lyrics { track, lyrics } => self.lyrics = lyrics.map(|l| (track, l)),
            Msg::Tracks { tracks, then } => self.on_tracks(tracks, then),
            Msg::Playlists(p) => {
                self.playlists = p;
                self.rebuild_side();
            }
            Msg::Liked(on) => self.info(if on { "Liked ♥" } else { "Like removed" }),
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
        let ms = self.engine.position().as_millis() as u64;
        Some((l, lrc::current(l, ms)))
    }

    pub fn send_job(&self, j: Job) {
        self.worker.send(j);
    }

    pub fn search(&mut self, q: String) {
        self.search_seq += 1;
        self.worker.send(Job::Search { seq: self.search_seq, query: q });
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
pub fn run(cfg: Config) -> Result<()> {
    let (lib_tx, lib_rx) = cb::unbounded();
    let lib = Library::new(crate::config::data_dir().join("library.db"), cfg.folders(), lib_tx);
    lib.measure_loudness.store(cfg.measure_loudness, std::sync::atomic::Ordering::Relaxed);
    let local = Arc::new(LocalSource::new(lib.clone())?);
    let mut sources = Sources::default();
    sources.add(local.clone());
    let sources = Arc::new(sources);
    let engine = Engine::start(sources.clone(), cfg.volume, cfg.gain())?;

    let ipc_rx = ipc::serve()?;

    // Terminal.
    let mut terminal = ratatui::init();
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let kitty_ok = picker.protocol_type() == ProtocolType::Kitty;
    // Real images on Kitty/Ghostty, half-blocks everywhere else.
    if !kitty_ok || cfg.images == "halfblocks" {
        picker.set_protocol_type(ProtocolType::Halfblocks);
    }
    let (in_tx, in_rx) = cb::unbounded();
    std::thread::Builder::new().name("input".into()).spawn(move || {
        while let Ok(e) = crossterm::event::read() {
            if in_tx.send(e).is_err() {
                break;
            }
        }
    })?;
    let (w_tx, w_rx) = cb::unbounded();
    let worker = Worker::start(local, w_tx);
    let (a_tx, a_rx) = cb::unbounded();
    let images_on = cfg.images != "off";
    let art = images_on.then(|| ArtWorker::start(sources.clone(), picker.clone(), a_tx.clone()));
    let (theme_tx, theme_rx) = cb::unbounded::<()>();
    let _theme_watch = watch_omarchy(theme_tx);
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
        mpris,
        preview_theme: None,
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

    let res = (|| -> Result<()> {
        loop {
            if app.dirty {
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
                recv(in_rx) -> e => app.on_term(e?),
                recv(app.engine.events) -> e => { let e = e?; app.on_engine(e) },
                recv(lib_rx) -> e => app.on_lib(e?),
                recv(w_rx) -> m => app.on_worker(m?),
                recv(a_rx) -> a => app.on_art(a?),
                recv(theme_rx) -> _ => { while theme_rx.try_recv().is_ok() {} app.reload_omarchy() },
                recv(ipc_rx) -> r => app.on_ipc(r?),
                recv(mpris_rx) -> c => { if let Ok(c) = c { app.on_mpris(c) } },
                recv(tick) -> _ => { app.next_tick = None; app.on_tick() },
            }
        }
        Ok(())
    })();
    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture).ok();
    ratatui::restore();
    let _ = app.cfg.save();
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
