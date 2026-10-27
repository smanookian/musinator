//! Drawing: screen A (Library), screen B (Now Playing), status bar.

use std::sync::Arc;

use musinator_themes::{Rgb, Theme};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui_image::Image;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::art::Slot;
use super::{App, Focus, ListKind, Screen, Side};
use crate::engine::PlayState;
use crate::keys::Action;
use crate::model::{Track, fmt_time, khz};
use crate::source::{Item, NodeKind};

pub fn c(x: Rgb) -> Color {
    Color::Rgb(x.0, x.1, x.2)
}

#[derive(Default)]
pub struct Hits {
    pub side: Rect,
    pub list: Rect,
    pub panel: Rect,
    pub progress: Rect,
    /// (screen row, item index) for the list under the mouse.
    pub rows: Vec<(u16, usize)>,
    pub grid_cols: usize,
    pub page: usize,
}

impl Hits {
    pub fn row_at(&self, p: Position) -> Option<usize> {
        self.rows.iter().find(|(y, _)| *y == p.y).map(|(_, i)| *i)
    }
}

/// Cut `s` to `w` columns, adding "…" when cut.
pub fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

pub fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, w: u16, st: Style) -> u16 {
    if w == 0 {
        return x;
    }
    let s = fit(s, w as usize);
    buf.set_stringn(x, y, &s, w as usize, st).0
}

/// Right-aligned text ending at `right` (exclusive).
pub fn put_right(buf: &mut Buffer, right: u16, y: u16, s: &str, max: u16, st: Style) -> u16 {
    let s = fit(s, max as usize);
    let w = s.width() as u16;
    let x = right.saturating_sub(w);
    buf.set_stringn(x, y, &s, w as usize, st);
    x
}

pub fn fill(buf: &mut Buffer, area: Rect, bg: Rgb) {
    buf.set_style(area, Style::default().bg(c(bg)));
}

/// A pane: full border (accent when focused) and a filled title bar with hints.
/// Returns the content area under the title bar.
pub fn pane(f: &mut Frame, app: &App, area: Rect, title: &str, hints: &str, focused: bool) -> Rect {
    let t = &app.theme;
    let border = if focused { t.accent } else { t.border };
    let kind = if app.cfg.corners == "square" { BorderType::Plain } else { BorderType::Rounded };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(kind)
        .border_style(Style::default().fg(c(border)).bg(c(t.bg)))
        .style(Style::default().bg(c(t.bg)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width < 4 {
        return inner;
    }
    let bar = Rect { height: 1, ..inner };
    let (bg, tfg, hfg) = if focused { (t.accent, t.on_accent, t.on_accent) } else { (t.title_bar, t.fg, t.muted) };
    let buf = f.buffer_mut();
    fill(buf, bar, bg);
    let hw = hints.width() as u16;
    let room = bar.width.saturating_sub(2);
    let title_w = title.width() as u16;
    let show_hints = hw > 0 && title_w + hw + 4 <= room;
    put(
        buf,
        bar.x + 1,
        bar.y,
        title,
        room.saturating_sub(if show_hints { hw + 2 } else { 0 }),
        Style::default().fg(c(tfg)).bg(c(bg)).add_modifier(Modifier::BOLD),
    );
    if show_hints {
        let hx = put_right(buf, bar.x + bar.width - 1, bar.y, hints, hw, Style::default().fg(c(hfg)).bg(c(bg)));
        super::clicks::hint_text(hx, bar.y, hints);
    }
    let mut content = Rect { y: inner.y + 1, height: inner.height - 1, ..inner };
    if app.cfg.density == "comfortable" && content.height > 4 {
        content.y += 1;
        content.height -= 1;
    }
    content
}

/// "enter play  a queue  / search" from actions.
pub fn hints(app: &App, items: &[(Action, &str)]) -> String {
    items.iter().map(|(a, l)| format!("{} {l}", app.keys.hint(*a))).collect::<Vec<_>>().join("  ")
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    fill(f.buffer_mut(), area, app.theme.bg);
    f.buffer_mut().set_style(area, Style::default().fg(c(app.theme.fg)));
    app.hit = Hits::default();
    super::clicks::clear();
    app.viz_area = Rect::default();
    if area.width < 20 || area.height < 3 {
        put(f.buffer_mut(), 0, 0, "Window too small", area.width, Style::default().fg(c(app.theme.muted)));
        return;
    }
    // Mini mode: chosen with `m`, or when the window is too small for the screens.
    if app.mini || area.height < 12 || area.width < 60 {
        mini(f, app, area);
        if let Some(mut d) = app.dialog.take() {
            d.draw(f, app);
            if app.dialog.is_none() {
                app.dialog = Some(d);
            }
        }
        return;
    }
    let main = Rect { height: area.height - 1, ..area };
    let status = Rect { y: area.y + area.height - 1, height: 1, ..area };
    if app.lyrics_full {
        full_lyrics(f, app, area);
        if let Some(mut d) = app.dialog.take() {
            d.draw(f, app);
            if app.dialog.is_none() {
                app.dialog = Some(d);
            }
        }
        return;
    }
    match app.screen {
        Screen::Library => library(f, app, main),
        Screen::NowPlaying => now_playing(f, app, main),
    }
    status_bar(f, app, status);
    if app.dialog.is_some() {
        let mut d = app.dialog.take().unwrap();
        d.draw(f, app);
        if app.dialog.is_none() {
            app.dialog = Some(d);
        }
    }
}

// ---------------- screen A ----------------

fn library(f: &mut Frame, app: &mut App, area: Rect) {
    let side_w = app.cfg.sidebar_width.min(area.width / 2).max(16);
    let side = Rect { width: side_w, ..area };
    let right = Rect { x: area.x + side_w + 1, width: area.width.saturating_sub(side_w + 1), ..area };
    // Mockup: the now-playing panel is about 40% of the right side.
    let np_h = if right.height >= 16 { (right.height * 2 / 5).clamp(9, 16) } else { 0 };
    let upper = Rect { height: right.height - np_h, ..right };
    let lower = Rect { y: right.y + right.height - np_h, height: np_h, ..right };
    sidebar(f, app, side);
    list_pane(f, app, upper);
    if np_h > 0 {
        now_panel(f, app, lower);
    }
}

fn sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Side;
    let h = format!("{}/{} · {}", app.keys.hint(Action::Down), app.keys.hint(Action::Up), app.keys.hint(Action::PlaySelected));
    let inner = pane(f, app, area, "Library", &h, focused);
    app.hit.side = inner;
    let t = app.theme.clone();
    let rows = inner.height as usize;
    if app.side_cursor < app.side_offset {
        app.side_offset = app.side_cursor;
    }
    if app.side_cursor >= app.side_offset + rows {
        app.side_offset = app.side_cursor + 1 - rows;
    }
    let up_next = app.engine.queue.lock().up_next().len();
    let buf = f.buffer_mut();
    for (row, i) in (app.side_offset..app.side.len()).take(rows).enumerate() {
        let y = inner.y + row as u16;
        let s = &app.side[i];
        let sel = i == app.side_cursor;
        let bg = if sel { if focused { t.selection } else { t.selection_dim } } else { t.bg };
        let rect = Rect { y, height: 1, ..inner };
        if sel {
            fill(buf, rect, bg);
        }
        let base = Style::default().bg(c(bg));
        let text_st = if sel { base.fg(c(t.selection_fg)).add_modifier(Modifier::BOLD) } else { base.fg(c(t.fg)) };
        let x0 = inner.x;
        let w = inner.width;
        let (glyph, gst) = match s {
            _ if sel => ("→", base.fg(c(t.marker))),
            Side::NowPlaying => ("♪", base.fg(c(t.accent))),
            Side::Header(_) => ("▾", base.fg(c(t.fg)).add_modifier(Modifier::BOLD)),
            _ => (" ", base),
        };
        put(buf, x0 + 1, y, glyph, 1, gst);
        let (label, st, count): (String, Style, Option<u64>) = match s {
            Side::NowPlaying => ("Now Playing".into(), if sel { text_st } else { base.fg(c(t.accent)) }, None),
            Side::Queue => ("Queue".into(), text_st, Some(up_next as u64)),
            Side::Header(h) => (h.to_string(), base.fg(c(t.fg)).add_modifier(Modifier::BOLD), None),
            Side::Node { label, .. } => (label.clone(), text_st, None),
            Side::Playlist { name, count, .. } => (name.clone(), text_st, Some(*count)),
            Side::Empty(e) => (e.to_string(), base.fg(c(t.muted)), None),
            Side::Blank => (String::new(), base, None),
        };
        let cnt = count.map(|n| n.to_string()).unwrap_or_default();
        let cw = cnt.width() as u16;
        put(buf, x0 + 3, y, &label, w.saturating_sub(4 + cw + 1), st);
        if !cnt.is_empty() {
            put_right(buf, x0 + w - 1, y, &cnt, cw, base.fg(c(if sel { t.selection_fg } else { t.muted })));
        }
    }
}

fn list_hints(app: &App) -> String {
    let h = list_hints_inner(app);
    // Inside a drilled-down list: say how to get back.
    if app.list().is_some_and(|v| v.filtering) {
        return "type to filter  enter open  esc clear".into();
    }
    if app.list().is_some_and(|v| v.all.is_some()) {
        return format!("{} clear filter  {h}", app.keys.hint(Action::Back));
    }
    if app.lists.len() > 1 { format!("{} back  {h}", app.keys.hint(Action::Back)) } else { h }
}

fn list_hints_inner(app: &App) -> String {
    let Some(v) = app.list() else { return String::new() };
    if v.kind == ListKind::Queue {
        return hints(
            app,
            &[(Action::PlaySelected, "play"), (Action::MoveDown, "move"), (Action::Remove, "remove"), (Action::ClearQueue, "clear")],
        )
        .replace("J move", "J/K move");
    }
    if v.playlist_id().is_some() {
        return hints(
            app,
            &[(Action::PlaySelected, "play"), (Action::MoveDown, "move"), (Action::Remove, "remove"), (Action::ExportPlaylist, "export")],
        )
        .replace("J move", "J/K move");
    }
    if v.is_albums() {
        return hints(
            app,
            &[(Action::PlaySelected, "open"), (Action::AddQueue, "queue"), (Action::GridView, "grid"), (Action::Search, "search")],
        );
    }
    if v.items.first().is_some_and(|i| matches!(i, Item::Node(_))) {
        return hints(app, &[(Action::PlaySelected, "open"), (Action::AddQueue, "queue"), (Action::Search, "search")]);
    }
    hints(app, &[(Action::PlaySelected, "play"), (Action::AddQueue, "queue"), (Action::PlayNext, "next"), (Action::Search, "search")])
}

fn list_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::List;
    let mut title = app.list().map(|v| v.title.clone()).unwrap_or_default();
    if app.list().is_some_and(|v| v.node == crate::source::podcast::DOWNLOADS && !v.items.is_empty()) {
        title = format!("{title} · {}", app.downloads_label);
    }
    let can_back = app.lists.len() > 1;
    if can_back {
        title = format!("‹ {title}");
    }
    if let Some(v) = app.list().filter(|v| v.filtering || v.all.is_some()) {
        let n = v.items.len();
        title = format!("{title} · filter: {}{} ({n})", v.filter, if v.filtering { "▏" } else { "" });
    }
    let h = list_hints(app);
    let inner = pane(f, app, area, &title, &h, focused);
    if can_back && inner.y > 0 {
        // Clicking the title goes back.
        let w = (title.width() as u16 + 1).min(inner.width);
        super::clicks::add(Rect { x: inner.x, y: inner.y - 1, width: w, height: 1 }, super::clicks::Click::Act(Action::Back));
    }
    app.hit.list = inner;
    let t = app.theme.clone();
    let playing = app.now().track.and_then(|t| t.r.clone());
    let Some(v) = app.lists.last() else { return };
    let buf = f.buffer_mut();
    if v.loading && v.items.is_empty() {
        put(buf, inner.x + 3, inner.y, "Loading…", inner.width, Style::default().fg(c(t.muted)));
        return;
    }
    if let Some(e) = &v.error {
        put(buf, inner.x + 3, inner.y, e, inner.width.saturating_sub(3), Style::default().fg(c(t.error)));
        return;
    }
    if v.items.is_empty() {
        let msg = match (v.kind, app.scan) {
            (ListKind::Queue, _) => "Queue is empty · a adds songs".to_string(),
            _ if v.all.is_some() => "Nothing matches the filter".into(),
            _ if v.node == super::RADIO_FIND => "Press enter to search stations by name, genre or country".into(),
            _ if v.node == super::PODCAST_FIND => "Press enter to find a podcast by name, or add one by its feed address".into(),
            _ if v.node == crate::source::podcast::SHOWS => {
                format!("No shows yet · Find a podcast, then {} on it subscribes", app.keys.hint(Action::Like))
            }
            _ if v.node == crate::source::podcast::NEW => "No new episodes".into(),
            _ if v.node == crate::source::podcast::DOWNLOADS => {
                format!("No downloads · {} on an episode downloads it", app.keys.hint(Action::Download))
            }
            _ if v.node.starts_with("podcast:") => "Nothing found".into(),
            _ if v.node == super::LASTFM_PROFILE => "Press enter to open your Last.fm profile in the browser".into(),
            _ if v.node == "lastfm:none" => "This one is not in your library".into(),
            _ if v.node.starts_with("lastfm:") => "Nothing on Last.fm yet".into(),
            _ if v.node == super::RADIO_ADD => "Press enter to add a station by its web address (it goes to Favorites)".into(),
            _ if v.node == crate::source::radio::FAVORITES => {
                format!("No favorite stations yet · {} on a station adds it", app.keys.hint(Action::Like))
            }
            _ if v.node.starts_with("radio:") => "No stations found".into(),
            (_, Some((d, n))) => format!("Scanning… {d}/{n}"),
            _ if app.library_size == 0 && v.node != "queue" => "No music yet. Add folders in Settings (S) → Library.".into(),
            _ => "Nothing here".into(),
        };
        put(buf, inner.x + 3, inner.y, &msg, inner.width.saturating_sub(3), Style::default().fg(c(t.muted)));
        return;
    }
    if app.cfg.album_grid && v.is_albums() {
        album_grid(f, app, inner);
        return;
    }
    let row_h: usize = if app.cfg.density == "comfortable" { 2 } else { 1 };
    let rows = (inner.height as usize / row_h).max(1);
    app.hit.page = rows.saturating_sub(1);
    let v = app.lists.last_mut().unwrap();
    if v.cursor < v.offset {
        v.offset = v.cursor;
    }
    if v.cursor >= v.offset + rows {
        v.offset = v.cursor + 1 - rows;
    }
    let album_view = v.node.starts_with("album:") || v.node.starts_with("subsonic:album:");
    let (offset, cursor) = (v.offset, v.cursor);
    let items: Vec<Item> = v.items[offset..(offset + rows).min(v.items.len())].to_vec();
    let buf = f.buffer_mut();
    for (r, it) in items.iter().enumerate() {
        let i = offset + r;
        let y = inner.y + (r * row_h) as u16;
        app.hit.rows.push((y, i));
        let sel = i == cursor;
        let is_playing = matches!(it, Item::Track(tr) if tr.r.is_some() && tr.r == playing);
        let rating = if let Item::Track(tr) = it { app.rating_of(tr) } else { None };
        row(buf, &t, Rect { y, height: 1, ..inner }, it, sel, focused, is_playing, album_view, rating);
    }
}

#[allow(clippy::too_many_arguments)]
fn row(buf: &mut Buffer, t: &Theme, r: Rect, it: &Item, sel: bool, focused: bool, playing: bool, album_view: bool, rating: Option<u8>) {
    let bg = if sel { if focused { t.selection } else { t.selection_dim } } else { t.bg };
    if sel {
        fill(buf, r, bg);
    }
    let base = Style::default().bg(c(bg));
    let fg = if playing {
        t.accent
    } else if sel {
        t.selection_fg
    } else {
        t.fg
    };
    let main = {
        let s = base.fg(c(fg));
        if sel || playing { s.add_modifier(Modifier::BOLD) } else { s }
    };
    let muted = base.fg(c(if playing { t.accent } else { t.muted }));
    let (glyph, gst) = if sel {
        ("→", base.fg(c(t.marker)))
    } else if playing {
        ("♪", base.fg(c(t.accent)))
    } else {
        (" ", base)
    };
    put(buf, r.x + 1, r.y, glyph, 1, gst);
    let x = r.x + 3;
    let right = r.x + r.width - 1;
    let w = right.saturating_sub(x);
    match it {
        Item::Track(tr) => {
            let dur = tr.duration_ms.map(fmt_time).unwrap_or_default();
            let mut dx = put_right(buf, right, r.y, &dur, 9, muted);
            if let Some(n) = rating {
                dx =
                    put_right(buf, dx.saturating_sub(2), r.y, &super::stars(n), 5, base.fg(c(if sel { t.selection_fg } else { t.accent })));
            }
            let avail = dx.saturating_sub(x + 2);
            if album_view {
                let no = tr.track_no.map(|n| n.to_string()).unwrap_or_default();
                put_right(buf, x + 3, r.y, &no, 3, muted);
                put(buf, x + 6, r.y, tr.display_title(), avail.saturating_sub(6), main);
            } else {
                let tw = (avail as f32 * 0.42) as u16;
                let aw = (avail as f32 * 0.28) as u16;
                put(buf, x, r.y, tr.display_title(), tw.saturating_sub(1), main);
                put(
                    buf,
                    x + tw,
                    r.y,
                    tr.display_artist(),
                    aw.saturating_sub(1),
                    if playing { main } else { base.fg(c(if sel { t.selection_fg } else { t.fg })) },
                );
                put(buf, x + tw + aw, r.y, &tr.album, avail.saturating_sub(tw + aw), muted);
            }
        }
        Item::Node(n) => {
            let right_txt = match (n.kind, n.count) {
                (_, Some(c)) if n.id.starts_with("radio:") => format!("{c} stations"),
                (_, Some(c)) if n.id.starts_with("podcast:") => format!("{c} new"),
                (NodeKind::Album, Some(c)) => format!("{c} songs"),
                (NodeKind::Artist, Some(c)) => format!("{c} songs"),
                (_, Some(c)) => c.to_string(),
                _ => String::new(),
            };
            let rx = put_right(buf, right, r.y, &right_txt, 14, muted);
            let avail = rx.saturating_sub(x + 2);
            let prefix = if n.kind == NodeKind::Folder { "▸ " } else { "" };
            let title = format!("{prefix}{}", n.title);
            if n.subtitle.is_empty() {
                put(buf, x, r.y, &title, avail, main);
            } else {
                let tw = (avail as f32 * 0.55) as u16;
                put(buf, x, r.y, &title, tw.saturating_sub(1), main);
                put(buf, x + tw, r.y, &n.subtitle, avail.saturating_sub(tw), muted);
            }
        }
    }
    let _ = w;
}

fn album_grid(f: &mut Frame, app: &mut App, inner: Rect) {
    let t = app.theme.clone();
    let cell_w: u16 = 24;
    let cover_h: u16 = 10;
    let cell_h = cover_h + 3;
    let cols = (inner.width / cell_w).max(1) as usize;
    let rows = (inner.height / cell_h).max(1) as usize;
    app.hit.grid_cols = cols;
    app.hit.page = rows;
    let focused = app.focus == Focus::List;
    let v = app.lists.last_mut().unwrap();
    let cur_row = v.cursor / cols;
    if cur_row < v.offset {
        v.offset = cur_row;
    }
    if cur_row >= v.offset + rows {
        v.offset = cur_row + 1 - rows;
    }
    let first = v.offset * cols;
    let cursor = v.cursor;
    let items: Vec<(usize, Item)> = v.items.iter().cloned().enumerate().skip(first).take(rows * cols).collect();
    let pad = (inner.width - cols as u16 * cell_w) / 2;
    for (i, it) in items {
        let k = i - first;
        let x = inner.x + pad + (k % cols) as u16 * cell_w;
        let y = inner.y + (k / cols) as u16 * cell_h;
        let Item::Node(n) = it else { continue };
        let cover = Rect { x: x + 1, y, width: cell_w - 2, height: cover_h };
        for yy in y..y + cell_h {
            app.hit.rows.push((yy, i));
        }
        let sel = i == cursor;
        art_or_placeholder(f, app, cover, n.art.as_ref(), Slot::Grid);
        let buf = f.buffer_mut();
        let bg = if sel { if focused { t.selection } else { t.selection_dim } } else { t.bg };
        let text = Rect { x, y: y + cover_h, width: cell_w, height: 2 };
        if sel {
            fill(buf, text, bg);
            put(buf, x, y + cover_h, "→", 1, Style::default().fg(c(t.marker)).bg(c(bg)));
        }
        let st = Style::default().bg(c(bg)).fg(c(if sel { t.selection_fg } else { t.fg }));
        put(buf, x + 2, y + cover_h, &n.title, cell_w - 3, if sel { st.add_modifier(Modifier::BOLD) } else { st });
        put(buf, x + 2, y + cover_h + 1, &n.subtitle, cell_w - 3, Style::default().bg(c(bg)).fg(c(t.muted)));
    }
}

/// Draw cover art in `area` or a themed placeholder (♪ on a soft tile).
fn art_or_placeholder(f: &mut Frame, app: &mut App, area: Rect, art: Option<&crate::model::ArtRef>, slot: Slot) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = app.theme.clone();
    if app.images_on
        && let Some(a) = art
    {
        app.want_art(a, Size::new(area.width, area.height), slot);
        let pic = match slot {
            Slot::Grid => app.grid_pics.get(a),
            _ => app.pics.iter().find(|(s, _)| *s == slot).map(|(_, p)| p),
        };
        if let Some(p) = pic.filter(|p| &p.art == a)
            && let Some(proto) = &p.proto
        {
            let a2 = proto.size();
            // Centre the image in the area.
            let r = Rect {
                x: area.x + area.width.saturating_sub(a2.width) / 2,
                y: area.y + area.height.saturating_sub(a2.height) / 2,
                width: a2.width.min(area.width),
                height: a2.height.min(area.height),
            };
            f.render_widget(Image::new(proto), r);
            return;
        }
    }
    let buf = f.buffer_mut();
    let tile = t.viz[1].mix(t.bg, 0.65);
    fill(buf, area, tile);
    let mid = Position::new(area.x + area.width / 2, area.y + area.height / 2);
    let label = if art.is_some() && app.images_on { "…" } else { "♪" };
    put(buf, mid.x.saturating_sub(0), mid.y, label, 1, Style::default().fg(c(t.viz[0])).bg(c(tile)).add_modifier(Modifier::BOLD));
}

/// Cover size in cells for a given height (cells are about twice as tall as wide).
fn cover_rect(x: u16, y: u16, h: u16, max_w: u16) -> Rect {
    let w = (h * 2).min(max_w);
    let h = w.div_ceil(2).min(h);
    Rect { x, y, width: w, height: h }
}

fn progress(buf: &mut Buffer, app: &mut App, r: Rect) {
    let t = &app.theme;
    let snap = app.now();
    let pos = app.engine.position().as_millis() as u64;
    if snap.track.as_deref().is_some_and(is_live) {
        // Radio: no end and no seeking; show how long you've been listening.
        let a = fmt_time(pos);
        put(buf, r.x, r.y, &a, 8, Style::default().fg(c(t.fg)));
        let bx = r.x + a.width() as u16 + 1;
        let bw = r.width.saturating_sub(a.width() as u16 + 9);
        for i in 0..bw {
            buf.set_string(bx + i, r.y, "─", Style::default().fg(c(t.track)));
        }
        put_right(buf, r.x + r.width, r.y, "● LIVE", 6, Style::default().fg(c(t.accent)).add_modifier(Modifier::BOLD));
        return;
    }
    let len = snap.track.as_ref().and_then(|t| t.duration_ms).unwrap_or(0);
    let pos = pos.min(len.max(pos));
    let a = fmt_time(pos);
    let b = fmt_time(len);
    put(buf, r.x, r.y, &a, 8, Style::default().fg(c(t.fg)));
    let bx = r.x + a.width() as u16 + 1;
    let bw = r.width.saturating_sub(a.width() as u16 + b.width() as u16 + 2);
    put_right(buf, r.x + r.width, r.y, &b, 8, Style::default().fg(c(t.muted)));
    if bw < 3 {
        return;
    }
    app.hit.progress = Rect { x: bx, y: r.y, width: bw, height: 1 };
    let frac = if len > 0 { (pos as f64 / len as f64).clamp(0.0, 1.0) } else { 0.0 };
    let knob = ((bw - 1) as f64 * frac).round() as u16;
    for i in 0..bw {
        let (ch, col) = if i < knob {
            ("━", t.accent)
        } else if i == knob {
            ("●", t.accent)
        } else {
            ("─", t.track)
        };
        buf.set_string(bx + i, r.y, ch, Style::default().fg(c(col)));
    }
}

/// Internet radio: endless, no seeking.
pub fn is_live(t: &Track) -> bool {
    t.r.as_ref().is_some_and(|r| &*r.source == crate::source::radio::ID)
}

/// Player controls: (text, color, bold, what a click does).
fn controls(app: &App, with_vol: bool) -> Vec<(String, Rgb, bool, Option<Action>)> {
    let t = &app.theme;
    let snap = app.now();
    let (shuffle, repeat) = {
        let q = app.engine.queue.lock();
        (q.shuffle, q.repeat)
    };
    let pp = if snap.state == PlayState::Playing { "⏸" } else { "⏯" };
    let mut v = vec![
        ("⏮".to_string(), t.fg, true, Some(Action::Prev)),
        ("  ".to_string(), t.fg, false, None),
        (pp.to_string(), t.fg, true, Some(Action::PlayPause)),
        ("  ".to_string(), t.fg, false, None),
        ("⏭".to_string(), t.fg, true, Some(Action::Next)),
        ("   ".to_string(), t.fg, false, None),
    ];
    v.push((format!("⇄ {}", if shuffle { "on" } else { "off" }), if shuffle { t.accent } else { t.muted }, false, Some(Action::Shuffle)));
    v.push(("   ".to_string(), t.fg, false, None));
    let rc = if repeat != crate::model::Repeat::Off { t.accent } else { t.muted };
    v.push((format!("↻ {}", repeat.label()), rc, false, Some(Action::Repeat)));
    if with_vol {
        v.push((format!("   vol {}%", (snap.volume * 100.0).round()), t.muted, false, None));
    }
    v
}

/// "FLAC 24/96 → 48 kHz · gapless · RG −6.2 dB"
pub fn audio_info(app: &App) -> Vec<(String, Rgb)> {
    let t = &app.theme;
    let s = app.now();
    let Some(f) = s.format.clone() else { return vec![] };
    let mut out = vec![(f.short(), t.muted)];
    let out_rate = app.engine.output_rate();
    let rate_ok = out_rate == 0 || out_rate == f.sample_rate;
    if !rate_ok {
        out.push((format!(" → {} kHz", khz(out_rate)), t.warn));
    }
    // Gapless: the next track shares the format, so it will flow straight on.
    let next_same = {
        let q = app.engine.queue.lock();
        let next = q.next_after(s.item_id, true).map(|i| i.track.clone());
        next.and_then(|n| n.format.clone()).is_some_and(|nf| nf.sample_rate == f.sample_rate && nf.channels == f.channels)
    };
    if next_same {
        out.push((" · gapless".into(), t.muted));
    }
    if app.eq_active() {
        out.push((format!(" · EQ {}", app.cfg.eq_preset), t.accent));
    }
    if let Some((s, done)) = app.lastfm_status() {
        out.push((format!(" · {s}"), if done { t.good } else { t.muted }));
    }
    match s.gain_db {
        Some(g) => {
            let tag = if s.gain_origin == crate::model::GainOrigin::Measured { "R128" } else { "RG" };
            out.push((format!(" · {tag} {}{:.1} dB", if g < 0.0 { "−" } else { "+" }, g.abs()), t.muted));
        }
        None if rate_ok && out_rate != 0 && s.volume >= 1.0 && !app.eq_active() => out.push((" · bit-perfect".into(), t.good)),
        None => {}
    }
    out
}

fn spans_width(v: &[(String, Rgb)]) -> u16 {
    v.iter().map(|(s, _)| s.width() as u16).sum()
}

fn put_spans(buf: &mut Buffer, mut x: u16, y: u16, v: &[(String, Rgb)], max_x: u16) {
    for (s, col) in v {
        if x >= max_x {
            break;
        }
        x = put(buf, x, y, s, max_x - x, Style::default().fg(c(*col)));
    }
}

fn visualizer(buf: &mut Buffer, app: &mut App, r: Rect) {
    if r.height == 0 || r.width < 4 || !app.cfg.visualizer {
        return;
    }
    app.viz_area = r;
    let n = viz_bars(r);
    let t = &app.theme;
    const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    for i in 0..n.min(app.viz.bars.len()) {
        let v = app.viz.bars[i];
        let p = i as f64 / (n.max(2) - 1) as f64;
        let col = if p < 0.5 { t.viz[0].mix(t.viz[1], p * 2.0) } else { t.viz[1].mix(t.viz[2], (p - 0.5) * 2.0) };
        let total = (v * r.height as f32 * 8.0).round() as u16;
        for row in 0..r.height {
            let level = total.saturating_sub(row * 8).min(8);
            if level == 0 {
                break;
            }
            let y = r.y + r.height - 1 - row;
            buf.set_string(r.x + (i * 2) as u16, y, EIGHTHS[level as usize], Style::default().fg(c(col)));
        }
    }
}

/// One bar per two columns (bar + gap), like the mockup.
pub fn viz_bars(r: Rect) -> usize {
    (r.width as usize).div_ceil(2)
}

fn now_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Panel;
    let k = |a| app.keys.hint(a);
    let h = format!(
        "{} pause  {}/{} skip  {}/{} seek  {} full",
        k(Action::PlayPause),
        k(Action::Next),
        k(Action::Prev),
        k(Action::SeekBack),
        k(Action::SeekFwd),
        k(Action::ToggleScreen)
    );
    let inner = pane(f, app, area, "Now Playing", &h, focused);
    app.hit.panel = inner;
    let snap = app.now();
    let t = app.theme.clone();
    let Some(track) = snap.track.clone() else {
        put(
            f.buffer_mut(),
            inner.x + 3,
            inner.y + 1,
            "Nothing playing · enter plays the selected song",
            inner.width.saturating_sub(4),
            Style::default().fg(c(t.muted)),
        );
        return;
    };
    let ih = inner.height.saturating_sub(1);
    let cover = cover_rect(inner.x + 1, inner.y + (if inner.height > ih { 1 } else { 0 }) / 2, ih, inner.width / 3);
    art_or_placeholder(f, app, cover, track.art.as_ref(), Slot::Panel);
    let x = cover.x + cover.width + 3;
    let w = (inner.x + inner.width).saturating_sub(x + 1);
    let buf = f.buffer_mut();
    let y0 = inner.y;
    put(buf, x, y0, track.display_title(), w, Style::default().fg(c(t.fg)).add_modifier(Modifier::BOLD));
    put(buf, x, y0 + 1, &meta_line(&track), w, Style::default().fg(c(t.muted)));
    if inner.height >= 5 {
        progress(buf, app, Rect { x, y: y0 + 3, width: w, height: 1 });
    }
    if inner.height >= 6 {
        let ctrl = controls(app, false);
        let mut cx = x;
        for (s, col, bold, act) in &ctrl {
            let st = Style::default().fg(c(*col));
            let x0 = cx;
            cx = put(buf, cx, y0 + 4, s, (x + w).saturating_sub(cx), if *bold { st.add_modifier(Modifier::BOLD) } else { st });
            if let Some(a) = act {
                super::clicks::add(Rect { x: x0, y: y0 + 4, width: (cx - x0).max(2), height: 1 }, super::clicks::Click::Act(*a));
            }
        }
        let mut info = audio_info(app);
        // Too wide: drop the least important parts first, keep format and EQ.
        for drop in [" · gapless", " · R", " · bit-perfect"] {
            if cx + 3 + spans_width(&info) <= x + w {
                break;
            }
            info.retain(|(s, _)| !s.starts_with(drop));
        }
        let iw = spans_width(&info);
        if cx + 3 + iw <= x + w {
            put_spans(buf, x + w - iw, y0 + 4, &info, x + w);
        }
    }
    if inner.height >= 6 {
        let vy = y0 + 5;
        let vr = Rect { x, y: vy, width: w, height: (inner.y + inner.height).saturating_sub(vy) };
        visualizer(buf, app, vr);
    }
}

fn meta_line(t: &Track) -> String {
    let mut parts = vec![t.display_artist().to_string()];
    if !t.album.is_empty() {
        parts.push(t.album.clone());
    }
    if let Some(y) = t.year {
        parts.push(y.to_string());
    }
    parts.join(" · ")
}

// ---------------- screen B ----------------

fn now_playing(f: &mut Frame, app: &mut App, area: Rect) {
    let left_w = (area.width as u32 * 2 / 3) as u16;
    let left = Rect { width: left_w, ..area };
    let right = Rect { x: area.x + left_w + 1, width: area.width.saturating_sub(left_w + 1), ..area };
    big_panel(f, app, left);
    up_next(f, app, right);
}

fn big_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Panel;
    let h =
        hints(app, &[(Action::ToggleScreen, "back"), (Action::Lyrics, "lyrics"), (Action::Visualizer, "visualizer")]).replace("  ", " · ");
    let inner = pane(f, app, area, "Now Playing", &h, focused);
    app.hit.panel = inner;
    let t = app.theme.clone();
    let snap = app.now();
    let Some(track) = snap.track.clone() else {
        put(
            f.buffer_mut(),
            inner.x + 2,
            inner.y + 1,
            "Nothing playing · press z to go back and pick music",
            inner.width.saturating_sub(3),
            Style::default().fg(c(t.muted)),
        );
        return;
    };
    // Bottom part: progress, controls, visualizer.
    let viz_h = if app.cfg.visualizer { (inner.height / 4).clamp(3, 8) } else { 0 };
    let bottom_h = 3 + viz_h;
    let top_h = inner.height.saturating_sub(bottom_h + 1);
    let cover = cover_rect(inner.x + 2, inner.y + 1, top_h.saturating_sub(1), inner.width * 2 / 5);
    art_or_placeholder(f, app, cover, track.art.as_ref(), Slot::Big);
    let x = cover.x + cover.width + 3;
    let w = (inner.x + inner.width).saturating_sub(x + 2);
    let buf = f.buffer_mut();
    let mut y = inner.y + 1;
    put(buf, x, y, track.display_title(), w, Style::default().fg(c(t.fg)).add_modifier(Modifier::BOLD));
    y += 1;
    put(buf, x, y, track.display_artist(), w, Style::default().fg(c(t.fg)));
    y += 1;
    let mut meta = Vec::new();
    if !track.album.is_empty() {
        meta.push(track.album.clone());
    }
    if let Some(yr) = track.year {
        meta.push(yr.to_string());
    }
    if let Some(d) = track.disc_no.filter(|d| *d > 1) {
        meta.push(format!("disc {d}"));
    }
    if let Some(n) = track.track_no {
        meta.push(match track.track_total {
            Some(tot) => format!("track {n} of {tot}"),
            None => format!("track {n}"),
        });
    }
    if let Some(n) = app.rating_of(&track) {
        meta.push(super::stars(n));
    }
    put(buf, x, y, &meta.join(" · "), w, Style::default().fg(c(t.muted)));
    y += 1;
    put_spans(buf, x, y, &audio_info(app), x + w);
    y += 2;
    // Lyrics: current line highlighted, a few around it.
    let lyr_bottom = inner.y + top_h;
    if app.cfg.lyrics && y < lyr_bottom {
        let avail = (lyr_bottom - y) as usize;
        if app.current_lyric().is_none() {
            let hint = match app.lookup_label() {
                Some(l) => format!("{l} online…"),
                None => format!("No lyrics · {} looks them up online", app.keys.hint(Action::FetchLyrics)),
            };
            put(buf, x, y, &hint, w, Style::default().fg(c(t.muted)));
        }
        if let Some((l, cur)) = app.current_lyric() {
            if !l.synced.is_empty() {
                let cur_i = cur.unwrap_or(0);
                let before = avail / 2;
                let start = cur_i.saturating_sub(before);
                for (k, (_, line)) in l.synced.iter().enumerate().skip(start).take(avail) {
                    let yy = y + (k - start) as u16;
                    if Some(k) == cur
                        && let Some((words, sung)) = app.current_words()
                    {
                        let bold = Style::default().add_modifier(Modifier::BOLD);
                        put(buf, x, yy, "♪ ", 2, bold.fg(c(t.accent)));
                        put_words(buf, x + 2, yy, &words, sung, w.saturating_sub(2), bold.fg(c(t.accent)), bold.fg(c(t.fg)));
                        continue;
                    }
                    let (txt, st) = if Some(k) == cur {
                        (format!("♪ {line}"), Style::default().fg(c(t.accent)).add_modifier(Modifier::BOLD))
                    } else if Some(k) == cur.map(|c| c + 1) || (cur.is_none() && k == 0) {
                        (line.clone(), Style::default().fg(c(t.fg)))
                    } else {
                        (line.clone(), Style::default().fg(c(t.muted)))
                    };
                    put(buf, x, yy, &txt, w, st);
                }
            } else if let Some(p) = &l.plain {
                for (k, line) in p.lines().take(avail).enumerate() {
                    put(buf, x, y + k as u16, line, w, Style::default().fg(c(t.muted)));
                }
            }
        }
    }
    let by = inner.y + inner.height - bottom_h;
    let full = Rect { x: inner.x + 2, y: by, width: inner.width.saturating_sub(4), height: 1 };
    progress(buf, app, full);
    let ctrl = controls(app, true);
    let mut cx = full.x;
    for (s, col, bold, act) in &ctrl {
        let st = Style::default().fg(c(*col));
        let x0 = cx;
        cx = put(buf, cx, by + 1, s, (full.x + full.width).saturating_sub(cx), if *bold { st.add_modifier(Modifier::BOLD) } else { st });
        if let Some(a) = act {
            super::clicks::add(Rect { x: x0, y: by + 1, width: (cx - x0).max(2), height: 1 }, super::clicks::Click::Act(*a));
        }
    }
    if viz_h > 0 {
        let vr = Rect { x: full.x, y: by + 3, width: full.width, height: viz_h };
        visualizer(buf, app, vr);
    }
}

fn up_next(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::List;
    let items: Vec<(Arc<Track>, Option<String>)> =
        app.engine.queue.lock().up_next().iter().map(|i| (i.track.clone(), i.from.clone())).collect();
    let title = format!("Up next · {}", items.len());
    let h =
        format!("{}/{} move · {} remove", app.keys.hint(Action::MoveDown), app.keys.hint(Action::MoveUp), app.keys.hint(Action::Remove));
    let inner = pane(f, app, area, &title, &h, focused);
    app.hit.list = inner;
    let t = app.theme.clone();
    if items.is_empty() {
        put(f.buffer_mut(), inner.x + 2, inner.y, "Nothing next", inner.width, Style::default().fg(c(t.muted)));
        return;
    }
    // Build display rows: "from: X" separators when the source list changes.
    let cur_from = app.engine.queue.lock().current().and_then(|i| i.from.clone());
    let mut rows: Vec<Option<usize>> = Vec::new();
    let mut last = cur_from;
    let mut labels = Vec::new();
    for (i, (_, from)) in items.iter().enumerate() {
        if from.is_some() && *from != last {
            rows.push(None);
            labels.push(from.clone().unwrap_or_default());
            last = from.clone();
        }
        rows.push(Some(i));
    }
    let n = inner.height as usize;
    app.hit.page = n.saturating_sub(1);
    let cur_row = rows.iter().position(|r| *r == Some(app.upnext_cursor)).unwrap_or(0);
    if cur_row < app.upnext_offset {
        app.upnext_offset = cur_row;
    }
    if cur_row >= app.upnext_offset + n {
        app.upnext_offset = cur_row + 1 - n;
    }
    let buf = f.buffer_mut();
    let mut label_i = rows[..app.upnext_offset].iter().filter(|r| r.is_none()).count();
    for (k, r) in rows.iter().enumerate().skip(app.upnext_offset).take(n) {
        let y = inner.y + (k - app.upnext_offset) as u16;
        match r {
            None => {
                put(
                    buf,
                    inner.x + 1,
                    y,
                    &format!("from: {}", labels[label_i]),
                    inner.width.saturating_sub(2),
                    Style::default().fg(c(t.muted)),
                );
                label_i += 1;
            }
            Some(i) => {
                app.hit.rows.push((y, *i));
                let sel = *i == app.upnext_cursor && focused;
                let rect = Rect { y, height: 1, ..inner };
                let bg = if sel { t.selection } else { t.bg };
                if sel {
                    fill(buf, rect, bg);
                    put(buf, inner.x, y, "→", 1, Style::default().fg(c(t.marker)).bg(c(bg)));
                }
                let tr = &items[*i].0;
                let dur = tr.duration_ms.map(fmt_time).unwrap_or_default();
                let dx = put_right(buf, inner.x + inner.width - 1, y, &dur, 9, Style::default().fg(c(t.muted)).bg(c(bg)));
                let st = Style::default().bg(c(bg)).fg(c(if sel { t.selection_fg } else { t.fg }));
                put(
                    buf,
                    inner.x + 1,
                    y,
                    tr.display_title(),
                    dx.saturating_sub(inner.x + 3),
                    if sel { st.add_modifier(Modifier::BOLD) } else { st },
                );
            }
        }
    }
}

// ---------------- status bar ----------------

fn status_bar(f: &mut Frame, app: &mut App, r: Rect) {
    let t = app.theme.clone();
    let buf = f.buffer_mut();
    fill(buf, r, t.status_bar);
    let snap = app.now();
    let up = app.engine.queue.lock().up_next().len();
    let left = if let Some((msg, err, _)) = app.status.as_ref().filter(|(_, e, _)| *e || app.lyrics_lookup.is_none()) {
        (format!("  {msg}"), if *err { t.error } else { t.fg })
    } else {
        let mut s = format!("♪ {} · {up} in queue · vol {}%", snap.state.label(), (snap.volume * 100.0).round());
        if let Some((d, n)) = app.scan {
            s.push_str(&format!(" · scanning {d}/{n}"));
        } else if let Some((d, n)) = app.loudness {
            s.push_str(&format!(" · measuring loudness {}%", d * 100 / n.max(1)));
        }
        if let Some(l) = app.sleep_label() {
            s.push_str(&format!(" · {l}"));
        }
        if let Some(l) = app.lookup_label() {
            s.push_str(&format!(" · {l}…"));
        }
        (format!(" {s}"), t.fg)
    };
    // Key hints on the right; drop them from the end before cutting the state text.
    let mut items: Vec<String> = [
        (Action::Search, "search"),
        (Action::Queue, "queue"),
        (Action::Themes, "themes"),
        (Action::Settings, "settings"),
        (Action::Help, "help"),
    ]
    .iter()
    .map(|(a, l)| format!("{} {l}", app.keys.hint(*a)))
    .collect();
    let lw0 = left.0.width() as u16;
    let mut right = items.join(" · ");
    while !items.is_empty() && lw0 + right.width() as u16 + 4 > r.width {
        items.pop();
        right = items.join(" · ");
    }
    let rw = right.width() as u16;
    put(buf, r.x, r.y, &left.0, r.width.saturating_sub(rw + 3), Style::default().fg(c(left.1)).bg(c(t.status_bar)));
    if rw > 0 {
        let hx = put_right(buf, r.x + r.width - 1, r.y, &right, rw, Style::default().fg(c(t.muted)).bg(c(t.status_bar)));
        super::clicks::hint_text(hx, r.y, &right);
    }
}

// ---------------- full-screen lyrics ----------------

/// Put `s` centered on row `y` of `r`.
fn put_center(buf: &mut Buffer, r: Rect, y: u16, s: &str, st: Style) {
    let s = fit(s, r.width.saturating_sub(4) as usize);
    let w = s.width() as u16;
    buf.set_string(r.x + (r.width.saturating_sub(w)) / 2, y, &s, st);
}

fn full_lyrics(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let buf = f.buffer_mut();
    fill(buf, area, t.bg);
    let body = Rect { y: area.y + 1, height: area.height.saturating_sub(3), ..area };
    let mid = body.y + body.height / 2;
    let muted = Style::default().fg(c(t.muted));
    let snap = app.now();
    match app.current_lyric() {
        Some((l, cur)) if !l.synced.is_empty() => {
            let lines: Vec<String> = l.synced.iter().map(|(_, s)| s.clone()).collect();
            let cur_i = cur.map(|c| c as i64).unwrap_or(-1);
            // Big current line (Kitty/Ghostty): a picture in a 7-row band in the middle.
            let band = Rect { x: body.x + 2, y: mid.saturating_sub(3), width: body.width.saturating_sub(4), height: 7.min(body.height) };
            let key = |i: i64| {
                lines.get(i.max(0) as usize).filter(|_| i >= 0).map(|text| super::biglyric::Key {
                    text: text.clone(),
                    color: t.accent,
                    cells: Size::new(band.width, band.height),
                    sung: None,
                })
            };
            let cur_words = app.current_words();
            let sung_chars = |words: &[String], n: usize| words.iter().take(n).map(|w| w.chars().count() + 1).sum::<usize>();
            let cur_key = key(cur_i).map(|mut k| {
                if let Some((words, sung)) = &cur_words {
                    k.text = words.join(" ");
                    k.sung = Some((sung_chars(words, *sung), t.fg));
                }
                k
            });
            // Karaoke: get every word state of this line ready ahead.
            if let (Some(k0), Some((words, _))) = (&cur_key, &cur_words) {
                for n in 0..=words.len() {
                    let mut k = k0.clone();
                    k.sung = Some((sung_chars(words, n), t.fg));
                    app.big_prefetch(k);
                }
            }
            let big = cur_key.as_ref().map(|k| app.big_pic(k).map(|p| p.cloned()));
            let big_mode = app.big_enabled() && big.is_some();
            if let Some(next) = key(cur_i + 1) {
                app.big_prefetch(next);
            }
            let buf = f.buffer_mut();
            for (k, line) in lines.iter().enumerate() {
                let d = k as i64 - cur_i;
                let gap = if big_mode && d != 0 { 3 * d.signum() } else { 0 };
                let y = mid as i64 + d * 2 + gap;
                if y < body.y as i64 || y >= (body.y + body.height) as i64 {
                    continue;
                }
                if d == 0 && matches!(big, Some(Some(Some(_)))) {
                    continue;
                }
                let st = if d == 0 {
                    Style::default().fg(c(t.accent)).add_modifier(Modifier::BOLD)
                } else if d == 1 {
                    Style::default().fg(c(t.fg))
                } else {
                    muted
                };
                if d == 0
                    && let Some((words, sung)) = &cur_words
                {
                    let ww = words_width(words).min(body.width.saturating_sub(4));
                    let x = body.x + body.width.saturating_sub(ww) / 2;
                    let bold = Style::default().add_modifier(Modifier::BOLD);
                    put_words(buf, x, y as u16, words, *sung, ww, bold.fg(c(t.accent)), bold.fg(c(t.fg)));
                    continue;
                }
                let text = if line.trim().is_empty() { "♪" } else { line.as_str() };
                put_center(buf, body, y as u16, text, st);
            }
            if let Some(Some(Some(proto))) = big {
                let s = proto.size();
                let r = Rect {
                    x: band.x + band.width.saturating_sub(s.width) / 2,
                    y: band.y,
                    width: s.width.min(band.width),
                    height: s.height.min(band.height),
                };
                f.render_widget(Image::new(&proto), r);
            }
        }
        Some((l, _)) => {
            // Plain lyrics: scroll with the song.
            let lines: Vec<&str> = l.plain.as_deref().unwrap_or("").lines().collect();
            let len = snap.track.as_ref().and_then(|t| t.duration_ms).unwrap_or(1).max(1);
            let frac = app.engine.position().as_millis() as f64 / len as f64;
            let top = ((lines.len() as f64 * frac) as usize).saturating_sub(body.height as usize / 2);
            for (k, line) in lines.iter().skip(top).take(body.height as usize).enumerate() {
                put_center(buf, body, body.y + k as u16, line, Style::default().fg(c(t.fg)));
            }
        }
        None => {
            let msg = match app.lookup_label() {
                Some(l) => format!("{l} online…"),
                None if snap.track.is_none() => "Nothing playing".into(),
                None => format!("No lyrics · {} looks them up online", app.keys.hint(Action::FetchLyrics)),
            };
            put_center(buf, body, mid, &msg, muted);
        }
    }
    // Bottom: title · artist, progress, close hint.
    let buf = f.buffer_mut();
    let y = area.y + area.height - 1;
    if let Some(tr) = snap.track.as_ref() {
        let left = format!("{} · {}", tr.display_title(), tr.display_artist());
        let lw = (left.width() as u16 + 2).min(area.width / 3);
        put(buf, area.x + 1, y, &left, lw.saturating_sub(1), Style::default().fg(c(t.fg)));
        let hint = format!("{} / esc close", app.keys.hint(Action::FullLyrics));
        let hw = hint.width() as u16;
        put_right(buf, area.x + area.width - 1, y, &hint, hw, muted);
        let px = area.x + lw + 1;
        let pw = area.width.saturating_sub(lw + hw + 4);
        progress(buf, app, Rect { x: px, y, width: pw, height: 1 });
    }
}

/// Draw words left to right: the first `sung` in `sung_st`, the rest in `rest_st`.
#[allow(clippy::too_many_arguments)]
fn put_words(buf: &mut Buffer, x: u16, y: u16, words: &[String], sung: usize, max_w: u16, sung_st: Style, rest_st: Style) {
    let mut cx = x;
    let end = x + max_w;
    for (i, w) in words.iter().enumerate() {
        if cx >= end {
            break;
        }
        let st = if i < sung { sung_st } else { rest_st };
        let text = if i + 1 < words.len() { format!("{w} ") } else { w.clone() };
        cx = put(buf, cx, y, &text, end - cx, st);
    }
}

fn words_width(words: &[String]) -> u16 {
    (words.iter().map(|w| w.width()).sum::<usize>() + words.len().saturating_sub(1)) as u16
}

// ---------------- mini mode ----------------

/// Three lines (title / artist, progress, controls + time) plus the cover when
/// there is room: above the text in a tall window, left of it in a wide one.
fn mini(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let inner = if area.height >= 5 {
        let kind = if app.cfg.corners == "square" { BorderType::Plain } else { BorderType::Rounded };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(kind)
            .border_style(Style::default().fg(c(t.border)).bg(c(t.bg)))
            .style(Style::default().bg(c(t.bg)));
        let i = block.inner(area);
        f.render_widget(block, area);
        Rect { x: i.x + 1, width: i.width.saturating_sub(2), ..i }
    } else {
        Rect { height: 3.min(area.height), ..area }
    };
    let snap = app.now();
    // One compact card centered in the window: cover + the three text lines.
    let mut text = Rect { height: 3.min(inner.height), ..inner };
    let mut cover: Option<Rect> = None;
    if snap.track.is_some() && inner.height >= 3 {
        if inner.width < inner.height * 3 && inner.height >= 8 {
            // Tall: cover on top, text right under it with the cover's width.
            let room = inner.height.saturating_sub(4).min(24);
            let r = cover_rect(0, 0, room, inner.width.min(48));
            let text_w = r.width.max(34).min(inner.width);
            let block_h = r.height + 1 + 3;
            let top = inner.y + (inner.height - block_h) / 2;
            cover = Some(Rect { x: inner.x + (inner.width - r.width) / 2, y: top, ..r });
            text = Rect { x: inner.x + (inner.width - text_w) / 2, y: top + r.height + 1, width: text_w, height: 3 };
        } else if inner.width >= 40 {
            // Wide: cover on the left, text next to it, the pair centered.
            let r = cover_rect(0, 0, inner.height.min(12), inner.width / 3);
            let text_w = (inner.width - r.width - 3).min(60);
            let block_w = r.width + 3 + text_w;
            let left = inner.x + (inner.width - block_w) / 2;
            let top = inner.y + (inner.height - r.height) / 2;
            cover = Some(Rect { x: left, y: top, ..r });
            text = Rect { x: left + r.width + 3, y: top + r.height.saturating_sub(3) / 2, width: text_w, height: 3 };
        }
    }
    if cover.is_none() {
        text.y = inner.y + inner.height.saturating_sub(3) / 2;
    }
    if let (Some(r), Some(tr)) = (cover, snap.track.as_ref()) {
        art_or_placeholder(f, app, r, tr.art.as_ref(), Slot::Panel);
    }
    let buf = f.buffer_mut();
    let inner = text;
    let right = inner.x + inner.width;
    let Some(track) = snap.track.clone() else {
        put(buf, inner.x, inner.y, "♪ Nothing playing", inner.width, Style::default().fg(c(t.muted)));
        if inner.height >= 3 {
            put(buf, inner.x, inner.y + 2, "/ search · m full view", inner.width, Style::default().fg(c(t.muted)));
        }
        return;
    };
    let artist = track.display_artist().to_string();
    let aw = (artist.width() as u16).min(inner.width / 2);
    let ax = put_right(buf, right, inner.y, &artist, aw, Style::default().fg(c(t.muted)));
    put(
        buf,
        inner.x,
        inner.y,
        &format!("♪ {}", track.display_title()),
        ax.saturating_sub(inner.x + 1),
        Style::default().fg(c(t.accent)).add_modifier(Modifier::BOLD),
    );
    if inner.height < 3 {
        return;
    }
    let pos = app.engine.position().as_millis() as u64;
    let len = track.duration_ms.unwrap_or(0);
    let bar = Rect { y: inner.y + 1, height: 1, ..inner };
    let live = is_live(&track);
    if !live {
        app.hit.progress = bar;
    }
    let frac = if len > 0 { (pos as f64 / len as f64).clamp(0.0, 1.0) } else { 0.0 };
    let fillw = (bar.width as f64 * frac).round() as u16;
    for i in 0..bar.width {
        let (ch, col) = if i < fillw { ("━", t.accent) } else { ("─", t.track) };
        buf.set_string(bar.x + i, bar.y, ch, Style::default().fg(c(col)));
    }
    let time =
        if live { format!("● LIVE  {}", fmt_time(pos)) } else { format!("{} / {}", fmt_time(pos.min(len.max(pos))), fmt_time(len)) };
    // The time always fits; the buttons give way in very narrow windows.
    let tw = (time.width() as u16).min(inner.width);
    put_right(buf, right, inner.y + 2, &time, tw, Style::default().fg(c(t.muted)));
    // Same spacing as the other screens: these symbols are drawn wide.
    let ctrl = controls(app, false);
    let ctrl = &ctrl[..5];
    let cw = ctrl.iter().map(|c| c.0.width() as u16).sum::<u16>() + 2;
    if inner.width >= tw + cw + 2 {
        let mut cx = inner.x;
        for (s, col, _, act) in ctrl {
            let x0 = cx;
            cx = put(buf, cx, inner.y + 2, s, cw, Style::default().fg(c(*col)).add_modifier(Modifier::BOLD));
            if let Some(a) = act {
                super::clicks::add(Rect { x: x0, y: inner.y + 2, width: (cx - x0).max(2), height: 1 }, super::clicks::Click::Act(*a));
            }
        }
    }
}
