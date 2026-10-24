//! ReplayGain: choose track or album gain, add preamp, keep peaks under 0 dBFS.

use crate::model::Gain;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RgMode {
    Off,
    Track,
    Album,
    /// Album gain when playing an album in order, else track gain.
    #[default]
    Auto,
}

impl RgMode {
    pub const ALL: [RgMode; 4] = [RgMode::Off, RgMode::Track, RgMode::Album, RgMode::Auto];
    pub fn id(self) -> &'static str {
        match self {
            RgMode::Off => "off",
            RgMode::Track => "track",
            RgMode::Album => "album",
            RgMode::Auto => "auto",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainCfg {
    pub mode: RgMode,
    /// Added to every ReplayGain value.
    pub preamp_db: f32,
    /// Used for files with no gain info.
    pub fallback_db: f32,
    /// Lower the gain so the peak stays at or below 0 dBFS.
    pub prevent_clip: bool,
}

impl Default for GainCfg {
    fn default() -> Self {
        Self { mode: RgMode::Auto, preamp_db: 0.0, fallback_db: 0.0, prevent_clip: true }
    }
}

/// The gain to apply, in dB, or None when nothing is applied.
pub fn gain_db(cfg: &GainCfg, g: &Gain, album_context: bool) -> Option<f32> {
    let (db, peak) = match cfg.mode {
        RgMode::Off => return None,
        RgMode::Track => (g.track_db.or(g.album_db), g.track_peak.or(g.album_peak)),
        RgMode::Album => (g.album_db.or(g.track_db), g.album_peak.or(g.track_peak)),
        RgMode::Auto if album_context => (g.album_db.or(g.track_db), g.album_peak.or(g.track_peak)),
        RgMode::Auto => (g.track_db.or(g.album_db), g.track_peak.or(g.album_peak)),
    };
    let mut db = match db {
        Some(d) => d + cfg.preamp_db,
        None if cfg.fallback_db != 0.0 => cfg.fallback_db,
        None => return None,
    };
    if cfg.prevent_clip {
        if let Some(p) = peak.filter(|p| *p > 0.0) {
            let max_db = -20.0 * p.log10();
            db = db.min(max_db);
        }
    }
    Some(db)
}

pub fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// User volume (0..=1) to amplitude. Cubic, like PipeWire's own volume sliders.
pub fn volume_curve(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v * v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_vs_track_and_clip_guard() {
        let g = Gain { track_db: Some(-6.0), track_peak: Some(0.9), album_db: Some(-8.0), album_peak: Some(0.95), ..Default::default() };
        let cfg = GainCfg::default();
        assert_eq!(gain_db(&cfg, &g, false), Some(-6.0));
        assert_eq!(gain_db(&cfg, &g, true), Some(-8.0));
        assert_eq!(gain_db(&GainCfg { mode: RgMode::Off, ..cfg }, &g, true), None);
        // +10 dB on a 0.5 peak would clip; limit to +6.02 dB.
        let loud = Gain { track_db: Some(10.0), track_peak: Some(0.5), ..Default::default() };
        let db = gain_db(&cfg, &loud, false).unwrap();
        assert!((db - 6.0206).abs() < 0.01, "{db}");
        // No info: nothing, unless a fallback is set.
        assert_eq!(gain_db(&cfg, &Gain::default(), false), None);
        assert_eq!(gain_db(&GainCfg { fallback_db: -6.0, ..cfg }, &Gain::default(), false), Some(-6.0));
    }
}
