//! Writes tags into encoded files with lofty.

use crate::formats::{Fmt, TagKind};
use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::id3::v2::{Frame, Id3v2Tag};
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct ReplayGain {
    pub track_gain: f64,
    pub track_peak: f64,
    pub album_gain: f64,
    pub album_peak: f64,
}

#[derive(Clone, Copy, Debug)]
pub enum GainTags {
    None,
    ReplayGain(ReplayGain),
    /// Opus R128 gains: Q7.8 integers relative to -23 LUFS.
    R128 {
        track: i32,
        album: i32,
    },
}

pub struct TrackTags<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album_artist: &'a str,
    pub album: &'a str,
    pub genre: &'a str,
    pub year: u32,
    pub track: u32,
    pub track_total: u32,
    pub disc: u32,
    pub disc_total: u32,
    pub compilation: bool,
    pub cover: Option<(&'a [u8], MimeType)>,
    /// LRC text stored in USLT (MP3/WAV), LYRICS (Vorbis) or ©lyr (M4A).
    pub lyrics: Option<&'a str>,
    pub gain: GainTags,
}

pub fn write(path: &Path, fmt: Fmt, t: &TrackTags) -> Result<()> {
    let kind = fmt.tag_kind();
    let tag_type = match kind {
        TagKind::Vorbis => TagType::VorbisComments,
        TagKind::Id3v2 | TagKind::Wav => TagType::Id3v2,
        TagKind::Mp4 => TagType::Mp4Ilst,
    };
    let mut tag = Tag::new(tag_type);
    tag.set_title(t.title.to_string());
    tag.set_artist(t.artist.to_string());
    tag.set_album(t.album.to_string());
    tag.set_genre(t.genre.to_string());
    tag.insert_text(ItemKey::AlbumArtist, t.album_artist.to_string());
    tag.insert_text(ItemKey::RecordingDate, t.year.to_string());
    tag.set_track(t.track);
    tag.set_track_total(t.track_total);
    tag.set_disk(t.disc);
    tag.set_disk_total(t.disc_total);
    if t.compilation {
        tag.insert_text(ItemKey::FlagCompilation, "1".to_string());
    }
    if let Some((data, mime)) = &t.cover {
        tag.push_picture(
            Picture::unchecked(data.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(mime.clone())
                .build(),
        );
    }
    if let Some(lyrics) = t.lyrics {
        let key = if tag_type == TagType::Id3v2 {
            ItemKey::UnsyncLyrics
        } else {
            ItemKey::Lyrics
        };
        tag.insert_text(key, lyrics.to_string());
    }
    if let GainTags::ReplayGain(rg) = t.gain {
        tag.insert_text(
            ItemKey::ReplayGainTrackGain,
            format!("{:+.2} dB", rg.track_gain),
        );
        tag.insert_text(
            ItemKey::ReplayGainTrackPeak,
            format!("{:.6}", rg.track_peak),
        );
        tag.insert_text(
            ItemKey::ReplayGainAlbumGain,
            format!("{:+.2} dB", rg.album_gain),
        );
        tag.insert_text(
            ItemKey::ReplayGainAlbumPeak,
            format!("{:.6}", rg.album_peak),
        );
    }

    let opts = WriteOptions::default();
    let ctx = || format!("write tags to {}", path.display());
    match kind {
        TagKind::Vorbis => {
            let mut vc = VorbisComments::from(tag);
            if let GainTags::R128 { track, album } = t.gain {
                vc.insert("R128_TRACK_GAIN".to_string(), track.to_string());
                vc.insert("R128_ALBUM_GAIN".to_string(), album.to_string());
            }
            vc.save_to_path(path, opts).with_context(ctx)?;
        }
        TagKind::Wav => {
            sorted_id3v2(tag)
                .save_to_path(path, opts)
                .with_context(ctx)?;
            let mut info = Tag::new(TagType::RiffInfo);
            info.set_title(t.title.to_string());
            info.set_artist(t.artist.to_string());
            info.set_album(t.album.to_string());
            info.set_genre(t.genre.to_string());
            info.set_track(t.track);
            info.insert_text(ItemKey::RecordingDate, t.year.to_string());
            info.save_to_path(path, opts).with_context(ctx)?;
        }
        TagKind::Id3v2 => sorted_id3v2(tag)
            .save_to_path(path, opts)
            .with_context(ctx)?,
        TagKind::Mp4 => tag.save_to_path(path, opts).with_context(ctx)?,
    }
    Ok(())
}

/// lofty builds ID3v2 frames through a HashMap, so their order changes between runs.
/// Sorting them makes the same input give the same bytes.
fn sorted_id3v2(tag: Tag) -> Id3v2Tag {
    let mut frames: Vec<Frame<'static>> = Id3v2Tag::from(tag).into_iter().collect();
    frames.sort_by_cached_key(|f| {
        let desc = if let Frame::UserText(t) = f {
            t.description.to_string()
        } else {
            String::new()
        };
        (f.id_str().to_string(), desc)
    });
    let mut out = Id3v2Tag::new();
    for f in frames {
        out.insert(f);
    }
    out
}
