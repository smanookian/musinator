//! Internet radio from radio-browser.info (free, no account), only when the user
//! opens the Radio lists. Favorites and station details live in `radio.db`.
//!
//! Playback: a network thread reads the stream, takes out the song titles
//! (ICY metadata) and passes the audio bytes to the normal decoder through a
//! channel, so a slow network never blocks anything else.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossbeam_channel as cb;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde_json::Value;

use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream, StreamInfo};
use crate::decode::FileStream;
use crate::model::{ArtRef, AudioFormat, SourceId, Track, TrackRef};

pub const ID: &str = "radio";
/// radio-browser.info mirrors; the one that worked last is tried first.
const MIRRORS: [&str; 4] = ["all", "de1", "de2", "fi1"];
static MIRROR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const UA: &str = concat!("Musinator/", env!("CARGO_PKG_VERSION"), " (https://github.com/smanookian/musinator)");

/// Nodes shown in the sidebar.
pub const FAVORITES: &str = "radio:favorites";
pub const POPULAR: &str = "radio:popular";
pub const GENRES: &str = "radio:genres";
pub const COUNTRIES: &str = "radio:countries";
/// Followed by a genre name / a country code.
const GENRE: &str = "radio:genre:";
const COUNTRY: &str = "radio:country:";
/// Station ids of streams the user added by address.
const CUSTOM: &str = "custom:";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Station {
    pub uuid: String,
    pub name: String,
    pub url: String,
    pub codec: String,
    pub bitrate: u32,
    pub country: String,
    pub tags: String,
    pub favicon: String,
}

impl Station {
    fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
        let st = Station {
            uuid: s("stationuuid"),
            name: s("name"),
            url: if s("url_resolved").is_empty() { s("url") } else { s("url_resolved") },
            codec: s("codec"),
            bitrate: v["bitrate"].as_u64().unwrap_or(0) as u32,
            country: s("country"),
            tags: s("tags"),
            favicon: s("favicon"),
        };
        // HLS playlists and unknown codecs can't be played. Own streams are checked when added.
        let hls = v["hls"].as_u64().unwrap_or(0) == 1;
        let custom = st.uuid.starts_with(CUSTOM);
        let ok = custom || matches!(st.codec.to_ascii_uppercase().as_str(), "MP3" | "AAC" | "AAC+" | "OGG" | "OPUS" | "FLAC");
        (!st.uuid.is_empty() && !st.url.is_empty() && !hls && ok).then_some(st)
    }

    /// Format hint for the decoder.
    fn ext(&self) -> &'static str {
        match self.codec.to_ascii_uppercase().as_str() {
            "AAC" | "AAC+" => "aac",
            "OGG" | "OPUS" => "ogg",
            "FLAC" => "flac",
            _ => "mp3",
        }
    }

    fn subtitle(&self) -> String {
        let tags: Vec<&str> = self.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).take(3).collect();
        let mut parts = Vec::new();
        if !self.country.is_empty() {
            parts.push(self.country.clone());
        }
        if !tags.is_empty() {
            parts.push(tags.join(", "));
        }
        parts.join(" · ")
    }

    pub fn track(&self) -> Track {
        let codec = match self.codec.to_ascii_uppercase().as_str() {
            "OGG" => "Vorbis".to_string(),
            "OPUS" => "Opus".to_string(),
            c => c.to_string(),
        };
        Track {
            r: Some(TrackRef { source: Arc::from(ID), id: self.uuid.clone() }),
            title: self.name.clone(),
            artist: self.subtitle(),
            album: match (codec.as_str(), self.bitrate) {
                ("", _) => "Radio".into(),
                (c, 0) => format!("Radio · {c}"),
                (c, b) => format!("Radio · {c} {b}k"),
            },
            format: Some(AudioFormat { codec, bitrate_kbps: (self.bitrate > 0).then_some(self.bitrate), ..Default::default() }),
            art: (!self.favicon.is_empty()).then(|| ArtRef { source: Arc::from(ID), id: self.favicon.clone() }),
            ..Default::default()
        }
    }
}

pub struct RadioSource {
    id: SourceId,
    db: Mutex<Connection>,
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(secs))).http_status_as_error(false).build().into()
}

fn api(path: &str, query: &[(&str, String)]) -> Result<Value> {
    use std::sync::atomic::Ordering::Relaxed;
    let first = MIRROR.load(Relaxed);
    let mut last = anyhow::anyhow!("radio-browser.info can't be reached");
    for k in 0..MIRRORS.len() {
        let m = (first + k) % MIRRORS.len();
        let mut req = agent(10).get(&format!("https://{}.api.radio-browser.info/json{path}", MIRRORS[m])).header("User-Agent", UA);
        for (k, v) in query {
            req = req.query(*k, v);
        }
        let body = req.call().map_err(anyhow::Error::from).and_then(|mut r| {
            if !r.status().is_success() {
                bail!("radio-browser.info answered {}", r.status());
            }
            Ok(r.body_mut().read_to_string()?)
        });
        match body.and_then(|b| Ok(serde_json::from_str::<Value>(&b)?)) {
            Ok(v) => {
                MIRROR.store(m, Relaxed);
                return Ok(v);
            }
            Err(e) => last = e.context("radio-browser.info can't be reached"),
        }
    }
    Err(last)
}

impl RadioSource {
    pub fn open(db_path: &Path) -> Result<Self> {
        let c = Connection::open(db_path)?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS stations (
                uuid TEXT PRIMARY KEY, json TEXT NOT NULL, fav INTEGER NOT NULL DEFAULT 0, fav_at INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(Self { id: Arc::from(ID), db: Mutex::new(c) })
    }

    fn remember(&self, list: &[Station]) {
        let c = self.db.lock();
        for s in list {
            let json = serde_json::json!({
                "stationuuid": s.uuid, "name": s.name, "url_resolved": s.url, "codec": s.codec,
                "bitrate": s.bitrate, "country": s.country, "tags": s.tags, "favicon": s.favicon,
            });
            let _ = c.execute(
                "INSERT INTO stations (uuid, json) VALUES (?1, ?2) ON CONFLICT(uuid) DO UPDATE SET json=excluded.json",
                (&s.uuid, json.to_string()),
            );
        }
    }

    fn station(&self, uuid: &str) -> Option<Station> {
        let json: String = self.db.lock().query_row("SELECT json FROM stations WHERE uuid=?1", [uuid], |r| r.get(0)).ok()?;
        Station::from_json(&serde_json::from_str(&json).ok()?)
    }

    pub fn is_fav(&self, uuid: &str) -> bool {
        self.db.lock().query_row("SELECT fav FROM stations WHERE uuid=?1", [uuid], |r| r.get::<_, i64>(0)).unwrap_or(0) == 1
    }

    /// Returns true when it is a favorite now.
    pub fn toggle_fav(&self, uuid: &str) -> Result<bool> {
        let on = !self.is_fav(uuid);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        self.db.lock().execute("UPDATE stations SET fav=?2, fav_at=?3 WHERE uuid=?1", (uuid, on as i64, now))?;
        Ok(on)
    }

    fn favorites(&self) -> Vec<Station> {
        let c = self.db.lock();
        let Ok(mut st) = c.prepare("SELECT json FROM stations WHERE fav=1 ORDER BY fav_at") else { return vec![] };
        st.query_map([], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().filter_map(|j| Station::from_json(&serde_json::from_str(&j).ok()?)).collect())
            .unwrap_or_default()
    }

    fn list(v: Value) -> Vec<Station> {
        let mut seen = std::collections::HashSet::new();
        v.as_array().into_iter().flatten().filter_map(Station::from_json).filter(|s| seen.insert(s.uuid.clone())).collect()
    }

    /// Genres or countries with the most stations, as lists to open.
    fn groups(node: &str) -> Result<Vec<Item>> {
        let genres = node == GENRES;
        let v = if genres {
            api(
                "/tags",
                &[("order", "stationcount".into()), ("reverse", "true".into()), ("hidebroken", "true".into()), ("limit", "200".into())],
            )?
        } else {
            api("/countries", &[("order", "stationcount".into()), ("reverse", "true".into()), ("hidebroken", "true".into())])?
        };
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| {
                let name = g["name"].as_str()?.trim();
                let count = g["stationcount"].as_u64().unwrap_or(0);
                let (id, kind) = if genres {
                    (format!("{GENRE}{name}"), NodeKind::Genre)
                } else {
                    (format!("{COUNTRY}{}", g["iso_3166_1"].as_str()?), NodeKind::Folder)
                };
                (!name.is_empty() && count >= 5)
                    .then(|| Item::Node(Node { id, kind, title: name.to_string(), subtitle: String::new(), count: Some(count), art: None }))
            })
            .collect())
    }

    fn stations_in(path: &str, key: &str) -> Result<Vec<Station>> {
        let q = [("order", "clickcount".to_string()), ("reverse", "true".into()), ("hidebroken", "true".into()), ("limit", "300".into())];
        Ok(Self::list(api(&format!("{path}/{}", urlencode(key)), &q)?))
    }

    /// Check a stream address the user typed and keep it as a favorite.
    /// Returns the station name (from the stream, else the host).
    pub fn add_custom(&self, url: &str) -> Result<String> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            bail!("The address must start with http:// or https://");
        }
        let real = resolve_playlist(url)?;
        let resp =
            agent(10).get(&real).header("User-Agent", UA).header("Icy-MetaData", "1").call().context("the address can't be reached")?;
        if !resp.status().is_success() {
            bail!("the address answered {}", resp.status());
        }
        let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let ctype = header("content-type").unwrap_or_default().to_ascii_lowercase();
        let codec = if ctype.contains("aac") {
            "AAC"
        } else if ctype.contains("ogg") || ctype.contains("opus") {
            "OGG"
        } else if ctype.contains("flac") {
            "FLAC"
        } else if ctype.contains("mpeg") || ctype.contains("mp3") {
            "MP3"
        } else {
            bail!("This address is not an audio stream ({})", if ctype.is_empty() { "unknown type" } else { &ctype });
        };
        let host = real.split("://").nth(1).and_then(|r| r.split(['/', ':']).next()).unwrap_or(url).to_string();
        let st = Station {
            uuid: format!("{CUSTOM}{url}"),
            name: header("icy-name").unwrap_or(host),
            url: url.to_string(),
            codec: codec.into(),
            bitrate: header("icy-br").and_then(|b| b.split(',').next()?.trim().parse().ok()).unwrap_or(0),
            country: String::new(),
            tags: header("icy-genre").unwrap_or_default(),
            favicon: String::new(),
        };
        self.remember(std::slice::from_ref(&st));
        if !self.is_fav(&st.uuid) {
            self.toggle_fav(&st.uuid)?;
        }
        Ok(st.name)
    }

    fn popular() -> Result<Vec<Station>> {
        Ok(Self::list(api("/stations/topclick/150", &[("hidebroken", "true".into())])?))
    }

    /// Stations whose name or tags match, most popular first.
    /// Stations for the main search: the most popular matches, remembered for playing.
    pub fn find(&self, q: &str, limit: usize) -> Result<Vec<Arc<Track>>> {
        let mut list = Self::search_stations(q)?;
        list.truncate(limit);
        self.remember(&list);
        Ok(list.iter().map(|s| Arc::new(s.track())).collect())
    }

    fn search_stations(q: &str) -> Result<Vec<Station>> {
        let base = |k: &'static str| {
            vec![
                (k, q.to_string()),
                ("hidebroken", "true".to_string()),
                ("order", "clickcount".to_string()),
                ("reverse", "true".to_string()),
                ("limit", "100".to_string()),
            ]
        };
        let mut v = Self::list(api("/stations/search", &base("name"))?);
        for s in Self::list(api("/stations/search", &base("tag"))?) {
            if !v.iter().any(|x| x.uuid == s.uuid) {
                v.push(s);
            }
        }
        Ok(v)
    }
}

impl Source for RadioSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::LIVE | Caps::ART
    }

    fn children(&self, node: &str, _page: Page) -> Result<Vec<Item>> {
        let list = if node == FAVORITES {
            self.favorites()
        } else if node == POPULAR {
            Self::popular()?
        } else if node == GENRES || node == COUNTRIES {
            return Self::groups(node);
        } else if let Some(g) = node.strip_prefix(GENRE) {
            Self::stations_in("/stations/bytagexact", g)?
        } else if let Some(c) = node.strip_prefix(COUNTRY) {
            Self::stations_in("/stations/bycountrycodeexact", c)?
        } else {
            bail!("unknown radio list {node}");
        };
        self.remember(&list);
        Ok(list.iter().map(|s| Item::Track(Arc::new(s.track()))).collect())
    }

    fn search(&self, _query: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        Ok(self.station(&r.id).map(|s| s.track()).unwrap_or_else(|| Track { r: Some(r.clone()), ..Default::default() }))
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        let st = self.station(&r.id).context("unknown station")?;
        let url = if st.uuid.starts_with(CUSTOM) {
            resolve_playlist(&st.url)?
        } else {
            // Tell radio-browser.info about the play (their rule) and get the current address.
            api(&format!("/url/{}", st.uuid), &[]).ok().and_then(|v| v["url"].as_str().map(str::to_string)).unwrap_or(st.url.clone())
        };
        Ok(Box::new(RadioStream::open(&url, &st)?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        let mut resp = agent(8).get(&a.id).header("User-Agent", UA).call()?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(Some(resp.body_mut().with_config().limit(4 << 20).read_to_vec()?))
    }

    fn lyrics(&self, _r: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The first stream address in a `.pls` / `.m3u` playlist text.
pub fn playlist_entry(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .map(|l| if l.to_ascii_lowercase().starts_with("file") { l.split_once('=').map_or(l, |(_, v)| v.trim()) } else { l })
        .find(|l| l.starts_with("http://") || l.starts_with("https://"))
        .map(str::to_string)
}

/// Station links are often small playlists (.pls / .m3u) pointing at the stream.
fn resolve_playlist(url: &str) -> Result<String> {
    let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".m3u8") {
        bail!("HLS streams (.m3u8) are not supported");
    }
    if !(path.ends_with(".pls") || path.ends_with(".m3u")) {
        return Ok(url.to_string());
    }
    let mut resp = agent(10).get(url).header("User-Agent", UA).call().context("the address can't be reached")?;
    let text = resp.body_mut().with_config().limit(256 << 10).read_to_string()?;
    playlist_entry(&text).context("no stream address in this playlist")
}

// ---------------- playback ----------------

/// Audio bytes from the network thread.
struct ChanReader {
    rx: cb::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChanReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv_timeout(Duration::from_secs(20)) {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                // Station gone or silent for too long: end of stream.
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Pull "StreamTitle='…';" out of an ICY metadata block.
pub fn stream_title(meta: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(meta);
    let start = s.find("StreamTitle='")? + "StreamTitle='".len();
    let rest = &s[start..];
    let end = rest.find("';").unwrap_or(rest.trim_end_matches('\0').len());
    let t = rest[..end].trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Reads the HTTP body, drops ICY metadata blocks every `metaint` bytes,
/// sends audio bytes on. Ends when the receiver is dropped.
fn pump(mut body: impl Read, metaint: usize, tx: cb::Sender<Vec<u8>>, title: Arc<Mutex<Option<String>>>) {
    let mut left = if metaint > 0 { metaint } else { usize::MAX };
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let want = buf.len().min(left);
        let n = match body.read(&mut buf[..want]) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if tx.send(buf[..n].to_vec()).is_err() {
            return;
        }
        if metaint == 0 {
            continue;
        }
        left -= n;
        if left == 0 {
            let mut len = [0u8; 1];
            if body.read_exact(&mut len).is_err() {
                return;
            }
            let mut meta = vec![0u8; len[0] as usize * 16];
            if body.read_exact(&mut meta).is_err() {
                return;
            }
            if let Some(t) = stream_title(&meta) {
                *title.lock() = Some(t);
            }
            left = metaint;
        }
    }
}

pub struct RadioStream {
    inner: FileStream,
    info: StreamInfo,
    station: Track,
    title: Arc<Mutex<Option<String>>>,
    shown: Option<String>,
}

impl RadioStream {
    fn open(url: &str, st: &Station) -> Result<Self> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();
        let resp = agent.get(url).header("User-Agent", UA).header("Icy-MetaData", "1").call().context("the station can't be reached")?;
        if !resp.status().is_success() {
            bail!("the station answered {}", resp.status());
        }
        let metaint = resp.headers().get("icy-metaint").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let ext = if ctype.contains("aac") {
            "aac"
        } else if ctype.contains("ogg") || ctype.contains("opus") {
            "ogg"
        } else if ctype.contains("flac") {
            "flac"
        } else if ctype.contains("mpeg") {
            "mp3"
        } else {
            st.ext()
        };
        // About 2 MB of read-ahead between network and decoder.
        let (tx, rx) = cb::bounded(128);
        let title = Arc::new(Mutex::new(None));
        let t2 = title.clone();
        let body = resp.into_body().into_reader();
        std::thread::Builder::new().name("radio".into()).spawn(move || pump(body, metaint, tx, t2))?;
        let src = symphonia::core::io::ReadOnlySource::new(ChanReader { rx, buf: Vec::new(), pos: 0 });
        let inner = FileStream::open_stream(Box::new(src), ext)?;
        let mut info = inner.info().clone();
        info.frames = None;
        if info.format.bitrate_kbps.is_none() && st.bitrate > 0 {
            info.format.bitrate_kbps = Some(st.bitrate);
        }
        Ok(Self { inner, info, station: st.track(), title, shown: None })
    }
}

impl Stream for RadioStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        self.inner.read(out)
    }

    fn seek(&mut self, _to: Duration) -> Result<u64> {
        Ok(0)
    }

    /// The song now on air: "Artist - Title" becomes artist and title;
    /// the station name moves to the album line.
    fn meta_update(&mut self) -> Option<Track> {
        let now = self.title.lock().clone();
        if now == self.shown {
            return None;
        }
        self.shown = now.clone();
        let mut t = self.station.clone();
        t.album = self.station.title.clone();
        match now.as_deref().and_then(|s| s.split_once(" - ")) {
            Some((a, b)) => {
                t.artist = a.trim().to_string();
                t.title = b.trim().to_string();
                t.on_air = !t.artist.is_empty() && !t.title.is_empty();
            }
            None => match now {
                Some(s) => {
                    t.title = s;
                    t.artist = self.station.artist.clone();
                }
                None => return Some(self.station.clone()),
            },
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stream_titles() {
        assert_eq!(stream_title(b"StreamTitle='AC/DC - Go Down';StreamUrl='';\0\0").as_deref(), Some("AC/DC - Go Down"));
        assert_eq!(stream_title(b"StreamTitle='';\0\0"), None);
        assert_eq!(stream_title(b"\0\0\0"), None);
    }

    #[test]
    fn pump_strips_metadata_blocks() {
        // 4 audio bytes, a 16-byte metadata block, 4 more audio bytes.
        let mut data = b"abcd".to_vec();
        let mut meta = b"StreamTitle='X';".to_vec();
        meta.resize(16, 0);
        data.push(1);
        data.extend(meta);
        data.extend(b"efgh");
        data.push(0);
        let (tx, rx) = cb::unbounded();
        let title = Arc::new(Mutex::new(None));
        pump(&data[..], 4, tx, title.clone());
        let audio: Vec<u8> = rx.try_iter().flatten().collect();
        assert_eq!(audio, b"abcdefgh");
        assert_eq!(title.lock().as_deref(), Some("X"));
    }

    #[test]
    fn finds_the_stream_in_playlists() {
        assert_eq!(
            playlist_entry("[playlist]\nNumberOfEntries=1\nFile1=http://a.example/live\nTitle1=X").as_deref(),
            Some("http://a.example/live")
        );
        assert_eq!(playlist_entry("#EXTM3U\n#EXTINF:-1,X\nhttps://b.example/s.mp3\n").as_deref(), Some("https://b.example/s.mp3"));
        assert_eq!(playlist_entry("nothing"), None);
    }

    #[test]
    fn skips_unplayable_stations() {
        let ok = serde_json::json!({"stationuuid": "u", "name": "N", "url_resolved": "http://x", "codec": "MP3", "hls": 0});
        assert!(Station::from_json(&ok).is_some());
        let hls = serde_json::json!({"stationuuid": "u", "name": "N", "url_resolved": "http://x", "codec": "MP3", "hls": 1});
        assert!(Station::from_json(&hls).is_none());
        let odd = serde_json::json!({"stationuuid": "u", "name": "N", "url_resolved": "http://x", "codec": "WMA", "hls": 0});
        assert!(Station::from_json(&odd).is_none());
    }
}
