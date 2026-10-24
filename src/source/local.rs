//! Local files. The catalog comes from the library database.
//!
//! Browse node ids: `artists`, `artist:<name>`, `albums`, `album:<key>`, `tracks`,
//! `genres`, `genre:<name>`, `folders`, `folder:<path>`, `playlist:<id>`,
//! `smart:most`, `smart:never`, `smart:week`, `smart:liked`, `smart:rated`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, bail};
use parking_lot::Mutex;
use rusqlite::{Connection, params};

use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::library::{Library, db, lrc, tags};
use crate::model::{ArtRef, SourceId, Track, TrackRef};

pub const ID: &str = "local";

pub struct LocalSource {
    id: SourceId,
    conn: Mutex<Connection>,
    lib: Arc<Library>,
}

impl LocalSource {
    pub fn new(lib: Arc<Library>) -> Result<Self> {
        Ok(Self { id: Arc::from(ID), conn: Mutex::new(db::open(&lib.db_path)?), lib })
    }

    fn album_nodes(&self, c: &Connection, where_: &str, p: impl rusqlite::Params) -> Result<Vec<Item>> {
        let sql = format!("SELECT key, title, artist, year, tracks, art FROM albums {where_}");
        let mut st = c.prepare_cached(&sql)?;
        let v = st.query_map(p, |r| {
            let year: Option<i32> = r.get(3)?;
            let artist: String = r.get(2)?;
            Ok(Node {
                id: format!("album:{}", r.get::<_, String>(0)?),
                kind: NodeKind::Album,
                title: r.get(1)?,
                subtitle: match year {
                    Some(y) => format!("{artist} · {y}"),
                    None => artist,
                },
                count: Some(r.get::<_, i64>(4)? as u64),
                art: r.get::<_, Option<String>>(5)?.map(|id| ArtRef { source: self.id.clone(), id }),
            })
        })?;
        Ok(v.filter_map(|r| r.ok()).map(Item::Node).collect())
    }

    fn tracks(&self, c: &Connection, where_order: &str, p: impl rusqlite::Params) -> Result<Vec<Item>> {
        Ok(db::tracks(c, &self.id, where_order, p)?.into_iter().map(Item::Track).collect())
    }

    fn folder(&self, c: &Connection, path: &str) -> Result<Vec<Item>> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut st = c.prepare_cached("SELECT DISTINCT folder FROM tracks WHERE substr(folder, 1, length(?1)) = ?1")?;
        let mut subs: Vec<String> = st
            .query_map([&prefix], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|f| f[prefix.len()..].split('/').next().map(str::to_string))
            .collect();
        subs.sort_by_key(|s| s.to_lowercase());
        subs.dedup();
        let mut out: Vec<Item> = subs
            .into_iter()
            .map(|name| {
                Item::Node(Node {
                    id: format!("folder:{prefix}{name}"),
                    kind: NodeKind::Folder,
                    title: name,
                    subtitle: String::new(),
                    count: None,
                    art: None,
                })
            })
            .collect();
        out.extend(self.tracks(c, "WHERE t.folder = ?1 ORDER BY t.disc_no, t.track_no, t.path", [path.trim_end_matches('/')])?);
        Ok(out)
    }

    /// Ids of playlists etc. live in the same database.
    pub fn conn(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }
}

fn page<T>(mut v: Vec<T>, p: Page) -> Vec<T> {
    if p.offset > 0 {
        v.drain(..p.offset.min(v.len()));
    }
    v.truncate(p.limit);
    v
}

fn fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Source for LocalSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::SEEK | Caps::PLAYLISTS | Caps::LYRICS | Caps::ART
    }

    fn children(&self, node: &str, p: Page) -> Result<Vec<Item>> {
        let c = self.conn.lock();
        let (kind, arg) = node.split_once(':').unwrap_or((node, ""));
        let week = db::now() - 7 * 86_400;
        let v = match kind {
            "artists" => {
                let mut st = c.prepare_cached("SELECT artist, COUNT(*), SUM(tracks) FROM albums GROUP BY lower(artist) ORDER BY lower(artist)")?;
                let v = st.query_map([], |r| {
                    let albums: i64 = r.get(1)?;
                    Ok(Item::Node(Node {
                        id: format!("artist:{}", r.get::<_, String>(0)?),
                        kind: NodeKind::Artist,
                        title: r.get(0)?,
                        subtitle: format!("{albums} album{}", if albums == 1 { "" } else { "s" }),
                        count: Some(r.get::<_, i64>(2)? as u64),
                        art: None,
                    }))
                })?;
                v.filter_map(|r| r.ok()).collect()
            }
            "artist" => self.album_nodes(&c, "WHERE lower(artist) = lower(?1) ORDER BY year, title", [arg])?,
            "albums" => self.album_nodes(&c, "ORDER BY lower(artist), year, lower(title)", [])?,
            "album" => self.tracks(&c, "WHERE t.album_key = ?1 ORDER BY t.disc_no, t.track_no, t.path", [arg])?,
            "tracks" => self.tracks(&c, "ORDER BY lower(t.artist), lower(t.album), t.disc_no, t.track_no", [])?,
            "genres" => {
                let mut st = c.prepare_cached("SELECT genre, COUNT(*) FROM tracks WHERE genre != '' GROUP BY genre ORDER BY lower(genre)")?;
                let v = st.query_map([], |r| {
                    Ok(Item::Node(Node {
                        id: format!("genre:{}", r.get::<_, String>(0)?),
                        kind: NodeKind::Genre,
                        title: r.get(0)?,
                        subtitle: String::new(),
                        count: Some(r.get::<_, i64>(1)? as u64),
                        art: None,
                    }))
                })?;
                v.filter_map(|r| r.ok()).collect()
            }
            "genre" => self.tracks(&c, "WHERE t.genre = ?1 ORDER BY lower(t.artist), lower(t.album), t.disc_no, t.track_no", [arg])?,
            "folders" => self
                .lib
                .folders()
                .into_iter()
                .map(|f| {
                    Item::Node(Node {
                        id: format!("folder:{}", f.to_string_lossy()),
                        kind: NodeKind::Folder,
                        title: f.to_string_lossy().into_owned(),
                        subtitle: String::new(),
                        count: None,
                        art: None,
                    })
                })
                .collect(),
            "folder" => self.folder(&c, arg)?,
            "playlist" => db::playlist_tracks(&c, &self.id, arg.parse()?)?.into_iter().map(Item::Track).collect(),
            "smart" => match arg {
                "most" => self.tracks(
                    &c,
                    "JOIN stats s ON s.source = 'local' AND s.id = t.path WHERE s.play_count > 0 ORDER BY s.play_count DESC, s.last_played DESC LIMIT 200",
                    [],
                )?,
                "never" => self.tracks(
                    &c,
                    "LEFT JOIN stats s ON s.source = 'local' AND s.id = t.path WHERE COALESCE(s.play_count, 0) = 0 ORDER BY lower(t.artist), lower(t.album), t.disc_no, t.track_no",
                    [],
                )?,
                "liked" => self.tracks(
                    &c,
                    "JOIN stats s ON s.source = 'local' AND s.id = t.path WHERE s.liked = 1 ORDER BY lower(t.artist), lower(t.album), t.disc_no, t.track_no",
                    [],
                )?,
                "rated" => self.tracks(
                    &c,
                    "JOIN stats s ON s.source = 'local' AND s.id = t.path WHERE s.rating >= 1 ORDER BY s.rating DESC, lower(t.artist), lower(t.album), t.track_no",
                    [],
                )?,
                "week" => self.tracks(&c, "WHERE t.added_at >= ?1 ORDER BY t.added_at DESC, lower(t.album), t.disc_no, t.track_no", [week])?,
                _ => bail!("unknown list {arg}"),
            },
            _ => bail!("unknown node {node}"),
        };
        Ok(page(v, p))
    }

    fn search(&self, q: &str, limit: usize) -> Result<SearchHits> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(SearchHits::default());
        }
        let c = self.conn.lock();
        let like = format!("%{}%", q.replace(['%', '_'], ""));
        let mut hits = SearchHits::default();
        let mut st = c.prepare_cached(
            "SELECT artist, COUNT(*) FROM albums WHERE artist LIKE ?1 GROUP BY lower(artist) ORDER BY lower(artist) LIMIT ?2",
        )?;
        hits.artists = st
            .query_map(params![like, limit as i64], |r| {
                Ok(Node {
                    id: format!("artist:{}", r.get::<_, String>(0)?),
                    kind: NodeKind::Artist,
                    title: r.get(0)?,
                    subtitle: format!("{} albums", r.get::<_, i64>(1)?),
                    count: None,
                    art: None,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        hits.albums = self
            .album_nodes(&c, "WHERE title LIKE ?1 OR artist LIKE ?1 ORDER BY lower(title) LIMIT ?2", params![like, limit as i64])?
            .into_iter()
            .filter_map(|i| if let Item::Node(n) = i { Some(n) } else { None })
            .collect();
        let fq = fts_query(q);
        if !fq.is_empty() {
            hits.tracks = db::tracks(
                &c,
                &self.id,
                "JOIN tracks_fts f ON f.rowid = t.id WHERE tracks_fts MATCH ?1 ORDER BY rank LIMIT ?2",
                params![fq, limit as i64],
            )?;
        }
        hits.playlists = db::playlists(&c)?
            .into_iter()
            .filter(|p| p.name.to_lowercase().contains(&q.to_lowercase()))
            .map(|p| Node {
                id: format!("playlist:{}", p.id),
                kind: NodeKind::Playlist,
                title: p.name,
                subtitle: String::new(),
                count: Some(p.count),
                art: None,
            })
            .collect();
        Ok(hits)
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        let c = self.conn.lock();
        Ok(db::track_by_path(&c, &self.id, &r.id)?.unwrap_or_else(|| Track { r: Some(r.clone()), ..Default::default() }))
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        Ok(Box::new(FileStream::open(Path::new(&r.id))?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        if let Some(p) = a.id.strip_prefix("embedded:") {
            return Ok(tags::embedded_art(Path::new(p)));
        }
        if let Some(p) = a.id.strip_prefix("file:") {
            return Ok(std::fs::read(p).ok());
        }
        Ok(None)
    }

    fn lyrics(&self, r: &TrackRef) -> Result<Option<Lyrics>> {
        let p = Path::new(&r.id);
        let text = std::fs::read_to_string(p.with_extension("lrc")).ok().or_else(|| tags::embedded_lyrics(p));
        Ok(text.map(|t| lrc::parse(&t)).filter(|l| !l.is_empty()))
    }
}
