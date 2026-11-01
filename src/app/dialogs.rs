//! Floating dialogs with a soft drop shadow: quit, confirm, search, themes,
//! settings, help, keys, folders, playlists, text input.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use musinator_themes::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use super::draw::{c, fill, put, put_right};
use super::worker::{CoverJob, Job, Then};
use super::{App, Focus, Screen, Side};
use crate::config::{Kind, SECTIONS, SETTINGS};
use crate::engine::Cmd;
use crate::keys::{ACTIONS, Chord, Keymap};
use crate::library::db::Playlist;
use crate::library::musicbrainz::Release;
use crate::library::tagedit::{self, Fields};
use crate::model::{Track, fmt_time};
use crate::source::{Item, Node, NodeKind};

/// Sleep timer choices: label, minutes (0 = off, -1 = end of song, -2 = end of album).
const SLEEP: &[(&str, i32)] = &[
    ("Off", 0),
    ("15 minutes", 15),
    ("30 minutes", 30),
    ("45 minutes", 45),
    ("60 minutes", 60),
    ("90 minutes", 90),
    ("End of this song", -1),
    ("End of this album", -2),
];

pub enum Confirm {
    DeletePlaylist(i64),
}

pub enum InputFor {
    NewPlaylist,
    NewPlaylistWith {
        nodes: Vec<String>,
        tracks: Vec<Arc<Track>>,
    },
    RenamePlaylist(i64),
    Export(i64),
    AddFolder,
    Import,
    RadioUrl,
    Podcast,
    ServerName,
    /// Rename the server with this id.
    RenameServer(String),
    ServerUrl(String),
    ServerUser(String, String),
    ServerPass(String, String, String),
}

pub enum Dialog {
    Quit,
    Confirm {
        msg: String,
        what: Confirm,
    },
    Search {
        input: String,
        cursor: usize,
        offset: usize,
        typed: Option<Instant>,
    },
    Themes {
        list: Vec<Theme>,
        cursor: usize,
        offset: usize,
    },
    Settings {
        section: usize,
        row: usize,
        right: bool,
    },
    Help {
        filter: String,
        offset: usize,
    },
    Keys {
        cursor: usize,
        offset: usize,
        capture: bool,
        /// The captured key goes next to the action's keys instead of replacing them.
        add: bool,
        msg: Option<String>,
    },
    Folders {
        cursor: usize,
        changed: bool,
    },
    /// Settings → Music servers.
    Servers {
        cursor: usize,
    },
    Sleep {
        cursor: usize,
    },
    Input {
        title: &'static str,
        label: &'static str,
        value: String,
        what: InputFor,
    },
    Tags(Box<TagEdit>),
    /// Equalizer; `band` is the selected band.
    Eq {
        band: usize,
    },
    PlaylistPick {
        nodes: Vec<String>,
        tracks: Vec<Arc<Track>>,
        entries: Vec<(Option<i64>, String)>,
        cursor: usize,
    },
}

/// The tag editor: one song, or many (then only shared fields are shown).
/// The last row is the cover picture.
pub struct TagEdit {
    paths: Vec<std::path::PathBuf>,
    orig: Fields,
    /// The songs differ in this field.
    mixed: [bool; 8],
    values: Fields,
    touched: [bool; 8],
    row: usize,
    note: Option<String>,
    /// What the songs have now, e.g. "600×600 JPEG inside the song".
    cover_now: String,
    folder_cover: Option<std::path::PathBuf>,
    /// A picture file path typed by the user.
    cover: String,
    cover_remove: bool,
    /// (disc, track) of each file, for matching MusicBrainz songs.
    files: Vec<(u32, u32)>,
    /// MusicBrainz search hits being picked from, with the cursor.
    mb: Option<(Vec<Release>, usize)>,
    busy: bool,
    /// Per-song tags from MusicBrainz (title, artist, track, disc).
    per_song: Vec<[Option<String>; 8]>,
    /// Cover from the Cover Art Archive and its label.
    cover_data: Option<(Vec<u8>, String)>,
}

impl TagEdit {
    fn rows(&self) -> Vec<usize> {
        let mut r: Vec<usize> = if self.paths.len() == 1 { (0..8).collect() } else { tagedit::ALBUM_FIELDS.to_vec() };
        r.push(COVER_ROW);
        r
    }
    /// What to write: only fields the user changed.
    fn changes(&self) -> [Option<String>; 8] {
        let mut out: [Option<String>; 8] = Default::default();
        for i in self.rows() {
            if i < 8 && self.touched[i] && (self.mixed[i] || self.values[i].trim() != self.orig[i]) {
                out[i] = Some(self.values[i].trim().to_string());
            }
        }
        out
    }
    fn cover_path(&self) -> Option<std::path::PathBuf> {
        let v = self.cover.trim();
        (!v.is_empty()).then(|| crate::config::expand(v))
    }
    /// Right-hand note for a typed picture path.
    fn cover_check(&self) -> Option<(&'static str, bool)> {
        let p = self.cover_path()?;
        Some(if !p.is_file() {
            ("not found", false)
        } else if image::ImageReader::open(&p).ok().and_then(|r| r.with_guessed_format().ok()).is_some_and(|r| r.format().is_some()) {
            ("✓ picture", true)
        } else {
            ("not a picture", false)
        })
    }
}

const COVER_ROW: usize = 8;

/// Row ids at or above this are Settings sections (left column).
const SECTION_ROW: usize = 1000;

impl Dialog {
    /// The selected row of a list dialog, if it has one.
    fn cursor(&self) -> Option<usize> {
        Some(match self {
            Dialog::Search { cursor, .. }
            | Dialog::Themes { cursor, .. }
            | Dialog::Keys { cursor, .. }
            | Dialog::Folders { cursor, .. }
            | Dialog::Servers { cursor }
            | Dialog::Sleep { cursor }
            | Dialog::PlaylistPick { cursor, .. } => *cursor,
            Dialog::Tags(e) => e.mb.as_ref().map_or(e.row, |(_, c)| *c),
            Dialog::Eq { band } => *band,
            Dialog::Settings { row, right: true, .. } => *row,
            _ => return None,
        })
    }

    /// A row was clicked: move there with the same steps as the keys (so
    /// previews etc. follow); a click on the selected row is `enter`.
    /// Returns false when the dialog closed.
    pub fn click_row(&mut self, app: &mut App, idx: usize) -> bool {
        let key = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        if let Dialog::Settings { section, row, right } = self {
            if idx >= SECTION_ROW {
                *section = idx - SECTION_ROW;
                *row = 0;
                *right = false;
                return true;
            }
            if !*right || *row != idx {
                *right = true;
                *row = idx;
                return true;
            }
            return self.key(app, key(KeyCode::Enter));
        }
        let Some(cur) = self.cursor() else { return true };
        // In the tag editor, clicking a field only selects it (enter would save).
        let fields = matches!(self, Dialog::Tags(e) if e.mb.is_none());
        let eq = matches!(self, Dialog::Eq { .. });
        let (down, up) = if eq { (KeyCode::Right, KeyCode::Left) } else { (KeyCode::Down, KeyCode::Up) };
        if idx == cur {
            if fields || eq {
                return true;
            }
            return self.key(app, key(KeyCode::Enter));
        }
        // Search rows skip headers, so step until the cursor gets there.
        for _ in 0..500 {
            let Some(c) = self.cursor() else { break };
            if c == idx {
                break;
            }
            if !self.key(app, key(if idx > c { down } else { up })) {
                return false;
            }
            if self.cursor() == Some(c) {
                break;
            }
        }
        true
    }
    pub fn tags(info: super::worker::EditInfo) -> Self {
        let all = info.fields;
        let num = |s: &str| s.split('/').next().and_then(|n| n.trim().parse::<u32>().ok()).unwrap_or(0);
        let files = all.iter().map(|f| (num(&f[6]), num(&f[5]))).collect();
        let mut orig: Fields = all[0].clone();
        let mut mixed = [false; 8];
        for f in &all[1..] {
            for i in 0..8 {
                if f[i] != orig[i] {
                    mixed[i] = true;
                }
            }
        }
        for i in 0..8 {
            if mixed[i] {
                orig[i].clear();
            }
        }
        let n = info.covers.len();
        let with = info.covers.iter().filter(|c| c.is_some()).count();
        let folder = info.folder_cover.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned());
        let cover_now = match (with, &info.covers[0], folder) {
            (0, _, Some(f)) => format!("none inside · {f} in the folder"),
            (0, _, None) => "none".into(),
            (w, Some(c), _) if w == n && info.covers.iter().all(|x| x.as_deref() == Some(c)) => format!("{c} inside"),
            (w, _, _) if w == n => "inside every song (they differ)".into(),
            (w, _, _) => format!("inside {w} of {n} songs"),
        };
        let e = TagEdit {
            paths: info.paths,
            values: orig.clone(),
            orig,
            mixed,
            touched: [false; 8],
            row: 0,
            note: None,
            cover_now,
            folder_cover: info.folder_cover,
            cover: String::new(),
            cover_remove: false,
            files,
            mb: None,
            busy: false,
            per_song: Vec::new(),
            cover_data: None,
        };
        Dialog::Tags(Box::new(e))
    }
    /// MusicBrainz search finished.
    pub fn mb_results(&mut self, r: Result<Vec<Release>, String>) {
        let Dialog::Tags(e) = self else { return };
        e.busy = false;
        match r {
            Ok(list) if list.is_empty() => e.note = Some("Nothing found on MusicBrainz. Fix album / artist and try again.".into()),
            Ok(mut list) => {
                // Same song count first, then the oldest (usually the original).
                let n = e.paths.len() as u32;
                list.sort_by_key(|r| (r.tracks != n, r.date.is_empty(), r.date.clone()));
                e.mb = Some((list, 0));
                e.note = None;
            }
            Err(err) => e.note = Some(err),
        }
    }

    /// A release was picked and loaded: fill the editor (nothing is saved yet).
    pub fn mb_fill(&mut self, r: Result<Box<super::worker::MbFill>, String>) {
        let Dialog::Tags(e) = self else { return };
        e.busy = false;
        let fill = match r {
            Ok(f) => f,
            Err(err) => return e.note = Some(err),
        };
        e.mb = None;
        let rel = &fill.release;
        let mut set = |i: usize, v: &str| {
            if !v.is_empty() && e.values[i] != v {
                e.values[i] = v.to_string();
                e.touched[i] = true;
            }
        };
        set(2, &rel.title);
        set(3, &rel.artist);
        set(4, &rel.year);
        let matched = fill.matched.iter().filter(|m| m.is_some()).count();
        if e.paths.len() == 1 {
            if let Some(Some(k)) = fill.matched.first() {
                let s = &rel.songs[*k];
                set(0, &s.title);
                set(1, &s.artist);
                set(5, &s.no.to_string());
                set(6, &if rel.discs > 1 { s.disc.to_string() } else { String::new() });
            }
        } else {
            set(1, &rel.artist);
            e.per_song = fill
                .matched
                .iter()
                .map(|m| {
                    let mut f: [Option<String>; 8] = Default::default();
                    if let Some(k) = m {
                        let s = &rel.songs[*k];
                        f[0] = Some(s.title.clone());
                        f[1] = Some(s.artist.clone());
                        f[5] = Some(s.no.to_string());
                        if rel.discs > 1 {
                            f[6] = Some(s.disc.to_string());
                        }
                    }
                    f
                })
                .collect();
        }
        if let Some(b) = &fill.cover {
            let size = image::ImageReader::new(std::io::Cursor::new(b))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok())
                .map(|(w, h)| format!(" · {w}×{h}"))
                .unwrap_or_default();
            e.cover_data = Some((b.clone(), format!("from MusicBrainz{size}")));
            e.cover.clear();
            e.cover_remove = false;
        }
        let n = e.paths.len();
        let cover = if fill.cover.is_some() { "cover found" } else { "no cover there" };
        e.note = Some(if matched == n {
            format!("All {n} songs matched · {cover} · check, enter saves")
        } else if matched == 0 {
            format!("Songs didn't match, only album tags filled · {cover}")
        } else {
            format!("{matched} of {n} songs matched · {cover} · enter saves")
        });
    }

    pub fn quit() -> Self {
        Dialog::Quit
    }
    pub fn confirm(msg: String, what: Confirm) -> Self {
        Dialog::Confirm { msg, what }
    }
    pub fn search() -> Self {
        Dialog::Search { input: String::new(), cursor: 0, offset: 0, typed: None }
    }
    pub fn sleep(app: &App) -> Self {
        let cursor = match app.sleep {
            None => 0,
            Some(super::Sleep::At(_)) => 3,
            Some(super::Sleep::Track) => SLEEP.len() - 2,
            Some(super::Sleep::Album) => SLEEP.len() - 1,
        };
        Dialog::Sleep { cursor }
    }
    pub fn settings() -> Self {
        Dialog::Settings { section: 0, row: 0, right: true }
    }
    pub fn help() -> Self {
        Dialog::Help { filter: String::new(), offset: 0 }
    }
    pub fn input(title: &'static str, label: &'static str, value: String, what: InputFor) -> Self {
        Dialog::Input { title, label, value, what }
    }
    pub fn themes(app: &App) -> Self {
        let mut list = Vec::new();
        if let Some(t) = musinator_themes::omarchy::current() {
            list.push(t);
        }
        list.extend(musinator_themes::builtin());
        let cursor = list.iter().position(|t| t.id == app.cfg.theme).unwrap_or(0);
        Dialog::Themes { list, cursor, offset: 0 }
    }
    pub fn playlist_pick(nodes: Vec<String>, tracks: Vec<Arc<Track>>, pls: &[Playlist]) -> Self {
        let mut entries = vec![(None, "＋ New playlist…".to_string())];
        entries.extend(pls.iter().map(|p| (Some(p.id), format!("{}  ({})", p.name, p.count))));
        Dialog::PlaylistPick { nodes, tracks, entries, cursor: 0 }
    }

    pub fn wants_tick(&self) -> bool {
        matches!(self, Dialog::Search { typed: Some(_), .. })
    }

    pub fn tick(&mut self, app: &mut App) {
        if let Dialog::Search { input, typed, .. } = self
            && typed.is_some_and(|t| t.elapsed() >= Duration::from_millis(120))
        {
            *typed = None;
            app.search(input.clone());
        }
    }

    /// Handle a key. Returns false to close.
    pub fn key(&mut self, app: &mut App, k: KeyEvent) -> bool {
        let esc = k.code == KeyCode::Esc;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let down = matches!(k.code, KeyCode::Down) || (ctrl && k.code == KeyCode::Char('j')) || k.code == KeyCode::Tab;
        let up = matches!(k.code, KeyCode::Up) || (ctrl && k.code == KeyCode::Char('k')) || k.code == KeyCode::BackTab;
        match self {
            Dialog::Quit => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    app.quit = true;
                    false
                }
                _ if ctrl && k.code == KeyCode::Char('c') => {
                    app.quit = true;
                    false
                }
                _ => !(esc || k.code == KeyCode::Char('n')),
            },
            Dialog::Confirm { what, .. } => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    match what {
                        Confirm::DeletePlaylist(id) => app.send_job(Job::DeletePlaylist(*id)),
                    }
                    false
                }
                _ => !(esc || k.code == KeyCode::Char('n')),
            },
            Dialog::Search { input, cursor, typed, .. } => {
                let n = flat_hits(app).len();
                if esc {
                    return false;
                }
                if down {
                    *cursor = (*cursor + 1).min(n.saturating_sub(1));
                } else if up {
                    *cursor = cursor.saturating_sub(1);
                } else if k.code == KeyCode::Enter {
                    let hits = flat_hits(app);
                    if let Some(h) = hits.get(*cursor) {
                        open_hit(app, h, &hits);
                    }
                    return false;
                } else if ctrl && k.code == KeyCode::Char('a') {
                    if let Some(h) = flat_hits(app).get(*cursor) {
                        let (nodes, tracks) = match h {
                            Item::Track(t) => (vec![], vec![t.clone()]),
                            Item::Node(n) => (vec![n.id.clone()], vec![]),
                        };
                        app.send_job(Job::Tracks { nodes, tracks, then: Then::Queue { from: "Search".into() } });
                    }
                } else if edit(input, &k) {
                    *cursor = 0;
                    *typed = Some(Instant::now());
                }
                true
            }
            Dialog::Themes { list, cursor, .. } => {
                if esc {
                    app.end_preview(false);
                    return false;
                }
                if k.code == KeyCode::Enter {
                    let id = list[*cursor].id.clone();
                    app.end_preview(true);
                    app.cfg.theme = id;
                    app.save_cfg();
                    return false;
                }
                let n = list.len();
                if down || k.code == KeyCode::Char('j') {
                    *cursor = (*cursor + 1) % n;
                } else if up || k.code == KeyCode::Char('k') {
                    *cursor = (*cursor + n - 1) % n;
                } else {
                    return true;
                }
                app.preview_theme(list[*cursor].clone());
                true
            }
            Dialog::Settings { section, row, right } => {
                if esc {
                    return false;
                }
                let rows: Vec<&crate::config::Setting> = SETTINGS.iter().filter(|s| s.section == SECTIONS[*section]).collect();
                match k.code {
                    KeyCode::Tab | KeyCode::BackTab => *right = !*right,
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *right {
                            *row = (*row + 1).min(rows.len() - 1);
                        } else {
                            *section = (*section + 1) % SECTIONS.len();
                            *row = 0;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if *right {
                            *row = row.saturating_sub(1);
                        } else {
                            *section = (*section + SECTIONS.len() - 1) % SECTIONS.len();
                            *row = 0;
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') if !*right => {}
                    KeyCode::Right | KeyCode::Char('l') if !*right => *right = true,
                    KeyCode::Left | KeyCode::Char('h') => {
                        let key = rows[*row].key;
                        if !change(app, key, -1) {
                            *right = false;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => {
                        if !*right {
                            *right = true;
                        } else {
                            let s = rows[*row];
                            if s.kind == Kind::Open {
                                return !open_setting(app, s.key);
                            }
                            change(app, s.key, 1);
                        }
                    }
                    _ => {}
                }
                true
            }
            Dialog::Help { filter, offset } => {
                if esc {
                    return false;
                }
                if down {
                    *offset += 1;
                } else if up {
                    *offset = offset.saturating_sub(1);
                } else if k.code == KeyCode::PageDown {
                    *offset += 10;
                } else if k.code == KeyCode::PageUp {
                    *offset = offset.saturating_sub(10);
                } else if edit(filter, &k) {
                    *offset = 0;
                }
                true
            }
            Dialog::Keys { cursor, capture, add, msg, .. } => {
                let info = &ACTIONS[*cursor];
                if *capture {
                    if esc {
                        *capture = false;
                        *msg = None;
                        return true;
                    }
                    let ch = Chord::from_event(&k);
                    if let Some(other) = app.keys.conflict(&ch, info.action) {
                        *msg = Some(format!("{} is used by \"{}\". Change that first.", ch, crate::keys::info(other).label));
                    } else {
                        let keys = app.keys.keys_with(info.action, ch, *add);
                        app.cfg.keys.insert(info.id.to_string(), keys);
                        app.keys = Keymap::new(&app.cfg.keys);
                        app.save_cfg();
                        *msg = Some(if *add {
                            format!("\"{}\" now also uses {}", info.label, ch)
                        } else {
                            format!("\"{}\" is now {}", info.label, ch)
                        });
                    }
                    *capture = false;
                    return true;
                }
                match k.code {
                    _ if esc => return false,
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(ACTIONS.len() - 1),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Enter | KeyCode::Char('a') => {
                        *capture = true;
                        *add = k.code == KeyCode::Char('a');
                        let what = if *add { "another" } else { "a new" };
                        *msg = Some(format!("Press {what} key for \"{}\" · esc cancels", info.label));
                    }
                    KeyCode::Char('r') => {
                        app.cfg.keys.remove(info.id);
                        app.keys = Keymap::new(&app.cfg.keys);
                        app.save_cfg();
                        *msg = Some(format!("\"{}\" reset to default", info.label));
                    }
                    _ => {}
                }
                true
            }
            Dialog::Servers { cursor } => {
                let list = app.servers.all();
                match k.code {
                    _ if esc => return false,
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(list.len().saturating_sub(1)),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Char('a') => {
                        app.dialog = Some(Dialog::input(
                            "Add music server",
                            "Name (e.g. Home; empty = the address)",
                            String::new(),
                            InputFor::ServerName,
                        ));
                        return false;
                    }
                    KeyCode::Char('r') => {
                        if let Some(srv) = list.get(*cursor) {
                            app.dialog = Some(Dialog::input(
                                "Rename music server",
                                "Name",
                                srv.name.clone(),
                                InputFor::RenameServer(srv.id.clone()),
                            ));
                            return false;
                        }
                    }
                    KeyCode::Char('d') | KeyCode::Delete => {
                        if let Some(srv) = list.get(*cursor)
                            && let Some(name) = app.servers.remove(&srv.id)
                        {
                            app.info(format!("Music server removed: {name}"));
                            app.lists.retain(|v| !v.node.starts_with(&format!("{}:", srv.id)));
                            app.rebuild_side();
                            *cursor = cursor.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
                true
            }
            Dialog::Folders { cursor, changed } => {
                match k.code {
                    _ if esc => {
                        if *changed {
                            app.lib.set_folders(app.cfg.folders());
                            app.info("Scanning music folders");
                        }
                        return false;
                    }
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(app.cfg.music_folders.len().saturating_sub(1)),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Char('a') => {
                        app.dialog = Some(Dialog::input("Add music folder", "Folder", "~/".into(), InputFor::AddFolder));
                        return false;
                    }
                    KeyCode::Char('d') | KeyCode::Delete if !app.cfg.music_folders.is_empty() => {
                        app.cfg.music_folders.remove(*cursor);
                        *cursor = cursor.saturating_sub(1);
                        *changed = true;
                        app.save_cfg();
                    }
                    _ => {}
                }
                true
            }
            Dialog::Input { value, what, .. } => {
                if esc {
                    return false;
                }
                if k.code == KeyCode::Enter {
                    // Passwords may start or end with spaces.
                    let v = if matches!(what, InputFor::ServerPass(..)) { value.clone() } else { value.trim().to_string() };
                    if v.is_empty() && !matches!(what, InputFor::ServerName) {
                        return true;
                    }
                    match std::mem::replace(what, InputFor::NewPlaylist) {
                        InputFor::NewPlaylist => app.send_job(Job::NewPlaylist { name: v, nodes: vec![], tracks: vec![] }),
                        InputFor::NewPlaylistWith { nodes, tracks } => app.send_job(Job::NewPlaylist { name: v, nodes, tracks }),
                        InputFor::RenamePlaylist(id) => app.send_job(Job::RenamePlaylist { id, name: v }),
                        InputFor::Export(id) => app.send_job(Job::Export { id, path: crate::config::expand(&v) }),
                        InputFor::Import => app.send_job(Job::Import { path: crate::config::expand(&v) }),
                        InputFor::RenameServer(id) => {
                            match app.servers.rename(&id, &v) {
                                Ok(()) => {
                                    app.info(format!("Renamed to {v}"));
                                    app.rebuild_side();
                                }
                                Err(e) => app.error(format!("Can't rename: {e:#}")),
                            }
                            let cursor = app.servers.all().iter().position(|s| s.id == id).unwrap_or(0);
                            app.dialog = Some(Dialog::Servers { cursor });
                        }
                        InputFor::ServerName => {
                            app.dialog = Some(Dialog::input(
                                "Add music server",
                                "Address (e.g. http://192.168.1.5:4533)",
                                String::new(),
                                InputFor::ServerUrl(v),
                            ));
                        }
                        InputFor::ServerUrl(name) => {
                            app.dialog = Some(Dialog::input("Add music server", "User name", String::new(), InputFor::ServerUser(name, v)));
                        }
                        InputFor::ServerUser(name, url) => {
                            app.dialog =
                                Some(Dialog::input("Add music server", "Password", String::new(), InputFor::ServerPass(name, url, v)));
                        }
                        InputFor::ServerPass(name, url, user) => {
                            app.info("Connecting to the music server…");
                            app.send_job(Job::AddServer { name, url, user, password: v });
                            app.dialog = Some(Dialog::Servers { cursor: 0 });
                        }
                        InputFor::Podcast if v.starts_with("http://") || v.starts_with("https://") => {
                            app.info("Checking the feed…");
                            app.send_job(Job::Subscribe(v));
                        }
                        InputFor::Podcast => app.drill(format!("Podcasts · {v}"), format!("{}{v}", crate::source::podcast::SEARCH)),
                        InputFor::RadioUrl => {
                            app.info("Checking the address…");
                            app.send_job(Job::AddStream(v));
                        }
                        InputFor::AddFolder => {
                            let p = crate::config::expand(&v);
                            if !p.is_dir() {
                                app.error(format!("{} is not a folder: check the path", p.display()));
                                return true;
                            }
                            app.cfg.music_folders.push(v);
                            app.save_cfg();
                            app.dialog = Some(Dialog::Folders { cursor: app.cfg.music_folders.len() - 1, changed: true });
                        }
                    }
                    return false;
                }
                edit(value, &k);
                true
            }
            Dialog::Sleep { cursor } => {
                match k.code {
                    _ if esc => return false,
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(SLEEP.len() - 1),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Enter => {
                        let s = match SLEEP[*cursor].1 {
                            0 => None,
                            -1 => Some(super::Sleep::Track),
                            -2 => Some(super::Sleep::Album),
                            m => Some(super::Sleep::At(Instant::now() + Duration::from_secs(m as u64 * 60))),
                        };
                        app.set_sleep(s);
                        return false;
                    }
                    _ => {}
                }
                true
            }
            Dialog::Eq { band } => {
                use crate::engine::eq::{MAX_DB, PRESETS};
                let cfg = &mut app.cfg;
                let mut changed = true;
                match k.code {
                    _ if esc => return false,
                    KeyCode::Left | KeyCode::Char('h') => {
                        *band = band.saturating_sub(1);
                        changed = false;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        *band = (*band + 1).min(9);
                        changed = false;
                    }
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('0') => {
                        let g = &mut cfg.eq_bands[*band];
                        *g = match k.code {
                            KeyCode::Char('0') => 0.0,
                            KeyCode::Up | KeyCode::Char('k') => (*g + 1.0).min(MAX_DB),
                            _ => (*g - 1.0).max(-MAX_DB),
                        };
                        cfg.eq_on = true;
                        cfg.eq_preset = PRESETS.iter().find(|p| p.1 == cfg.eq_bands).map(|p| p.0).unwrap_or("Custom").to_string();
                    }
                    KeyCode::Char('p') | KeyCode::Tab | KeyCode::Char('P') | KeyCode::BackTab => {
                        let back = matches!(k.code, KeyCode::Char('P') | KeyCode::BackTab);
                        let n = PRESETS.len();
                        let i = PRESETS.iter().position(|p| p.0 == cfg.eq_preset).map(|i| if back { (i + n - 1) % n } else { (i + 1) % n });
                        let i = i.unwrap_or(if back { n - 1 } else { 0 });
                        cfg.eq_preset = PRESETS[i].0.to_string();
                        cfg.eq_bands = PRESETS[i].1;
                        cfg.eq_on = true;
                    }
                    KeyCode::Char(' ') | KeyCode::Char('e') => cfg.eq_on = !cfg.eq_on,
                    KeyCode::Char('r') => {
                        cfg.eq_bands = [0.0; 10];
                        cfg.eq_preset = "Flat".into();
                    }
                    _ => changed = false,
                }
                if changed {
                    app.apply_eq();
                }
                true
            }
            Dialog::Tags(e) => {
                let rows = e.rows();
                let on_cover = rows[e.row] == COVER_ROW;
                if let Some((list, cur)) = &mut e.mb {
                    match k.code {
                        _ if esc => e.mb = None,
                        _ if e.busy => {}
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                            *cur = (*cur + 1).min(list.len() - 1);
                            e.note = None;
                        }
                        KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => *cur = cur.saturating_sub(1),
                        KeyCode::Enter => {
                            let id = list[*cur].id.clone();
                            e.busy = true;
                            e.note = Some("Loading songs and cover…".into());
                            app.send_job(Job::MbRelease { id, files: e.files.clone() });
                        }
                        _ => {}
                    }
                    return true;
                }
                if esc {
                    return false;
                }
                if e.busy {
                    return true;
                }
                if down {
                    e.row = (e.row + 1) % rows.len();
                } else if up {
                    e.row = (e.row + rows.len() - 1) % rows.len();
                } else if k.code == KeyCode::Enter {
                    let cover = if e.cover_remove {
                        CoverJob::Remove
                    } else if let Some(p) = e.cover_path() {
                        if e.cover_check().is_some_and(|(_, ok)| !ok) {
                            e.note = Some("The cover picture can't be used. Fix the path or clear it (ctrl+u).".into());
                            return true;
                        }
                        CoverJob::Set(p)
                    } else if let Some((b, _)) = e.cover_data.take() {
                        CoverJob::Data(b)
                    } else {
                        CoverJob::Keep
                    };
                    let fields = e.changes();
                    let per_song = std::mem::take(&mut e.per_song);
                    if fields.iter().all(Option::is_none) && matches!(cover, CoverJob::Keep) && per_song.is_empty() {
                        app.info("Nothing changed");
                    } else {
                        app.send_job(Job::WriteTags { paths: std::mem::take(&mut e.paths), fields, cover, per_song });
                    }
                    return false;
                } else if ctrl && k.code == KeyCode::Char('f') {
                    let guess = tagedit::from_filename(&e.paths[0]);
                    let mut n = 0;
                    for &i in &rows {
                        if i < 8 && !guess[i].is_empty() && guess[i] != e.values[i] {
                            e.values[i] = guess[i].clone();
                            e.touched[i] = true;
                            n += 1;
                        }
                    }
                    // No picture inside the songs yet: use the folder's cover file.
                    if e.cover_now.starts_with("none")
                        && e.cover.is_empty()
                        && let Some(p) = &e.folder_cover
                    {
                        e.cover = p.to_string_lossy().into_owned();
                        n += 1;
                    }
                    e.note = Some(if n == 0 {
                        "Nothing new in the file name".into()
                    } else {
                        format!("Filled {n} from the file name · check, then enter to save")
                    });
                } else if ctrl && k.code == KeyCode::Char('b') {
                    let artist = if e.values[3].trim().is_empty() { e.values[1].clone() } else { e.values[3].clone() };
                    e.busy = true;
                    e.note = Some("Searching MusicBrainz…".into());
                    app.send_job(Job::MbSearch { artist, album: e.values[2].clone() });
                } else if on_cover && ctrl && k.code == KeyCode::Char('x') {
                    e.cover_data = None;
                    e.cover_remove = !e.cover_remove;
                    e.cover.clear();
                } else {
                    let i = rows[e.row];
                    let v = if i == COVER_ROW { &mut e.cover } else { &mut e.values[i] };
                    if edit(v, &k) {
                        if i == COVER_ROW {
                            e.cover_remove = false;
                            e.cover_data = None;
                        } else {
                            e.touched[i] = true;
                        }
                        e.note = None;
                    }
                }
                true
            }
            Dialog::PlaylistPick { nodes, tracks, entries, cursor } => {
                match k.code {
                    _ if esc => return false,
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(entries.len() - 1),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Enter => {
                        match entries[*cursor].0 {
                            None => {
                                app.dialog = Some(Dialog::input(
                                    "New playlist",
                                    "Name",
                                    String::new(),
                                    InputFor::NewPlaylistWith { nodes: std::mem::take(nodes), tracks: std::mem::take(tracks) },
                                ));
                            }
                            Some(id) => app.send_job(Job::Tracks {
                                nodes: std::mem::take(nodes),
                                tracks: std::mem::take(tracks),
                                then: Then::AddToPlaylist(id),
                            }),
                        }
                        return false;
                    }
                    _ => {}
                }
                true
            }
        }
    }

    pub fn draw(&mut self, f: &mut Frame, app: &mut App) {
        let area = f.area();
        let t = app.theme.clone();
        let muted = Style::default().fg(c(t.muted)).bg(c(t.dialog_bg));
        let text = Style::default().fg(c(t.fg)).bg(c(t.dialog_bg));
        let bold = text.add_modifier(Modifier::BOLD);
        match self {
            Dialog::Quit => {
                let inner = frame(f, app, 44, 5, "Quit");
                let b = f.buffer_mut();
                put(b, inner.x + 2, inner.y + 1, "Quit Musinator?", inner.width - 4, bold);
                hints_row(b, inner, "y quit · esc cancel", muted);
            }
            Dialog::Confirm { msg, .. } => {
                let w = (msg.width() as u16 + 8).clamp(40, 70);
                let inner = frame(f, app, w, 5, "Confirm");
                let b = f.buffer_mut();
                put(b, inner.x + 2, inner.y + 1, msg, inner.width - 4, bold);
                hints_row(b, inner, "y yes · esc cancel", muted);
            }
            Dialog::Input { title, label, value, what } => {
                let inner = frame(f, app, 60, 6, title);
                let b = f.buffer_mut();
                put(b, inner.x + 2, inner.y + 1, label, inner.width.saturating_sub(4), muted);
                let field = Rect { x: inner.x + 2, y: inner.y + 2, width: inner.width - 4, height: 1 };
                fill(b, field, t.title_bar);
                // Passwords show as dots.
                let secret = matches!(what, InputFor::ServerPass(..));
                let value = if secret { "•".repeat(value.chars().count()) } else { value.clone() };
                let shown = tail(&value, field.width as usize - 2);
                put(b, field.x + 1, field.y, &format!("{shown}▏"), field.width - 1, Style::default().fg(c(t.fg)).bg(c(t.title_bar)));
                hints_row(b, inner, "enter ok · esc cancel", muted);
            }
            Dialog::Eq { band } => draw_eq(f, app, *band),
            Dialog::Tags(e) if e.mb.is_some() => {
                let (list, cur) = e.mb.as_ref().expect("checked");
                let h = (list.len() as u16 + 5).min(area.height.saturating_sub(4));
                let inner = frame(f, app, 90, h, "MusicBrainz · pick the release");
                let rows = inner.height.saturating_sub(3) as usize;
                let start = cur.saturating_sub(rows.saturating_sub(1));
                for (k, r) in list.iter().enumerate().skip(start).take(rows) {
                    let y = inner.y + 1 + (k - start) as u16;
                    let year: String = r.date.chars().take(4).collect();
                    let extra = [year, r.country.clone(), format!("{} songs", r.tracks), r.format.clone()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    sel_row(f, app, inner, y, "", k == *cur);
                    row_hit(inner, y, k);
                    let bg = if k == *cur { t.selection } else { t.dialog_bg };
                    let ew = (extra.width() as u16).min(inner.width / 2);
                    let ex = put_right(f.buffer_mut(), inner.x + inner.width - 2, y, &extra, ew, Style::default().fg(c(t.muted)).bg(c(bg)));
                    let st = Style::default().fg(c(if k == *cur { t.selection_fg } else { t.fg })).bg(c(bg));
                    put(f.buffer_mut(), inner.x + 3, y, &format!("{} — {}", r.title, r.artist), ex.saturating_sub(inner.x + 5), st);
                }
                let hint = e.note.clone().unwrap_or_else(|| "enter use this one · esc back".into());
                hints_row(f.buffer_mut(), inner, &hint, muted);
            }
            Dialog::Tags(e) => {
                let rows = e.rows();
                let title = if e.paths.len() == 1 { "Edit tags".to_string() } else { format!("Edit tags · {} songs", e.paths.len()) };
                let inner = frame(f, app, 76, rows.len() as u16 + 8, &title);
                let b = f.buffer_mut();
                let what = if e.paths.len() == 1 {
                    e.paths[0].file_name().unwrap_or_default().to_string_lossy().into_owned()
                } else {
                    "Changes apply to every song. Empty = remove.".into()
                };
                put(b, inner.x + 2, inner.y + 1, &what, inner.width.saturating_sub(4), muted);
                let fx = inner.x + 17;
                let fw = (inner.x + inner.width).saturating_sub(fx + 2);
                for (k, &i) in rows.iter().enumerate() {
                    // A blank line between the tags and the cover.
                    let y = inner.y + 3 + k as u16 + u16::from(i == COVER_ROW);
                    let sel = k == e.row;
                    let bg = if sel { t.title_bar } else { t.dialog_bg };
                    let field = Rect { x: fx, y, width: fw, height: 1 };
                    let st = Style::default().fg(c(t.fg)).bg(c(bg));
                    let dim = Style::default().fg(c(t.muted)).bg(c(bg));
                    let cursor = if sel { "▏" } else { "" };
                    let (label, changed) = if i == COVER_ROW {
                        ("Cover", e.cover_remove || !e.cover.trim().is_empty() || e.cover_data.is_some())
                    } else {
                        (tagedit::FIELDS[i], e.touched[i] && (e.mixed[i] || e.values[i].trim() != e.orig[i]))
                    };
                    put(b, inner.x + 2, y, if changed { "•" } else { " " }, 1, Style::default().fg(c(t.warn)).bg(c(t.dialog_bg)));
                    put(b, inner.x + 4, y, label, 13, if sel { bold } else { muted });
                    row_hit(inner, y, k);
                    fill(b, field, bg);
                    if i == COVER_ROW {
                        if e.cover_remove {
                            put(
                                b,
                                fx + 1,
                                y,
                                &format!("{cursor}will be removed from the songs"),
                                fw.saturating_sub(1),
                                Style::default().fg(c(t.warn)).bg(c(bg)),
                            );
                        } else if let Some((_, label)) = e.cover.is_empty().then_some(()).and(e.cover_data.as_ref()) {
                            put(b, fx + 1, y, &format!("{cursor}{label}"), fw.saturating_sub(1), Style::default().fg(c(t.good)).bg(c(bg)));
                        } else if e.cover.is_empty() {
                            let hint = if sel { format!("{cursor}{} · type a picture path", e.cover_now) } else { e.cover_now.clone() };
                            put(b, fx + 1, y, &hint, fw.saturating_sub(1), dim);
                        } else {
                            let (msg, ok) = e.cover_check().unwrap_or(("", true));
                            let mw = msg.width() as u16;
                            put_right(b, fx + fw - 1, y, msg, mw, Style::default().fg(c(if ok { t.good } else { t.warn })).bg(c(bg)));
                            let room = fw.saturating_sub(mw + 3) as usize;
                            put(b, fx + 1, y, &format!("{}{cursor}", tail(&e.cover, room)), room as u16 + 1, st);
                        }
                    } else if e.values[i].is_empty() && e.mixed[i] && !e.touched[i] {
                        put(b, fx + 1, y, &format!("{cursor}(mixed)"), fw.saturating_sub(1), dim);
                    } else if sel {
                        put(b, fx + 1, y, &format!("{}▏", tail(&e.values[i], fw as usize - 2)), fw.saturating_sub(1), st);
                    } else {
                        put(b, fx + 1, y, &e.values[i], fw.saturating_sub(2), st);
                    }
                }
                let on_cover = rows[e.row] == COVER_ROW;
                let hint = e.note.clone().unwrap_or_else(|| {
                    if on_cover {
                        "type a picture path (~ ok) · ctrl+x remove · enter save · esc cancel".into()
                    } else {
                        "enter save · ctrl+f from file name · ctrl+b MusicBrainz · esc cancel".into()
                    }
                });
                hints_row(b, inner, &hint, muted);
            }
            Dialog::Sleep { cursor } => {
                let inner = frame(f, app, 40, SLEEP.len() as u16 + 4, "Sleep timer");
                for (k, (label, _)) in SLEEP.iter().enumerate() {
                    sel_row(f, app, inner, inner.y + 1 + k as u16, label, k == *cursor);
                    row_hit(inner, inner.y + 1 + k as u16, k);
                }
                let left = app
                    .sleep_label()
                    .map(|l| format!("now: {l} · enter set · esc close"))
                    .unwrap_or_else(|| "enter set · esc close".into());
                hints_row(f.buffer_mut(), inner, &left, muted);
            }
            Dialog::PlaylistPick { entries, cursor, .. } => {
                let h = (entries.len() as u16 + 4).min(area.height - 4);
                let inner = frame(f, app, 50, h, "Add to playlist");
                let rows = inner.height.saturating_sub(3) as usize;
                let start = cursor.saturating_sub(rows.saturating_sub(1));
                for (k, (_, name)) in entries.iter().enumerate().skip(start).take(rows) {
                    let y = inner.y + 1 + (k - start) as u16;
                    sel_row(f, app, inner, y, name, k == *cursor);
                    row_hit(inner, y, k);
                }
                hints_row(f.buffer_mut(), inner, "enter add · esc cancel", muted);
            }
            Dialog::Search { input, cursor, offset, .. } => {
                let w = (area.width * 7 / 10).clamp(50, 110);
                let h = (area.height * 7 / 10).clamp(12, 40);
                let inner = frame(f, app, w, h, "Search");
                let b = f.buffer_mut();
                let field = Rect { x: inner.x + 2, y: inner.y + 1, width: inner.width - 4, height: 1 };
                fill(b, field, t.title_bar);
                put(
                    b,
                    field.x + 1,
                    field.y,
                    &format!("/ {}▏", tail(input, field.width as usize - 4)),
                    field.width - 1,
                    Style::default().fg(c(t.fg)).bg(c(t.title_bar)),
                );
                let hits = flat_hits(app);
                let list = Rect { x: inner.x + 1, y: inner.y + 3, width: inner.width - 2, height: inner.height.saturating_sub(5) };
                let what = if app.stations_only {
                    "Type a station name, genre or country"
                } else {
                    "Type to search artists, albums, songs, playlists and radio stations"
                };
                if input.trim().is_empty() {
                    put(f.buffer_mut(), list.x + 2, list.y, what, list.width - 2, muted);
                } else if hits.is_empty() && !app.stations_busy {
                    put(f.buffer_mut(), list.x + 2, list.y, "No matches", list.width - 2, muted);
                }
                // Rows with section headers.
                let mut rows: Vec<(Option<usize>, String)> = Vec::new();
                let mut last = String::new();
                for (i, h) in hits.iter().enumerate() {
                    let sec = section_of(app, h);
                    if sec != last {
                        rows.push((None, sec.clone()));
                        last = sec;
                    }
                    rows.push((Some(i), String::new()));
                }
                if app.stations_busy {
                    rows.push((None, "Stations · searching…".to_string()));
                }
                let cur_row = rows.iter().position(|r| r.0 == Some(*cursor)).unwrap_or(0);
                let n = list.height as usize;
                if cur_row < *offset {
                    *offset = cur_row.saturating_sub(1);
                }
                if cur_row >= *offset + n {
                    *offset = cur_row + 1 - n;
                }
                for (k, (idx, sec)) in rows.iter().enumerate().skip(*offset).take(n) {
                    let y = list.y + (k - *offset) as u16;
                    match idx {
                        None => {
                            put(f.buffer_mut(), list.x + 1, y, sec, list.width, muted.add_modifier(Modifier::BOLD));
                        }
                        Some(i) => {
                            let (title, right) = hit_text(&hits[*i]);
                            let sel = *i == *cursor;
                            // Leave room for the right-hand text.
                            let rw = (right.width() as u16).min(list.width / 2);
                            let title = super::draw::fit(&title, list.width.saturating_sub(rw + 6) as usize);
                            sel_row(f, app, list, y, &title, sel);
                            row_hit(list, y, *i);
                            let bg = if sel { t.selection } else { t.dialog_bg };
                            put_right(
                                f.buffer_mut(),
                                list.x + list.width - 1,
                                y,
                                &right,
                                list.width / 2,
                                Style::default().fg(c(t.muted)).bg(c(bg)),
                            );
                        }
                    }
                }
                hints_row(f.buffer_mut(), inner, "↑/↓ move · enter open/play · ctrl+a queue · esc close", muted);
            }
            Dialog::Themes { list, cursor, offset } => {
                let h = (area.height * 8 / 10).clamp(10, list.len() as u16 + 4);
                let inner = frame(f, app, 52, h, "Themes");
                let n = inner.height.saturating_sub(3) as usize;
                if *cursor < *offset {
                    *offset = *cursor;
                }
                if *cursor >= *offset + n {
                    *offset = *cursor + 1 - n;
                }
                for (k, th) in list.iter().enumerate().skip(*offset).take(n) {
                    let y = inner.y + 1 + (k - *offset) as u16;
                    let saved = th.id == app.cfg.theme;
                    let name = format!("{}{}", th.label, if saved { "  ✓" } else { "" });
                    sel_row(f, app, inner, y, &name, k == *cursor);
                    row_hit(inner, y, k);
                    let b = f.buffer_mut();
                    let sx = inner.x + inner.width - 12;
                    for (j, col) in [th.bg, th.fg, th.accent, th.viz[0], th.viz[1], th.viz[2]].iter().enumerate() {
                        b.set_string(sx + j as u16 * 2, y, "██", Style::default().fg(c(*col)));
                    }
                }
                hints_row(f.buffer_mut(), inner, "↑/↓ preview · enter keep · esc undo", muted);
            }
            Dialog::Settings { section, row, right } => {
                let w = (area.width * 8 / 10).clamp(60, 100);
                let h = (area.height * 7 / 10).clamp(14, 30);
                let inner = frame(f, app, w, h, "Settings");
                let left_w = 16;
                for (i, s) in SECTIONS.iter().enumerate() {
                    let y = inner.y + 1 + i as u16;
                    let sel = i == *section;
                    let r = Rect { x: inner.x + 1, width: left_w, ..inner };
                    row_hit(r, y, SECTION_ROW + i);
                    if sel && !*right {
                        sel_row(f, app, r, y, s, true);
                    } else {
                        let st = if sel { bold.fg(c(t.accent)) } else { text };
                        put(f.buffer_mut(), r.x + 2, y, s, left_w - 2, st);
                    }
                }
                let rx = inner.x + left_w + 2;
                let rr = Rect { x: rx, y: inner.y, width: inner.width.saturating_sub(left_w + 3), height: inner.height };
                let rows: Vec<&crate::config::Setting> = SETTINGS.iter().filter(|s| s.section == SECTIONS[*section]).collect();
                for (i, s) in rows.iter().enumerate() {
                    let y = inner.y + 1 + i as u16;
                    if y >= inner.y + inner.height - 3 {
                        break;
                    }
                    let sel = *right && i == *row;
                    sel_row(f, app, rr, y, s.label, sel);
                    row_hit(rr, y, i);
                    let v = match s.kind {
                        Kind::Open if s.key == "theme" => format!("{} ›", app.theme.label),
                        Kind::Open if s.key == "keys" => "›".into(),
                        Kind::Open if s.key == "rescan" || s.key == "import_playlist" => "›".into(),
                        Kind::Open => format!("{} ›", app.cfg.display(s.key)),
                        Kind::Toggle | Kind::Choice(_) | Kind::Number { .. } => format!("‹ {} ›", app.cfg.display(s.key)),
                    };
                    let bg = if sel { t.selection } else { t.dialog_bg };
                    put_right(
                        f.buffer_mut(),
                        rr.x + rr.width - 1,
                        y,
                        &v,
                        rr.width / 2,
                        Style::default().fg(c(if sel { t.selection_fg } else { t.accent })).bg(c(bg)),
                    );
                }
                if let Some(s) = rows.get(*row).filter(|_| *right) {
                    put(f.buffer_mut(), rr.x + 1, inner.y + inner.height - 2, s.help, rr.width - 2, muted);
                }
                hints_row(f.buffer_mut(), inner, "tab switch side · ↑/↓ move · ←/→ change · enter open · esc close", muted);
            }
            Dialog::Help { filter, offset } => {
                let w = (area.width * 7 / 10).clamp(56, 96);
                let h = (area.height * 8 / 10).clamp(14, 50);
                let inner = frame(f, app, w, h, "Help");
                let b = f.buffer_mut();
                let field = Rect { x: inner.x + 2, y: inner.y + 1, width: inner.width - 4, height: 1 };
                fill(b, field, t.title_bar);
                put(
                    b,
                    field.x + 1,
                    field.y,
                    &format!("filter: {}▏", filter),
                    field.width - 1,
                    Style::default().fg(c(t.fg)).bg(c(t.title_bar)),
                );
                let lines = help_lines(&app.keys, filter);
                let n = inner.height.saturating_sub(5) as usize;
                *offset = (*offset).min(lines.len().saturating_sub(n));
                for (k, (head, key, label)) in lines.iter().enumerate().skip(*offset).take(n) {
                    let y = inner.y + 3 + (k - *offset) as u16;
                    if *head {
                        put(b, inner.x + 2, y, label, inner.width - 4, bold.fg(c(t.accent)));
                    } else {
                        put(b, inner.x + 4, y, key, 18, text.add_modifier(Modifier::BOLD));
                        put(b, inner.x + 23, y, label, inner.width.saturating_sub(25), muted);
                    }
                }
                hints_row(b, inner, "type to filter · ↑/↓ scroll · esc close", muted);
            }
            Dialog::Keys { cursor, offset, msg, .. } => {
                let w = 64;
                let h = (area.height * 8 / 10).clamp(12, 44);
                let inner = frame(f, app, w, h, "Keys");
                let n = inner.height.saturating_sub(4) as usize;
                if *cursor < *offset {
                    *offset = *cursor;
                }
                if *cursor >= *offset + n {
                    *offset = *cursor + 1 - n;
                }
                for (k, a) in ACTIONS.iter().enumerate().skip(*offset).take(n) {
                    let y = inner.y + 1 + (k - *offset) as u16;
                    let label = format!("{:<9} {}", a.group, a.label);
                    sel_row(f, app, inner, y, &label, k == *cursor);
                    row_hit(inner, y, k);
                    let keys = app.keys.keys(a.action).iter().map(|c| c.to_string()).collect::<Vec<_>>().join("  ");
                    let custom = app.cfg.keys.contains_key(a.id);
                    let bg = if k == *cursor { t.selection } else { t.dialog_bg };
                    put_right(
                        f.buffer_mut(),
                        inner.x + inner.width - 2,
                        y,
                        &keys,
                        20,
                        Style::default().fg(c(if custom { t.warn } else { t.accent })).bg(c(bg)),
                    );
                }
                let line = msg.clone().unwrap_or_else(|| "enter set key · a add key · r reset · esc close".into());
                hints_row(f.buffer_mut(), inner, &line, muted);
            }
            Dialog::Servers { cursor } => {
                let list = app.servers.all();
                let h = (list.len() as u16 + 5).clamp(7, 20);
                let inner = frame(f, app, 76, h, "Music servers");
                if list.is_empty() {
                    put(f.buffer_mut(), inner.x + 3, inner.y + 1, "No music servers yet. Press a to add one.", inner.width - 4, muted);
                }
                for (k, s) in list.iter().enumerate() {
                    let y = inner.y + 1 + k as u16;
                    let sel = k == *cursor;
                    sel_row(f, app, inner, y, &s.name, sel);
                    row_hit(inner, y, k);
                    let bg = if sel { t.selection } else { t.dialog_bg };
                    let right = format!("{} · {}", s.label(), s.kind_name());
                    put_right(
                        f.buffer_mut(),
                        inner.x + inner.width - 2,
                        y,
                        &right,
                        inner.width * 2 / 3,
                        Style::default().fg(c(t.muted)).bg(c(bg)),
                    );
                }
                hints_row(f.buffer_mut(), inner, "a add · r rename · d remove · esc done", muted);
            }
            Dialog::Folders { cursor, .. } => {
                let h = (app.cfg.music_folders.len() as u16 + 5).clamp(7, 20);
                let inner = frame(f, app, 64, h, "Music folders");
                if app.cfg.music_folders.is_empty() {
                    put(f.buffer_mut(), inner.x + 3, inner.y + 1, "No folders. Press a to add one.", inner.width - 4, muted);
                }
                for (k, p) in app.cfg.music_folders.clone().iter().enumerate() {
                    sel_row(f, app, inner, inner.y + 1 + k as u16, p, k == *cursor);
                    row_hit(inner, inner.y + 1 + k as u16, k);
                }
                hints_row(f.buffer_mut(), inner, "a add · d remove · esc done", muted);
            }
        }
    }
}

/// Basic line editing. Returns true if the text changed.
fn edit(s: &mut String, k: &KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Char('u') if ctrl => {
            s.clear();
            true
        }
        KeyCode::Char('w') if ctrl => {
            let t = s.trim_end().rfind(' ').map(|i| i + 1).unwrap_or(0);
            s.truncate(t);
            true
        }
        KeyCode::Char(ch) if !ctrl => {
            s.push(ch);
            true
        }
        KeyCode::Backspace => s.pop().is_some(),
        _ => false,
    }
}

fn tail(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut used = 0;
    for ch in chars.iter().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.insert(0, *ch);
        used += cw;
    }
    format!("…{out}")
}

/// Dialog chrome: soft shadow, filled body, accent border with title.
fn frame(f: &mut Frame, app: &App, w: u16, h: u16, title: &str) -> Rect {
    let area = f.area();
    let w = w.min(area.width.saturating_sub(4));
    let h = h.min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    let t = &app.theme;
    if app.cfg.shadow {
        let buf = f.buffer_mut();
        // Shadow: two columns right, one row down; keeps the text under it, dimmed.
        for (sx, sy, sw, sh) in [(r.x + 2, r.y + h, w, 1), (r.x + w, r.y + 1, 2, h)] {
            let sr = Rect { x: sx, y: sy, width: sw, height: sh }.intersection(area);
            buf.set_style(sr, Style::default().bg(c(t.shadow)).fg(c(t.muted.mix(t.shadow, 0.5))));
        }
    }
    f.render_widget(Clear, r);
    let kind = if app.cfg.corners == "square" { BorderType::Plain } else { BorderType::Rounded };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(kind)
        .border_style(Style::default().fg(c(t.accent)).bg(c(t.dialog_bg)))
        .style(Style::default().bg(c(t.dialog_bg)).fg(c(t.fg)))
        .title(ratatui::text::Span::styled(
            format!(" {title} "),
            Style::default().fg(c(t.accent)).bg(c(t.dialog_bg)).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(r);
    f.render_widget(block, r);
    inner
}

fn hints_row(b: &mut ratatui::buffer::Buffer, inner: Rect, s: &str, st: Style) {
    if inner.height < 2 {
        return;
    }
    put(b, inner.x + 2, inner.y + inner.height - 1, s, inner.width.saturating_sub(4), st);
    super::clicks::hint_text(inner.x + 2, inner.y + inner.height - 1, s);
}

/// A clickable dialog row.
fn row_hit(area: Rect, y: u16, idx: usize) {
    super::clicks::add(Rect { x: area.x, y, width: area.width, height: 1 }, super::clicks::Click::Row(idx));
}

/// A dialog list row: tint + `→` when selected.
fn sel_row(f: &mut Frame, app: &App, area: Rect, y: u16, s: &str, sel: bool) {
    let t = &app.theme;
    let b = f.buffer_mut();
    let bg = if sel { t.selection } else { t.dialog_bg };
    let r = Rect { x: area.x, y, width: area.width, height: 1 };
    if sel {
        fill(b, r, bg);
        put(b, r.x + 1, y, "→", 1, Style::default().fg(c(t.marker)).bg(c(bg)));
    }
    let st = Style::default().bg(c(bg)).fg(c(if sel { t.selection_fg } else { t.fg }));
    put(b, r.x + 3, y, s, r.width.saturating_sub(4), if sel { st.add_modifier(Modifier::BOLD) } else { st });
}

fn flat_hits(app: &App) -> Vec<Item> {
    let h = &app.search_hits;
    let mut v: Vec<Item> = Vec::new();
    v.extend(h.artists.iter().cloned().map(Item::Node));
    v.extend(h.albums.iter().cloned().map(Item::Node));
    v.extend(h.tracks.iter().cloned().map(Item::Track));
    v.extend(h.playlists.iter().cloned().map(Item::Node));
    v.extend(app.server_hits.iter().cloned());
    if app.stations_only {
        v.clear();
    }
    v.extend(app.search_stations.iter().cloned().map(Item::Track));
    v
}

fn section_of(app: &App, i: &Item) -> String {
    let server = match i {
        Item::Node(n) => app.servers.for_node(&n.id),
        Item::Track(t) => t.r.as_ref().and_then(|r| app.servers.by_id(&r.source)),
    };
    if let Some(s) = server {
        return format!("On {}", s.name);
    }
    section_name(i).to_string()
}

fn section_name(i: &Item) -> &'static str {
    match i {
        Item::Track(t) if super::draw::is_live(t) => "Stations",
        Item::Track(_) => "Songs",
        Item::Node(n) => match n.kind {
            NodeKind::Artist => "Artists",
            NodeKind::Album => "Albums",
            NodeKind::Playlist => "Playlists",
            _ => "Other",
        },
    }
}

fn hit_text(i: &Item) -> (String, String) {
    match i {
        // Station: name, then country / genres and format.
        Item::Track(t) if super::draw::is_live(t) => {
            (t.title.clone(), format!("{} · {}", t.artist, t.album.trim_start_matches("Radio · ")))
        }
        Item::Track(t) => (
            t.display_title().to_string(),
            format!("{} · {}  {}", t.display_artist(), t.album, t.duration_ms.map(fmt_time).unwrap_or_default()),
        ),
        Item::Node(n) => (n.title.clone(), n.subtitle.clone()),
    }
}

fn open_hit(app: &mut App, h: &Item, all: &[Item]) {
    app.screen = Screen::Library;
    match h {
        Item::Track(t) if super::draw::is_live(t) => {
            let id = app.engine.queue.lock().replace(vec![t.clone()], Some("Radio".into()), 0);
            if let Some(id) = id {
                app.engine.send(Cmd::PlayItem(id));
            }
        }
        Item::Track(t) => {
            let tracks: Vec<Arc<Track>> = all
                .iter()
                .filter_map(|i| if let Item::Track(t) = i { (!super::draw::is_live(t)).then(|| t.clone()) } else { None })
                .collect();
            let start = tracks.iter().position(|x| Arc::ptr_eq(x, t)).unwrap_or(0);
            let id = app.engine.queue.lock().replace(tracks, Some("Search".into()), start);
            if let Some(id) = id {
                app.engine.send(Cmd::PlayItem(id));
            }
        }
        Item::Node(Node { kind: NodeKind::Playlist, id, .. }) => {
            if let Some(i) = app.side.iter().position(|s| matches!(s, Side::Playlist { id: pid, .. } if format!("playlist:{pid}") == *id)) {
                app.side_cursor = i;
                app.open_side();
                app.focus = Focus::List;
            }
        }
        Item::Node(n) => {
            let title = if n.kind == NodeKind::Album { format!("{} · {}", n.title, n.subtitle) } else { n.title.clone() };
            app.drill(title, n.id.clone());
        }
    }
}

/// Change a setting and apply it right away. Returns false if nothing changed.
fn change(app: &mut App, key: &str, dir: i32) -> bool {
    if key == "rate_switching" {
        let on = !crate::pwrate::enabled();
        match crate::pwrate::set(on) {
            Ok(()) => {
                app.engine.send(Cmd::ReopenOutput);
                app.info(if on { "PipeWire may now switch to the file's rate" } else { "PipeWire rate switching off" })
            }
            Err(e) => app.error(format!("Can't change PipeWire config: {e:#}")),
        }
        return true;
    }
    if !app.cfg.step(key, dir) {
        return false;
    }
    apply(app, key);
    app.save_cfg();
    true
}

fn apply(app: &mut App, key: &str) {
    match key {
        "podcast_auto_download" => app.podcasts.auto_download.store(app.cfg.podcast_auto_download, std::sync::atomic::Ordering::Relaxed),
        "rg_mode" | "rg_preamp" | "rg_fallback" | "prevent_clip" => app.engine.send(Cmd::Gain(app.cfg.gain())),
        "images" => app.restart_images(),
        "album_grid" => app.grid_pics.clear(),
        "watch_folders" => {
            if app.cfg.watch_folders {
                app.lib.watch();
            } else {
                app.lib.unwatch();
            }
        }
        "measure_loudness" => {
            app.lib.measure_loudness.store(app.cfg.measure_loudness, std::sync::atomic::Ordering::Relaxed);
            if app.cfg.measure_loudness {
                app.lib.scan();
            }
        }
        "omarchy_bar" => {
            if !crate::bar::available() {
                app.cfg.omarchy_bar = false;
                app.error("Omarchy bar not found (~/.config/omarchy/shell.json)");
            } else if let Err(e) = crate::bar::set(app.cfg.omarchy_bar) {
                app.error(format!("Omarchy bar: {e:#}"));
            } else {
                app.info(if app.cfg.omarchy_bar { "Added to the Omarchy bar" } else { "Removed from the Omarchy bar" });
            }
        }
        "mpris" => app.restart_mpris(),
        _ => {}
    }
}

/// Settings that open something. Returns true if the settings dialog was replaced.
fn open_setting(app: &mut App, key: &str) -> bool {
    match key {
        "theme" => app.dialog = Some(Dialog::themes(app)),
        "keys" => app.dialog = Some(Dialog::Keys { cursor: 0, offset: 0, capture: false, add: false, msg: None }),
        "music_folders" => app.dialog = Some(Dialog::Folders { cursor: 0, changed: false }),
        "import_playlist" => app.dialog = Some(Dialog::input("Import playlist", "File", "~/".into(), InputFor::Import)),
        "servers" => app.dialog = Some(Dialog::Servers { cursor: 0 }),
        "lastfm" => {
            if let Some(u) = crate::lastfm::user() {
                crate::lastfm::logout();
                app.info(format!("Logged out of Last.fm ({u})"));
                app.lastfm_user = None;
                app.rebuild_side();
            } else {
                app.info("Opening Last.fm in your browser… press Allow there");
                app.send_job(Job::LastfmLogin);
            }
            return false;
        }
        "rescan" => {
            app.lib.scan();
            app.info("Scanning music folders");
            return false;
        }
        _ => return false,
    }
    true
}

/// (is_header, keys, label)
fn help_lines(keys: &Keymap, filter: &str) -> Vec<(bool, String, String)> {
    let f = filter.to_lowercase();
    let mut out = Vec::new();
    let mut group = "";
    for a in ACTIONS {
        let k = keys.keys(a.action).iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" / ");
        if !f.is_empty() && !a.label.to_lowercase().contains(&f) && !k.to_lowercase().contains(&f) && !a.group.to_lowercase().contains(&f) {
            continue;
        }
        if a.group != group {
            out.push((true, String::new(), a.group.to_string()));
            group = a.group;
        }
        out.push((false, k, a.label.to_string()));
    }
    let tips: &[(&str, &str)] = &[
        ("z", "Library ↔ Now Playing. esc also goes back."),
        ("audio line", "Shows the real format. \"→ 48 kHz\" means PipeWire resamples; \"bit-perfect\" means it does not."),
        ("gapless", "Songs with the same format flow into each other with no gap."),
        ("ReplayGain", "RG = from tags, R128 = measured by Musinator. Settings → Audio."),
        ("commands", "musinator play | pause | toggle | next | prev | status --json"),
        ("bar", "musinator --bar prints JSON for the Omarchy bar or Waybar."),
    ];
    let tips: Vec<_> = tips.iter().filter(|(k, l)| f.is_empty() || k.contains(&f) || l.to_lowercase().contains(&f)).collect();
    if !tips.is_empty() {
        out.push((true, String::new(), "Tips".into()));
        out.extend(tips.into_iter().map(|(k, l)| (false, k.to_string(), l.to_string())));
    }
    out
}

/// The equalizer window: the curve on top, then one column per band.
fn draw_eq(f: &mut Frame, app: &mut App, band: usize) {
    use crate::engine::eq::{BANDS, Eq, LABELS, MAX_DB};
    let t = app.theme.clone();
    let cfg = &app.cfg;
    let state = if cfg.eq_on { "on" } else { "off" };
    let inner = frame(f, app, 74, 22, &format!("Equalizer · {state} · {}", cfg.eq_preset));
    let gains = cfg.eq_bands;
    let on = cfg.eq_on;
    // The curve shows the shape; the level drop that prevents distortion is noted below.
    let pre = crate::engine::eq::preamp_db(&gains);
    let b = f.buffer_mut();
    let muted = Style::default().fg(c(t.muted)).bg(c(t.dialog_bg));
    // Curve area: braille dots, 2×4 per cell, log frequency from 20 Hz to 20 kHz.
    let gx = inner.x + 6;
    let gw = inner.width.saturating_sub(8);
    let gy = inner.y + 1;
    let gh: u16 = 11;
    let rows = gh as usize * 4;
    let ypos = |db: f32| -> usize { (((MAX_DB - db.clamp(-MAX_DB, MAX_DB)) / (2.0 * MAX_DB)) * (rows - 1) as f32).round() as usize };
    let fx = |x: usize, w: usize| -> f32 { 20.0 * 1000f32.powf(x as f32 / (w - 1) as f32) };
    let xof = |freq: f32, w: usize| -> usize { ((freq / 20.0).log10() / 3.0 * (w - 1) as f32).round() as usize };
    for (lbl, db) in [("+12", MAX_DB), ("  0", 0.0), ("−12", -MAX_DB)] {
        put(b, inner.x + 1, gy + (ypos(db) / 4) as u16, lbl, 4, muted);
    }
    // Zero line.
    let zy = gy + (ypos(0.0) / 4) as u16;
    for x in 0..gw {
        put(b, gx + x, zy, "┈", 1, Style::default().fg(c(t.border)).bg(c(t.dialog_bg)));
    }
    let dots_w = gw as usize * 2;
    if dots_w > 2 {
        let mut cells = vec![0u8; gw as usize * gh as usize];
        let mut prev: Option<usize> = None;
        for x in 0..dots_w {
            let db = if on { Eq::response_db(&gains, fx(x, dots_w), 48_000) - pre } else { 0.0 };
            let y = ypos(db);
            // Fill between neighbours so steep parts stay connected.
            let (lo, hi) = match prev {
                Some(p) => (p.min(y), p.max(y)),
                None => (y, y),
            };
            // Two dots thick.
            for yy in lo..=(hi + 1).min(rows - 1) {
                let bit = match (x % 2, yy % 4) {
                    (0, 0) => 0x01,
                    (0, 1) => 0x02,
                    (0, 2) => 0x04,
                    (0, _) => 0x40,
                    (_, 0) => 0x08,
                    (_, 1) => 0x10,
                    (_, 2) => 0x20,
                    _ => 0x80,
                };
                cells[(yy / 4) * gw as usize + x / 2] |= bit;
            }
            prev = Some(y);
        }
        let col = if on { t.accent } else { t.muted };
        for (i, &m) in cells.iter().enumerate() {
            if m != 0 {
                let ch = char::from_u32(0x2800 + m as u32).unwrap_or(' ');
                let (x, y) = (gx + (i % gw as usize) as u16, gy + (i / gw as usize) as u16);
                b[(x, y)].set_char(ch).set_style(Style::default().fg(c(col)).bg(c(t.dialog_bg)));
            }
        }
        // Mark the selected band on the curve.
        let x = xof(BANDS[band], dots_w) / 2;
        let db = if on { Eq::response_db(&gains, BANDS[band], 48_000) - pre } else { 0.0 };
        put(b, gx + x as u16, gy + (ypos(db) / 4) as u16, "●", 1, Style::default().fg(c(t.marker)).bg(c(t.dialog_bg)));
    }
    // Band columns: value and label under each band's spot on the curve.
    let vy = gy + gh + 1;
    for i in 0..10 {
        let x = gx + (xof(BANDS[i], dots_w.max(2)) / 2) as u16;
        let sel = i == band;
        let v = gains[i];
        let val = if v == 0.0 { "0".to_string() } else { format!("{v:+.0}") };
        let st = if sel {
            Style::default().fg(c(t.selection_fg)).bg(c(t.selection)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(c(if v == 0.0 { t.muted } else { t.fg })).bg(c(t.dialog_bg))
        };
        let lx = x.saturating_sub(1).max(gx);
        put(b, lx, vy, &format!("{val:^3}"), 3, st);
        put(b, lx, vy + 1, &format!("{:^3}", LABELS[i]), 3, if sel { st } else { muted });
        super::clicks::add(Rect { x: lx, y: vy, width: 3, height: 2 }, super::clicks::Click::Row(i));
    }
    put(b, inner.x + 1, vy, "dB", 3, muted);
    if on && pre < 0.0 {
        let note = format!("level lowered {:.0} dB so boosts don't distort", -pre);
        put(b, gx, vy + 3, &note, gw, muted);
    }
    put(b, inner.x + 1, vy + 1, "Hz", 3, muted);
    hints_row(b, Rect { height: inner.height - 1, ..inner }, "←/→ band · ↑/↓ ±1 dB · 0 reset band · r flat", muted);
    hints_row(b, inner, "p / P preset · space on/off · esc close", muted);
}
