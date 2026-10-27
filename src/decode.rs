//! Decoding with symphonia (FLAC, MP3, AAC, ALAC, Vorbis, WAV) and libopus (Opus).
//!
//! Gapless:
//! - symphonia trims MP3 (LAME header) and Vorbis delay/padding itself.
//! - Opus: frames before timestamp 0 are the pre-skip; we drop them.
//! - MP4 (AAC): symphonia ignores the edit list, so we read it (or iTunSMPB)
//!   ourselves and drop the priming and padding frames.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use symphonia::core::codecs::audio::well_known::*;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use crate::model::AudioFormat;
use crate::source::{Stream, StreamInfo};

/// Largest Opus frame: 120 ms at 48 kHz.
const OPUS_MAX_FRAMES: usize = 5760;

enum Dec {
    Sym(Box<dyn AudioDecoder>),
    Opus { dec: opus::Decoder, channels: usize, gain: f32, buf: Vec<f32> },
}

pub struct FileStream {
    reader: Box<dyn FormatReader>,
    dec: Dec,
    track_id: u32,
    info: StreamInfo,
    /// Drop decoded frames before this timestamp (after a seek, and Opus pre-skip).
    skip_until: Option<Timestamp>,
    /// Track time base as (numer, denom).
    tb: (u32, u32),
    /// MP4 priming frames at the start of the raw stream.
    priming: u64,
    /// Priming frames still to drop.
    head_left: u64,
    /// Playable frames left (MP4 with a known length).
    remaining: Option<u64>,
}

pub fn codec_name(id: symphonia::core::codecs::audio::AudioCodecId) -> &'static str {
    match id {
        CODEC_ID_FLAC => "FLAC",
        CODEC_ID_MP3 => "MP3",
        CODEC_ID_MP2 => "MP2",
        CODEC_ID_MP1 => "MP1",
        CODEC_ID_AAC => "AAC",
        CODEC_ID_ALAC => "ALAC",
        CODEC_ID_VORBIS => "Vorbis",
        CODEC_ID_OPUS => "Opus",
        _ => "PCM",
    }
}

impl FileStream {
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("can't open {} (moved, deleted, or is the drive not plugged in?)", path.display()))?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let mp4 = matches!(ext.as_str(), "m4a" | "mp4" | "m4b" | "aac" | "alac");
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(&ext);
        let reader = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .with_context(|| format!("{} is not a format Musinator can play", path.display()))?;
        let gapless = if mp4 { mp4_gapless(path) } else { None };
        let mut s = Self::from_reader(reader, size, gapless)?;
        if s.info.format.codec == "PCM" {
            s.info.format.codec = match ext.as_str() {
                "wav" | "wave" => "WAV",
                "aif" | "aiff" => "AIFF",
                _ => "PCM",
            }
            .into();
        }
        Ok(s)
    }

    /// Decode from a stream that cannot seek (internet radio). `ext` is a format hint.
    pub fn open_stream(src: Box<dyn symphonia::core::io::MediaSource>, ext: &str) -> Result<Self> {
        let mss = MediaSourceStream::new(src, Default::default());
        let mut hint = Hint::new();
        hint.with_extension(ext);
        let reader = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .context("this audio format can't be played")?;
        Self::from_reader(reader, 0, None)
    }

    fn from_reader(reader: Box<dyn FormatReader>, size: u64, mp4: Option<Mp4Gapless>) -> Result<Self> {
        let track = reader.default_track(TrackType::Audio).ok_or_else(|| anyhow!("this file has no sound in it"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| anyhow!("this file looks damaged (its audio format can't be read)"))?
            .clone();
        let track_id = track.id;
        let mut frames = track.num_frames;
        let delay = track.delay.unwrap_or(0) as u64;
        let rate = if params.codec == CODEC_ID_OPUS { 48_000 } else { params.sample_rate.unwrap_or(0) };
        let tb = track.time_base.map(|t| (t.numer.get(), t.denom.get())).unwrap_or((1, rate.max(1)));
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(0);
        if rate == 0 || channels == 0 {
            bail!("this file looks damaged (its sample rate or channels can't be read)");
        }
        let codec = codec_name(params.codec);

        let skip_until = None;
        let mut opus_preskip = 0;
        let dec = if params.codec == CODEC_ID_OPUS {
            let ch = match channels {
                1 => opus::Channels::Mono,
                2 => opus::Channels::Stereo,
                n => bail!("Opus files with {n} channels can't be played yet (1 or 2 work)"),
            };
            // OpusHead output gain: bytes 16..18, Q7.8 dB.
            let gain_q78 = params.extra_data.as_ref().filter(|d| d.len() >= 18).map(|d| i16::from_le_bytes([d[16], d[17]])).unwrap_or(0);
            // symphonia reports the pre-skip as track delay; we drop it like MP4 priming.
            opus_preskip = delay;
            Dec::Opus {
                dec: opus::Decoder::new(48_000, ch)?,
                channels,
                gain: 10f32.powf(gain_q78 as f32 / 256.0 / 20.0),
                buf: vec![0.0; OPUS_MAX_FRAMES * channels],
            }
        } else {
            Dec::Sym(
                symphonia::default::get_codecs()
                    .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
                    .with_context(|| format!("Musinator can't play {codec} audio"))?,
            )
        };
        let (priming, remaining) = match mp4 {
            Some(g) => {
                frames = Some(g.frames);
                (g.priming, Some(g.frames))
            }
            None => (opus_preskip, None),
        };
        let lossless = matches!(codec, "FLAC" | "ALAC" | "PCM");
        let bits = if lossless { params.bits_per_sample } else { None };
        let bitrate_kbps = match frames {
            Some(f) if !lossless && f > 0 => Some(((size as f64 * 8.0) / (f as f64 / rate as f64) / 1000.0).round() as u32),
            _ => None,
        };
        let info = StreamInfo {
            format: AudioFormat { codec: codec.into(), sample_rate: rate, bits, channels: channels as u16, bitrate_kbps },
            frames,
        };
        Ok(Self { reader, dec, track_id, info, skip_until, tb, priming, head_left: priming, remaining })
    }

    fn ts_to_frames(&self, ts: i64) -> i64 {
        (ts as f64 * self.tb.0 as f64 * self.info.format.sample_rate as f64 / self.tb.1 as f64).round() as i64
    }

    fn frames_to_ts(&self, frames: u64) -> i64 {
        (frames as f64 * self.tb.1 as f64 / (self.tb.0 as f64 * self.info.format.sample_rate as f64)).round() as i64
    }
}

impl Stream for FileStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        if self.remaining == Some(0) {
            return Ok(false);
        }
        let channels = self.info.format.channels as usize;
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymErr::ResetRequired) => return Ok(false),
                Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(e) => return Err(e.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let pts = packet.pts;
            let start = out.len();
            match &mut self.dec {
                Dec::Sym(dec) => match dec.decode(&packet) {
                    Ok(buf) => {
                        let n = buf.samples_interleaved();
                        out.resize(start + n, 0.0);
                        buf.copy_to_slice_interleaved(&mut out[start..]);
                    }
                    Err(SymErr::DecodeError(_)) | Err(SymErr::IoError(_)) => continue,
                    Err(e) => return Err(e.into()),
                },
                Dec::Opus { dec, channels, gain, buf } => {
                    let n = match dec.decode_float(&packet.data, buf, false) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let ts = (packet.trim_start.get() as usize).min(n);
                    let te = (packet.trim_end.get() as usize).min(n - ts);
                    let s = &buf[ts * *channels..(n - te) * *channels];
                    if *gain != 1.0 {
                        out.extend(s.iter().map(|x| x * *gain));
                    } else {
                        out.extend_from_slice(s);
                    }
                }
            }
            let mut frames = (out.len() - start) / channels;
            // Drop frames before the target timestamp.
            if let Some(until) = self.skip_until {
                let skip = self.ts_to_frames(until.get() - pts.get());
                if skip <= 0 {
                    self.skip_until = None;
                } else {
                    let skip = (skip as usize).min(frames);
                    out.drain(start..start + skip * channels);
                    frames -= skip;
                    if frames > 0 {
                        self.skip_until = None;
                    }
                }
            }
            // MP4 priming.
            if self.head_left > 0 && frames > 0 {
                let skip = (self.head_left as usize).min(frames);
                out.drain(start..start + skip * channels);
                frames -= skip;
                self.head_left -= skip as u64;
            }
            // MP4 padding: stop at the real length.
            if let Some(rem) = self.remaining.as_mut() {
                let keep = (*rem as usize).min(frames);
                out.truncate(start + keep * channels);
                frames = keep;
                *rem -= keep as u64;
            }
            if frames > 0 {
                return Ok(true);
            }
            if self.remaining == Some(0) {
                return Ok(false);
            }
        }
    }

    fn seek(&mut self, to: Duration) -> Result<u64> {
        let rate = self.info.format.sample_rate as f64;
        let mut target = (to.as_secs_f64() * rate) as u64;
        if let Some(total) = self.info.frames {
            target = target.min(total);
        }
        let raw = self.frames_to_ts(target + self.priming);
        let seeked = self.reader.seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(raw), track_id: self.track_id })?;
        match &mut self.dec {
            Dec::Sym(d) => d.reset(),
            Dec::Opus { dec, .. } => dec.reset_state()?,
        }
        self.skip_until = Some(seeked.required_ts);
        self.head_left = 0;
        let pos = (self.ts_to_frames(seeked.required_ts.get()) - self.priming as i64).max(0) as u64;
        if let (Some(total), Some(rem)) = (self.info.frames, self.remaining.as_mut()) {
            *rem = total.saturating_sub(pos);
        }
        Ok(pos)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mp4Gapless {
    priming: u64,
    frames: u64,
}

/// Read priming and real length from an MP4 file: the audio track's edit list,
/// or the iTunSMPB tag that iTunes writes.
fn mp4_gapless(path: &Path) -> Option<Mp4Gapless> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let moov = find_top_box(&mut f, len, b"moov")?;
    let mvhd_ts = child(&moov, b"mvhd").and_then(timescale)?;
    let mut from_elst = None;
    for trak in children(&moov).filter(|(t, _)| t == b"trak").map(|(_, b)| b) {
        let Some(mdia) = child(trak, b"mdia") else { continue };
        let is_audio = child(mdia, b"hdlr").is_some_and(|h| h.len() >= 12 && &h[8..12] == b"soun");
        if !is_audio {
            continue;
        }
        let media_ts = child(mdia, b"mdhd").and_then(timescale)?;
        if let Some(elst) = child(trak, b"edts").and_then(|e| child(e, b"elst")) {
            from_elst = parse_elst(elst, mvhd_ts, media_ts);
        }
        break;
    }
    // iTunes files are exact; prefer iTunSMPB when present.
    itunsmpb(&moov).or(from_elst)
}

fn parse_elst(b: &[u8], movie_ts: u32, media_ts: u32) -> Option<Mp4Gapless> {
    let version = *b.first()?;
    let count = u32::from_be_bytes(b.get(4..8)?.try_into().ok()?);
    let mut p = 8;
    for _ in 0..count {
        let (dur, media_time) = if version == 1 {
            let d = u64::from_be_bytes(b.get(p..p + 8)?.try_into().ok()?);
            let m = i64::from_be_bytes(b.get(p + 8..p + 16)?.try_into().ok()?);
            p += 20;
            (d, m)
        } else {
            let d = u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?) as u64;
            let m = i32::from_be_bytes(b.get(p + 4..p + 8)?.try_into().ok()?) as i64;
            p += 12;
            (d, m)
        };
        if media_time < 0 {
            continue; // empty edit
        }
        // Older ffmpeg writes the movie timescale in ms, so this can be off by up to
        // half a millisecond; iTunes files carry the exact length in iTunSMPB instead.
        let movie_ts = movie_ts.max(1) as u128;
        let frames = ((dur as u128 * media_ts as u128 + movie_ts / 2) / movie_ts) as u64;
        if frames == 0 {
            return None;
        }
        return Some(Mp4Gapless { priming: media_time as u64, frames });
    }
    None
}

fn itunsmpb(moov: &[u8]) -> Option<Mp4Gapless> {
    let meta = child(moov, b"udta").and_then(|u| child(u, b"meta"))?;
    // `meta` is a full box: skip version/flags.
    let ilst = child(meta.get(4..)?, b"ilst")?;
    for (t, item) in children(ilst) {
        if t != *b"----" {
            continue;
        }
        let name = child(item, b"name").and_then(|n| n.get(4..)).unwrap_or(&[]);
        if name != b"iTunSMPB" {
            continue;
        }
        let data = child(item, b"data")?.get(8..)?;
        let s = std::str::from_utf8(data).ok()?;
        let f: Vec<u64> = s.split_whitespace().filter_map(|h| u64::from_str_radix(h, 16).ok()).collect();
        if f.len() >= 4 && f[3] > 0 {
            return Some(Mp4Gapless { priming: f[1], frames: f[3] });
        }
    }
    None
}

/// Iterate (type, body) over boxes in `b`.
fn children(b: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut p = 0usize;
    std::iter::from_fn(move || {
        if p + 8 > b.len() {
            return None;
        }
        let size32 = u32::from_be_bytes(b[p..p + 4].try_into().ok()?) as usize;
        let typ: [u8; 4] = b[p + 4..p + 8].try_into().ok()?;
        let (hdr, size) = match size32 {
            0 => (8, b.len() - p),
            1 => (16, u64::from_be_bytes(b.get(p + 8..p + 16)?.try_into().ok()?) as usize),
            n => (8, n),
        };
        if size < hdr || p + size > b.len() {
            return None;
        }
        let body = &b[p + hdr..p + size];
        p += size;
        Some((typ, body))
    })
}

fn child<'a>(b: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    children(b).find(|(t, _)| t == typ).map(|(_, body)| body)
}

/// Timescale from an mvhd/mdhd body.
fn timescale(b: &[u8]) -> Option<u32> {
    let off = if *b.first()? == 1 { 20 } else { 12 };
    Some(u32::from_be_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// Read one top-level box fully (moov is small even when mdat is huge).
fn find_top_box(f: &mut File, len: u64, typ: &[u8; 4]) -> Option<Vec<u8>> {
    let mut pos = 0u64;
    while pos + 8 <= len {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 16];
        f.read_exact(&mut h[..8]).ok()?;
        let size32 = u32::from_be_bytes(h[..4].try_into().ok()?) as u64;
        let (hdr, size) = match size32 {
            0 => (8, len - pos),
            1 => {
                f.read_exact(&mut h[8..16]).ok()?;
                (16, u64::from_be_bytes(h[8..16].try_into().ok()?))
            }
            n => (8, n),
        };
        if size < hdr {
            return None;
        }
        if &h[4..8] == typ {
            if size > 64 << 20 {
                return None;
            }
            let mut body = vec![0u8; (size - hdr) as usize];
            f.read_exact(&mut body).ok()?;
            return Some(body);
        }
        pos += size;
    }
    None
}
