//! Edit tags: read the editable fields, guess them from the file name, and write
//! them safely (copy, write the copy, then replace the original).

use std::path::Path;

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey, Tag};

pub const FIELDS: [&str; 8] = ["Title", "Artist", "Album", "Album artist", "Year", "Track", "Disc", "Genre"];
/// Fields that make sense for many songs at once.
pub const ALBUM_FIELDS: [usize; 5] = [1, 2, 3, 4, 7];

pub type Fields = [String; 8];

pub fn read(path: &Path) -> Result<Fields> {
    let f = lofty::read_from_path(path)?;
    let mut out: Fields = Default::default();
    if let Some(t) = f.primary_tag().or_else(|| f.first_tag()) {
        out[0] = t.title().map(|s| s.into_owned()).unwrap_or_default();
        out[1] = t.artist().map(|s| s.into_owned()).unwrap_or_default();
        out[2] = t.album().map(|s| s.into_owned()).unwrap_or_default();
        out[3] = t.get_string(ItemKey::AlbumArtist).unwrap_or_default().to_string();
        out[4] = t.date().map(|d| d.year.to_string()).or_else(|| t.get_string(ItemKey::Year).map(str::to_string)).unwrap_or_default();
        out[5] = t.track().map(|n| n.to_string()).unwrap_or_default();
        out[6] = t.disk().map(|n| n.to_string()).unwrap_or_default();
        out[7] = t.genre().map(|s| s.into_owned()).unwrap_or_default();
    }
    Ok(out)
}

/// Guess fields from `Artist/Year - Album/NN - Title.ext` style names.
/// Empty strings mean "no guess".
pub fn from_filename(path: &Path) -> Fields {
    let mut out: Fields = Default::default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.replace('_', " ");
    let parts: Vec<&str> = stem.split(" - ").map(str::trim).filter(|p| !p.is_empty()).collect();
    let num = |s: &str| {
        let d: String = s.trim_end_matches('.').chars().filter(|c| c.is_ascii_digit()).collect();
        (!d.is_empty() && d.len() <= 3 && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-'))
            .then(|| d.trim_start_matches('0').to_string())
    };
    // "05. Title" / "05 Title"
    let split_lead = |s: &str| -> Option<(String, String)> {
        let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if d.is_empty() || d.len() > 3 {
            return None;
        }
        let rest = s[d.len()..].trim_start_matches(['.', ' ', ')']).trim();
        (!rest.is_empty()).then(|| (d.trim_start_matches('0').to_string(), rest.to_string()))
    };
    match parts.as_slice() {
        [n, artist, title] if num(n).is_some() => {
            out[5] = num(n).unwrap();
            out[1] = artist.to_string();
            out[0] = title.to_string();
        }
        [n, title] if num(n).is_some() => {
            out[5] = num(n).unwrap();
            out[0] = title.to_string();
        }
        [artist, title] => {
            if let Some((n, a)) = split_lead(artist) {
                out[5] = n;
                out[1] = a;
            } else {
                out[1] = artist.to_string();
            }
            out[0] = title.to_string();
        }
        [single] => match split_lead(single) {
            Some((n, t)) => {
                out[5] = n;
                out[0] = t;
            }
            None => out[0] = single.to_string(),
        },
        _ => out[0] = stem.clone(),
    }
    // Folder: "Year - Album" or "Album"; its parent: artist.
    if let Some(dir) = path.parent() {
        let name = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = if name.to_lowercase().starts_with("cd") || name.to_lowercase().starts_with("disc") {
            out[6] = name.chars().filter(|c| c.is_ascii_digit()).collect::<String>().trim_start_matches('0').to_string();
            dir.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            name
        };
        match name.split_once(" - ") {
            Some((y, album)) if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) => {
                out[4] = y.to_string();
                out[2] = album.trim().to_string();
            }
            _ => out[2] = name.trim().to_string(),
        }
        let album_dir = if out[6].is_empty() { dir } else { dir.parent().unwrap_or(dir) };
        if let Some(artist) = album_dir.parent().and_then(|p| p.file_name()) {
            out[3] = artist.to_string_lossy().into_owned();
            if out[1].is_empty() {
                out[1] = out[3].clone();
            }
        }
    }
    out
}

/// Write the given fields (None = leave as is). Writes a copy first, then
/// replaces the original, so a crash can never leave a half-written song.
pub fn write(path: &Path, fields: &[Option<String>; 8]) -> Result<()> {
    let tmp = path.with_file_name(format!(".musinator-tmp-{}", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::copy(path, &tmp).context("copy")?;
    let res = (|| -> Result<()> {
        let mut f = lofty::read_from_path(&tmp)?;
        let tag_type = f.primary_tag_type();
        if f.primary_tag().is_none() {
            f.insert_tag(Tag::new(tag_type));
        }
        let t = f.primary_tag_mut().expect("tag");
        let set = |t: &mut Tag, k: ItemKey, v: &str| {
            if v.is_empty() {
                t.remove_key(k);
            } else {
                t.insert_text(k, v.to_string());
            }
        };
        for (i, v) in fields.iter().enumerate() {
            let Some(v) = v else { continue };
            let v = v.trim();
            match i {
                0 => set(t, ItemKey::TrackTitle, v),
                1 => set(t, ItemKey::TrackArtist, v),
                2 => set(t, ItemKey::AlbumTitle, v),
                3 => set(t, ItemKey::AlbumArtist, v),
                4 => set(t, ItemKey::Year, v),
                5 => match v.parse::<u32>() {
                    Ok(n) => t.set_track(n),
                    Err(_) => t.remove_track(),
                },
                6 => match v.parse::<u32>() {
                    Ok(n) => t.set_disk(n),
                    Err(_) => t.remove_disk(),
                },
                _ => set(t, ItemKey::Genre, v),
            }
        }
        f.save_to_path(&tmp, WriteOptions::default())?;
        Ok(())
    })();
    match res {
        Ok(()) => {
            std::fs::rename(&tmp, path).context("replace")?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_file_and_folder_names() {
        let f = from_filename(Path::new("/m/AC-DC/1977 - Let There Be Rock/05 - Problem Child.mp3"));
        assert_eq!(
            (f[0].as_str(), f[5].as_str(), f[2].as_str(), f[4].as_str(), f[3].as_str()),
            ("Problem Child", "5", "Let There Be Rock", "1977", "AC-DC")
        );
        let f = from_filename(Path::new("/m/ACDC/Stiff Upper Lip/ACDC - Safe In New York City (video).mp4"));
        assert_eq!((f[1].as_str(), f[0].as_str(), f[2].as_str()), ("ACDC", "Safe In New York City (video)", "Stiff Upper Lip"));
        let f = from_filename(Path::new("/m/X/Album/CD2/03. Song.flac"));
        assert_eq!((f[0].as_str(), f[5].as_str(), f[6].as_str(), f[2].as_str(), f[3].as_str()), ("Song", "3", "2", "Album", "X"));
    }

    #[test]
    fn writes_tags_safely() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.flac");
        let st = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=duration=1", "-c:a", "flac"])
            .arg(&p)
            .status()
            .unwrap();
        assert!(st.success());
        let mut f: [Option<String>; 8] = Default::default();
        f[0] = Some("New Title".into());
        f[1] = Some("Someone".into());
        f[5] = Some("7".into());
        write(&p, &f).unwrap();
        let r = read(&p).unwrap();
        assert_eq!((r[0].as_str(), r[1].as_str(), r[5].as_str()), ("New Title", "Someone", "7"));
        // Only given fields change; no temp file left behind.
        let mut g: [Option<String>; 8] = Default::default();
        g[7] = Some("Rock".into());
        write(&p, &g).unwrap();
        let r = read(&p).unwrap();
        assert_eq!((r[0].as_str(), r[7].as_str()), ("New Title", "Rock"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
