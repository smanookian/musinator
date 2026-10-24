//! Musinator color themes.
//!
//! Every theme starts as a small upstream palette (or the user's Omarchy
//! colors) and goes through one function, [`derive`], which computes the UI
//! roles and then fixes contrast: text 4.5:1, borders and the selection
//! marker 3:1. [`contrast_report`] lists every checked pair.

mod color;
pub mod omarchy;
mod palettes;

use color::legible_on;
pub use color::{Rgb, contrast, legible};

/// Id of the theme used when nothing else is set.
pub const DEFAULT_ID: &str = "catppuccin-mocha";

/// Every color the UI draws with.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Kebab-case, e.g. `catppuccin-mocha`.
    pub id: String,
    /// Shown in the theme picker, e.g. `Catppuccin Mocha`.
    pub label: String,
    pub dark: bool,
    /// App background.
    pub bg: Rgb,
    /// Normal text.
    pub fg: Rgb,
    /// Secondary text: track numbers, durations, hints.
    pub muted: Rgb,
    /// Unfocused pane border.
    pub border: Rgb,
    /// Focused border, focused title-bar fill, playing row text, progress fill.
    pub accent: Rgb,
    /// Text on the accent title bar.
    pub on_accent: Rgb,
    /// Unfocused pane title-bar fill.
    pub title_bar: Rgb,
    /// Selected-row tint in the focused pane.
    pub selection: Rgb,
    /// Selected-row tint in an unfocused pane.
    pub selection_dim: Rgb,
    /// Text on both selection tints.
    pub selection_fg: Rgb,
    /// The `→` selection marker.
    pub marker: Rgb,
    /// Bottom status bar fill.
    pub status_bar: Rgb,
    /// Empty part of the progress bar.
    pub track: Rgb,
    /// Floating dialog fill.
    pub dialog_bg: Rgb,
    /// Dialog drop shadow.
    pub shadow: Rgb,
    pub good: Rgb,
    pub warn: Rgb,
    pub error: Rgb,
    /// Visualizer gradient: low, mid, high.
    pub viz: [Rgb; 3],
}

/// The few colors a theme is built from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub dark: bool,
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    /// A surface/line color a step or two above `bg`.
    pub border: Rgb,
    pub accent: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub viz: [Rgb; 3],
}

/// Required contrast for text.
const TEXT: f64 = 4.5;
/// Required contrast for borders and the selection marker.
const UI: f64 = 3.0;

/// Builds a [`Theme`] from a palette: blends the fills, then nudges every
/// foreground role until it is legible on every fill it is drawn on.
pub(crate) fn derive(id: &str, label: &str, p: &Palette) -> Theme {
    let bg = p.bg;

    // Fills. These stay subtle on purpose (as in the mockup).
    let title_bar = bg.mix(p.border, 0.45);
    let selection = bg.mix(p.accent, 0.20);
    let selection_dim = bg.mix(p.border, 0.50);
    let status_bar = bg.mix(p.accent, 0.18);
    let dialog_bg = if p.dark { bg.mix(p.fg, 0.06) } else { bg.mix(Rgb::WHITE, 0.6) };
    let shadow = if p.dark { bg.mix(Rgb::BLACK, 0.6) } else { bg.mix(Rgb::BLACK, 0.2) };
    let track = p.border;

    // Text that can sit on any fill.
    let fills = [bg, title_bar, selection, selection_dim, status_bar, dialog_bg];
    let fg = legible_on(p.fg, &fills, TEXT);
    let muted = legible_on(p.muted, &fills, TEXT);

    // Accent is text on rows (playing track, maybe selected) and in dialogs.
    let accent = legible_on(p.accent, &[bg, selection, selection_dim, dialog_bg], TEXT);
    let on_accent = legible(bg, accent, TEXT);
    let marker = legible_on(accent, &[bg, selection, selection_dim], UI);
    let border = legible(p.border, bg, UI);

    let good = legible_on(p.green, &[bg, dialog_bg], TEXT);
    let warn = legible_on(p.yellow, &[bg, dialog_bg], TEXT);
    let error = legible_on(p.red, &[bg, dialog_bg], TEXT);
    let viz = p.viz.map(|c| legible(c, bg, UI));

    Theme {
        id: id.to_string(),
        label: label.to_string(),
        dark: p.dark,
        bg,
        fg,
        muted,
        border,
        accent,
        on_accent,
        title_bar,
        selection,
        selection_dim,
        selection_fg: fg,
        marker,
        status_bar,
        track,
        dialog_bg,
        shadow,
        good,
        warn,
        error,
        viz,
    }
}

/// All built-in themes, families next to each other.
pub fn builtin() -> Vec<Theme> {
    palettes::ALL.iter().map(|e| derive(e.id, e.label, &e.palette)).collect()
}

/// A built-in theme by id.
pub fn by_id(id: &str) -> Option<Theme> {
    palettes::ALL.iter().find(|e| e.id == id).map(|e| derive(e.id, e.label, &e.palette))
}

/// Every color pair the UI draws, as `(pair, ratio, required)`.
pub fn contrast_report(t: &Theme) -> Vec<(&'static str, f64, f64)> {
    let pairs: [(&'static str, Rgb, Rgb, f64); 29] = [
        ("fg/bg", t.fg, t.bg, TEXT),
        ("muted/bg", t.muted, t.bg, TEXT),
        ("accent/bg", t.accent, t.bg, TEXT),
        ("on_accent/accent", t.on_accent, t.accent, TEXT),
        ("fg/title_bar", t.fg, t.title_bar, TEXT),
        ("muted/title_bar", t.muted, t.title_bar, TEXT),
        ("selection_fg/selection", t.selection_fg, t.selection, TEXT),
        ("selection_fg/selection_dim", t.selection_fg, t.selection_dim, TEXT),
        ("muted/selection", t.muted, t.selection, TEXT),
        ("muted/selection_dim", t.muted, t.selection_dim, TEXT),
        ("accent/selection", t.accent, t.selection, TEXT),
        ("accent/selection_dim", t.accent, t.selection_dim, TEXT),
        ("marker/bg", t.marker, t.bg, UI),
        ("marker/selection", t.marker, t.selection, UI),
        ("marker/selection_dim", t.marker, t.selection_dim, UI),
        ("border/bg", t.border, t.bg, UI),
        ("accent/bg (focused border)", t.accent, t.bg, UI),
        ("fg/status_bar", t.fg, t.status_bar, TEXT),
        ("muted/status_bar", t.muted, t.status_bar, TEXT),
        ("fg/dialog_bg", t.fg, t.dialog_bg, TEXT),
        ("muted/dialog_bg", t.muted, t.dialog_bg, TEXT),
        ("accent/dialog_bg", t.accent, t.dialog_bg, TEXT),
        ("viz low/bg", t.viz[0], t.bg, UI),
        ("viz mid/bg", t.viz[1], t.bg, UI),
        ("viz high/bg", t.viz[2], t.bg, UI),
        ("good/bg", t.good, t.bg, TEXT),
        ("warn/bg", t.warn, t.bg, TEXT),
        ("error/bg", t.error, t.bg, TEXT),
        ("selection_fg/bg", t.selection_fg, t.bg, TEXT),
    ];
    pairs.iter().map(|&(name, a, b, min)| (name, contrast(a, b), min)).collect()
}
