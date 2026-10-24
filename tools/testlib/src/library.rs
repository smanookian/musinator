//! The seeded library plan (artists, albums, tracks) and writing it to disk.

use crate::cover;
use crate::ffmpeg::Loudness;
use crate::formats::Fmt;
use crate::masters::Masters;
use crate::names;
use crate::rng::Rng;
use crate::tags::{self, GainTags, ReplayGain, TrackTags};
use anyhow::{Context, Result};
use lofty::picture::MimeType;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const VARIOUS: &str = "Various Artists";
/// Names used by the special albums; library artists never use them.
pub const RESERVED: [&str; 4] = [VARIOUS, "Musinator Demo", "Gapless Test", "Loudness Test"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lyrics {
    None,
    Lrc,
    Embedded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    None,
    Embedded { png: bool },
    File(&'static str),
}

pub struct TrackPlan {
    pub title: String,
    pub artist: String,
    pub disc: u32,
    pub track: u32,
    pub master: usize,
    pub lyrics: Lyrics,
}

pub struct AlbumPlan {
    pub album_artist: String,
    pub title: String,
    pub year: u32,
    pub genre: &'static str,
    pub fmt: Fmt,
    pub compilation: bool,
    pub disc_total: u32,
    /// Multi-disc albums: `CD1/`, `CD2/` folders instead of `1-01 - ...` file names.
    pub subfolders: bool,
    pub cover: Cover,
    pub cover_size: u32,
    pub gain: bool,
    pub seed: u64,
    pub tracks: Vec<TrackPlan>,
}

/// Counts for the summary and manifest.
#[derive(Default)]
pub struct Stats {
    pub formats: BTreeMap<Fmt, usize>,
    pub albums: usize,
    pub cover_embedded: usize,
    pub cover_file: usize,
    pub cover_none: usize,
    pub lrc: usize,
    pub lyrics_embedded: usize,
    pub replaygain_albums: usize,
    pub r128_albums: usize,
}

impl Stats {
    pub fn merge(&mut self, o: Stats) {
        for (f, n) in o.formats {
            *self.formats.entry(f).or_default() += n;
        }
        self.albums += o.albums;
        self.cover_embedded += o.cover_embedded;
        self.cover_file += o.cover_file;
        self.cover_none += o.cover_none;
        self.lrc += o.lrc;
        self.lyrics_embedded += o.lyrics_embedded;
        self.replaygain_albums += o.replaygain_albums;
        self.r128_albums += o.r128_albums;
    }

    pub fn tracks(&self) -> usize {
        self.formats.values().sum()
    }
}

fn unique(rng: &mut Rng, used: &mut HashSet<String>, make: fn(&mut Rng) -> String) -> String {
    for _ in 0..50 {
        let name = make(rng);
        if used.insert(name.clone()) {
            return name;
        }
    }
    // Word lists ran out of fresh combinations: number it.
    let base = make(rng);
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| used.insert(n.clone()))
        .expect("endless range")
}

/// Plan the library. `master_durations` are the lengths of the library masters; short ones
/// are picked more often.
pub fn plan(seed: u64, n_tracks: usize, master_durations: &[f64]) -> Vec<AlbumPlan> {
    let mut rng = Rng::new(seed);
    let n_artists = (n_tracks as f64 / 33.0).round().max(1.0) as usize;
    let mut used: HashSet<String> = RESERVED.iter().map(|s| s.to_string()).collect();
    let artists: Vec<String> = (0..n_artists)
        .map(|_| unique(&mut rng, &mut used, names::artist_name))
        .collect();
    let weights: Vec<u32> = Fmt::ALL.iter().map(|f| f.album_weight()).collect();
    let master_weights: Vec<u32> = master_durations
        .iter()
        .map(|&d| {
            if d <= 4.0 {
                6
            } else if d <= 5.0 {
                3
            } else if d <= 7.0 {
                2
            } else {
                1
            }
        })
        .collect();
    // Big formats (WAV, 24-bit ALAC, 24/192) only use short masters.
    let short_weights: Vec<u32> = master_durations
        .iter()
        .zip(&master_weights)
        .map(|(&d, &w)| if d <= 5.0 { w } else { 0 })
        .collect();
    let mut titles: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut albums = Vec::new();
    let mut remaining = n_tracks;

    while remaining > 0 {
        let mut r = rng.fork();
        let compilation = r.chance(0.025);
        let album_artist = if compilation {
            VARIOUS.to_string()
        } else if albums.len() < n_artists {
            artists[albums.len()].clone()
        } else {
            // Skewed: some artists have many albums, most have a few.
            artists[(n_artists as f64 * r.f64().powf(1.6)) as usize].clone()
        };
        let title = unique(
            &mut r,
            titles.entry(album_artist.clone()).or_default(),
            names::album_title,
        );
        let fmt = Fmt::ALL[r.weighted(&weights)];
        let track_weights =
            if matches!(fmt, Fmt::Wav16 | Fmt::Wav24 | Fmt::Alac24 | Fmt::Flac24_192) {
                &short_weights
            } else {
                &master_weights
            };
        let disc_total = if r.chance(0.04) {
            if r.chance(0.2) { 3 } else { 2 }
        } else {
            1
        };
        let max_per_disc = if disc_total > 1 { 12 } else { 14 };
        let cover = match r.weighted(&[50, 25, 25]) {
            0 => Cover::Embedded { png: r.chance(0.1) },
            1 => Cover::File(*r.pick(&[
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "cover.jpg",
                "folder.jpg",
                "front.png",
            ])),
            _ => Cover::None,
        };
        let cover_size = match cover {
            Cover::Embedded { png: true } => 300,
            Cover::Embedded { png: false } => *r.pick(&[300, 400, 500]),
            _ => *r.pick(&[300, 400, 500, 600]),
        };
        let mut album = AlbumPlan {
            album_artist,
            title,
            year: r.range(1965, 2025),
            genre: r.pick(&names::GENRES),
            fmt,
            compilation,
            disc_total,
            subfolders: disc_total > 1 && r.chance(0.5),
            cover,
            cover_size,
            gain: r.chance(0.6),
            seed: r.next_u64(),
            tracks: Vec::new(),
        };
        let mut track_titles = HashSet::new();
        'discs: for disc in 1..=disc_total {
            let count = r.range(5, max_per_disc);
            for track in 1..=count {
                if remaining == 0 {
                    break 'discs;
                }
                let artist = if compilation {
                    r.pick(&artists).clone()
                } else {
                    album.album_artist.clone()
                };
                let artist = if r.chance(0.04) {
                    format!("{artist} feat. {}", r.pick(&artists))
                } else {
                    artist
                };
                let x = r.f64();
                album.tracks.push(TrackPlan {
                    title: unique(&mut r, &mut track_titles, names::track_title),
                    artist,
                    disc,
                    track,
                    master: r.weighted(track_weights),
                    lyrics: if x < 0.05 {
                        Lyrics::Lrc
                    } else if x < 0.07 {
                        Lyrics::Embedded
                    } else {
                        Lyrics::None
                    },
                });
                remaining -= 1;
            }
        }
        album.disc_total = album.tracks.last().map_or(1, |t| t.disc);
        albums.push(album);
    }
    albums
}

/// Safe single path component.
pub fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c == '/' || c == '\0' { '-' } else { c })
        .collect();
    while s.len() > 180 {
        s.pop();
    }
    let s = s.trim().trim_end_matches('.').to_string();
    if s.starts_with('.') {
        format!("_{s}")
    } else {
        s
    }
}

pub fn album_dir(root: &Path, artist: &str, year: u32, title: &str) -> PathBuf {
    root.join(sanitize(artist))
        .join(sanitize(&format!("{year} - {title}")))
}

/// Album loudness: duration-weighted energy mean of the tracks; peak: the max.
fn album_loudness(tracks: &[(f64, Loudness)]) -> Loudness {
    let total: f64 = tracks.iter().map(|(d, _)| d).sum();
    let energy: f64 = tracks
        .iter()
        .map(|(d, l)| d * 10f64.powf(l.lufs / 10.0))
        .sum::<f64>()
        / total;
    let peak_db = tracks
        .iter()
        .map(|(_, l)| l.peak_db)
        .fold(f64::MIN, f64::max);
    Loudness {
        lufs: 10.0 * energy.log10(),
        peak_db,
    }
}

/// Gain tags for each track: R128 for Opus, ReplayGain (-18 LUFS reference) otherwise.
pub fn gain_tags(fmt: Fmt, tracks: &[(f64, Loudness)]) -> Vec<GainTags> {
    let album = album_loudness(tracks);
    let q78 = |lufs: f64| ((-23.0 - lufs) * 256.0).round().clamp(-32768.0, 32767.0) as i32;
    tracks
        .iter()
        .map(|(_, l)| {
            if fmt == Fmt::Opus {
                GainTags::R128 {
                    track: q78(l.lufs),
                    album: q78(album.lufs),
                }
            } else {
                GainTags::ReplayGain(ReplayGain {
                    track_gain: -18.0 - l.lufs,
                    track_peak: l.peak_linear(),
                    album_gain: -18.0 - album.lufs,
                    album_peak: album.peak_linear(),
                })
            }
        })
        .collect()
}

pub fn cover_mime(png: bool) -> MimeType {
    if png { MimeType::Png } else { MimeType::Jpeg }
}

pub fn copy_and_tag(src: &Path, dst: &Path, fmt: Fmt, tags: &TrackTags) -> Result<()> {
    std::fs::copy(src, dst)
        .with_context(|| format!("copy {} to {}", src.display(), dst.display()))?;
    tags::write(dst, fmt, tags)
}

/// Write one planned album: folders, cover, audio files with tags, lyrics.
pub fn write_album(root: &Path, a: &AlbumPlan, m: &Masters) -> Result<Stats> {
    let dir = album_dir(root, &a.album_artist, a.year, &a.title);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let mut stats = Stats {
        albums: 1,
        ..Stats::default()
    };
    let mut rng = Rng::new(a.seed);

    let png =
        matches!(a.cover, Cover::Embedded { png: true }) || a.cover == Cover::File("front.png");
    let cover_bytes = match a.cover {
        Cover::None => {
            stats.cover_none += 1;
            None
        }
        Cover::Embedded { .. } => {
            stats.cover_embedded += 1;
            Some(cover::make(a.seed, a.cover_size, png)?)
        }
        Cover::File(name) => {
            stats.cover_file += 1;
            std::fs::write(dir.join(name), cover::make(a.seed, a.cover_size, png)?)?;
            None
        }
    };

    let louds: Vec<(f64, Loudness)> = a
        .tracks
        .iter()
        .map(|t| (m.lib[t.master].dur, m.lib[t.master].loud))
        .collect();
    let gains = if a.gain {
        if a.fmt == Fmt::Opus {
            stats.r128_albums += 1;
        } else {
            stats.replaygain_albums += 1;
        }
        gain_tags(a.fmt, &louds)
    } else {
        vec![GainTags::None; a.tracks.len()]
    };

    for (t, gain) in a.tracks.iter().zip(gains) {
        let track_total = a.tracks.iter().filter(|x| x.disc == t.disc).count() as u32;
        let (folder, prefix) = if a.disc_total == 1 {
            (dir.clone(), format!("{:02}", t.track))
        } else if a.subfolders {
            (dir.join(format!("CD{}", t.disc)), format!("{:02}", t.track))
        } else {
            (dir.clone(), format!("{}-{:02}", t.disc, t.track))
        };
        if folder != dir {
            std::fs::create_dir_all(&folder)?;
        }
        let stem = sanitize(&format!("{prefix} - {}", t.title));
        let dst = folder.join(format!("{stem}.{}", a.fmt.ext()));
        let dur = m.lib[t.master].dur;
        let lrc = match t.lyrics {
            Lyrics::None => None,
            _ => Some(names::lrc(
                &mut rng,
                &t.title,
                &t.artist,
                &a.title,
                dur,
                (1.0, 2.2),
            )),
        };
        let tags = TrackTags {
            title: &t.title,
            artist: &t.artist,
            album_artist: &a.album_artist,
            album: &a.title,
            genre: a.genre,
            year: a.year,
            track: t.track,
            track_total,
            disc: t.disc,
            disc_total: a.disc_total,
            compilation: a.compilation,
            cover: cover_bytes.as_deref().map(|b| (b, cover_mime(png))),
            lyrics: if t.lyrics == Lyrics::Embedded {
                lrc.as_deref()
            } else {
                None
            },
            gain,
        };
        copy_and_tag(&m.lib_path(a.fmt, t.master), &dst, a.fmt, &tags)?;
        match t.lyrics {
            Lyrics::Lrc => {
                std::fs::write(
                    folder.join(format!("{stem}.lrc")),
                    lrc.as_deref().unwrap_or_default(),
                )?;
                stats.lrc += 1;
            }
            Lyrics::Embedded => stats.lyrics_embedded += 1,
            Lyrics::None => {}
        }
        *stats.formats.entry(a.fmt).or_default() += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURS: [f64; 4] = [3.0, 4.0, 6.0, 12.0];

    #[test]
    fn plan_has_exact_track_count_and_is_deterministic() {
        let a = plan(7, 2000, &DURS);
        let b = plan(7, 2000, &DURS);
        assert_eq!(a.iter().map(|x| x.tracks.len()).sum::<usize>(), 2000);
        let key = |p: &[AlbumPlan]| {
            p.iter()
                .map(|x| (x.album_artist.clone(), x.title.clone(), x.fmt))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&a), key(&b));
        assert_ne!(key(&a), key(&plan(8, 2000, &DURS)));
    }

    #[test]
    fn album_folders_are_unique() {
        let p = plan(1, 3000, &DURS);
        let dirs: HashSet<PathBuf> = p
            .iter()
            .map(|a| album_dir(Path::new("/x"), &a.album_artist, a.year, &a.title))
            .collect();
        assert_eq!(dirs.len(), p.len());
    }

    #[test]
    fn sanitize_makes_one_safe_component() {
        assert_eq!(sanitize("AC/DC"), "AC-DC");
        assert_eq!(sanitize(".hidden. "), "_.hidden");
        assert!(sanitize(&"é".repeat(200)).len() <= 180);
    }

    #[test]
    fn r128_gain_is_q78_relative_to_minus_23() {
        let l = Loudness {
            lufs: -13.0,
            peak_db: -1.0,
        };
        match gain_tags(Fmt::Opus, &[(5.0, l)])[0] {
            GainTags::R128 { track, album } => assert_eq!((track, album), (-2560, -2560)),
            _ => panic!("expected R128"),
        }
    }
}
