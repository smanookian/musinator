//! Last.fm: log in once in the browser, then "now playing" and scrobbles.
//! Scrobbles wait in `~/.local/state/musinator/lastfm.json` until they are sent,
//! so nothing is lost while offline. All network work runs on its own thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use crossbeam_channel as cb;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEY: &str = "979e9c7038592c0ba6dbc266054bfabe";
const SECRET: &str = "bedd1838ee553583fbfe990eff6bf5ba";
const API: &str = "https://ws.audioscrobbler.com/2.0/";
const UA: &str = concat!("Musinator/", env!("CARGO_PKG_VERSION"), " (https://github.com/smanookian/musinator)");

/// One play to send.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Play {
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    /// Seconds, 0 = unknown.
    #[serde(default)]
    pub duration: u64,
    /// When it started (unix seconds).
    pub at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    user: String,
    session: String,
    #[serde(default)]
    queue: Vec<Play>,
}

fn path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".local/state"))
        .join("musinator/lastfm.json")
}

fn load() -> State {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &State) {
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().expect("state dir"));
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(s).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

/// The logged-in user name, if any.
pub fn user() -> Option<String> {
    let s = load();
    (!s.session.is_empty()).then_some(s.user)
}

pub fn logout() {
    let mut s = load();
    s.user.clear();
    s.session.clear();
    save(&s);
}

/// Last.fm signature: md5 of sorted key+value pairs plus the secret.
fn sign(params: &BTreeMap<String, String>) -> String {
    let mut s = String::new();
    for (k, v) in params {
        if k != "format" {
            s.push_str(k);
            s.push_str(v);
        }
    }
    s.push_str(SECRET);
    format!("{:x}", md5::compute(s.as_bytes()))
}

/// Signed API call. Errors carry Last.fm's error code in the text ("error 9: ...").
fn call(method: &str, mut p: BTreeMap<String, String>, post: bool) -> Result<Value> {
    p.insert("method".into(), method.into());
    p.insert("api_key".into(), KEY.into());
    let sig = sign(&p);
    p.insert("api_sig".into(), sig);
    p.insert("format".into(), "json".into());
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into();
    let mut resp = if post {
        let form: Vec<(&str, &str)> = p.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        agent.post(API).header("User-Agent", UA).send_form(form)?
    } else {
        let mut r = agent.get(API).header("User-Agent", UA);
        for (k, v) in &p {
            r = r.query(k, v);
        }
        r.call()?
    };
    let v: Value = serde_json::from_str(&resp.body_mut().read_to_string()?)?;
    if let Some(code) = v["error"].as_u64() {
        bail!("error {code}: {}", v["message"].as_str().unwrap_or("Last.fm error"));
    }
    Ok(v)
}

fn code_of(e: &anyhow::Error) -> Option<u64> {
    e.to_string().strip_prefix("error ")?.split(':').next()?.parse().ok()
}

/// Log in: opens the browser, waits up to 3 minutes for "Allow".
/// Returns the user name.
pub fn login() -> Result<String> {
    let token = call("auth.getToken", BTreeMap::new(), false)?["token"].as_str().unwrap_or("").to_string();
    if token.is_empty() {
        bail!("Last.fm gave no login token");
    }
    let url = format!("https://www.last.fm/api/auth/?api_key={KEY}&token={token}");
    if std::process::Command::new("xdg-open")
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_err()
    {
        bail!("Can't open the browser. Open this page: {url}");
    }
    for _ in 0..60 {
        std::thread::sleep(Duration::from_secs(3));
        let mut p = BTreeMap::new();
        p.insert("token".to_string(), token.clone());
        match call("auth.getSession", p, false) {
            Ok(v) => {
                let mut s = load();
                s.user = v["session"]["name"].as_str().unwrap_or("").into();
                s.session = v["session"]["key"].as_str().unwrap_or("").into();
                if s.session.is_empty() {
                    bail!("Last.fm gave no session");
                }
                let name = s.user.clone();
                save(&s);
                return Ok(name);
            }
            // 14: not allowed yet (keep waiting).
            Err(e) if code_of(&e) == Some(14) => continue,
            Err(e) => return Err(e),
        }
    }
    bail!("No answer from Last.fm in 3 minutes. Try again.")
}

fn track_params(p: &Play, idx: Option<usize>) -> BTreeMap<String, String> {
    let k = |name: &str| match idx {
        Some(i) => format!("{name}[{i}]"),
        None => name.to_string(),
    };
    let mut m = BTreeMap::new();
    m.insert(k("artist"), p.artist.clone());
    m.insert(k("track"), p.title.clone());
    if !p.album.is_empty() {
        m.insert(k("album"), p.album.clone());
    }
    if !p.album_artist.is_empty() && p.album_artist != p.artist {
        m.insert(k("albumArtist"), p.album_artist.clone());
    }
    if p.duration > 0 {
        m.insert(k("duration"), p.duration.to_string());
    }
    if idx.is_some() {
        m.insert(k("timestamp"), p.at.to_string());
    }
    m
}

enum Cmd {
    NowPlaying(Play),
    Scrobble(Play),
}

/// Handle to the Last.fm thread. Sending never blocks.
pub struct Scrobbler {
    tx: cb::Sender<Cmd>,
}

impl Scrobbler {
    pub fn start() -> Self {
        let (tx, rx) = cb::unbounded();
        std::thread::Builder::new().name("lastfm".into()).spawn(move || run(rx)).ok();
        Self { tx }
    }
    pub fn now_playing(&self, p: Play) {
        let _ = self.tx.send(Cmd::NowPlaying(p));
    }
    pub fn scrobble(&self, p: Play) {
        let _ = self.tx.send(Cmd::Scrobble(p));
    }
}

/// Send waiting scrobbles, 50 at a time. Keeps them on network errors.
fn flush(s: &mut State) {
    while !s.queue.is_empty() && !s.session.is_empty() {
        let n = s.queue.len().min(50);
        let mut p = BTreeMap::new();
        for (i, play) in s.queue[..n].iter().enumerate() {
            p.extend(track_params(play, Some(i)));
        }
        p.insert("sk".into(), s.session.clone());
        match call("track.scrobble", p, true) {
            Ok(_) => {
                s.queue.drain(..n);
                save(s);
            }
            Err(e) => {
                // 9: session no longer valid (access removed on the website).
                if code_of(&e) == Some(9) {
                    s.session.clear();
                    s.user.clear();
                    save(s);
                }
                return;
            }
        }
    }
}

fn run(rx: cb::Receiver<Cmd>) {
    let mut s = load();
    flush(&mut s);
    loop {
        match rx.recv_timeout(Duration::from_secs(300)) {
            Ok(Cmd::NowPlaying(p)) => {
                s = load();
                if !s.session.is_empty() {
                    let mut m = track_params(&p, None);
                    m.insert("sk".into(), s.session.clone());
                    let _ = call("track.updateNowPlaying", m, true);
                }
            }
            Ok(Cmd::Scrobble(p)) => {
                s = load();
                if s.session.is_empty() {
                    continue;
                }
                s.queue.push(p);
                // Last.fm keeps only 2 weeks; very old waiting plays would be refused.
                let cutoff = now() - 13 * 24 * 3600;
                s.queue.retain(|q| q.at > cutoff);
                save(&s);
                flush(&mut s);
            }
            // Retry waiting scrobbles every 5 minutes.
            Err(cb::RecvTimeoutError::Timeout) => {
                s = load();
                flush(&mut s);
            }
            Err(cb::RecvTimeoutError::Disconnected) => return,
        }
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Last.fm's rule: longer than 30 s, and heard for half of it or 4 minutes.
/// Radio songs have no length: they count after 30 s heard when the song ended.
pub fn counts(duration_ms: Option<u64>, heard: Duration) -> bool {
    match duration_ms {
        Some(len) if len > 0 => len > 30_000 && heard >= Duration::from_millis((len / 2).min(240_000)),
        _ => heard >= Duration::from_secs(30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_lastfm_rules() {
        // Sorted keys, "format" left out, secret at the end.
        let mut p = BTreeMap::new();
        p.insert("method".to_string(), "auth.getSession".to_string());
        p.insert("api_key".to_string(), "k".to_string());
        p.insert("token".to_string(), "t".to_string());
        p.insert("format".to_string(), "json".to_string());
        let want = format!("{:x}", md5::compute(format!("api_keykmethodauth.getSessiontokent{SECRET}")));
        assert_eq!(sign(&p), want);
    }

    #[test]
    fn scrobble_rule() {
        let s = Duration::from_secs;
        assert!(!counts(Some(25_000), s(25)));
        assert!(counts(Some(200_000), s(100)));
        assert!(!counts(Some(200_000), s(99)));
        assert!(counts(Some(900_000), s(240)));
        assert!(counts(None, s(30)));
        assert!(!counts(None, s(29)));
    }
}
