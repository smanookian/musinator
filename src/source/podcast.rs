//! Podcasts: subscribe to RSS feeds (found via the free Apple Podcasts search, or by
//! address), list episodes, play them streamed (with seeking) or downloaded, and
//! remember where you stopped. Shows and episodes live in `podcasts.db`; downloads
//! in `~/.local/share/musinator/podcasts/`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::model::{ArtRef, SourceId, Track, TrackRef};

pub const ID: &str = "podcast";
pub const NEW: &str = "podcast:new";
pub const SHOWS: &str = "podcast:shows";
pub const DOWNLOADS: &str = "podcast:downloads";
/// Followed by the feed address.
pub const FEED: &str = "podcast:feed:";
/// Followed by the search text.
pub const SEARCH: &str = "podcast:search:";
const UA: &str = concat!("Musinator/", env!("CARGO_PKG_VERSION"), " (https://github.com/smanookian/musinator)");

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(secs))).http_status_as_error(false).build().into()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ---------------- feeds ----------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Episode {
    pub guid: String,
    pub title: String,
    pub url: String,
    pub date: i64,
    pub duration_s: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Feed {
    pub title: String,
    pub author: String,
    pub image: String,
    pub episodes: Vec<Episode>,
}

/// "Tue, 10 Jun 2003 04:00:00 GMT" / "+0200" → unix seconds.
pub fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|p| !p.is_empty()).collect();
    let start = usize::from(parts.first()?.chars().next()?.is_alphabetic());
    let (d, mon, y, time) =
        (parts.get(start)?, parts.get(start + 1)?, parts.get(start + 2)?, parts.get(start + 3).copied().unwrap_or("0:0:0"));
    const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = M.iter().position(|x| mon.to_ascii_lowercase().starts_with(x))? as i64 + 1;
    let (d, mut y): (i64, i64) = (d.parse().ok()?, y.parse().ok()?);
    if y < 100 {
        y += 2000;
    }
    let t: Vec<i64> = time.split(':').map(|x| x.parse().unwrap_or(0)).collect();
    let secs = t.first().unwrap_or(&0) * 3600 + t.get(1).unwrap_or(&0) * 60 + t.get(2).unwrap_or(&0);
    let tz = parts.get(start + 4).map_or(0, |z| {
        let z = z.trim();
        if (z.starts_with('+') || z.starts_with('-')) && z.len() == 5 {
            let v: i64 = z[1..].parse().unwrap_or(0);
            let off = (v / 100) * 3600 + (v % 100) * 60;
            if z.starts_with('-') { -off } else { off }
        } else {
            0
        }
    });
    // Days from civil date (Howard Hinnant).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + secs - tz)
}

/// "1:02:03", "62:03", "3723" → seconds.
pub fn parse_duration(s: &str) -> u64 {
    s.trim().split(':').fold(0, |acc, p| acc * 60 + p.trim().parse::<f64>().unwrap_or(0.0) as u64)
}

pub fn parse_feed(xml: &str) -> Result<Feed> {
    let doc = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
        .context("this is not a podcast feed")?;
    let channel = doc.descendants().find(|n| n.has_tag_name("channel")).context("this is not a podcast feed")?;
    let child_text = |n: roxmltree::Node, name: &str| {
        n.children().find(|c| c.tag_name().name() == name).and_then(|c| c.text()).map(|t| t.trim().to_string()).unwrap_or_default()
    };
    let image = channel
        .children()
        .find(|c| c.tag_name().name() == "image" && c.attribute("href").is_some())
        .and_then(|c| c.attribute("href").map(str::to_string))
        .or_else(|| channel.children().find(|c| c.has_tag_name("image")).map(|i| child_text(i, "url")))
        .unwrap_or_default();
    let mut feed = Feed { title: child_text(channel, "title"), author: child_text(channel, "author"), image, episodes: vec![] };
    for item in channel.children().filter(|c| c.has_tag_name("item")) {
        let Some(enc) = item.children().find(|c| c.has_tag_name("enclosure")) else { continue };
        let url = enc.attribute("url").unwrap_or("").trim().to_string();
        let kind = enc.attribute("type").unwrap_or("audio");
        if url.is_empty() || kind.starts_with("video") {
            continue;
        }
        let guid = child_text(item, "guid");
        feed.episodes.push(Episode {
            guid: if guid.is_empty() { url.clone() } else { guid },
            title: child_text(item, "title"),
            date: parse_date(&child_text(item, "pubDate")).unwrap_or(0),
            duration_s: parse_duration(&child_text(item, "duration")),
            url,
        });
    }
    if feed.title.is_empty() {
        bail!("this feed has no title");
    }
    Ok(feed)
}

fn fetch_feed(url: &str) -> Result<Feed> {
    let mut resp = agent(20).get(url).header("User-Agent", UA).call().map_err(|e| super::net_error(e, "the feed"))?;
    if !resp.status().is_success() {
        bail!("the feed answered {}", resp.status());
    }
    parse_feed(&resp.body_mut().with_config().limit(30 << 20).read_to_string()?)
}

// ---------------- streaming with seeking ----------------

/// An episode file over HTTP that can seek (Range requests), so streamed
/// episodes support seeking and resuming like downloaded ones.
pub(crate) struct HttpFile {
    /// What the server says it sends ("audio/flac", "application/json", …).
    pub(crate) content_type: String,
    url: String,
    len: u64,
    pos: u64,
    body: Option<Mutex<Box<dyn Read + Send>>>,
}

impl HttpFile {
    pub(crate) fn open(url: &str) -> Result<Self> {
        let mut f = Self { content_type: String::new(), url: url.to_string(), len: 0, pos: 0, body: None };
        let resp = f.request(0)?;
        f.content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        f.len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
        f.body = Some(Mutex::new(Box::new(resp.into_body().into_reader())));
        Ok(f)
    }

    fn request(&self, from: u64) -> Result<ureq::http::Response<ureq::Body>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut req = agent.get(&self.url).header("User-Agent", UA);
        if from > 0 {
            req = req.header("Range", format!("bytes={from}-"));
        }
        let resp = req.call().map_err(|e| super::net_error(e, "the episode"))?;
        if !resp.status().is_success() {
            bail!("the episode answered {}", resp.status());
        }
        if from > 0 && resp.status() != 206 {
            bail!("no range");
        }
        Ok(resp)
    }

    fn reopen(&mut self, to: u64) -> std::io::Result<()> {
        match self.request(to) {
            Ok(r) => self.body = Some(Mutex::new(Box::new(r.into_body().into_reader()))),
            // The server can't jump: start over and skip ahead.
            Err(_) => {
                let r = self.request(0).map_err(std::io::Error::other)?;
                let mut b: Box<dyn Read + Send> = Box::new(r.into_body().into_reader());
                std::io::copy(&mut (&mut b).take(to), &mut std::io::sink())?;
                self.body = Some(Mutex::new(b));
            }
        }
        self.pos = to;
        Ok(())
    }
}

impl Read for HttpFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(b) = self.body.as_mut() else { return Ok(0) };
        let n = b.get_mut().read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpFile {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
            SeekFrom::End(d) => self.len.saturating_add_signed(d),
        };
        if target == self.pos {
            return Ok(target);
        }
        // Small jumps forward: just read on instead of a new request.
        if target > self.pos && target - self.pos < 256 * 1024 {
            let skip = target - self.pos;
            std::io::copy(&mut self.take(skip), &mut std::io::sink())?;
            return Ok(self.pos);
        }
        self.reopen(target)?;
        Ok(target)
    }
}

impl symphonia::core::io::MediaSource for HttpFile {
    fn is_seekable(&self) -> bool {
        self.len > 0
    }
    fn byte_len(&self) -> Option<u64> {
        (self.len > 0).then_some(self.len)
    }
}

pub(crate) fn ext_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let e = path.rsplit('.').next().unwrap_or("mp3").to_ascii_lowercase();
    if matches!(e.as_str(), "mp3" | "m4a" | "aac" | "ogg" | "opus" | "mp4") { e } else { "mp3".into() }
}

// ---------------- the source ----------------

pub struct PodcastSource {
    id: SourceId,
    db: Mutex<Connection>,
    dir: PathBuf,
    /// Download the newest episode of each show by itself.
    pub auto_download: AtomicBool,
}

impl PodcastSource {
    pub fn open(db_path: &Path, dir: PathBuf) -> Result<Self> {
        let c = Connection::open(db_path)?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS shows (
                feed TEXT PRIMARY KEY, title TEXT NOT NULL, author TEXT NOT NULL DEFAULT '', image TEXT NOT NULL DEFAULT '',
                subscribed INTEGER NOT NULL DEFAULT 0, fetched INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS episodes (
                id TEXT PRIMARY KEY, feed TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL, date INTEGER NOT NULL,
                duration INTEGER NOT NULL, position INTEGER NOT NULL DEFAULT 0, played INTEGER NOT NULL DEFAULT 0,
                file TEXT
            );
            CREATE INDEX IF NOT EXISTS episodes_feed ON episodes(feed, date);",
        )?;
        // Added later: 1 = downloaded automatically (deleted once played).
        let has_auto: bool = c.prepare("SELECT auto FROM episodes LIMIT 1").is_ok();
        if !has_auto {
            c.execute_batch("ALTER TABLE episodes ADD COLUMN auto INTEGER NOT NULL DEFAULT 0;")?;
        }
        Ok(Self { id: Arc::from(ID), db: Mutex::new(c), dir, auto_download: AtomicBool::new(true) })
    }

    /// Fetch a feed and store show + episodes. Keeps positions and downloads.
    pub fn refresh(&self, feed_url: &str) -> Result<Feed> {
        let f = fetch_feed(feed_url)?;
        let c = self.db.lock();
        c.execute(
            "INSERT INTO shows (feed, title, author, image, fetched) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(feed) DO UPDATE SET title=excluded.title, author=excluded.author, image=excluded.image, fetched=excluded.fetched",
            params![feed_url, f.title, f.author, f.image, now()],
        )?;
        for e in &f.episodes {
            c.execute(
                "INSERT INTO episodes (id, feed, title, url, date, duration) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET title=excluded.title, url=excluded.url, date=excluded.date, duration=excluded.duration",
                params![format!("{feed_url}#{}", e.guid), feed_url, e.title, e.url, e.date, e.duration_s as i64],
            )?;
        }
        Ok(f)
    }

    pub fn subscribed(&self, feed: &str) -> bool {
        self.db.lock().query_row("SELECT subscribed FROM shows WHERE feed=?1", [feed], |r| r.get::<_, i64>(0)).unwrap_or(0) == 1
    }

    /// Subscribe (fetching the feed) or unsubscribe. Returns (show title, now subscribed).
    pub fn toggle_subscription(&self, feed: &str) -> Result<(String, bool)> {
        let on = !self.subscribed(feed);
        if on {
            self.refresh(feed)?;
        }
        let c = self.db.lock();
        c.execute("UPDATE shows SET subscribed=?2 WHERE feed=?1", params![feed, on as i64])?;
        let title = c.query_row("SELECT title FROM shows WHERE feed=?1", [feed], |r| r.get(0)).unwrap_or_default();
        Ok((title, on))
    }

    /// The show an episode belongs to.
    pub fn feed_of(&self, episode: &str) -> Option<String> {
        self.db.lock().query_row("SELECT feed FROM episodes WHERE id=?1", [episode], |r| r.get(0)).ok()
    }

    pub fn position(&self, episode: &str) -> (u64, bool) {
        self.db
            .lock()
            .query_row("SELECT position, played FROM episodes WHERE id=?1", [episode], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? == 1))
            })
            .unwrap_or((0, false))
    }

    /// Remember where you are; near the end counts as played.
    pub fn save_position(&self, episode: &str, ms: u64, duration_ms: u64) {
        let done = duration_ms > 0 && ms + 60_000 >= duration_ms;
        if done {
            // Played to the end: automatic downloads are not needed any more.
            let c = self.db.lock();
            let auto: Option<String> =
                c.query_row("SELECT file FROM episodes WHERE id=?1 AND auto=1 AND file IS NOT NULL", [episode], |r| r.get(0)).ok();
            if let Some(f) = auto {
                let _ = std::fs::remove_file(f);
                let _ = c.execute("UPDATE episodes SET file=NULL, auto=0 WHERE id=?1", [episode]);
            }
        }
        let _ = self.db.lock().execute(
            "UPDATE episodes SET position=?2, played=CASE WHEN ?3 THEN 1 ELSE played END WHERE id=?1",
            params![episode, if done { 0 } else { ms as i64 }, done],
        );
    }

    fn path_for(&self, id: &str, url: &str) -> PathBuf {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut h);
        self.dir.join(format!("{:016x}.{}", h.finish(), ext_of(url)))
    }

    /// Download an episode, or delete its download. Returns (title, downloaded now).
    pub fn toggle_download(&self, id: &str) -> Result<(String, bool)> {
        let (title, url, file): (String, String, Option<String>) = self
            .db
            .lock()
            .query_row("SELECT title, url, file FROM episodes WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?
            .context("unknown episode")?;
        if let Some(f) = file {
            let _ = std::fs::remove_file(&f);
            self.db.lock().execute("UPDATE episodes SET file=NULL, auto=0 WHERE id=?1", [id])?;
            return Ok((title, false));
        }
        self.download(id, &url, false)?;
        Ok((title, true))
    }

    fn download(&self, id: &str, url: &str, auto: bool) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path_for(id, url);
        let tmp = path.with_extension("part");
        let resp = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .new_agent()
            .get(url)
            .header("User-Agent", UA)
            .call()
            .map_err(|e| super::net_error(e, "the episode"))?;
        if !resp.status().is_success() {
            bail!("the episode answered {}", resp.status());
        }
        let mut out = std::fs::File::create(&tmp)?;
        let res = std::io::copy(&mut resp.into_body().into_reader(), &mut out);
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        std::fs::rename(&tmp, &path)?;
        self.db.lock().execute("UPDATE episodes SET file=?2, auto=?3 WHERE id=?1", params![id, path.to_string_lossy(), auto as i64])?;
        Ok(())
    }

    /// Refresh all subscribed shows; download each show's newest unplayed episode
    /// when auto download is on.
    pub fn refresh_all(&self) {
        let feeds: Vec<String> = {
            let c = self.db.lock();
            let Ok(mut st) = c.prepare("SELECT feed FROM shows WHERE subscribed=1") else { return };
            st.query_map([], |r| r.get(0)).map(|r| r.flatten().collect()).unwrap_or_default()
        };
        for f in feeds {
            if self.refresh(&f).is_err() {
                continue;
            }
            if !self.auto_download.load(Relaxed) {
                continue;
            }
            let newest: Option<(String, String, Option<String>, i64)> = self
                .db
                .lock()
                .query_row("SELECT id, url, file, played FROM episodes WHERE feed=?1 ORDER BY date DESC LIMIT 1", [&f], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .ok();
            if let Some((id, url, None, 0)) = newest {
                let _ = self.download(&id, &url, true);
            }
        }
    }

    /// Keep feeds fresh in the background (now, then every 2 hours).
    pub fn start_refresher(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("podcasts".into())
            .spawn(move || {
                loop {
                    me.refresh_all();
                    std::thread::sleep(Duration::from_secs(2 * 3600));
                }
            })
            .ok();
    }

    fn episodes(&self, where_: &str, p: impl rusqlite::Params) -> Vec<Item> {
        let c = self.db.lock();
        let sql = format!(
            "SELECT e.id, e.title, e.date, e.duration, e.position, e.played, e.file, s.title, s.image, e.auto
             FROM episodes e JOIN shows s ON s.feed = e.feed {where_}"
        );
        let Ok(mut st) = c.prepare(&sql) else { return vec![] };
        st.query_map(p, |r| {
            Ok(episode_track(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)? as u64,
                r.get::<_, i64>(4)? as u64,
                r.get::<_, i64>(5)? == 1,
                r.get::<_, Option<String>>(6)?
                    .and_then(|f| std::fs::metadata(f).ok())
                    .map(|m| (m.len(), r.get::<_, i64>(9).unwrap_or(0) == 1)),
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .map(|rows| rows.flatten().map(|t| Item::Track(Arc::new(t))).collect())
        .unwrap_or_default()
    }

    /// Size of all downloaded episodes together.
    pub fn downloads_size(&self) -> u64 {
        let c = self.db.lock();
        let Ok(mut st) = c.prepare("SELECT file FROM episodes WHERE file IS NOT NULL") else { return 0 };
        st.query_map([], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum())
            .unwrap_or(0)
    }

    fn shows(&self) -> Vec<Item> {
        let c = self.db.lock();
        let Ok(mut st) = c.prepare(
            "SELECT s.feed, s.title, s.author, s.image, (SELECT COUNT(*) FROM episodes e WHERE e.feed=s.feed AND e.played=0)
             FROM shows s WHERE subscribed=1 ORDER BY lower(s.title)",
        ) else {
            return vec![];
        };
        st.query_map([], |r| {
            let image: String = r.get(3)?;
            let new: i64 = r.get(4)?;
            Ok(Item::Node(Node {
                id: format!("{FEED}{}", r.get::<_, String>(0)?),
                kind: NodeKind::Folder,
                title: r.get(1)?,
                subtitle: r.get(2)?,
                count: Some(new as u64),
                art: (!image.is_empty()).then(|| ArtRef { source: Arc::from(ID), id: image }),
            }))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Apple Podcasts directory search (free, no account).
    fn search(q: &str) -> Result<Vec<Item>> {
        let mut resp = agent(15)
            .get("https://itunes.apple.com/search")
            .header("User-Agent", UA)
            .query("media", "podcast")
            .query("limit", "40")
            .query("term", q)
            .call()
            .map_err(|e| super::net_error(e, "the podcast directory"))?;
        let v: Value = serde_json::from_str(&resp.body_mut().read_to_string()?)?;
        Ok(v["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let feed = r["feedUrl"].as_str()?;
                Some(Item::Node(Node {
                    id: format!("{FEED}{feed}"),
                    kind: NodeKind::Folder,
                    title: r["collectionName"].as_str().unwrap_or("").into(),
                    subtitle: r["artistName"].as_str().unwrap_or("").into(),
                    count: None,
                    art: r["artworkUrl600"].as_str().map(|a| ArtRef { source: Arc::from(ID), id: a.into() }),
                }))
            })
            .collect())
    }
}

/// An episode as a track: show as artist, "12 Jun 2024 · 45 min · 12 min left · ↓" as album.
#[allow(clippy::too_many_arguments)]
fn episode_track(
    id: String,
    title: String,
    date: i64,
    dur: u64,
    pos: u64,
    played: bool,
    file: Option<(u64, bool)>,
    show: String,
    image: String,
) -> Track {
    let mut info = Vec::new();
    if date > 0 {
        info.push(fmt_date(date));
    }
    if dur > 0 {
        info.push(format!("{} min", dur.div_ceil(60)));
    }
    if played {
        info.push("✓ played".into());
    } else if pos > 0 {
        info.push(format!("{} min left", (dur.saturating_sub(pos / 1000)).div_ceil(60)));
    }
    if let Some((bytes, auto)) = file {
        info.push(format!("↓ {}{}", if auto { "auto " } else { "" }, fmt_size(bytes)));
    }
    Track {
        r: Some(TrackRef { source: Arc::from(ID), id }),
        title,
        artist: show,
        album: info.join(" · "),
        duration_ms: (dur > 0).then_some(dur * 1000),
        art: (!image.is_empty()).then(|| ArtRef { source: Arc::from(ID), id: image }),
        ..Default::default()
    }
}

pub fn fmt_size(b: u64) -> String {
    if b >= 1 << 30 { format!("{:.1} GB", b as f64 / (1u64 << 30) as f64) } else { format!("{} MB", (b + (1 << 19)) >> 20) }
}

fn fmt_date(t: i64) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d} {} {y}", M[(m - 1) as usize])
}

impl Source for PodcastSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEEK | Caps::ART
    }

    fn children(&self, node: &str, _page: Page) -> Result<Vec<Item>> {
        if node == NEW {
            return Ok(self.episodes("WHERE s.subscribed=1 AND e.played=0 ORDER BY e.date DESC LIMIT 100", []));
        }
        if node == SHOWS {
            return Ok(self.shows());
        }
        if node == DOWNLOADS {
            return Ok(self.episodes("WHERE e.file IS NOT NULL ORDER BY e.date DESC", []));
        }
        if let Some(q) = node.strip_prefix(SEARCH) {
            return Self::search(q);
        }
        if let Some(feed) = node.strip_prefix(FEED) {
            // Fresh enough when subscribed (the refresher keeps it current).
            let fetched: i64 = self.db.lock().query_row("SELECT fetched FROM shows WHERE feed=?1", [feed], |r| r.get(0)).unwrap_or(0);
            if !self.subscribed(feed) || now() - fetched > 3600 {
                self.refresh(feed)?;
            }
            return Ok(self.episodes("WHERE e.feed=?1 ORDER BY e.date DESC", [feed]));
        }
        bail!("unknown podcast list {node}")
    }

    fn search(&self, _q: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        self.episodes("WHERE e.id=?1", [&r.id])
            .into_iter()
            .find_map(|i| if let Item::Track(t) = i { Some((*t).clone()) } else { None })
            .context("unknown episode")
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        let (url, file): (String, Option<String>) = self
            .db
            .lock()
            .query_row("SELECT url, file FROM episodes WHERE id=?1", [&r.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .context("unknown episode")?;
        if let Some(f) = file.filter(|f| Path::new(f).exists()) {
            return Ok(Box::new(FileStream::open(Path::new(&f))?));
        }
        let src = HttpFile::open(&url)?;
        Ok(Box::new(FileStream::open_stream(Box::new(src), &ext_of(&url))?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        let mut resp = agent(10).get(&a.id).header("User-Agent", UA).call()?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(Some(resp.body_mut().with_config().limit(8 << 20).read_to_vec()?))
    }

    fn lyrics(&self, _r: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_feeds() {
        let xml = r#"<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>My Show</title><itunes:author>Ann</itunes:author><itunes:image href="http://x/i.jpg"/>
<item><title>Ep 2</title><guid>g2</guid><pubDate>Tue, 10 Jun 2003 04:00:00 GMT</pubDate>
<itunes:duration>1:02:03</itunes:duration><enclosure url="http://x/2.mp3" type="audio/mpeg"/></item>
<item><title>Video</title><enclosure url="http://x/v.mp4" type="video/mp4"/></item>
<item><title>No file</title></item>
</channel></rss>"#;
        let f = parse_feed(xml).unwrap();
        assert_eq!((f.title.as_str(), f.author.as_str(), f.image.as_str()), ("My Show", "Ann", "http://x/i.jpg"));
        assert_eq!(
            f.episodes,
            vec![Episode { guid: "g2".into(), title: "Ep 2".into(), url: "http://x/2.mp3".into(), date: 1_055_217_600, duration_s: 3723 }]
        );
    }

    #[test]
    fn played_auto_downloads_are_deleted_but_own_downloads_stay() {
        let dir = tempfile::tempdir().unwrap();
        let src = PodcastSource::open(&dir.path().join("p.db"), dir.path().join("dl")).unwrap();
        let (auto_f, own_f) = (dir.path().join("a.mp3"), dir.path().join("b.mp3"));
        std::fs::write(&auto_f, b"x").unwrap();
        std::fs::write(&own_f, b"x").unwrap();
        {
            let c = src.db.lock();
            for (id, f, auto) in [("a", &auto_f, 1), ("b", &own_f, 0)] {
                c.execute(
                    "INSERT INTO episodes (id, feed, title, url, date, duration, file, auto) VALUES (?1, 'f', 't', 'u', 0, 600, ?2, ?3)",
                    params![id, f.to_string_lossy(), auto],
                )
                .unwrap();
            }
        }
        // Halfway: nothing is deleted.
        src.save_position("a", 300_000, 600_000);
        assert!(auto_f.exists());
        // Played to the end.
        src.save_position("a", 590_000, 600_000);
        src.save_position("b", 590_000, 600_000);
        assert!(!auto_f.exists());
        assert!(own_f.exists());
        assert_eq!(src.position("a"), (0, true));
    }

    #[test]
    fn reads_dates_and_lengths() {
        assert_eq!(parse_date("Tue, 10 Jun 2003 04:00:00 GMT"), Some(1_055_217_600));
        assert_eq!(parse_date("Tue, 10 Jun 2003 06:00:00 +0200"), Some(1_055_217_600));
        assert_eq!(parse_date("nonsense"), None);
        assert_eq!(parse_duration("62:03"), 3723);
        assert_eq!(parse_duration("3723"), 3723);
        assert_eq!(fmt_date(1_055_217_600), "10 Jun 2003");
    }
}
