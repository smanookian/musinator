//! Read tags and audio properties from one file with lofty.

use std::path::Path;

use anyhow::Result;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey, Tag};

/// Everything the scanner stores for one file.
#[derive(Clone, Debug, Default)]
pub struct TrackRow {
    pub path: String,
    pub folder: String,
    pub mtime: i64,
    pub size: i64,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub year: Option<i32>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub genre: String,
    pub duration_ms: u64,
    pub codec: String,
    pub sample_rate: u32,
    pub bits: Option<u32>,
    pub channels: u16,
    pub bitrate: Option<u32>,
    pub rg_track: Option<f32>,
    pub rg_track_peak: Option<f32>,
    pub rg_album: Option<f32>,
    pub rg_album_peak: Option<f32>,
    pub embedded_art: bool,
    pub embedded_lyrics: bool,
}

/// Video files count too: Musinator plays their sound, and `V` opens them in mpv.
pub const AUDIO_EXT: &[&str] =
    &["flac", "mp3", "m4a", "mp4", "m4b", "m4v", "mov", "mkv", "mka", "webm", "aac", "ogg", "oga", "opus", "wav", "wave", "aif", "aiff"];

pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

/// Parse "-6.20 dB" style values.
fn db(s: &str) -> Option<f32> {
    s.trim().trim_end_matches("dB").trim_end_matches("db").trim().replace(',', ".").parse().ok()
}

pub fn read(path: &Path, mtime: i64, size: i64) -> Result<TrackRow> {
    let f = match lofty::read_from_path(path) {
        Ok(f) => f,
        // lofty can't read Matroska/WebM or QuickTime (.mov): take the format from the decoder.
        Err(e) => return from_decoder(path, mtime, size).map_err(|_| e.into()),
    };
    let p = f.properties();
    let ft = f.file_type();
    let codec = match ft {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        FileType::Opus => "Opus",
        FileType::Vorbis => "Vorbis",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Mp4 => {
            match lofty::mp4::Mp4File::read_from(&mut std::fs::File::open(path)?, Default::default())
                .ok()
                .and_then(|m| m.properties().codec())
            {
                Some(lofty::mp4::Mp4Codec::ALAC) => "ALAC",
                Some(lofty::mp4::Mp4Codec::FLAC) => "FLAC",
                Some(lofty::mp4::Mp4Codec::MP3) => "MP3",
                _ => "AAC",
            }
        }
        _ => "PCM",
    }
    .to_string();
    let lossless = matches!(codec.as_str(), "FLAC" | "ALAC" | "PCM" | "WAV" | "AIFF");
    let mut r = TrackRow {
        path: path.to_string_lossy().into_owned(),
        folder: path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        mtime,
        size,
        duration_ms: p.duration().as_millis() as u64,
        sample_rate: if ft == FileType::Opus { 48_000 } else { p.sample_rate().unwrap_or(0) },
        bits: if lossless { p.bit_depth().map(u32::from) } else { None },
        channels: p.channels().unwrap_or(2) as u16,
        bitrate: p.audio_bitrate(),
        codec,
        ..Default::default()
    };
    let tag: Option<&Tag> = f.primary_tag().or_else(|| f.first_tag());
    if let Some(t) = tag {
        r.title = t.title().map(|s| s.into_owned()).unwrap_or_default();
        r.artist = t.artist().map(|s| s.into_owned()).unwrap_or_default();
        r.album = t.album().map(|s| s.into_owned()).unwrap_or_default();
        r.genre = t.genre().map(|s| s.into_owned()).unwrap_or_default();
        r.album_artist = t.get_string(ItemKey::AlbumArtist).unwrap_or_default().to_string();
        r.track_no = t.track();
        r.track_total = t.track_total();
        r.disc_no = t.disk();
        r.year = t.date().map(|d| d.year as i32).or_else(|| t.get_string(ItemKey::Year).and_then(|y| y.get(..4)?.parse().ok()));
        r.rg_track = t.get_string(ItemKey::ReplayGainTrackGain).and_then(db);
        r.rg_track_peak = t.get_string(ItemKey::ReplayGainTrackPeak).and_then(db);
        r.rg_album = t.get_string(ItemKey::ReplayGainAlbumGain).and_then(db);
        r.rg_album_peak = t.get_string(ItemKey::ReplayGainAlbumPeak).and_then(db);
        // Opus R128: Q7.8 dB relative to -23 LUFS; ReplayGain 2 uses -18 LUFS (+5 dB).
        let r128 = |k| t.get_string(k).and_then(|s: &str| s.trim().parse::<i32>().ok()).map(|v| v as f32 / 256.0 + 5.0);
        if r.rg_track.is_none() {
            r.rg_track = r128(ItemKey::R128TrackGain);
        }
        if r.rg_album.is_none() {
            r.rg_album = r128(ItemKey::R128AlbumGain);
        }
        r.embedded_art = !t.pictures().is_empty();
        r.embedded_lyrics =
            t.get_string(ItemKey::Lyrics).or_else(|| t.get_string(ItemKey::UnsyncLyrics)).is_some_and(|l| !l.trim().is_empty());
    }
    Ok(r)
}

/// Format and length from the decoder, for files without tags lofty can read.
/// The title is the file name.
fn from_decoder(path: &Path, mtime: i64, size: i64) -> Result<TrackRow> {
    use crate::source::Stream;
    let s = crate::decode::FileStream::open(path)?;
    let info = s.info();
    Ok(TrackRow {
        path: path.to_string_lossy().into_owned(),
        folder: path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        title: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        mtime,
        size,
        duration_ms: info.duration().map(|d| d.as_millis() as u64).unwrap_or(0),
        sample_rate: info.format.sample_rate,
        bits: info.format.bits,
        channels: info.format.channels,
        bitrate: info.format.bitrate_kbps,
        codec: info.format.codec.clone(),
        ..Default::default()
    })
}

/// Embedded front cover bytes.
pub fn embedded_art(path: &Path) -> Option<Vec<u8>> {
    let f = lofty::read_from_path(path).ok()?;
    let mut best: Option<Vec<u8>> = None;
    for t in f.tags() {
        for p in t.pictures() {
            if p.pic_type() == lofty::picture::PictureType::CoverFront {
                return Some(p.data().to_vec());
            }
            if best.is_none() {
                best = Some(p.data().to_vec());
            }
        }
    }
    best
}

/// Embedded lyrics text.
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    let f = lofty::read_from_path(path).ok()?;
    f.tags().iter().find_map(|t| t.get_string(ItemKey::Lyrics).or_else(|| t.get_string(ItemKey::UnsyncLyrics)).map(str::to_string))
}
