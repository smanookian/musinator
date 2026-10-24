//! Core data types shared by sources, the engine, the library and the UI.

use std::fmt;
use std::sync::Arc;

/// Which source a track comes from: "local" in v1; later e.g. "subsonic:home", "spotify".
pub type SourceId = Arc<str>;

/// A stable reference to one track inside one source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackRef {
    pub source: SourceId,
    /// Source-specific id. Local: absolute file path.
    pub id: String,
}

impl TrackRef {
    #[cfg(test)]
    pub fn new(source: &SourceId, id: impl Into<String>) -> Self {
        Self { source: source.clone(), id: id.into() }
    }
}

/// The real audio format of a track, as decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioFormat {
    /// Short codec name: "FLAC", "MP3", "AAC", "ALAC", "Vorbis", "Opus", "PCM".
    pub codec: String,
    pub sample_rate: u32,
    /// Bits per sample for lossless/PCM codecs. None for lossy codecs.
    pub bits: Option<u32>,
    pub channels: u16,
    /// Average bitrate in kbit/s, when known.
    pub bitrate_kbps: Option<u32>,
}

impl AudioFormat {
    pub fn lossless(&self) -> bool {
        matches!(self.codec.as_str(), "FLAC" | "ALAC" | "PCM" | "WAV" | "AIFF")
    }

    /// "FLAC 24/96", "MP3 320k", "Opus 128k", "AAC 44.1k".
    pub fn short(&self) -> String {
        let khz = khz(self.sample_rate);
        match (self.bits, self.bitrate_kbps) {
            (Some(b), _) if self.lossless() => format!("{} {}/{}", self.codec, b, khz),
            (_, Some(kbps)) => format!("{} {}k", self.codec, kbps),
            _ => format!("{} {}k", self.codec, khz),
        }
    }
}

/// 44100 -> "44.1", 96000 -> "96".
pub fn khz(rate: u32) -> String {
    if rate.is_multiple_of(1000) { format!("{}", rate / 1000) } else { format!("{:.1}", rate as f64 / 1000.0) }
}

/// Where a gain value came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GainOrigin {
    #[default]
    None,
    /// REPLAYGAIN_* or R128_* tags.
    Tags,
    /// Measured by Musinator (EBU R128).
    Measured,
}

/// Loudness info for a track, normalised to the ReplayGain 2 reference (-18 LUFS).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gain {
    pub track_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_db: Option<f32>,
    pub album_peak: Option<f32>,
    pub origin: GainOrigin,
}

/// A link to cover art that a source can load.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArtRef {
    pub source: SourceId,
    /// Source-specific. Local: "embedded:<path>" or "file:<path>".
    pub id: String,
}

/// Full metadata for one track.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub r: Option<TrackRef>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: Option<i32>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub duration_ms: Option<u64>,
    pub format: Option<AudioFormat>,
    pub gain: Gain,
    pub art: Option<ArtRef>,
}

impl Track {
    pub fn tref(&self) -> &TrackRef {
        self.r.as_ref().expect("track without ref")
    }
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() { self.r.as_ref().map(|r| file_stem(&r.id)).unwrap_or("Unknown") } else { &self.title }
    }
    pub fn display_artist(&self) -> &str {
        if self.artist.is_empty() { "Unknown artist" } else { &self.artist }
    }
}

fn file_stem(p: &str) -> &str {
    let name = p.rsplit('/').next().unwrap_or(p);
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

/// Format milliseconds as m:ss or h:mm:ss.
pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Repeat mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
}

impl fmt::Display for Repeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
