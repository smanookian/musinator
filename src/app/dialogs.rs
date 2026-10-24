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
use super::worker::{Job, Then};
use super::{App, Focus, Screen, Side};
use crate::config::{Kind, SECTIONS, SETTINGS};
use crate::engine::Cmd;
use crate::keys::{ACTIONS, Chord, Keymap, chord_id};
use crate::library::db::Playlist;
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
    NewPlaylistWith { nodes: Vec<String>, tracks: Vec<Arc<Track>> },
    RenamePlaylist(i64),
    Export(i64),
    AddFolder,
    Import,
}

pub enum Dialog {
    Quit,
    Confirm { msg: String, what: Confirm },
    Search { input: String, cursor: usize, offset: usize, typed: Option<Instant> },
    Themes { list: Vec<Theme>, cursor: usize, offset: usize },
    Settings { section: usize, row: usize, right: bool },
    Help { filter: String, offset: usize },
    Keys { cursor: usize, offset: usize, capture: bool, msg: Option<String> },
    Folders { cursor: usize, changed: bool },
    Sleep { cursor: usize },
    Input { title: &'static str, label: &'static str, value: String, what: InputFor },
    Tags(Box<TagEdit>),
    PlaylistPick { nodes: Vec<String>, tracks: Vec<Arc<Track>>, entries: Vec<(Option<i64>, String)>, cursor: usize },
}

/// The tag editor: one song, or many (then only shared fields are shown).
pub struct TagEdit {
    paths: Vec<std::path::PathBuf>,
    orig: Fields,
    /// The songs differ in this field.
    mixed: [bool; 8],
    values: Fields,
    touched: [bool; 8],
    row: usize,
    note: Option<String>,
}

impl TagEdit {
    fn rows(&self) -> Vec<usize> {
        if self.paths.len() == 1 { (0..8).collect() } else { tagedit::ALBUM_FIELDS.to_vec() }
    }
    /// What to write: only fields the user changed.
    fn changes(&self) -> [Option<String>; 8] {
        let mut out: [Option<String>; 8] = Default::default();
        for i in self.rows() {
            if self.touched[i] && (self.mixed[i] || self.values[i].trim() != self.orig[i]) {
                out[i] = Some(self.values[i].trim().to_string());
            }
        }
        out
    }
}

impl Dialog {
    pub fn tags(paths: Vec<std::path::PathBuf>, all: Vec<Fields>) -> Self {
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
        let e = TagEdit { paths, values: orig.clone(), orig, mixed, touched: [false; 8], row: 0, note: None };
        Dialog::Tags(Box::new(e))
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
            Dialog::Keys { cursor, capture, msg, .. } => {
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
                        app.cfg.keys.insert(info.id.to_string(), vec![chord_id(&ch)]);
                        app.keys = Keymap::new(&app.cfg.keys);
                        app.save_cfg();
                        *msg = Some(format!("\"{}\" is now {}", info.label, ch));
                    }
                    *capture = false;
                    return true;
                }
                match k.code {
                    _ if esc => return false,
                    KeyCode::Down | KeyCode::Char('j') => *cursor = (*cursor + 1).min(ACTIONS.len() - 1),
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Enter => {
                        *capture = true;
                        *msg = Some(format!("Press a new key for \"{}\" · esc cancels", info.label));
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
                    let v = value.trim().to_string();
                    if v.is_empty() {
                        return true;
                    }
                    match std::mem::replace(what, InputFor::NewPlaylist) {
                        InputFor::NewPlaylist => app.send_job(Job::NewPlaylist { name: v, nodes: vec![], tracks: vec![] }),
                        InputFor::NewPlaylistWith { nodes, tracks } => app.send_job(Job::NewPlaylist { name: v, nodes, tracks }),
                        InputFor::RenamePlaylist(id) => app.send_job(Job::RenamePlaylist { id, name: v }),
                        InputFor::Export(id) => app.send_job(Job::Export { id, path: crate::config::expand(&v) }),
                        InputFor::Import => app.send_job(Job::Import { path: crate::config::expand(&v) }),
                        InputFor::AddFolder => {
                            let p = crate::config::expand(&v);
                            if !p.is_dir() {
                                app.error(format!("Not a folder: {}", p.display()));
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
            Dialog::Tags(e) => {
                let rows = e.rows();
                if esc {
                    return false;
                }
                if down {
                    e.row = (e.row + 1) % rows.len();
                } else if up {
                    e.row = (e.row + rows.len() - 1) % rows.len();
                } else if k.code == KeyCode::Enter {
                    let fields = e.changes();
                    if fields.iter().all(Option::is_none) {
                        app.info("Nothing changed");
                    } else {
                        app.send_job(Job::WriteTags { paths: std::mem::take(&mut e.paths), fields });
                    }
                    return false;
                } else if ctrl && k.code == KeyCode::Char('f') {
                    let guess = tagedit::from_filename(&e.paths[0]);
                    let mut n = 0;
                    for &i in &rows {
                        if !guess[i].is_empty() && guess[i] != e.values[i] {
                            e.values[i] = guess[i].clone();
                            e.touched[i] = true;
                            n += 1;
                        }
                    }
                    e.note = Some(if n == 0 {
                        "Nothing new in the file name".into()
                    } else {
                        format!("Filled {n} from the file name · check, then enter to save")
                    });
                } else {
                    let i = rows[e.row];
                    if edit(&mut e.values[i], &k) {
                        e.touched[i] = true;
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
            Dialog::Input { title, label, value, .. } => {
                let inner = frame(f, app, 60, 6, title);
                let b = f.buffer_mut();
                put(b, inner.x + 2, inner.y + 1, label, 12, muted);
                let field = Rect { x: inner.x + 2, y: inner.y + 2, width: inner.width - 4, height: 1 };
                fill(b, field, t.title_bar);
                let shown = tail(value, field.width as usize - 2);
                put(b, field.x + 1, field.y, &format!("{shown}▏"), field.width - 1, Style::default().fg(c(t.fg)).bg(c(t.title_bar)));
                hints_row(b, inner, "enter ok · esc cancel", muted);
            }
            Dialog::Tags(e) => {
                let rows = e.rows();
                let title = if e.paths.len() == 1 { "Edit tags".to_string() } else { format!("Edit tags · {} songs", e.paths.len()) };
                let inner = frame(f, app, 72, rows.len() as u16 + 7, &title);
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
                    let y = inner.y + 3 + k as u16;
                    let sel = k == e.row;
                    let changed = e.touched[i] && (e.mixed[i] || e.values[i].trim() != e.orig[i]);
                    put(b, inner.x + 2, y, if changed { "•" } else { " " }, 1, Style::default().fg(c(t.warn)).bg(c(t.dialog_bg)));
                    put(b, inner.x + 4, y, tagedit::FIELDS[i], 13, if sel { bold } else { muted });
                    let bg = if sel { t.title_bar } else { t.dialog_bg };
                    let field = Rect { x: fx, y, width: fw, height: 1 };
                    fill(b, field, bg);
                    let st = Style::default().fg(c(t.fg)).bg(c(bg));
                    if e.values[i].is_empty() && e.mixed[i] && !e.touched[i] {
                        put(
                            b,
                            fx + 1,
                            y,
                            if sel { "▏(mixed)" } else { "(mixed)" },
                            fw.saturating_sub(1),
                            Style::default().fg(c(t.muted)).bg(c(bg)),
                        );
                    } else if sel {
                        put(b, fx + 1, y, &format!("{}▏", tail(&e.values[i], fw as usize - 2)), fw.saturating_sub(1), st);
                    } else {
                        put(b, fx + 1, y, &e.values[i], fw.saturating_sub(2), st);
                    }
                }
                let hint = e.note.clone().unwrap_or_else(|| "enter save · ctrl+f from file name · esc cancel".into());
                hints_row(b, inner, &hint, muted);
            }
            Dialog::Sleep { cursor } => {
                let inner = frame(f, app, 40, SLEEP.len() as u16 + 4, "Sleep timer");
                for (k, (label, _)) in SLEEP.iter().enumerate() {
                    sel_row(f, app, inner, inner.y + 1 + k as u16, label, k == *cursor);
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
                if input.trim().is_empty() {
                    put(f.buffer_mut(), list.x + 2, list.y, "Type to search artists, albums, songs and playlists", list.width - 2, muted);
                } else if hits.is_empty() {
                    put(f.buffer_mut(), list.x + 2, list.y, "No matches", list.width - 2, muted);
                }
                // Rows with section headers.
                let mut rows: Vec<(Option<usize>, String)> = Vec::new();
                let mut last = "";
                for (i, h) in hits.iter().enumerate() {
                    let sec = section_of(h);
                    if sec != last {
                        rows.push((None, sec.to_string()));
                        last = sec;
                    }
                    rows.push((Some(i), String::new()));
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
                            sel_row(f, app, list, y, &title, sel);
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
                let line = msg.clone().unwrap_or_else(|| "enter set key · r reset · esc close".into());
                hints_row(f.buffer_mut(), inner, &line, muted);
            }
            Dialog::Folders { cursor, .. } => {
                let h = (app.cfg.music_folders.len() as u16 + 5).clamp(7, 20);
                let inner = frame(f, app, 64, h, "Music folders");
                if app.cfg.music_folders.is_empty() {
                    put(f.buffer_mut(), inner.x + 3, inner.y + 1, "No folders. Press a to add one.", inner.width - 4, muted);
                }
                for (k, p) in app.cfg.music_folders.clone().iter().enumerate() {
                    sel_row(f, app, inner, inner.y + 1 + k as u16, p, k == *cursor);
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
    v
}

fn section_of(i: &Item) -> &'static str {
    match i {
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
        Item::Track(t) => {
            let tracks: Vec<Arc<Track>> = all.iter().filter_map(|i| if let Item::Track(t) = i { Some(t.clone()) } else { None }).collect();
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
        "keys" => app.dialog = Some(Dialog::Keys { cursor: 0, offset: 0, capture: false, msg: None }),
        "music_folders" => app.dialog = Some(Dialog::Folders { cursor: 0, changed: false }),
        "import_playlist" => app.dialog = Some(Dialog::input("Import playlist", "File", "~/".into(), InputFor::Import)),
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
