//! Import and export playlists as `.m3u8` (UTF-8 M3U with #EXTINF).

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::Track;

/// Write tracks; paths are relative to the playlist file when possible.
pub fn export(path: &Path, tracks: &[std::sync::Arc<Track>]) -> Result<()> {
    let base = path.parent().unwrap_or(Path::new("."));
    let mut s = String::from("#EXTM3U\n");
    for t in tracks {
        let secs = t.duration_ms.map(|d| (d / 1000) as i64).unwrap_or(-1);
        let name = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
        s.push_str(&format!("#EXTINF:{secs},{name}\n"));
        let p = Path::new(&t.tref().id);
        let rel = p.strip_prefix(base).map(Path::to_path_buf).unwrap_or_else(|_| relative(p, base));
        s.push_str(&rel.to_string_lossy());
        s.push('\n');
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Read entries as absolute paths (URLs are kept as written).
pub fn import(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let base = path.parent().unwrap_or(Path::new("."));
    Ok(text
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            if l.contains("://") {
                l.to_string()
            } else {
                let p = Path::new(l);
                let abs = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
                normalize(&abs).to_string_lossy().into_owned()
            }
        })
        .collect())
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn relative(p: &Path, base: &Path) -> PathBuf {
    let pc: Vec<_> = p.components().collect();
    let bc: Vec<_> = base.components().collect();
    let common = pc.iter().zip(&bc).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return p.to_path_buf();
    }
    let mut out = PathBuf::new();
    for _ in common..bc.len() {
        out.push("..");
    }
    for c in &pc[common..] {
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackRef;
    use std::sync::Arc;

    #[test]
    fn round_trip_with_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music/A/x.flac");
        let pl = dir.path().join("lists/mix.m3u8");
        std::fs::create_dir_all(pl.parent().unwrap()).unwrap();
        let t = Track {
            r: Some(TrackRef::new(&Arc::from("local"), music.to_string_lossy())),
            title: "X".into(),
            artist: "Y".into(),
            duration_ms: Some(61_000),
            ..Default::default()
        };
        export(&pl, &[Arc::new(t)]).unwrap();
        let text = std::fs::read_to_string(&pl).unwrap();
        assert!(text.contains("#EXTINF:61,Y - X\n../Music/A/x.flac\n"), "{text}");
        assert_eq!(import(&pl).unwrap(), vec![music.to_string_lossy().into_owned()]);
    }
}
