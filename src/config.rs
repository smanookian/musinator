//! Settings. One registry (`SETTINGS`) drives the settings dialog, the config
//! file (`~/.config/musinator/config.toml`) and help. No file editing needed.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::engine::{GainCfg, RgMode};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub density: String,
    pub corners: String,
    pub shadow: bool,
    pub term_colors: bool,
    pub big_lyrics: bool,
    pub lyrics_lead: f32,
    pub resume: bool,
    pub images: String,
    pub album_grid: bool,
    pub visualizer: bool,
    pub lyrics: bool,
    pub viz_fps: u32,
    pub volume: f32,
    pub rg_mode: String,
    pub rg_preamp: f32,
    pub rg_fallback: f32,
    pub prevent_clip: bool,
    pub confirm_quit: bool,
    pub notifications: bool,
    pub seek_small: u32,
    pub seek_big: u32,
    pub music_folders: Vec<String>,
    pub watch_folders: bool,
    pub measure_loudness: bool,
    pub save_lyrics_next: bool,
    pub omarchy_bar: bool,
    pub mpris: bool,
    pub sidebar_width: u16,
    /// Equalizer on/off, preset name ("Custom" = own settings), band gains in dB.
    pub lastfm_scrobble: bool,
    pub lastfm_radio: bool,
    pub eq_on: bool,
    pub eq_preset: String,
    pub eq_bands: [f32; 10],
    pub keys: HashMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        let omarchy = musinator_themes::omarchy::theme_dir().is_some();
        Self {
            theme: if omarchy { musinator_themes::omarchy::ID.into() } else { musinator_themes::DEFAULT_ID.into() },
            density: "compact".into(),
            corners: "round".into(),
            shadow: true,
            term_colors: true,
            big_lyrics: true,
            lyrics_lead: 0.3,
            resume: true,
            images: "auto".into(),
            album_grid: false,
            visualizer: true,
            lyrics: true,
            viz_fps: 30,
            volume: 0.7,
            rg_mode: "auto".into(),
            rg_preamp: 0.0,
            rg_fallback: 0.0,
            prevent_clip: true,
            confirm_quit: true,
            notifications: false,
            seek_small: 5,
            seek_big: 30,
            music_folders: vec!["~/Music".into()],
            watch_folders: true,
            measure_loudness: true,
            save_lyrics_next: false,
            omarchy_bar: false,
            mpris: true,
            lastfm_scrobble: true,
            lastfm_radio: true,
            eq_on: false,
            eq_preset: "Flat".into(),
            eq_bands: [0.0; 10],
            sidebar_width: 30,
            keys: HashMap::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("musinator")
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("musinator")
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("musinator")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(config_dir())?;
        let tmp = Self::path().with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, Self::path())?;
        Ok(())
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.music_folders.iter().map(|f| expand(f)).collect()
    }

    pub fn gain(&self) -> GainCfg {
        GainCfg {
            mode: RgMode::from_id(&self.rg_mode).unwrap_or_default(),
            preamp_db: self.rg_preamp,
            fallback_db: self.rg_fallback,
            prevent_clip: self.prevent_clip,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Toggle,
    Choice(&'static [(&'static str, &'static str)]),
    Number {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    /// Opens a picker or editor.
    Open,
}

pub struct Setting {
    pub key: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
}

pub const SECTIONS: &[&str] = &["Appearance", "Playback", "Audio", "Library", "Desktop", "Keys"];

macro_rules! s {
    ($key:literal, $sec:literal, $label:literal, $help:literal, $kind:expr) => {
        Setting { key: $key, section: $sec, label: $label, help: $help, kind: $kind }
    };
}

pub const SETTINGS: &[Setting] = &[
    s!("theme", "Appearance", "Theme", "Colors. Match Omarchy follows your desktop theme.", Kind::Open),
    s!(
        "density",
        "Appearance",
        "Density",
        "Compact or comfortable list spacing.",
        Kind::Choice(&[("compact", "Compact"), ("comfortable", "Comfortable")])
    ),
    s!("corners", "Appearance", "Pane corners", "Round or square pane corners.", Kind::Choice(&[("round", "Round"), ("square", "Square")])),
    s!("shadow", "Appearance", "Dialog shadow", "Soft shadow under floating dialogs.", Kind::Toggle),
    s!(
        "term_colors",
        "Appearance",
        "Match terminal colors",
        "Give the terminal the theme's background and text color while Musinator is open.",
        Kind::Toggle
    ),
    s!(
        "images",
        "Appearance",
        "Album art",
        "Auto: real images on Kitty/Ghostty, half-blocks elsewhere.",
        Kind::Choice(&[("auto", "Auto"), ("kitty", "Kitty images"), ("halfblocks", "Half-blocks"), ("off", "Off")])
    ),
    s!("album_grid", "Appearance", "Albums as grid", "Show albums as a grid of covers.", Kind::Toggle),
    s!("visualizer", "Appearance", "Visualizer", "Spectrum from the real audio.", Kind::Toggle),
    s!(
        "viz_fps",
        "Appearance",
        "Visualizer speed",
        "Frames per second. Lower uses less CPU.",
        Kind::Number { min: 10.0, max: 60.0, step: 5.0, unit: "fps" }
    ),
    s!(
        "big_lyrics",
        "Appearance",
        "Big current lyric line",
        "In full-screen lyrics (f), draw the current line in big text. Kitty and Ghostty only.",
        Kind::Toggle
    ),
    s!(
        "lyrics_lead",
        "Appearance",
        "Lyrics lead time",
        "Show the next lyric line this much before it is sung, so you can read it in time.",
        Kind::Number { min: 0.0, max: 1.5, step: 0.1, unit: "s" }
    ),
    s!("lyrics", "Appearance", "Lyrics", "Show synced lyrics on Now Playing when a song has them.", Kind::Toggle),
    s!("confirm_quit", "Playback", "Confirm before quitting", "Ask before Musinator closes.", Kind::Toggle),
    s!(
        "resume",
        "Playback",
        "Resume where you left off",
        "Bring back the queue and position when Musinator starts (paused).",
        Kind::Toggle
    ),
    s!("seek_small", "Playback", "Seek step", "Seconds for ← and →.", Kind::Number { min: 1.0, max: 30.0, step: 1.0, unit: "s" }),
    s!(
        "seek_big",
        "Playback",
        "Big seek step",
        "Seconds for shift+← and shift+→.",
        Kind::Number { min: 5.0, max: 120.0, step: 5.0, unit: "s" }
    ),
    s!(
        "rg_mode",
        "Audio",
        "ReplayGain",
        "Auto uses album gain when you play an album, else track gain.",
        Kind::Choice(&[("auto", "Auto"), ("track", "Track"), ("album", "Album"), ("off", "Off")])
    ),
    s!(
        "rg_preamp",
        "Audio",
        "ReplayGain preamp",
        "Added to every ReplayGain value.",
        Kind::Number { min: -15.0, max: 15.0, step: 0.5, unit: "dB" }
    ),
    s!(
        "rg_fallback",
        "Audio",
        "Gain without info",
        "Gain for files with no loudness info.",
        Kind::Number { min: -15.0, max: 0.0, step: 0.5, unit: "dB" }
    ),
    s!("prevent_clip", "Audio", "Prevent clipping", "Lower the gain so peaks stay below 0 dBFS.", Kind::Toggle),
    s!(
        "rate_switching",
        "Audio",
        "PipeWire rate switching",
        "Lets PipeWire run at the file's rate (bit-perfect). Writes ~/.config/pipewire/pipewire.conf.d/50-musinator.conf.",
        Kind::Toggle
    ),
    s!("music_folders", "Library", "Music folders", "Folders to scan. Enter to edit.", Kind::Open),
    s!("watch_folders", "Library", "Watch for changes", "Rescan when files change.", Kind::Toggle),
    s!("measure_loudness", "Library", "Measure loudness", "Measure EBU R128 loudness for files without ReplayGain tags.", Kind::Toggle),
    s!(
        "save_lyrics_next",
        "Library",
        "Save lyrics next to songs",
        "Lyrics found online also go into a .lrc file next to the song. Off: Musinator's cache only.",
        Kind::Toggle
    ),
    s!("rescan", "Library", "Rescan now", "Scan the music folders again.", Kind::Open),
    s!("import_playlist", "Library", "Import playlist", "Import an .m3u8 file.", Kind::Open),
    s!(
        "omarchy_bar",
        "Desktop",
        "Show in Omarchy bar",
        "Adds the playing song to the Omarchy bar (backs up shell.json first).",
        Kind::Toggle
    ),
    s!("mpris", "Desktop", "Media keys (MPRIS)", "Media keys, playerctl and the Omarchy media controls.", Kind::Toggle),
    s!("notifications", "Desktop", "Notify on track change", "Desktop notification when a new song starts.", Kind::Toggle),
    s!("lastfm", "Desktop", "Last.fm account", "Log in once in the browser. Press again to log out.", Kind::Open),
    s!("lastfm_scrobble", "Desktop", "Scrobble to Last.fm", "Send the songs you hear to your Last.fm profile.", Kind::Toggle),
    s!(
        "lastfm_radio",
        "Desktop",
        "Scrobble radio songs",
        "Also send songs heard on the radio (when the station names them).",
        Kind::Toggle
    ),
    s!("keys", "Keys", "Remap keys", "Change the key for any action.", Kind::Open),
];

impl Config {
    /// Text shown on the right of a setting row.
    pub fn display(&self, key: &str) -> String {
        let on = |b: bool| if b { "On" } else { "Off" }.to_string();
        let choice = |k: &str, v: &str| {
            SETTINGS.iter().find(|s| s.key == k).and_then(|s| match s.kind {
                Kind::Choice(opts) => opts.iter().find(|o| o.0 == v).map(|o| o.1.to_string()),
                _ => None,
            })
        };
        match key {
            "theme" => self.theme.clone(),
            "density" | "corners" | "images" | "rg_mode" => {
                let v = self.str_value(key);
                choice(key, &v).unwrap_or(v)
            }
            "shadow" => on(self.shadow),
            "term_colors" => on(self.term_colors),
            "big_lyrics" => on(self.big_lyrics),
            "resume" => on(self.resume),
            "album_grid" => on(self.album_grid),
            "visualizer" => on(self.visualizer),
            "lyrics" => on(self.lyrics),
            "viz_fps" => format!("{} fps", self.viz_fps),
            "confirm_quit" => on(self.confirm_quit),
            "seek_small" => format!("{} s", self.seek_small),
            "lyrics_lead" => format!("{:.1} s", self.lyrics_lead),
            "seek_big" => format!("{} s", self.seek_big),
            "rg_preamp" => format!("{:+.1} dB", self.rg_preamp),
            "rg_fallback" => format!("{:+.1} dB", self.rg_fallback),
            "prevent_clip" => on(self.prevent_clip),
            "rate_switching" => on(crate::pwrate::enabled()),
            "music_folders" => self.music_folders.join(", "),
            "watch_folders" => on(self.watch_folders),
            "measure_loudness" => on(self.measure_loudness),
            "save_lyrics_next" => on(self.save_lyrics_next),
            "omarchy_bar" => on(self.omarchy_bar),
            "mpris" => on(self.mpris),
            "lastfm" => crate::lastfm::user().map(|u| format!("Logged in as {u}")).unwrap_or_else(|| "Not connected".into()),
            "lastfm_scrobble" => on(self.lastfm_scrobble),
            "lastfm_radio" => on(self.lastfm_radio),
            "notifications" => on(self.notifications),
            _ => String::new(),
        }
    }

    fn str_value(&self, key: &str) -> String {
        match key {
            "density" => self.density.clone(),
            "corners" => self.corners.clone(),
            "images" => self.images.clone(),
            "rg_mode" => self.rg_mode.clone(),
            _ => String::new(),
        }
    }

    fn bool_mut(&mut self, key: &str) -> Option<&mut bool> {
        Some(match key {
            "shadow" => &mut self.shadow,
            "term_colors" => &mut self.term_colors,
            "big_lyrics" => &mut self.big_lyrics,
            "resume" => &mut self.resume,
            "album_grid" => &mut self.album_grid,
            "visualizer" => &mut self.visualizer,
            "lyrics" => &mut self.lyrics,
            "confirm_quit" => &mut self.confirm_quit,
            "prevent_clip" => &mut self.prevent_clip,
            "watch_folders" => &mut self.watch_folders,
            "measure_loudness" => &mut self.measure_loudness,
            "save_lyrics_next" => &mut self.save_lyrics_next,
            "omarchy_bar" => &mut self.omarchy_bar,
            "mpris" => &mut self.mpris,
            "lastfm_scrobble" => &mut self.lastfm_scrobble,
            "lastfm_radio" => &mut self.lastfm_radio,
            "notifications" => &mut self.notifications,
            _ => return None,
        })
    }

    /// Change a value: `dir` is +1 / -1 (toggles flip either way).
    /// Returns false for settings this cannot change (`Kind::Open`, rate switching).
    pub fn step(&mut self, key: &str, dir: i32) -> bool {
        if let Some(b) = self.bool_mut(key) {
            *b = !*b;
            return true;
        }
        let Some(s) = SETTINGS.iter().find(|s| s.key == key) else { return false };
        match s.kind {
            Kind::Choice(opts) => {
                let cur = self.str_value(key);
                let i = opts.iter().position(|o| o.0 == cur).unwrap_or(0) as i32;
                let n = opts.len() as i32;
                let next = opts[((i + dir).rem_euclid(n)) as usize].0.to_string();
                match key {
                    "density" => self.density = next,
                    "corners" => self.corners = next,
                    "images" => self.images = next,
                    "rg_mode" => self.rg_mode = next,
                    _ => return false,
                }
                true
            }
            Kind::Number { min, max, step, .. } => {
                let d = step * dir as f64;
                match key {
                    "viz_fps" => self.viz_fps = (self.viz_fps as f64 + d).clamp(min, max) as u32,
                    "seek_small" => self.seek_small = (self.seek_small as f64 + d).clamp(min, max) as u32,
                    "seek_big" => self.seek_big = (self.seek_big as f64 + d).clamp(min, max) as u32,
                    "lyrics_lead" => self.lyrics_lead = ((self.lyrics_lead as f64 + d).clamp(min, max) * 10.0).round() as f32 / 10.0,
                    "rg_preamp" => self.rg_preamp = (self.rg_preamp as f64 + d).clamp(min, max) as f32,
                    "rg_fallback" => self.rg_fallback = (self.rg_fallback as f64 + d).clamp(min, max) as f32,
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_shows_and_changes() {
        let mut c = Config::default();
        for s in SETTINGS {
            if s.key == "rate_switching" {
                continue; // touches PipeWire config; covered by the UI.
            }
            let before = c.display(s.key);
            match s.kind {
                Kind::Open => assert!(!c.step(s.key, 1)),
                _ => {
                    // Step whichever way moves (a value may sit at its limit).
                    let dir = if c.step(s.key, 1) && c.display(s.key) != before { 1 } else { -1 };
                    if dir == -1 {
                        assert!(c.step(s.key, -1), "{} can't change", s.key);
                    }
                    assert_ne!(c.display(s.key), before, "{} display did not change", s.key);
                    c.step(s.key, -dir);
                    assert_eq!(c.display(s.key), before, "{} did not step back", s.key);
                }
            }
        }
    }

    #[test]
    fn saves_and_loads() {
        let c = Config {
            theme: "nord".into(),
            volume: 0.3,
            keys: HashMap::from([("next".into(), vec!["ctrl+n".into()])]),
            ..Default::default()
        };
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
