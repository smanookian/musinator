//! SQLite database: tracks, albums, play stats, playlists, search index.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::tags::TrackRow;
use crate::model::{ArtRef, AudioFormat, Gain, GainOrigin, SourceId, Track, TrackRef};

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS tracks(
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  folder TEXT NOT NULL,
  mtime INTEGER NOT NULL, size INTEGER NOT NULL,
  title TEXT NOT NULL, artist TEXT NOT NULL, album_artist TEXT NOT NULL, album TEXT NOT NULL,
  album_key TEXT NOT NULL,
  year INTEGER, track_no INTEGER, track_total INTEGER, disc_no INTEGER, genre TEXT NOT NULL,
  duration_ms INTEGER NOT NULL, codec TEXT NOT NULL, sample_rate INTEGER NOT NULL, bits INTEGER,
  channels INTEGER NOT NULL, bitrate INTEGER,
  rg_track REAL, rg_track_peak REAL, rg_album REAL, rg_album_peak REAL,
  gain_origin INTEGER NOT NULL DEFAULT 0,
  art TEXT,
  lyrics INTEGER NOT NULL DEFAULT 0,
  added_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tracks_album ON tracks(album_key, disc_no, track_no);
CREATE INDEX IF NOT EXISTS tracks_folder ON tracks(folder);
CREATE INDEX IF NOT EXISTS tracks_genre ON tracks(genre);
CREATE INDEX IF NOT EXISTS tracks_added ON tracks(added_at);
CREATE TABLE IF NOT EXISTS albums(
  key TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL, year INTEGER,
  tracks INTEGER NOT NULL, art TEXT, genre TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS stats(
  source TEXT NOT NULL, id TEXT NOT NULL,
  play_count INTEGER NOT NULL DEFAULT 0, last_played INTEGER, liked INTEGER NOT NULL DEFAULT 0, rating INTEGER,
  PRIMARY KEY(source, id)
);
CREATE TABLE IF NOT EXISTS playlists(id INTEGER PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS playlist_items(
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  pos INTEGER NOT NULL, source TEXT NOT NULL, track_id TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '', duration_ms INTEGER
);
CREATE INDEX IF NOT EXISTS pl_items ON playlist_items(playlist_id, pos);
CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(title, artist, album, genre, content='tracks', content_rowid='id');
CREATE TRIGGER IF NOT EXISTS tracks_ai AFTER INSERT ON tracks BEGIN
  INSERT INTO tracks_fts(rowid, title, artist, album, genre) VALUES (new.id, new.title, new.artist || ' ' || new.album_artist, new.album, new.genre);
END;
CREATE TRIGGER IF NOT EXISTS tracks_ad AFTER DELETE ON tracks BEGIN
  INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, genre) VALUES ('delete', old.id, old.title, old.artist || ' ' || old.album_artist, old.album, old.genre);
END;
CREATE TRIGGER IF NOT EXISTS tracks_au AFTER UPDATE ON tracks BEGIN
  INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, genre) VALUES ('delete', old.id, old.title, old.artist || ' ' || old.album_artist, old.album, old.genre);
  INSERT INTO tracks_fts(rowid, title, artist, album, genre) VALUES (new.id, new.title, new.artist || ' ' || new.album_artist, new.album, new.genre);
END;
"#;

pub const TRACK_COLS: &str = "t.id, t.path, t.title, t.artist, t.album_artist, t.album, t.year, t.track_no, t.track_total, t.disc_no, t.genre, \
t.duration_ms, t.codec, t.sample_rate, t.bits, t.channels, t.bitrate, t.rg_track, t.rg_track_peak, t.rg_album, t.rg_album_peak, t.gain_origin, t.art";

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(c)
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn album_key(r: &TrackRow) -> String {
    let artist = if r.album_artist.is_empty() { &r.artist } else { &r.album_artist };
    format!("{}\u{1f}{}", artist.to_lowercase(), r.album.to_lowercase())
}

/// Insert or update one scanned file. `art` = "embedded" or a cover file path.
pub fn upsert(c: &Connection, r: &TrackRow, art: Option<&str>, lyrics: i64, added_at: i64) -> Result<()> {
    let origin = if r.rg_track.is_some() || r.rg_album.is_some() { 1 } else { 0 };
    c.prepare_cached(
        "INSERT INTO tracks(path, folder, mtime, size, title, artist, album_artist, album, album_key, year, track_no, track_total, disc_no, genre,
          duration_ms, codec, sample_rate, bits, channels, bitrate, rg_track, rg_track_peak, rg_album, rg_album_peak, gain_origin, art, lyrics, added_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)
         ON CONFLICT(path) DO UPDATE SET folder=excluded.folder, mtime=excluded.mtime, size=excluded.size, title=excluded.title,
          artist=excluded.artist, album_artist=excluded.album_artist, album=excluded.album, album_key=excluded.album_key, year=excluded.year,
          track_no=excluded.track_no, track_total=excluded.track_total, disc_no=excluded.disc_no, genre=excluded.genre,
          duration_ms=excluded.duration_ms, codec=excluded.codec, sample_rate=excluded.sample_rate, bits=excluded.bits,
          channels=excluded.channels, bitrate=excluded.bitrate, rg_track=excluded.rg_track, rg_track_peak=excluded.rg_track_peak,
          rg_album=excluded.rg_album, rg_album_peak=excluded.rg_album_peak, gain_origin=excluded.gain_origin, art=excluded.art, lyrics=excluded.lyrics",
    )?
    .execute(params![
        r.path,
        r.folder,
        r.mtime,
        r.size,
        r.title,
        r.artist,
        r.album_artist,
        r.album,
        album_key(r),
        r.year,
        r.track_no,
        r.track_total,
        r.disc_no,
        r.genre,
        r.duration_ms as i64,
        r.codec,
        r.sample_rate,
        r.bits,
        r.channels,
        r.bitrate,
        r.rg_track,
        r.rg_track_peak,
        r.rg_album,
        r.rg_album_peak,
        origin,
        art,
        lyrics,
        added_at
    ])?;
    Ok(())
}

/// Rebuild the album table from tracks.
pub fn rebuild_albums(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
         DELETE FROM albums;
         INSERT INTO albums(key, title, artist, year, tracks, art, genre)
           SELECT album_key, MAX(album), COALESCE(NULLIF(MAX(album_artist),''), MAX(artist)), MAX(year), COUNT(*),
                  MAX(CASE WHEN art='embedded' THEN 'embedded:' || path WHEN art IS NOT NULL THEN 'file:' || art END), MAX(genre)
           FROM tracks GROUP BY album_key;
         COMMIT;",
    )?;
    Ok(())
}

pub fn track_from_row(src: &SourceId, r: &Row) -> rusqlite::Result<Track> {
    let path: String = r.get(1)?;
    let art: Option<String> = r.get(22)?;
    let origin: i64 = r.get(21)?;
    let art =
        art.map(|a| ArtRef { source: src.clone(), id: if a == "embedded" { format!("embedded:{path}") } else { format!("file:{a}") } });
    Ok(Track {
        title: r.get(2)?,
        artist: r.get(3)?,
        album_artist: r.get(4)?,
        album: r.get(5)?,
        year: r.get(6)?,
        track_no: r.get(7)?,
        track_total: r.get(8)?,
        disc_no: r.get(9)?,
        duration_ms: Some(r.get::<_, i64>(11)? as u64),
        format: Some(AudioFormat {
            codec: r.get(12)?,
            sample_rate: r.get(13)?,
            bits: r.get(14)?,
            channels: r.get(15)?,
            bitrate_kbps: r.get(16)?,
        }),
        gain: Gain {
            track_db: r.get(17)?,
            track_peak: r.get(18)?,
            album_db: r.get(19)?,
            album_peak: r.get(20)?,
            origin: match origin {
                1 => GainOrigin::Tags,
                2 => GainOrigin::Measured,
                _ => GainOrigin::None,
            },
        },
        art,
        r: Some(TrackRef { source: src.clone(), id: path }),
    })
}

pub fn tracks(c: &Connection, src: &SourceId, where_order: &str, p: impl rusqlite::Params) -> Result<Vec<Arc<Track>>> {
    let sql = format!("SELECT {TRACK_COLS} FROM tracks t {where_order}");
    let mut st = c.prepare_cached(&sql)?;
    let rows = st.query_map(p, |r| track_from_row(src, r))?;
    Ok(rows.filter_map(|r| r.ok()).map(Arc::new).collect())
}

pub fn track_by_path(c: &Connection, src: &SourceId, path: &str) -> Result<Option<Track>> {
    let sql = format!("SELECT {TRACK_COLS} FROM tracks t WHERE t.path = ?1");
    Ok(c.query_row(&sql, [path], |r| track_from_row(src, r)).optional()?)
}

/// Count a play.
pub fn record_play(c: &Connection, source: &str, id: &str) -> Result<()> {
    c.execute(
        "INSERT INTO stats(source, id, play_count, last_played) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(source, id) DO UPDATE SET play_count = play_count + 1, last_played = excluded.last_played",
        params![source, id, now()],
    )?;
    Ok(())
}

pub fn toggle_like(c: &Connection, source: &str, id: &str) -> Result<bool> {
    c.execute(
        "INSERT INTO stats(source, id, liked) VALUES (?1, ?2, 1) ON CONFLICT(source, id) DO UPDATE SET liked = 1 - liked",
        params![source, id],
    )?;
    Ok(c.query_row("SELECT liked FROM stats WHERE source=?1 AND id=?2", params![source, id], |r| r.get::<_, i64>(0))? == 1)
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub count: u64,
}

pub fn playlists(c: &Connection) -> Result<Vec<Playlist>> {
    let mut st = c.prepare_cached(
        "SELECT p.id, p.name, (SELECT COUNT(*) FROM playlist_items i WHERE i.playlist_id = p.id) FROM playlists p ORDER BY p.name COLLATE NOCASE",
    )?;
    let v = st.query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, count: r.get::<_, i64>(2)? as u64 }))?;
    Ok(v.filter_map(|r| r.ok()).collect())
}

pub fn create_playlist(c: &Connection, name: &str) -> Result<i64> {
    c.execute("INSERT INTO playlists(name, created_at) VALUES (?1, ?2)", params![name, now()])?;
    Ok(c.last_insert_rowid())
}

pub fn rename_playlist(c: &Connection, id: i64, name: &str) -> Result<()> {
    c.execute("UPDATE playlists SET name=?2 WHERE id=?1", params![id, name])?;
    Ok(())
}

pub fn delete_playlist(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM playlist_items WHERE playlist_id=?1", [id])?;
    c.execute("DELETE FROM playlists WHERE id=?1", [id])?;
    Ok(())
}

pub fn add_to_playlist(c: &mut Connection, id: i64, tracks: &[Arc<Track>]) -> Result<()> {
    let tx = c.transaction()?;
    let mut pos: i64 = tx.query_row("SELECT COALESCE(MAX(pos), -1) + 1 FROM playlist_items WHERE playlist_id=?1", [id], |r| r.get(0))?;
    for t in tracks {
        let r = t.tref();
        tx.execute(
            "INSERT INTO playlist_items(playlist_id, pos, source, track_id, title, artist, duration_ms) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, pos, &*r.source, r.id, t.title, t.artist, t.duration_ms.map(|d| d as i64)],
        )?;
        pos += 1;
    }
    tx.commit()?;
    Ok(())
}

/// Remove the item at `pos` and close the gap.
pub fn remove_from_playlist(c: &mut Connection, id: i64, pos: i64) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("DELETE FROM playlist_items WHERE playlist_id=?1 AND pos=?2", params![id, pos])?;
    tx.execute("UPDATE playlist_items SET pos = pos - 1 WHERE playlist_id=?1 AND pos > ?2", params![id, pos])?;
    tx.commit()?;
    Ok(())
}

/// Swap two positions (reorder with J/K).
pub fn move_in_playlist(c: &mut Connection, id: i64, a: i64, b: i64) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("UPDATE playlist_items SET pos = -1 WHERE playlist_id=?1 AND pos=?2", params![id, a])?;
    tx.execute("UPDATE playlist_items SET pos = ?3 WHERE playlist_id=?1 AND pos=?2", params![id, b, a])?;
    tx.execute("UPDATE playlist_items SET pos = ?2 WHERE playlist_id=?1 AND pos=-1", params![id, b])?;
    tx.commit()?;
    Ok(())
}

/// Playlist items, resolved to library tracks where possible.
pub fn playlist_tracks(c: &Connection, src: &SourceId, id: i64) -> Result<Vec<Arc<Track>>> {
    let mut st =
        c.prepare_cached("SELECT source, track_id, title, artist, duration_ms FROM playlist_items WHERE playlist_id=?1 ORDER BY pos")?;
    let items: Vec<(String, String, String, String, Option<i64>)> =
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.filter_map(|r| r.ok()).collect();
    let mut out = Vec::with_capacity(items.len());
    for (source, tid, title, artist, dur) in items {
        let t = if source == **src { track_by_path(c, src, &tid)? } else { None };
        out.push(Arc::new(t.unwrap_or_else(|| Track {
            r: Some(TrackRef { source: Arc::from(source.as_str()), id: tid }),
            title,
            artist,
            duration_ms: dur.map(|d| d as u64),
            ..Default::default()
        })));
    }
    Ok(out)
}

/// Tracks still needing a loudness measurement.
pub fn unmeasured(c: &Connection) -> Result<Vec<(i64, String, String)>> {
    let mut st = c.prepare("SELECT id, path, album_key FROM tracks WHERE rg_track IS NULL ORDER BY album_key, disc_no, track_no")?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(v.filter_map(|r| r.ok()).collect())
}

pub fn set_measured(c: &Connection, id: i64, gain: f32, peak: f32) -> Result<()> {
    c.execute("UPDATE tracks SET rg_track=?2, rg_track_peak=?3, gain_origin=2 WHERE id=?1", params![id, gain, peak])?;
    Ok(())
}

/// After every track of an album is measured, set album gain from the duration-weighted energy mean.
pub fn set_measured_album(c: &Connection, album_key: &str) -> Result<()> {
    let mut st = c.prepare_cached("SELECT duration_ms, rg_track, rg_track_peak, gain_origin FROM tracks WHERE album_key=?1")?;
    let rows: Vec<(i64, Option<f64>, Option<f64>, i64)> =
        st.query_map([album_key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.filter_map(|r| r.ok()).collect();
    // Only albums measured entirely by us; tagged albums keep their own album gain.
    if rows.is_empty() || rows.iter().any(|r| r.1.is_none() || r.3 != 2) {
        return Ok(());
    }
    let (mut e, mut d, mut peak) = (0.0f64, 0.0f64, 0.0f64);
    for (dur, g, p) in rows.iter().map(|r| (r.0 as f64, r.1.unwrap(), r.2.unwrap_or(0.0))) {
        // gain = -18 - L  =>  energy ~ 10^(-gain/10)
        e += dur * 10f64.powf(-g / 10.0);
        d += dur;
        peak = peak.max(p);
    }
    if d > 0.0 && e > 0.0 {
        let album = -10.0 * (e / d).log10();
        c.execute("UPDATE tracks SET rg_album=?2, rg_album_peak=?3 WHERE album_key=?1", params![album_key, album, peak])?;
    }
    Ok(())
}
