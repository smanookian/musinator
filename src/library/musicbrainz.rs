//! Look up album tags on MusicBrainz and the cover on the Cover Art Archive,
//! only when the user asks. Sends artist and album names.
//! MusicBrainz rules: a real User-Agent, at most one request per second.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde_json::Value;

const UA: &str = concat!("Musinator/", env!("CARGO_PKG_VERSION"), " (https://github.com/smanookian/musinator)");
const API: &str = "https://musicbrainz.org/ws/2";

/// One search hit.
#[derive(Clone, Debug)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub country: String,
    pub tracks: u32,
    pub format: String,
}

/// One song of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub disc: u32,
    pub no: u32,
    pub title: String,
    pub artist: String,
}

#[derive(Clone, Debug)]
pub struct Full {
    pub title: String,
    pub artist: String,
    pub year: String,
    pub songs: Vec<Song>,
    pub discs: u32,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into()
}

/// Wait so calls are at least 1.1 s apart.
fn pace() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        let gap = Duration::from_millis(1100);
        if t.elapsed() < gap {
            std::thread::sleep(gap - t.elapsed());
        }
    }
    *last = Some(Instant::now());
}

fn get(url: &str, query: &[(&str, String)]) -> Result<Value> {
    for wait in [0, 2, 4] {
        std::thread::sleep(Duration::from_secs(wait));
        pace();
        let mut req = agent().get(url).header("User-Agent", UA).header("Accept", "application/json");
        for (k, v) in query {
            req = req.query(*k, v);
        }
        let mut resp = req.call()?;
        match resp.status().as_u16() {
            200 => return Ok(serde_json::from_str(&resp.body_mut().read_to_string()?)?),
            503 | 429 | 502 => continue,
            s => bail!("MusicBrainz answered {s}"),
        }
    }
    bail!("MusicBrainz is busy, try again in a moment")
}

fn credit(v: &Value) -> String {
    v["artist-credit"]
        .as_array()
        .map(|a| a.iter().map(|c| format!("{}{}", c["name"].as_str().unwrap_or(""), c["joinphrase"].as_str().unwrap_or(""))).collect())
        .unwrap_or_default()
}

/// Lucene phrase: quote it, escape quotes and backslashes.
fn phrase(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn search(artist: &str, album: &str) -> Result<Vec<Release>> {
    let mut q = Vec::new();
    if !album.trim().is_empty() {
        q.push(format!("release:{}", phrase(album.trim())));
    }
    if !artist.trim().is_empty() {
        q.push(format!("artist:{}", phrase(artist.trim())));
    }
    if q.is_empty() {
        bail!("Fill in album or artist first");
    }
    let mut v = get(&format!("{API}/release"), &[("query", q.join(" AND ")), ("fmt", "json".into()), ("limit", "15".into())])?;
    // Nothing with the exact spelling ("und" vs "&", typos): search loosely.
    if v["releases"].as_array().is_none_or(|a| a.is_empty()) {
        let words = |s: &str| {
            s.split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 2 && !["und", "and", "the", "der", "die", "das"].contains(&w.to_lowercase().as_str()))
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
        };
        let mut loose = Vec::new();
        if !album.trim().is_empty() {
            loose.push(format!("release:({})", words(album).join(" ")));
        }
        if !artist.trim().is_empty() {
            loose.push(format!("artist:({})", words(artist).join(" ")));
        }
        v = get(&format!("{API}/release"), &[("query", loose.join(" AND ")), ("fmt", "json".into()), ("limit", "15".into())])?;
    }
    Ok(v["releases"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| Release {
                    id: r["id"].as_str().unwrap_or("").into(),
                    title: r["title"].as_str().unwrap_or("").into(),
                    artist: credit(r),
                    date: r["date"].as_str().unwrap_or("").into(),
                    country: r["country"].as_str().unwrap_or("").into(),
                    tracks: r["track-count"].as_u64().unwrap_or(0) as u32,
                    format: r["media"]
                        .as_array()
                        .map(|m| {
                            let f: Vec<&str> = m.iter().filter_map(|x| x["format"].as_str()).collect();
                            if f.len() > 1 && f.iter().all(|x| *x == f[0]) { format!("{}×{}", f.len(), f[0]) } else { f.join("+") }
                        })
                        .unwrap_or_default(),
                })
                .filter(|r| !r.id.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

pub fn release(id: &str) -> Result<Full> {
    let v = get(&format!("{API}/release/{id}"), &[("inc", "recordings+artist-credits".into()), ("fmt", "json".into())])?;
    let artist = credit(&v);
    let mut songs = Vec::new();
    let media = v["media"].as_array().cloned().unwrap_or_default();
    for (i, m) in media.iter().enumerate() {
        let disc = m["position"].as_u64().unwrap_or(i as u64 + 1) as u32;
        for (k, t) in m["tracks"].as_array().into_iter().flatten().enumerate() {
            let a = credit(t);
            songs.push(Song {
                disc,
                no: t["position"].as_u64().unwrap_or(k as u64 + 1) as u32,
                title: t["title"].as_str().unwrap_or("").into(),
                artist: if a.is_empty() { artist.clone() } else { a },
            });
        }
    }
    Ok(Full {
        title: v["title"].as_str().unwrap_or("").into(),
        year: v["date"].as_str().unwrap_or("").chars().take(4).collect(),
        artist,
        discs: media.len() as u32,
        songs,
    })
}

/// Front cover (500 px) from the Cover Art Archive. None if it has none.
pub fn cover(id: &str) -> Result<Option<Vec<u8>>> {
    let mut resp = agent().get(&format!("https://coverartarchive.org/release/{id}/front-500")).header("User-Agent", UA).call()?;
    match resp.status().as_u16() {
        200 => Ok(Some(resp.body_mut().with_config().limit(20 << 20).read_to_vec()?)),
        404 => Ok(None),
        s => bail!("Cover Art Archive answered {s}"),
    }
}

/// Which release song each file is. Files are (disc, track) from their tags
/// (0 = unknown). Uses disc + track numbers when every file has a track
/// number; otherwise the file order, if the counts match.
pub fn match_songs(files: &[(u32, u32)], songs: &[Song]) -> Vec<Option<usize>> {
    if !files.is_empty() && files.iter().all(|f| f.1 > 0) {
        let multi = songs.iter().any(|s| s.disc > 1);
        return files
            .iter()
            .map(|&(d, n)| {
                let d = if d == 0 && !multi { 1 } else { d.max(1) };
                songs.iter().position(|s| s.disc == d && s.no == n)
            })
            .collect();
    }
    if files.len() == songs.len() { (0..files.len()).map(Some).collect() } else { vec![None; files.len()] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(disc: u32, no: u32) -> Song {
        Song { disc, no, title: format!("{disc}-{no}"), artist: String::new() }
    }

    #[test]
    fn matches_by_number_then_by_order() {
        let songs = [s(1, 1), s(1, 2), s(2, 1), s(2, 2)];
        // Disc + track numbers, in any file order.
        assert_eq!(match_songs(&[(2, 1), (1, 2)], &songs), vec![Some(2), Some(1)]);
        // A track that is not on the release.
        assert_eq!(match_songs(&[(1, 9)], &songs), vec![None]);
        // No numbers: file order, only if the counts match.
        assert_eq!(match_songs(&[(0, 0); 4], &songs), vec![Some(0), Some(1), Some(2), Some(3)]);
        assert_eq!(match_songs(&[(0, 0); 3], &songs), vec![None; 3]);
        // Single-disc release, files without a disc number.
        assert_eq!(match_songs(&[(0, 2)], &[s(1, 1), s(1, 2)]), vec![Some(1)]);
    }

    #[test]
    fn quotes_search_phrases() {
        assert_eq!(phrase(r#"Say "Hi""#), r#""Say \"Hi\"""#);
    }
}
