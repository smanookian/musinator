//! The `match-omarchy` theme: builds a theme from the current Omarchy colors.
//!
//! Reads `colors.toml` (newer Omarchy) or, if that is missing,
//! `alacritty.toml` from the theme dir, then runs the same derivation as the
//! built-in themes.

use std::path::{Path, PathBuf};

use toml::Table;

use crate::{Palette, Rgb, Theme, derive};

/// Id of the theme that follows Omarchy.
pub const ID: &str = "match-omarchy";

/// Omarchy "current" dirs, newest layout first (relative to `$HOME`).
const CURRENT: [&str; 2] = [".local/state/omarchy/current", ".config/omarchy/current"];

fn current_dirs() -> Vec<PathBuf> {
    std::env::home_dir().map(|h| CURRENT.iter().map(|c| h.join(c)).collect()).unwrap_or_default()
}

/// The active Omarchy theme dir, if Omarchy is installed.
pub fn theme_dir() -> Option<PathBuf> {
    current_dirs().into_iter().map(|c| c.join("theme")).find(|d| d.is_dir())
}

/// Existing paths a file watcher should watch to notice a theme switch:
/// the `current` dir (theme swap, `theme.name` rewrite), the theme dir
/// itself (files changed in place) and `theme.name`.
pub fn watch_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for cur in current_dirs() {
        for p in [cur.join("theme"), cur.join("theme.name"), cur] {
            if p.exists() {
                out.push(p);
            }
        }
    }
    out
}

/// Builds the `match-omarchy` theme from an Omarchy theme dir.
pub fn load(dir: &Path) -> Option<Theme> {
    let palette = read_toml(&dir.join("colors.toml"))
        .and_then(|t| from_colors(&t))
        .or_else(|| read_toml(&dir.join("alacritty.toml")).and_then(|t| from_alacritty(&t)))?;
    let label = format!("Match Omarchy ({})", theme_name(dir));
    Some(derive(ID, &label, &palette))
}

/// The `match-omarchy` theme for the active Omarchy theme.
pub fn current() -> Option<Theme> {
    load(&theme_dir()?)
}

/// `retro-stripes` -> `Retro Stripes`, from `<dir>/../theme.name` or the
/// dir name (after following symlinks).
fn theme_name(dir: &Path) -> String {
    let from_file = dir
        .parent()
        .and_then(|p| std::fs::read_to_string(p.join("theme.name")).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let raw = from_file.unwrap_or_else(|| {
        let real = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    });
    raw.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_toml(path: &Path) -> Option<Table> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

fn color(t: &Table, key: &str) -> Option<Rgb> {
    t.get(key)?.as_str().and_then(Rgb::hex)
}

/// Maps Omarchy's `colors.toml` to a palette.
pub(crate) fn from_colors(t: &Table) -> Option<Palette> {
    let bg = color(t, "background")?;
    let fg = color(t, "foreground")?;
    let get = |keys: &[&str], fallback: Rgb| keys.iter().find_map(|k| color(t, k)).unwrap_or(fallback);
    let dark = match t.get("mode").and_then(|v| v.as_str()) {
        Some("light") => false,
        Some("dark") => true,
        _ => bg.luminance() < fg.luminance(),
    };
    let blue = get(&["blue", "bright_blue"], fg);
    let red = get(&["red", "bright_red"], fg);
    Some(Palette {
        dark,
        bg,
        fg,
        muted: get(&["muted", "bright_black", "dark_foreground"], bg.mix(fg, 0.5)),
        border: get(&["selection", "lighter_background"], bg.mix(fg, 0.25)),
        accent: get(&["accent", "blue"], fg),
        red,
        green: get(&["green", "bright_green"], fg),
        yellow: get(&["yellow", "bright_yellow"], fg),
        viz: [blue, get(&["magenta", "bright_magenta"], blue), red],
    })
}

/// Maps `alacritty.toml` (`[colors.primary]`, `[colors.normal]`,
/// `[colors.bright]`) to a palette.
pub(crate) fn from_alacritty(t: &Table) -> Option<Palette> {
    let colors = t.get("colors")?.as_table()?;
    let section = |name: &str| colors.get(name).and_then(|v| v.as_table());
    let primary = section("primary")?;
    let bg = color(primary, "background")?;
    let fg = color(primary, "foreground")?;
    let (normal, bright) = (section("normal"), section("bright"));
    let pick = |s: Option<&Table>, key: &str| s.and_then(|s| color(s, key));
    let hue = |key: &str| pick(normal, key).or_else(|| pick(bright, key)).unwrap_or(fg);
    let blue = hue("blue");
    let red = hue("red");
    Some(Palette {
        dark: bg.luminance() < fg.luminance(),
        bg,
        fg,
        muted: pick(bright, "black").unwrap_or(bg.mix(fg, 0.5)),
        border: pick(section("selection"), "background").unwrap_or(bg.mix(fg, 0.25)),
        accent: blue,
        red,
        green: hue("green"),
        yellow: hue("yellow"),
        viz: [blue, hue("magenta"), red],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contrast_report;

    /// Every Omarchy theme dir on this machine (empty if none).
    fn theme_dirs_on_disk() -> Vec<PathBuf> {
        let mut roots = vec![PathBuf::from("/usr/share/omarchy/themes")];
        if let Some(h) = std::env::home_dir() {
            roots.push(h.join(".config/omarchy/themes"));
            roots.push(h.join(".local/share/omarchy/themes"));
        }
        let mut dirs: Vec<PathBuf> = roots
            .iter()
            .filter_map(|r| std::fs::read_dir(r).ok())
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        dirs.extend(theme_dir());
        dirs.sort();
        dirs
    }

    fn failures(name: &str, t: &Theme) -> Vec<String> {
        contrast_report(t)
            .into_iter()
            .filter(|&(_, ratio, min)| ratio < min)
            .map(|(pair, ratio, min)| format!("{name}: {pair} = {ratio:.2} (needs {min})"))
            .collect()
    }

    #[test]
    fn omarchy_themes_on_disk_pass() {
        let mut bad = Vec::new();
        for dir in theme_dirs_on_disk() {
            let has_colors = dir.join("colors.toml").is_file() || dir.join("alacritty.toml").is_file();
            if !has_colors {
                continue;
            }
            let name = dir.display().to_string();
            match load(&dir) {
                Some(t) => {
                    assert_eq!(t.id, ID);
                    assert!(t.label.starts_with("Match Omarchy ("), "{}", t.label);
                    bad.extend(failures(&name, &t));
                }
                None => bad.push(format!("{name}: could not load")),
            }
        }
        assert!(bad.is_empty(), "contrast failures:\n{}", bad.join("\n"));
    }

    #[test]
    fn alacritty_fallback_on_disk_passes() {
        let mut bad = Vec::new();
        for dir in theme_dirs_on_disk() {
            let Some(t) = read_toml(&dir.join("alacritty.toml")) else { continue };
            let name = format!("{} (alacritty)", dir.display());
            match from_alacritty(&t) {
                Some(p) => bad.extend(failures(&name, &derive(ID, "test", &p))),
                None => bad.push(format!("{name}: could not map")),
            }
        }
        assert!(bad.is_empty(), "contrast failures:\n{}", bad.join("\n"));
    }

    #[test]
    fn maps_colors_toml() {
        let t: Table = r##"
            mode = "light"
            accent = "#6e6e6e"
            selection = "#c0c0c0"
            muted = "#808080"
            background = "#ffffff"
            foreground = "#000000"
            red = "#aa0000"
            green = "#00aa00"
            yellow = "#aaaa00"
            blue = "#0000aa"
            magenta = "#aa00aa"
        "##
        .parse()
        .unwrap();
        let p = from_colors(&t).unwrap();
        assert!(!p.dark);
        assert_eq!(p.accent, Rgb(0x6e, 0x6e, 0x6e));
        assert_eq!(p.border, Rgb(0xc0, 0xc0, 0xc0));
        assert_eq!(p.viz, [Rgb(0, 0, 0xaa), Rgb(0xaa, 0, 0xaa), Rgb(0xaa, 0, 0)]);
        assert!(failures("inline", &derive(ID, "x", &p)).is_empty());
    }

    #[test]
    fn maps_alacritty_toml() {
        let t: Table = r##"
            [colors.primary]
            background = "0x1e1e2e"
            foreground = "#cdd6f4"
            [colors.normal]
            red = "#f38ba8"
            blue = "#89b4fa"
            [colors.bright]
            black = "#585b70"
            magenta = "#f5c2e7"
        "##
        .parse()
        .unwrap();
        let p = from_alacritty(&t).unwrap();
        assert!(p.dark);
        assert_eq!(p.bg, Rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(p.accent, Rgb(0x89, 0xb4, 0xfa));
        assert_eq!(p.muted, Rgb(0x58, 0x5b, 0x70));
        assert_eq!(p.viz[1], Rgb(0xf5, 0xc2, 0xe7));
    }

    #[test]
    fn theme_name_from_file_or_dir() {
        let tmp = std::env::temp_dir().join(format!("musinator-themes-{}", std::process::id()));
        let dir = tmp.join("rose-pine");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(theme_name(&dir), "Rose Pine");
        std::fs::write(tmp.join("theme.name"), "retro-stripes\n").unwrap();
        assert_eq!(theme_name(&dir), "Retro Stripes");
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
