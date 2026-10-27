//! Music servers that speak the Subsonic API (Navidrome, Gonic, Airsonic, Ampache, …).
//! The login is kept as a salted token in `~/.config/musinator/server.json`;
//! the password itself is never stored.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::podcast::HttpFile;
use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::model::{ArtRef, AudioFormat, SourceId, Track, TrackRef};

const CLIENT: &str = "Musinator";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Login {
    /// "https://music.example.com" (no trailing slash).
    pub url: String,
    pub user: String,
    pub salt: String,
    /// md5(password + salt).
    pub token: String,
}

impl Login {
    pub fn new(url: &str, user: &str, password: &str) -> Self {
        let mut url = url.trim().trim_end_matches('/').to_string();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            url = format!("http://{url}");
        }
        let salt = format!("{:x}", md5::compute(format!("{:?}{}", std::time::SystemTime::now(), std::process::id())))[..12].to_string();
        let token = format!("{:x}", md5::compute(format!("{password}{salt}")));
        Self { url, user: user.trim().to_string(), salt, token }
    }

    /// "user@host" for the settings.
    pub fn label(&self) -> String {
        let host = self.url.split("://").nth(1).unwrap_or(&self.url);
        format!("{}@{host}", self.user)
    }

    fn query(&self) -> String {
        format!("u={}&t={}&s={}&v=1.16.1&c={CLIENT}&f=json", enc(&self.user), self.token, self.salt)
    }

    fn endpoint(&self, method: &str, extra: &[(&str, &str)]) -> String {
        let mut u = format!("{}/rest/{method}?{}", self.url, self.query());
        for (k, v) in extra {
            u.push_str(&format!("&{k}={}", enc(v)));
        }
        u
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn get(l: &Login, method: &str, extra: &[(&str, &str)]) -> Result<Value> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into();
    let mut resp = agent.get(&l.endpoint(method, extra)).call().map_err(|e| super::net_error(e, "the server"))?;
    if !resp.status().is_success() {
        return Err(super::http_error("the server", resp.status()));
    }
    let v: Value = serde_json::from_str(&resp.body_mut().read_to_string()?).context("this is not a Subsonic / Navidrome server")?;
    let r = v["subsonic-response"].clone();
    if r["status"] != "ok" {
        let msg = r["error"]["message"].as_str().unwrap_or("unknown error");
        if r["error"]["code"].as_u64() == Some(40) {
            bail!("wrong user name or password");
        }
        bail!("the server says: {msg}");
    }
    Ok(r)
}

/// Check address and login; returns the server's name and version.
pub fn ping(l: &Login) -> Result<String> {
    let r = get(l, "ping", &[])?;
    let kind = r["type"].as_str().unwrap_or("Subsonic");
    Ok(format!("{kind} {}", r["serverVersion"].as_str().or(r["version"].as_str()).unwrap_or("")).trim().to_string())
}

fn arr(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![v.clone()],
        _ => vec![],
    }
}

/// One Subsonic server. Its id ("srv1") prefixes its list nodes ("srv1:artists").
pub struct SubsonicSource {
    id: SourceId,
    login: Login,
}

impl SubsonicSource {
    pub fn new(id: &str, login: Login) -> Self {
        Self { id: Arc::from(id), login }
    }

    fn login(&self) -> Result<Login> {
        Ok(self.login.clone())
    }

    fn node(&self, what: &str) -> String {
        format!("{}:{what}", self.id)
    }

    fn song(&self, s: &Value) -> Track {
        let str_ = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let num = |k: &str| s[k].as_u64();
        let suffix = str_("suffix").to_ascii_lowercase();
        let codec = match suffix.as_str() {
            "flac" => "FLAC",
            "mp3" => "MP3",
            "m4a" | "aac" | "mp4" => "AAC",
            "ogg" | "oga" => "Vorbis",
            "opus" => "Opus",
            "wav" => "WAV",
            _ => "",
        };
        let rg = &s["replayGain"];
        Track {
            r: Some(TrackRef { source: self.id.clone(), id: str_("id") }),
            title: str_("title"),
            artist: str_("artist"),
            album: str_("album"),
            album_artist: s["displayAlbumArtist"].as_str().unwrap_or("").to_string(),
            year: num("year").map(|y| y as i32),
            track_no: num("track").map(|n| n as u32),
            disc_no: num("discNumber").map(|n| n as u32),
            duration_ms: num("duration").map(|d| d * 1000),
            format: (!codec.is_empty()).then(|| AudioFormat {
                codec: codec.into(),
                sample_rate: num("samplingRate").unwrap_or(0) as u32,
                bits: num("bitDepth").filter(|b| *b > 0).map(|b| b as u32),
                channels: num("channelCount").unwrap_or(2) as u16,
                bitrate_kbps: num("bitRate").map(|b| b as u32),
            }),
            gain: crate::model::Gain {
                track_db: rg["trackGain"].as_f64().map(|g| g as f32),
                track_peak: rg["trackPeak"].as_f64().map(|g| g as f32),
                album_db: rg["albumGain"].as_f64().map(|g| g as f32),
                album_peak: rg["albumPeak"].as_f64().map(|g| g as f32),
                origin: if rg["trackGain"].is_number() || rg["albumGain"].is_number() {
                    crate::model::GainOrigin::Tags
                } else {
                    Default::default()
                },
            },
            art: s["coverArt"].as_str().map(|c| ArtRef { source: self.id.clone(), id: c.into() }),
            ..Default::default()
        }
    }

    fn album_node(&self, a: &Value) -> Item {
        let artist = a["artist"].as_str().unwrap_or("");
        let subtitle = match a["year"].as_u64() {
            Some(y) if y > 0 => format!("{artist} · {y}"),
            _ => artist.to_string(),
        };
        Item::Node(Node {
            id: self.node(&format!("album:{}", a["id"].as_str().unwrap_or(""))),
            kind: NodeKind::Album,
            title: a["name"].as_str().or(a["title"].as_str()).unwrap_or("").into(),
            subtitle,
            count: a["songCount"].as_u64(),
            art: a["coverArt"].as_str().map(|c| ArtRef { source: self.id.clone(), id: c.into() }),
        })
    }

    fn artist_node(&self, a: &Value) -> Item {
        let n = a["albumCount"].as_u64().unwrap_or(0);
        Item::Node(Node {
            id: self.node(&format!("artist:{}", a["id"].as_str().unwrap_or(""))),
            kind: NodeKind::Artist,
            title: a["name"].as_str().unwrap_or("").into(),
            subtitle: format!("{n} album{}", if n == 1 { "" } else { "s" }),
            count: None,
            art: None,
        })
    }

    /// All albums of one kind ("newest", "alphabeticalByName"), page by page.
    fn album_list(&self, l: &Login, kind: &str, max: usize) -> Result<Vec<Item>> {
        let mut out = Vec::new();
        while out.len() < max {
            let off = out.len().to_string();
            let r = get(l, "getAlbumList2", &[("type", kind), ("size", "500"), ("offset", &off)])?;
            let page = arr(&r["albumList2"]["album"]);
            let n = page.len();
            out.extend(page.iter().map(|a| self.album_node(a)));
            if n < 500 {
                break;
            }
        }
        out.truncate(max);
        Ok(out)
    }

    /// Server results for the main search: artists, albums, songs.
    pub fn find(&self, q: &str) -> Result<Vec<Item>> {
        let l = self.login()?;
        let r = get(&l, "search3", &[("query", q), ("artistCount", "5"), ("albumCount", "10"), ("songCount", "20")])?;
        let s = &r["searchResult3"];
        let mut v: Vec<Item> = arr(&s["artist"]).iter().map(|a| self.artist_node(a)).collect();
        v.extend(arr(&s["album"]).iter().map(|a| self.album_node(a)));
        v.extend(arr(&s["song"]).iter().map(|x| Item::Track(Arc::new(self.song(x)))));
        Ok(v)
    }

    /// Tell the server a song was played (its own "most played" lists).
    pub fn scrobble(&self, id: &str) {
        if let Ok(l) = self.login() {
            let _ = get(&l, "scrobble", &[("id", id), ("submission", "true")]);
        }
    }
}

impl Source for SubsonicSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::SEEK | Caps::ART | Caps::PLAYLISTS
    }

    fn children(&self, node: &str, _page: Page) -> Result<Vec<Item>> {
        let l = self.login()?;
        let node = node.strip_prefix(&*self.id).and_then(|n| n.strip_prefix(':')).unwrap_or(node);
        if node == "artists" {
            let r = match get(&l, "getArtists", &[]) {
                Ok(r) => r,
                // Navidrome makes its artist list only after the first full scan.
                Err(e) => match get(&l, "getScanStatus", &[]) {
                    Ok(s) if s["scanStatus"]["scanning"] == true => bail!(
                        "The server is still reading your library ({} songs so far). Artists show when it's done; try again in a few minutes.",
                        s["scanStatus"]["count"].as_u64().unwrap_or(0)
                    ),
                    _ => return Err(e),
                },
            };
            return Ok(arr(&r["artists"]["index"]).iter().flat_map(|i| arr(&i["artist"])).map(|a| self.artist_node(&a)).collect());
        }
        if node == "albums" {
            return self.album_list(&l, "alphabeticalByName", 20_000);
        }
        if node == "newest" {
            return self.album_list(&l, "newest", 100);
        }
        if node == "playlists" {
            let r = get(&l, "getPlaylists", &[])?;
            return Ok(arr(&r["playlists"]["playlist"])
                .iter()
                .map(|p| {
                    Item::Node(Node {
                        id: self.node(&format!("playlist:{}", p["id"].as_str().unwrap_or(""))),
                        kind: NodeKind::Folder,
                        title: p["name"].as_str().unwrap_or("").into(),
                        subtitle: p["owner"].as_str().unwrap_or("").into(),
                        count: p["songCount"].as_u64(),
                        art: None,
                    })
                })
                .collect());
        }
        if let Some(id) = node.strip_prefix("artist:") {
            let r = get(&l, "getArtist", &[("id", id)])?;
            return Ok(arr(&r["artist"]["album"]).iter().map(|a| self.album_node(a)).collect());
        }
        if let Some(id) = node.strip_prefix("album:") {
            let r = get(&l, "getAlbum", &[("id", id)])?;
            return Ok(arr(&r["album"]["song"]).iter().map(|s| Item::Track(Arc::new(self.song(s)))).collect());
        }
        if let Some(id) = node.strip_prefix("playlist:") {
            let r = get(&l, "getPlaylist", &[("id", id)])?;
            return Ok(arr(&r["playlist"]["entry"]).iter().map(|s| Item::Track(Arc::new(self.song(s)))).collect());
        }
        bail!("unknown server list {node}")
    }

    fn search(&self, _q: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        let l = self.login()?;
        let v = get(&l, "getSong", &[("id", &r.id)])?;
        Ok(self.song(&v["song"]))
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        let l = self.login()?;
        // The original file (no re-encoding), with seeking.
        let url = l.endpoint("stream", &[("id", &r.id), ("format", "raw")]);
        let suffix = get(&l, "getSong", &[("id", &r.id)])
            .ok()
            .and_then(|v| v["song"]["suffix"].as_str().map(str::to_string))
            .unwrap_or("mp3".into());
        let mut src = HttpFile::open(&url)?;
        if src.content_type.contains("json") || src.content_type.contains("xml") {
            // An error instead of the song.
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut src, &mut text);
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["subsonic-response"]["error"]["message"].as_str().map(str::to_string));
            bail!(
                "the server can't send this song{}: is its file still in the server's music folder?",
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            );
        }
        Ok(Box::new(FileStream::open_stream(Box::new(src), &suffix)?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        let l = self.login()?;
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into();
        let mut resp = agent.get(&l.endpoint("getCoverArt", &[("id", &a.id), ("size", "600")])).call()?;
        let image = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("image/"));
        if !resp.status().is_success() || !image {
            return Ok(None);
        }
        Ok(Some(resp.body_mut().with_config().limit(16 << 20).read_to_vec()?))
    }

    fn lyrics(&self, _r: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_keeps_no_password() {
        let l = Login::new("music.example.com/", "ann", "secret pw");
        assert_eq!(l.url, "http://music.example.com");
        assert_eq!(l.token, format!("{:x}", md5::compute(format!("secret pw{}", l.salt))));
        assert!(!serde_json::to_string(&l).unwrap().contains("secret"));
        assert_eq!(l.label(), "ann@music.example.com");
        assert!(l.endpoint("ping", &[("q", "a b&c")]).ends_with("&q=a%20b%26c"));
    }
}
