//! Special albums: `Musinator Demo` (long tracks), `Gapless Test` and `Loudness Test`.

use crate::cover;
use crate::formats::Fmt;
use crate::library::{Stats, album_dir, copy_and_tag, cover_mime, gain_tags, sanitize};
use crate::masters::{self, DEMO_ALBUMS, DEMO_TRACKS, LOUDNESS_FMT, LOUDNESS_TRACKS, Masters};
use crate::names;
use crate::rng::Rng;
use crate::tags::{GainTags, TrackTags};
use anyhow::Result;
use serde_json::json;
use std::path::Path;

const TEST_YEAR: u32 = 2026;

fn plain_tags<'a>(title: &'a str, artist: &'a str, album: &'a str, track: u32, total: u32) -> TrackTags<'a> {
    TrackTags {
        title,
        artist,
        album_artist: artist,
        album,
        genre: "Test",
        year: TEST_YEAR,
        track,
        track_total: total,
        disc: 1,
        disc_total: 1,
        compilation: false,
        cover: None,
        lyrics: None,
        gain: GainTags::None,
    }
}

/// Two albums of long, musical tracks with cover, lyrics and ReplayGain.
pub fn write_demo(root: &Path, m: &Masters) -> Result<Stats> {
    let artist = "Musinator Demo";
    let mut stats = Stats::default();
    for (a, &(album, year, fmt, genre)) in DEMO_ALBUMS.iter().enumerate() {
        let mut rng = Rng::new(9000 + a as u64);
        let dir = album_dir(root, artist, year, album);
        std::fs::create_dir_all(&dir)?;
        let art = cover::make(9100 + a as u64, 600, false)?;
        std::fs::write(dir.join("cover.jpg"), &art)?;
        let louds: Vec<_> = m.demo[a].iter().map(|t| (t.dur, t.loud)).collect();
        let gains = gain_tags(fmt, &louds);
        let mut seen = std::collections::HashSet::new();
        for (t, gain) in gains.into_iter().enumerate() {
            let title = loop {
                let candidate = names::track_title(&mut rng);
                if candidate.len() < 40 && seen.insert(candidate.clone()) {
                    break candidate;
                }
            };
            let stem = sanitize(&format!("{:02} - {title}", t + 1));
            let lrc = names::lrc(&mut rng, &title, artist, album, m.demo[a][t].dur, (3.5, 6.5));
            let tags = TrackTags {
                genre,
                year,
                cover: Some((&art, cover_mime(false))),
                gain,
                ..plain_tags(&title, artist, album, t as u32 + 1, DEMO_TRACKS as u32)
            };
            copy_and_tag(&m.demo_path(a, t), &dir.join(format!("{stem}.{}", fmt.ext())), fmt, &tags)?;
            std::fs::write(dir.join(format!("{stem}.lrc")), lrc)?;
            stats.lrc += 1;
            *stats.formats.entry(fmt).or_default() += 1;
        }
        stats.albums += 1;
        stats.cover_embedded += 1;
        stats.replaygain_albums += 1;
    }
    Ok(stats)
}

/// One album per codec: a continuous sine cut into pieces, plus `gapless.json`.
pub fn write_gapless(root: &Path, m: &Masters) -> Result<Stats> {
    let artist = "Gapless Test";
    let mut stats = Stats::default();
    let mut albums_json = Vec::new();
    for g in &m.gapless {
        let album = format!("Gapless {}", g.name);
        let dir = album_dir(root, artist, TEST_YEAR, &album);
        std::fs::create_dir_all(&dir)?;
        let total = g.pieces.len() as u32;
        let mut start = 0;
        let mut tracks_json = Vec::new();
        for (i, &samples) in g.pieces.iter().enumerate() {
            let title = format!("Sine Part {}", i + 1);
            let file = format!("{:02} - {title}.{}", i + 1, g.fmt.ext());
            let tags = plain_tags(&title, artist, &album, i as u32 + 1, total);
            copy_and_tag(&m.gapless_path(g.fmt, i), &dir.join(&file), g.fmt, &tags)?;
            *stats.formats.entry(g.fmt).or_default() += 1;
            tracks_json.push(json!({
                "file": dir.strip_prefix(root.join(artist))?.join(&file),
                "start_sample": start,
                "samples": samples,
            }));
            start += samples;
        }
        stats.albums += 1;
        stats.cover_none += 1;
        albums_json.push(json!({
            "album": album,
            "format": g.fmt.label(),
            "sample_rate": g.rate,
            "channels": 2,
            "lossless": matches!(g.fmt, Fmt::Flac16 | Fmt::Alac16 | Fmt::Wav16),
            "total_samples": g.total,
            "tracks": tracks_json,
        }));
    }
    let doc = json!({
        "about": "One continuous sine per album, cut into tracks at exact sample positions. \
                  Each track was encoded on its own, so encoder delay and padding are present. \
                  Gapless playback must give back exactly total_samples samples with no jump in phase. \
                  Lossless albums decode bit-exact; lossy ones have small codec errors near the cuts.",
        "frequency_hz": masters::GAPLESS_FREQ,
        "amplitude_dbfs": masters::GAPLESS_DBFS,
        "amplitude_linear": 10f64.powf(masters::GAPLESS_DBFS / 20.0),
        "duration_s": masters::GAPLESS_SECONDS,
        "sample_formula": "round(10^(amplitude_dbfs/20) * 32767 * sin(2*pi*frequency_hz*n/sample_rate)) / 32768, same on both channels, n from 0",
        "albums": albums_json,
    });
    std::fs::write(root.join(artist).join("gapless.json"), serde_json::to_string_pretty(&doc)?)?;
    Ok(stats)
}

/// Tone and pink noise tracks at known loudness, no gain tags, plus `loudness.json`.
pub fn write_loudness(root: &Path, m: &Masters) -> Result<Stats> {
    let artist = "Loudness Test";
    let album = "Loudness Test";
    let dir = album_dir(root, artist, TEST_YEAR, album);
    std::fs::create_dir_all(&dir)?;
    let mut stats = Stats { albums: 1, cover_none: 1, ..Stats::default() };
    let mut tracks_json = Vec::new();
    let total = LOUDNESS_TRACKS.len() as u32;
    for (i, &(signal, target)) in LOUDNESS_TRACKS.iter().enumerate() {
        let title = format!("{} at {target} LUFS", capitalize(signal.name()));
        let file = format!("{:02} - {title}.{}", i + 1, LOUDNESS_FMT.ext());
        copy_and_tag(&m.loudness_path(i), &dir.join(&file), LOUDNESS_FMT, &plain_tags(&title, artist, album, i as u32 + 1, total))?;
        *stats.formats.entry(LOUDNESS_FMT).or_default() += 1;
        let measured = m.loudness[i];
        tracks_json.push(json!({
            "file": dir.strip_prefix(root.join(artist))?.join(&file),
            "signal": signal.name(),
            "target_lufs": target,
            "measured_lufs": (measured.lufs * 100.0).round() / 100.0,
            "sample_peak_dbfs": (measured.peak_db * 100.0).round() / 100.0,
            "duration_s": masters::LOUDNESS_SECONDS,
        }));
    }
    let doc = json!({
        "about": "Integrated loudness (EBU R128) of each track. measured_lufs comes from ffmpeg's ebur128 filter on the encoded file. These files have no ReplayGain tags.",
        "format": LOUDNESS_FMT.label(),
        "tracks": tracks_json,
    });
    std::fs::write(root.join(artist).join("loudness.json"), serde_json::to_string_pretty(&doc)?)?;
    Ok(stats)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
