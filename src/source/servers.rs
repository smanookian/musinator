//! The music servers the user added (Navidrome/Subsonic or Jellyfin, any number).
//! Kept in `~/.config/musinator/servers.json` (logins as tokens, never passwords).
//! Each server is its own source with an id like "srv1"; its lists are "srv1:albums".

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, bail};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::jellyfin::{self, JellyfinSource};
use super::subsonic::{self, SubsonicSource};
use super::{Item, Source, Sources};

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subsonic: Option<subsonic::Login>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jellyfin: Option<jellyfin::Login>,
}

pub enum Kind {
    Subsonic(Arc<SubsonicSource>),
    Jellyfin(Arc<JellyfinSource>),
}

pub struct Server {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    saved: Saved,
}

impl Server {
    fn from_saved(s: Saved) -> Option<Self> {
        let kind = match (&s.subsonic, &s.jellyfin) {
            (Some(l), _) => Kind::Subsonic(Arc::new(SubsonicSource::new(&s.id, l.clone()))),
            (_, Some(l)) => Kind::Jellyfin(Arc::new(JellyfinSource::new(&s.id, l.clone()))),
            _ => return None,
        };
        Some(Self { id: s.id.clone(), name: s.name.clone(), kind, saved: s })
    }

    pub fn source(&self) -> Arc<dyn Source> {
        match &self.kind {
            Kind::Subsonic(s) => s.clone(),
            Kind::Jellyfin(s) => s.clone(),
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Subsonic(_) => "Navidrome / Subsonic",
            Kind::Jellyfin(_) => "Jellyfin",
        }
    }

    /// "you@host".
    pub fn label(&self) -> String {
        match (&self.saved.subsonic, &self.saved.jellyfin) {
            (Some(l), _) => l.label(),
            (_, Some(l)) => l.label(),
            _ => String::new(),
        }
    }

    /// Sidebar entries: (label, list node).
    pub fn nodes(&self) -> Vec<(&'static str, String)> {
        [("Artists", "artists"), ("Albums", "albums"), ("Recently added", "newest"), ("Playlists", "playlists")]
            .into_iter()
            .map(|(l, n)| (l, format!("{}:{n}", self.id)))
            .collect()
    }

    pub fn find(&self, q: &str) -> Result<Vec<Item>> {
        match &self.kind {
            Kind::Subsonic(s) => s.find(q),
            Kind::Jellyfin(s) => s.find(q),
        }
    }

    pub fn scrobble(&self, id: &str) {
        match &self.kind {
            Kind::Subsonic(s) => s.scrobble(id),
            Kind::Jellyfin(s) => s.scrobble(id),
        }
    }
}

fn path() -> PathBuf {
    crate::config::config_dir().join("servers.json")
}

pub struct Servers {
    list: RwLock<Vec<Arc<Server>>>,
    sources: Arc<Sources>,
}

impl Servers {
    /// Load the saved servers and register them as sources.
    pub fn load(sources: Arc<Sources>) -> Arc<Self> {
        let saved: Vec<Saved> = match std::fs::read_to_string(path()) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            Err(_) => migrate(),
        };
        let list: Vec<Arc<Server>> = saved.into_iter().filter_map(Server::from_saved).map(Arc::new).collect();
        for s in &list {
            sources.add(s.source());
        }
        Arc::new(Self { list: RwLock::new(list), sources })
    }

    fn save(&self) -> Result<()> {
        let saved: Vec<Saved> = self.list.read().iter().map(|s| s.saved.clone()).collect();
        std::fs::create_dir_all(crate::config::config_dir())?;
        let p = path();
        std::fs::write(&p, serde_json::to_string_pretty(&saved)?)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn all(&self) -> Vec<Arc<Server>> {
        self.list.read().clone()
    }

    pub fn is_empty(&self) -> bool {
        self.list.read().is_empty()
    }

    /// The server a song or picture belongs to (by source id).
    pub fn by_id(&self, id: &str) -> Option<Arc<Server>> {
        self.list.read().iter().find(|s| s.id == id).cloned()
    }

    /// The server a list node belongs to ("srv1:albums").
    pub fn for_node(&self, node: &str) -> Option<Arc<Server>> {
        let id = node.split(':').next()?;
        self.by_id(id)
    }

    /// Find out what kind of server it is, log in, and add it.
    /// Returns its name and what it is ("Home · Jellyfin 12.4").
    pub fn add(&self, name: &str, url: &str, user: &str, password: &str) -> Result<String> {
        let (subsonic, jellyfin, version) = if let Some(version) = jellyfin::detect(url) {
            (None, Some(jellyfin::login(url, user, password)?), version)
        } else {
            let l = subsonic::Login::new(url, user, password);
            let version = subsonic::ping(&l).map_err(|e| {
                let s = e.to_string();
                if s.contains("not a Subsonic") || s.contains("answered 404") {
                    anyhow::anyhow!("no Navidrome, Subsonic or Jellyfin server found at this address")
                } else {
                    e
                }
            })?;
            (Some(l), None, version)
        };
        let host = subsonic.as_ref().map(|l| l.label()).or(jellyfin.as_ref().map(|l| l.label())).unwrap_or_default();
        let mut list = self.list.write();
        if list.iter().any(|s| s.label() == host) {
            bail!("{host} is already in your list");
        }
        let id = (1..).map(|n| format!("srv{n}")).find(|id| !list.iter().any(|s| &s.id == id)).expect("free id");
        let name = if name.trim().is_empty() { host.split('@').nth(1).unwrap_or(&host).to_string() } else { name.trim().to_string() };
        let server = Server::from_saved(Saved { id, name: name.clone(), subsonic, jellyfin }).expect("login");
        self.sources.add(server.source());
        list.push(Arc::new(server));
        drop(list);
        self.save()?;
        Ok(format!("{name} · {version}"))
    }

    /// Remove a server; returns its name.
    pub fn remove(&self, id: &str) -> Option<String> {
        let mut list = self.list.write();
        let i = list.iter().position(|s| s.id == id)?;
        let s = list.remove(i);
        drop(list);
        self.sources.remove(id);
        let _ = self.save();
        Some(s.name.clone())
    }
}

/// One-time move from the single-server files (server.json / jellyfin.json).
fn migrate() -> Vec<Saved> {
    let dir = crate::config::config_dir();
    let mut out = Vec::new();
    let old_sub = dir.join("server.json");
    let old_jf = dir.join("jellyfin.json");
    if let Some(l) = std::fs::read_to_string(&old_sub).ok().and_then(|t| serde_json::from_str::<subsonic::Login>(&t).ok()) {
        out.push(Saved { id: "srv1".into(), name: "Music server".into(), subsonic: Some(l), jellyfin: None });
    }
    if let Some(l) = std::fs::read_to_string(&old_jf).ok().and_then(|t| serde_json::from_str::<jellyfin::Login>(&t).ok()) {
        let id = format!("srv{}", out.len() + 1);
        out.push(Saved { id, name: "Jellyfin".into(), subsonic: None, jellyfin: Some(l) });
    }
    if !out.is_empty() {
        let tmp = Servers {
            list: RwLock::new(out.iter().cloned().filter_map(Server::from_saved).map(Arc::new).collect()),
            sources: Arc::default(),
        };
        if tmp.save().is_ok() {
            let _ = std::fs::remove_file(old_sub);
            let _ = std::fs::remove_file(old_jf);
        }
    }
    out
}
