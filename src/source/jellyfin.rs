//! Jellyfin music libraries. The login is kept as an access token in
//! `~/.config/musinator/jellyfin.json`; the password is never stored.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::podcast::HttpFile;
use super::{Caps, Item, Lyrics, Node, NodeKind, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::model::{ArtRef, AudioFormat, SourceId, Track, TrackRef};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Login {
    pub url: String,
    pub user: String,
    pub user_id: String,
    pub token: String,
    pub device_id: String,
}

impl Login {
    pub fn label(&self) -> String {
        let host = self.url.split("://").nth(1).unwrap_or(&self.url);
        format!("{}@{host}", self.user)
    }
}

fn auth_header(device_id: &str, token: Option<&str>) -> String {
    let mut h = format!(
        "MediaBrowser Client=\"Musinator\", Device=\"Terminal\", DeviceId=\"{device_id}\", Version=\"{}\"",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(t) = token {
        h.push_str(&format!(", Token=\"{t}\""));
    }
    h
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

/// Is there a Jellyfin server at this address? Returns its name and version.
pub fn detect(url: &str) -> Option<String> {
    let mut r = agent().get(&format!("{}/System/Info/Public", norm_url(url))).call().ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: Value = serde_json::from_str(&r.body_mut().read_to_string().ok()?).ok()?;
    let product = v["ProductName"].as_str()?;
    product.to_lowercase().contains("jellyfin").then(|| format!("Jellyfin {}", v["Version"].as_str().unwrap_or("")).trim().to_string())
}

/// Log in with user name and password; keeps only the token.
pub fn login(url: &str, user: &str, password: &str) -> Result<Login> {
    let url = norm_url(url);
    let device_id = format!("{:x}", md5::compute(format!("{:?}{}", std::time::SystemTime::now(), std::process::id())));
    let body = serde_json::json!({ "Username": user.trim(), "Pw": password });
    let mut r = agent()
        .post(&format!("{url}/Users/AuthenticateByName"))
        .header("Authorization", auth_header(&device_id, None))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| super::net_error(e, "the server"))?;
    match r.status().as_u16() {
        200 => {}
        401 => bail!("wrong user name or password"),
        s => bail!("the server answered {s}"),
    }
    let v: Value = serde_json::from_str(&r.body_mut().read_to_string()?)?;
    Ok(Login {
        url,
        user: v["User"]["Name"].as_str().unwrap_or(user).into(),
        user_id: v["User"]["Id"].as_str().context("no user in the answer")?.into(),
        token: v["AccessToken"].as_str().context("no token in the answer")?.into(),
        device_id,
    })
}

fn items(v: &Value) -> Vec<Value> {
    v["Items"].as_array().cloned().unwrap_or_default()
}

/// One Jellyfin server. Its id ("srv2") prefixes its list nodes ("srv2:artists").
pub struct JellyfinSource {
    id: SourceId,
    login: Login,
}

impl JellyfinSource {
    pub fn new(id: &str, login: Login) -> Self {
        Self { id: Arc::from(id), login }
    }

    fn login(&self) -> Result<Login> {
        Ok(self.login.clone())
    }

    fn node(&self, what: &str) -> String {
        format!("{}:{what}", self.id)
    }

    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let l = self.login()?;
        let mut req = agent().get(&format!("{}{path}", l.url)).header("Authorization", auth_header(&l.device_id, Some(&l.token)));
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut r = req.call().map_err(|e| super::net_error(e, "the server"))?;
        match r.status().as_u16() {
            200 => Ok(serde_json::from_str(&r.body_mut().read_to_string()?)?),
            401 => bail!("the login is no longer valid; set up the music server again in Settings"),
            s => bail!("the server answered {s}"),
        }
    }

    /// Music items of the user (albums, songs, artists, playlists).
    fn user_items(&self, query: &[(&str, &str)]) -> Result<Vec<Value>> {
        let l = self.login()?;
        let mut q = vec![("Recursive", "true"), ("Fields", "MediaStreams,ProductionYear,ChildCount,DateCreated")];
        q.extend_from_slice(query);
        Ok(items(&self.get(&format!("/Users/{}/Items", l.user_id), &q)?))
    }

    fn art(&self, v: &Value) -> Option<ArtRef> {
        // A song's own picture, else its album's.
        let id = if v["ImageTags"]["Primary"].is_string() {
            v["Id"].as_str()
        } else if v["AlbumPrimaryImageTag"].is_string() {
            v["AlbumId"].as_str()
        } else {
            None
        }?;
        Some(ArtRef { source: self.id.clone(), id: id.into() })
    }

    fn song(&self, v: &Value) -> Track {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let stream = v["MediaStreams"].as_array().and_then(|a| a.iter().find(|m| m["Type"] == "Audio")).cloned().unwrap_or_default();
        let codec = match stream["Codec"].as_str().unwrap_or("").to_ascii_lowercase().as_str() {
            "flac" => "FLAC",
            "mp3" => "MP3",
            "aac" => "AAC",
            "alac" => "ALAC",
            "vorbis" => "Vorbis",
            "opus" => "Opus",
            c if c.starts_with("pcm") => "WAV",
            _ => "",
        };
        let artists: Vec<&str> = v["Artists"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        Track {
            r: Some(TrackRef { source: self.id.clone(), id: s("Id") }),
            title: s("Name"),
            artist: if artists.is_empty() { s("AlbumArtist") } else { artists.join(", ") },
            album: s("Album"),
            album_artist: s("AlbumArtist"),
            year: v["ProductionYear"].as_i64().map(|y| y as i32),
            track_no: v["IndexNumber"].as_u64().map(|n| n as u32),
            disc_no: v["ParentIndexNumber"].as_u64().map(|n| n as u32),
            // Ticks are 100 ns.
            duration_ms: v["RunTimeTicks"].as_u64().map(|t| t / 10_000),
            format: (!codec.is_empty()).then(|| AudioFormat {
                codec: codec.into(),
                sample_rate: stream["SampleRate"].as_u64().unwrap_or(0) as u32,
                bits: stream["BitDepth"].as_u64().filter(|b| *b > 0).map(|b| b as u32),
                channels: stream["Channels"].as_u64().unwrap_or(2) as u16,
                bitrate_kbps: stream["BitRate"].as_u64().map(|b| (b / 1000) as u32),
            }),
            art: self.art(v),
            ..Default::default()
        }
    }

    fn album_node(&self, v: &Value) -> Item {
        let artist = v["AlbumArtist"].as_str().unwrap_or("");
        let subtitle = match v["ProductionYear"].as_u64() {
            Some(y) => format!("{artist} · {y}"),
            None => artist.to_string(),
        };
        Item::Node(Node {
            id: self.node(&format!("album:{}", v["Id"].as_str().unwrap_or(""))),
            kind: NodeKind::Album,
            title: v["Name"].as_str().unwrap_or("").into(),
            subtitle,
            count: v["ChildCount"].as_u64(),
            art: self.art(v),
        })
    }

    fn artist_node(&self, v: &Value) -> Item {
        Item::Node(Node {
            id: self.node(&format!("artist:{}", v["Id"].as_str().unwrap_or(""))),
            kind: NodeKind::Artist,
            title: v["Name"].as_str().unwrap_or("").into(),
            subtitle: String::new(),
            count: None,
            art: None,
        })
    }

    /// Results for the main search.
    pub fn find(&self, q: &str) -> Result<Vec<Item>> {
        let found = self.user_items(&[("searchTerm", q), ("IncludeItemTypes", "MusicArtist,MusicAlbum,Audio"), ("Limit", "40")])?;
        let mut v: Vec<Item> = found.iter().filter(|i| i["Type"] == "MusicArtist").map(|a| self.artist_node(a)).collect();
        v.extend(found.iter().filter(|i| i["Type"] == "MusicAlbum").map(|a| self.album_node(a)));
        v.extend(found.iter().filter(|i| i["Type"] == "Audio").take(20).map(|s| Item::Track(Arc::new(self.song(s)))));
        Ok(v)
    }

    /// Mark a song as played on the server (its play counts and "played" marks).
    pub fn scrobble(&self, id: &str) {
        let Ok(l) = self.login() else { return };
        let _ = agent()
            .post(&format!("{}/Users/{}/PlayedItems/{id}", l.url, l.user_id))
            .header("Authorization", auth_header(&l.device_id, Some(&l.token)))
            .send_empty();
    }
}

impl Source for JellyfinSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::SEEK | Caps::ART | Caps::PLAYLISTS
    }

    fn children(&self, node: &str, _page: Page) -> Result<Vec<Item>> {
        let l = self.login()?;
        let songs = |parent: &str| -> Result<Vec<Item>> {
            Ok(self
                .user_items(&[("ParentId", parent), ("IncludeItemTypes", "Audio"), ("SortBy", "ParentIndexNumber,IndexNumber,SortName")])?
                .iter()
                .map(|s| Item::Track(Arc::new(self.song(s))))
                .collect())
        };
        let node = node.strip_prefix(&*self.id).and_then(|n| n.strip_prefix(':')).unwrap_or(node);
        if node == "artists" {
            let v = self.get("/Artists/AlbumArtists", &[("userId", &l.user_id), ("SortBy", "SortName")])?;
            return Ok(items(&v).iter().map(|a| self.artist_node(a)).collect());
        }
        if node == "albums" {
            return Ok(self
                .user_items(&[("IncludeItemTypes", "MusicAlbum"), ("SortBy", "SortName")])?
                .iter()
                .map(|a| self.album_node(a))
                .collect());
        }
        if node == "newest" {
            return Ok(self
                .user_items(&[
                    ("IncludeItemTypes", "MusicAlbum"),
                    ("SortBy", "DateCreated"),
                    ("SortOrder", "Descending"),
                    ("Limit", "100"),
                ])?
                .iter()
                .map(|a| self.album_node(a))
                .collect());
        }
        if node == "playlists" {
            return Ok(self
                .user_items(&[("IncludeItemTypes", "Playlist"), ("SortBy", "SortName")])?
                .iter()
                .map(|p| {
                    Item::Node(Node {
                        id: self.node(&format!("playlist:{}", p["Id"].as_str().unwrap_or(""))),
                        kind: NodeKind::Folder,
                        title: p["Name"].as_str().unwrap_or("").into(),
                        subtitle: String::new(),
                        count: p["ChildCount"].as_u64(),
                        art: None,
                    })
                })
                .collect());
        }
        if let Some(id) = node.strip_prefix("artist:") {
            return Ok(self
                .user_items(&[("IncludeItemTypes", "MusicAlbum"), ("AlbumArtistIds", id), ("SortBy", "ProductionYear,SortName")])?
                .iter()
                .map(|a| self.album_node(a))
                .collect());
        }
        if let Some(id) = node.strip_prefix("album:") {
            return songs(id);
        }
        if let Some(id) = node.strip_prefix("playlist:") {
            // Playlists keep their own order.
            let v = self.get(&format!("/Playlists/{id}/Items"), &[("userId", &l.user_id), ("Fields", "MediaStreams,ProductionYear")])?;
            return Ok(items(&v).iter().map(|s| Item::Track(Arc::new(self.song(s)))).collect());
        }
        bail!("unknown server list {node}")
    }

    fn search(&self, _q: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }

    fn track(&self, r: &TrackRef) -> Result<Track> {
        let l = self.login()?;
        Ok(self.song(&self.get(&format!("/Users/{}/Items/{}", l.user_id, r.id), &[("Fields", "MediaStreams")])?))
    }

    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        let l = self.login()?;
        // The original file (no re-encoding), with seeking.
        let url = format!("{}/Audio/{}/stream?static=true&api_key={}", l.url, r.id, l.token);
        let item = self.get(&format!("/Users/{}/Items/{}", l.user_id, r.id), &[("Fields", "MediaSources")]).ok();
        let ext = item
            .as_ref()
            .and_then(|v| v["MediaSources"][0]["Container"].as_str().map(|c| c.split(',').next().unwrap_or(c).to_string()))
            .unwrap_or("mp3".into());
        let src = HttpFile::open(&url)?;
        if !src.content_type.starts_with("audio") && !src.content_type.contains("octet-stream") && !src.content_type.is_empty() {
            bail!("the server can't send this song ({}): is its file still in the server's library?", src.content_type);
        }
        Ok(Box::new(FileStream::open_stream(Box::new(src), &ext)?))
    }

    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>> {
        let l = self.login()?;
        let mut r = agent()
            .get(&format!("{}/Items/{}/Images/Primary", l.url, a.id))
            .query("maxWidth", "600")
            .header("Authorization", auth_header(&l.device_id, Some(&l.token)))
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
