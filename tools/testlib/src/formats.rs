//! Audio formats used in the test library and how ffmpeg encodes them.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Fmt {
    Flac16,
    Flac24_96,
    Flac24_192,
    Mp3Cbr320,
    Mp3V0,
    Aac,
    Alac16,
    Alac24,
    Vorbis,
    Opus,
    Wav16,
    Wav24,
}

/// How tags are written for a format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    /// FLAC, Ogg Vorbis, Opus.
    Vorbis,
    /// MP3.
    Id3v2,
    /// M4A (AAC and ALAC).
    Mp4,
    /// WAV: ID3v2 chunk plus RIFF INFO.
    Wav,
}

impl Fmt {
    pub const ALL: [Fmt; 12] = [
        Fmt::Flac16,
        Fmt::Flac24_96,
        Fmt::Flac24_192,
        Fmt::Mp3Cbr320,
        Fmt::Mp3V0,
        Fmt::Aac,
        Fmt::Alac16,
        Fmt::Alac24,
        Fmt::Vorbis,
        Fmt::Opus,
        Fmt::Wav16,
        Fmt::Wav24,
    ];

    /// Short stable id, used for cache file names.
    pub fn id(self) -> &'static str {
        match self {
            Fmt::Flac16 => "flac16",
            Fmt::Flac24_96 => "flac24-96",
            Fmt::Flac24_192 => "flac24-192",
            Fmt::Mp3Cbr320 => "mp3-320",
            Fmt::Mp3V0 => "mp3-v0",
            Fmt::Aac => "aac",
            Fmt::Alac16 => "alac16",
            Fmt::Alac24 => "alac24",
            Fmt::Vorbis => "vorbis",
            Fmt::Opus => "opus",
            Fmt::Wav16 => "wav16",
            Fmt::Wav24 => "wav24",
        }
    }

    /// Human label, used in the summary and manifest.
    pub fn label(self) -> &'static str {
        match self {
            Fmt::Flac16 => "FLAC 16/44.1",
            Fmt::Flac24_96 => "FLAC 24/96",
            Fmt::Flac24_192 => "FLAC 24/192",
            Fmt::Mp3Cbr320 => "MP3 CBR 320",
            Fmt::Mp3V0 => "MP3 VBR V0",
            Fmt::Aac => "M4A AAC 192k",
            Fmt::Alac16 => "M4A ALAC 16/44.1",
            Fmt::Alac24 => "M4A ALAC 24/48",
            Fmt::Vorbis => "Ogg Vorbis q5",
            Fmt::Opus => "Opus 128k",
            Fmt::Wav16 => "WAV 16/44.1",
            Fmt::Wav24 => "WAV 24/48",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Flac16 | Fmt::Flac24_96 | Fmt::Flac24_192 => "flac",
            Fmt::Mp3Cbr320 | Fmt::Mp3V0 => "mp3",
            Fmt::Aac | Fmt::Alac16 | Fmt::Alac24 => "m4a",
            Fmt::Vorbis => "ogg",
            Fmt::Opus => "opus",
            Fmt::Wav16 | Fmt::Wav24 => "wav",
        }
    }

    pub fn rate(self) -> u32 {
        match self {
            Fmt::Flac24_96 => 96_000,
            Fmt::Flac24_192 => 192_000,
            Fmt::Alac24 | Fmt::Opus | Fmt::Wav24 => 48_000,
            _ => 44_100,
        }
    }

    pub fn tag_kind(self) -> TagKind {
        match self {
            Fmt::Flac16 | Fmt::Flac24_96 | Fmt::Flac24_192 | Fmt::Vorbis | Fmt::Opus => {
                TagKind::Vorbis
            }
            Fmt::Mp3Cbr320 | Fmt::Mp3V0 => TagKind::Id3v2,
            Fmt::Aac | Fmt::Alac16 | Fmt::Alac24 => TagKind::Mp4,
            Fmt::Wav16 | Fmt::Wav24 => TagKind::Wav,
        }
    }

    /// The ffmpeg encoder this format needs.
    pub fn encoder(self) -> &'static str {
        match self {
            Fmt::Flac16 | Fmt::Flac24_96 | Fmt::Flac24_192 => "flac",
            Fmt::Mp3Cbr320 | Fmt::Mp3V0 => "libmp3lame",
            Fmt::Aac => "aac",
            Fmt::Alac16 | Fmt::Alac24 => "alac",
            Fmt::Vorbis => "libvorbis",
            Fmt::Opus => "libopus",
            Fmt::Wav16 => "pcm_s16le",
            Fmt::Wav24 => "pcm_s24le",
        }
    }

    /// ffmpeg output options (codec, quality, muxer).
    pub fn ffmpeg_args(self) -> &'static [&'static str] {
        match self {
            Fmt::Flac16 => &[
                "-c:a",
                "flac",
                "-sample_fmt",
                "s16",
                "-compression_level",
                "8",
                "-f",
                "flac",
            ],
            Fmt::Flac24_96 | Fmt::Flac24_192 => &[
                "-c:a",
                "flac",
                "-sample_fmt",
                "s32",
                "-compression_level",
                "8",
                "-f",
                "flac",
            ],
            Fmt::Mp3Cbr320 => &["-c:a", "libmp3lame", "-b:a", "320k", "-f", "mp3"],
            Fmt::Mp3V0 => &["-c:a", "libmp3lame", "-q:a", "0", "-f", "mp3"],
            Fmt::Aac => &["-c:a", "aac", "-b:a", "192k", "-f", "ipod"],
            Fmt::Alac16 => &["-c:a", "alac", "-sample_fmt", "s16p", "-f", "ipod"],
            Fmt::Alac24 => &["-c:a", "alac", "-sample_fmt", "s32p", "-f", "ipod"],
            Fmt::Vorbis => &["-c:a", "libvorbis", "-q:a", "5", "-f", "ogg"],
            Fmt::Opus => &["-c:a", "libopus", "-b:a", "128k", "-f", "opus"],
            Fmt::Wav16 => &["-c:a", "pcm_s16le", "-f", "wav"],
            Fmt::Wav24 => &["-c:a", "pcm_s24le", "-f", "wav"],
        }
    }

    /// Relative weight when picking a format for a library album.
    pub fn album_weight(self) -> u32 {
        match self {
            Fmt::Flac16 => 30,
            Fmt::Flac24_96 => 5,
            Fmt::Flac24_192 => 1,
            Fmt::Mp3Cbr320 => 11,
            Fmt::Mp3V0 => 14,
            Fmt::Aac => 12,
            Fmt::Alac16 => 5,
            Fmt::Alac24 => 2,
            Fmt::Vorbis => 6,
            Fmt::Opus => 6,
            Fmt::Wav16 => 1,
            Fmt::Wav24 => 1,
        }
    }
}
