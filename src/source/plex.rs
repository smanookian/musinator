//! Plex Media Server music libraries. The login is kept as a Plex token;
//! the password is never stored.
//!
//! Signing in: user name (or email) and password of the plex.tv account
//! (with two-factor login on, put the 6-digit code right after the password).
//! Or type `token` as the user name and an X-Plex-Token as the password.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::podcast::HttpFile;
use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::model::{ArtRef, AudioFormat, SourceId, Track, TrackRef};

const PRODUCT: &str = "Musinator";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Login {
    pub url: String,
    pub user: String,
    pub token: String,
    pub client_id: String,
}

impl Login {
    pub fn label(&self) -> String {
        let host = self.url.split("://").nth(1).unwrap_or(&self.url);
        format!("{}@{host}", self.user)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into()
}

fn norm_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    if !u.starts_with("http://") && !u.starts_with("https://") {
        u = format!("http://{u}");
    }
    u
}

fn read_json(r: &mut ureq::http::Response<ureq::Body>) -> Result<Value> {
    Ok(serde_json::from_str(&r.body_mut().with_config().limit(64 << 20).read_to_string()?)?)
}

/// Is there a Plex Media Server at this address? Returns "Plex 1.41.0".
/// `/identity` answers without a login.
pub fn detect(url: &str) -> Option<String> {
    let mut r = agent().get(&format!("{}/identity", norm_url(url))).header("Accept", "application/json").call().ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v = read_json(&mut r).ok()?;
    let mc = &v["MediaContainer"];
    mc["machineIdentifier"].as_str()?;
    let version = mc["version"].as_str().unwrap_or("").split('-').next().unwrap_or("");
    Some(format!("Plex {version}").trim().to_string())
}

fn machine_id(url: &str) -> Option<String> {
    let mut r = agent().get(&format!("{url}/identity")).header("Accept", "application/json").call().ok()?;
    read_json(&mut r).ok()?["MediaContainer"]["machineIdentifier"].as_str().map(String::from)
}

/// Can this token read the server's libraries? (Some(true) yes, Some(false) refused.)
fn token_works(url: &str, token: &str, client_id: &str) -> Result<bool> {
    let r = agent()
        .get(&format!("{url}/library/sections"))
        .header("Accept", "application/json")
        .header("X-Plex-Token", token)
        .header("X-Plex-Client-Identifier", client_id)
        .call()
        .map_err(|e| super::net_error(e, "the server"))?;
    match r.status().as_u16() {
        200 => Ok(true),
        401 | 403 => Ok(false),
        s => Err(super::http_error("the server", s)),
    }
}

/// Log in. Keeps only the token.
pub fn login(url: &str, user: &str, password: &str) -> Result<Login> {
    let url = norm_url(url);
    let client_id = format!("{:x}", md5::compute(format!("{:?}{}", std::time::SystemTime::now(), std::process::id())));
    let user = user.trim();

    // A token typed in directly.
    if user.eq_ignore_ascii_case("token") {
        let token = password.trim();
        if !token_works(&url, token, &client_id)? {
            bail!("the server refused this token");
        }
        return Ok(Login { url, user: "token".into(), token: token.into(), client_id });
    }

    // plex.tv account. A 2FA code may be glued to the end of the password.
    let (pw, code) = match password.len().checked_sub(6).map(|i| password.split_at(i)) {
        Some((p, c)) if c.chars().all(|ch| ch.is_ascii_digit()) => (p, Some(c)),
        _ => (password, None),
    };
    let sign_in = |pw: &str, code: Option<&str>| -> Result<(u16, Value)> {
        let mut form = vec![("login", user), ("password", pw)];
        if let Some(c) = code {
            form.push(("verificationCode", c));
        }
        let mut r = agent()
            .post("https://plex.tv/api/v2/users/signin")
            .header("Accept", "application/json")
            .header("X-Plex-Product", PRODUCT)
            .header("X-Plex-Client-Identifier", &client_id)
            .send_form(form)
            .map_err(|e| super::net_error(e, "plex.tv"))?;
        let status = r.status().as_u16();
        Ok((status, read_json(&mut r).unwrap_or_default()))
    };
    // Try the whole thing as the password first (passwords may end in digits).
    let (mut status, mut v) = sign_in(password, None)?;
    if status == 401 && code.is_some() {
        (status, v) = sign_in(pw, code)?;
    }
    match status {
        200 | 201 => {}
        401 => bail!("wrong user name or password (with two-factor login, put the code right after the password)"),
        s => return Err(super::http_error("plex.tv", s)),
    }
    let account_token = v["authToken"].as_str().context("plex.tv's answer was incomplete")?.to_string();
    let name = v["username"].as_str().unwrap_or(user).to_string();

    // The owner's token works on the server. A shared server wants its own token.
    let token =
        if token_works(&url, &account_token, &client_id)? { account_token } else { server_token(&url, &account_token, &client_id)? };
    Ok(Login { url, user: name, token, client_id })
}

/// The access token for a server shared with this account.
fn server_token(url: &str, account_token: &str, client_id: &str) -> Result<String> {
    let id = machine_id(url).context("the server didn't say who it is")?;
    let mut r = agent()
        .get("https://plex.tv/api/v2/resources")
        .query("includeHttps", "1")
        .header("Accept", "application/json")
        .header("X-Plex-Product", PRODUCT)
        .header("X-Plex-Client-Identifier", client_id)
        .header("X-Plex-Token", account_token)
        .call()
        .map_err(|e| super::net_error(e, "plex.tv"))?;
    let v = read_json(&mut r)?;
    let token = v
        .as_array()
        .and_then(|a| a.iter().find(|s| s["clientIdentifier"] == id.as_str()))
        .and_then(|s| s["accessToken"].as_str())
        .context("this Plex account has no access to this server")?
        .to_string();
    if !token_works(url, &token, client_id)? {
        bail!("the server refused this Plex account");
    }
    Ok(token)
}

fn metadata(v: &Value) -> Vec<Value> {
    v["MediaContainer"]["Metadata"].as_array().cloned().unwrap_or_default()
}

fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// One Plex server. Its id ("srv3") prefixes its list nodes ("srv3:albums").
pub struct PlexSource {
    id: SourceId,
    login: Login,
    /// Keys of the music libraries, found on first use.
    sections: Mutex<Option<Vec<String>>>,
}

impl PlexSource {
    pub fn new(id: &str, login: Login) -> Self {
        Self { id: Arc::from(id), login, sections: Mutex::new(None) }
    }

    fn node(&self, what: &str) -> String {
        format!("{}:{what}", self.id)
    }

    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let l = &self.login;
        let mut req = agent()
            .get(&format!("{}{path}", l.url))
            .header("Accept", "application/json")
            .header("X-Plex-Token", &l.token)
            .header("X-Plex-Product", PRODUCT)
            .header("X-Plex-Client-Identifier", &l.client_id);
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut r = req.call().map_err(|e| super::net_error(e, "the server"))?;
        match r.status().as_u16() {
            200 => read_json(&mut r),
            401 => bail!("the login is no longer valid; set up the music server again in Settings"),
            s => Err(super::http_error("the server", s)),
        }
    }

    /// The music libraries' keys.
    fn sections(&self) -> Result<Vec<String>> {
        if let Some(s) = self.sections.lock().clone() {
            return Ok(s);
        }
        let v = self.get("/library/sections", &[])?;
        let keys: Vec<String> = v["MediaContainer"]["Directory"]
            .as_array()
            .map(|a| a.iter().filter(|d| d["type"] == "artist").map(|d| str_of(&d["key"])).collect())
            .unwrap_or_default();
        *self.sections.lock() = Some(keys.clone());
        Ok(keys)
    }

    /// Items of one type (8 artists, 9 albums, 10 tracks) from every music library.
    fn all(&self, kind: &str, sort: &str, limit: Option<&str>) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for key in self.sections()? {
            let mut q = vec![("type", kind), ("sort", sort)];
            if let Some(n) = limit {
                q.push(("X-Plex-Container-Start", "0"));
                q.push(("X-Plex-Container-Size", n));
            }
            out.extend(metadata(&self.get(&format!("/library/sections/{key}/all"), &q)?));
        }
        Ok(out)
    }

    fn art(&self, v: &Value) -> Option<ArtRef> {
        // The song's own picture, else its album's.
        let thumb = v["thumb"].as_str().or(v["parentThumb"].as_str())?;
        Some(ArtRef { source: self.id.clone(), id: thumb.into() })
    }

    fn song(&self, v: &Value) -> Track {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let media = &v["Media"][0];
        let stream = media["Part"][0]["Stream"]
            .as_array()
            .and_then(|a| a.iter().find(|m| m["streamType"].as_u64() == Some(2)))
            .cloned()
            .unwrap_or_default();
        let codec = match media["audioCodec"].as_str().unwrap_or("").to_ascii_lowercase().as_str() {
            "flac" => "FLAC",
            "mp3" => "MP3",
            "aac" => "AAC",
            "alac" => "ALAC",
            "vorbis" => "Vorbis",
            "opus" => "Opus",
            c if c.starts_with("pcm") || c == "wav" => "WAV",
            _ => "",
        };
        let album_artist = s("grandparentTitle");
        let artist = v["originalTitle"].as_str().filter(|a| !a.is_empty()).map(String::from).unwrap_or(album_artist.clone());
        Track {
            r: Some(TrackRef { source: self.id.clone(), id: str_of(&v["ratingKey"]) }),
            title: s("title"),
            artist,
            album: s("parentTitle"),
            album_artist,
            year: v["parentYear"].as_i64().or(v["year"].as_i64()).map(|y| y as i32),
            track_no: v["index"].as_u64().map(|n| n as u32),
            disc_no: v["parentIndex"].as_u64().map(|n| n as u32),
            duration_ms: v["duration"].as_u64(),
            format: (!codec.is_empty()).then(|| AudioFormat {
                codec: codec.into(),
                sample_rate: stream["samplingRate"].as_u64().unwrap_or(0) as u32,
                bits: stream["bitDepth"].as_u64().filter(|b| *b > 0).map(|b| b as u32),
                channels: media["audioChannels"].as_u64().unwrap_or(2) as u16,
                bitrate_kbps: media["bitrate"].as_u64().map(|b| b as u32),
            }),
            art: self.art(v),
            ..Default::default()
        }
    }

    fn album_node(&self, v: &Value) -> Item {
        let artist = v["parentTitle"].as_str().unwrap_or("");
        let subtitle = match v["year"].as_u64() {
            Some(y) => format!("{artist} · {y}"),
            None => artist.to_string(),
        };
        Item::Node(Node {
            id: self.node(&format!("album:{}", str_of(&v["ratingKey"]))),
            kind: NodeKind::Album,
            title: v["title"].as_str().unwrap_or("").into(),
            subtitle,
            count: v["leafCount"].as_u64(),
            art: self.art(v),
        })
    }

    fn artist_node(&self, v: &Value) -> Item {
        Item::Node(Node {
            id: self.node(&format!("artist:{}", str_of(&v["ratingKey"]))),
            kind: NodeKind::Artist,
            title: v["title"].as_str().unwrap_or("").into(),
            subtitle: String::new(),
            count: v["childCount"].as_u64(),
            art: None,
        })
    }

    fn tracks(&self, v: &Value) -> Vec<Item> {
        metadata(v).iter().filter(|t| t["type"] == "track").map(|t| Item::Track(Arc::new(self.song(t)))).collect()
    }

    /// Results for the main search.
    pub fn find(&self, q: &str) -> Result<Vec<Item>> {
        let v = self.get("/hubs/search", &[("query", q), ("limit", "20")])?;
        let hubs = v["MediaContainer"]["Hub"].as_array().cloned().unwrap_or_default();
        let hub = |t: &str| -> Vec<Value> {
            hubs.iter().find(|h| h["type"] == t).and_then(|h| h["Metadata"].as_array().cloned()).unwrap_or_default()
        };
        let mut out: Vec<Item> = hub("artist").iter().map(|a| self.artist_node(a)).collect();
        out.extend(hub("album").iter().map(|a| self.album_node(a)));
        out.extend(hub("track").iter().take(20).map(|t| Item::Track(Arc::new(self.song(t)))));
        Ok(out)
    }

    /// Mark a song as played on the server.
    pub fn scrobble(&self, id: &str) {
        let _ = self.get("/:/scrobble", &[("key", id), ("identifier", "com.plexapp.plugins.library")]);
    }
}

impl Source for PlexSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::SEEK | Caps::ART | Caps::PLAYLISTS
    }

    fn children(&self, node: &str, _page: Page) -> Result<Vec<Item>> {
        let node = node.strip_prefix(&*self.id).and_then(|n| n.strip_prefix(':')).unwrap_or(node);
        match node {
            "artists" => {
                let mut v = self.all("8", "titleSort", None)?;
                v.sort_by_key(|a| a["titleSort"].as_str().or(a["title"].as_str()).unwrap_or("").to_lowercase());
                return Ok(v.iter().map(|a| self.artist_node(a)).collect());
            }
            "albums" => {
                let mut v = self.all("9", "titleSort", None)?;
                v.sort_by_key(|a| a["titleSort"].as_str().or(a["title"].as_str()).unwrap_or("").to_lowercase());
                return Ok(v.iter().map(|a| self.album_node(a)).collect());
            }
            "newest" => {
                let mut v = self.all("9", "addedAt:desc", Some("100"))?;
                v.sort_by_key(|a| std::cmp::Reverse(a["addedAt"].as_i64().unwrap_or(0)));
                v.truncate(100);
                return Ok(v.iter().map(|a| self.album_node(a)).collect());
            }
            "playlists" => {
                let v = self.get("/playlists", &[("playlistType", "audio")])?;
                return Ok(metadata(&v)
                    .iter()
                    .map(|p| {
                        Item::Node(Node {
                            id: self.node(&format!("playlist:{}", str_of(&p["ratingKey"]))),
                            kind: NodeKind::Folder,
                            title: p["title"].as_str().unwrap_or("").into(),
                            subtitle: String::new(),
                            count: p["leafCount"].as_u64(),
                            art: None,
                        })
                    })
                    .collect());
            }
            _ => {}
        }
        if let Some(id) = node.strip_prefix("artist:") {
            let mut v = metadata(&self.get(&format!("/library/metadata/{id}/children"), &[])?);
            v.sort_by_key(|a| (a["year"].as_i64().unwrap_or(0), a["title"].as_str().unwrap_or("").to_lowercase()));
            return Ok(v.iter().map(|a| self.album_node(a)).collect());
        }
        if let Some(id) = node.strip_prefix("album:") {
            return Ok(self.tracks(&self.get(&format!("/library/metadata/{id}/children"), &[])?));
        }
        if let Some(id) = node.strip_prefix("playlist:") {
            // Playlists keep their own order.
            return Ok(self.tracks(&self.get(&format!("/playlists/{id}/items"), &[])?));
        }
        bail!("unknown server list {node}")
    }

    fn search(&self, _q: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        let v = self.get(&format!("/library/metadata/{}", r.id), &[])?;
        let m = metadata(&v).into_iter().next().context("the server doesn't have this song any more")?;
        Ok(self.song(&m))
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        let v = self.get(&format!("/library/metadata/{}", r.id), &[])?;
        let m = metadata(&v).into_iter().next().context("the server doesn't have this song any more")?;
        let media = &m["Media"][0];
        let part = media["Part"][0]["key"].as_str().context("the server has no file for this song")?;
        let ext = media["container"].as_str().or(media["Part"][0]["container"].as_str()).unwrap_or("mp3").to_string();
        // The original file (no re-encoding), with seeking.
        let l = &self.login;
        let sep = if part.contains('?') { '&' } else { '?' };
        let url = format!("{}{part}{sep}X-Plex-Token={}", l.url, l.token);
        let src = HttpFile::open(&url)?;
        if src.content_type.contains("json") || src.content_type.contains("html") || src.content_type.contains("xml") {
            bail!("the server can't send this song ({}): is its file still in the server's library?", src.content_type);
        }
        Ok(Box::new(FileStream::open_stream(Box::new(src), &ext)?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        let l = &self.login;
        let mut r = agent()
            .get(&format!("{}/photo/:/transcode", l.url))
            .query("width", "600")
            .query("height", "600")
            .query("minSize", "1")
            .query("url", &a.id)
            .header("X-Plex-Token", &l.token)
            .header("X-Plex-Client-Identifier", &l.client_id)
            .call()?;
        if !r.status().is_success() {
            return Ok(None);
        }
        Ok(Some(r.body_mut().with_config().limit(16 << 20).read_to_vec()?))
    }

    fn lyrics(&self, _r: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}
