//! Look up lyrics on LRCLIB (https://lrclib.net), only when the user asks.
//! Sends artist, title, album and duration. Results are cached by Musinator.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use crate::model::Track;

const UA: &str = concat!("Musinator/", env!("CARGO_PKG_VERSION"), " (https://github.com/smanookian/musinator)");

/// Cached lyrics file for a song (keyed by its path).
pub fn cache_file(song: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    song.hash(&mut h);
    crate::config::cache_dir().join("lyrics").join(format!("{:016x}.lrc", h.finish()))
}

fn get(url: &str, query: &[(&str, String)]) -> Result<Option<Value>> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build().into();
    let mut req = agent.get(url).header("User-Agent", UA);
    for (k, v) in query {
        req = req.query(*k, v);
    }
    let mut resp = req.call()?;
    if resp.status() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("lrclib.net answered {}", resp.status());
    }
    Ok(Some(serde_json::from_str(&resp.body_mut().read_to_string()?)?))
}

fn text_of(v: &Value) -> Option<String> {
    v["syncedLyrics"].as_str().or(v["plainLyrics"].as_str()).filter(|s| !s.trim().is_empty()).map(str::to_string)
}

/// Synced lyrics if available, else plain. None if LRCLIB has nothing.
pub fn fetch(t: &Track) -> Result<Option<String>> {
    if t.title.is_empty() || t.artist.is_empty() {
        anyhow::bail!("this song has no title or artist tag");
    }
    let mut q = vec![("artist_name", t.artist.clone()), ("track_name", t.title.clone())];
    if !t.album.is_empty() {
        q.push(("album_name", t.album.clone()));
    }
    if let Some(d) = t.duration_ms {
        q.push(("duration", (d / 1000).to_string()));
    }
    if let Some(v) = get("https://lrclib.net/api/get", &q)?
        && let Some(s) = text_of(&v)
    {
        return Ok(Some(s));
    }
    // Looser search: best result, synced first.
    let q = [("artist_name", t.artist.clone()), ("track_name", t.title.clone())];
    let Some(Value::Array(hits)) = get("https://lrclib.net/api/search", &q)? else { return Ok(None) };
    let best = hits.iter().find(|h| h["syncedLyrics"].is_string()).or(hits.first());
    Ok(best.and_then(text_of))
}

/// Store found lyrics in the cache, and next to the song if asked (never overwrites).
pub fn store(song: &str, text: &str, next_to_song: bool) -> Result<()> {
    let c = cache_file(song);
    std::fs::create_dir_all(c.parent().unwrap())?;
    std::fs::write(&c, text)?;
    if next_to_song {
        let side = Path::new(song).with_extension("lrc");
        if !side.exists() {
            std::fs::write(side, text)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs internet; run with `cargo test -- --ignored lrclib`.
    #[test]
    #[ignore]
    fn finds_a_known_song() {
        let t = Track {
            title: "One More Time".into(),
            artist: "Daft Punk".into(),
            album: "Discovery".into(),
            duration_ms: Some(320_000),
            ..Default::default()
        };
        let text = fetch(&t).unwrap().expect("lyrics");
        assert!(text.contains("One more time"));
    }
}
