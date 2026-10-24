use std::collections::HashSet;

use musinator_themes::{DEFAULT_ID, Rgb, Theme, builtin, by_id, contrast_report, omarchy};

fn failures(t: &Theme) -> Vec<String> {
    contrast_report(t)
        .into_iter()
        .filter(|&(_, ratio, min)| ratio < min)
        .map(|(pair, ratio, min)| format!("{}: {pair} = {ratio:.2} (needs {min})", t.id))
        .collect()
}

#[test]
fn every_builtin_passes_every_pair() {
    let bad: Vec<String> = builtin().iter().flat_map(failures).collect();
    assert!(bad.is_empty(), "contrast failures:\n{}", bad.join("\n"));
}

#[test]
fn report_covers_required_pairs() {
    let names: Vec<&str> = contrast_report(&builtin()[0]).iter().map(|r| r.0).collect();
    for want in [
        "fg/bg",
        "muted/bg",
        "accent/bg",
        "on_accent/accent",
        "fg/title_bar",
        "muted/title_bar",
        "selection_fg/selection",
        "selection_fg/selection_dim",
        "muted/selection",
        "accent/selection",
        "marker/bg",
        "marker/selection",
        "marker/selection_dim",
        "border/bg",
        "accent/bg (focused border)",
        "fg/status_bar",
        "muted/status_bar",
        "fg/dialog_bg",
        "muted/dialog_bg",
        "accent/dialog_bg",
        "viz low/bg",
        "viz mid/bg",
        "viz high/bg",
        "good/bg",
        "warn/bg",
        "error/bg",
    ] {
        assert!(names.contains(&want), "missing pair {want}");
    }
}

#[test]
fn ids_unique_and_kebab_case() {
    let themes = builtin();
    assert!(themes.len() >= 22);
    let mut seen = HashSet::new();
    for t in &themes {
        assert!(seen.insert(t.id.clone()), "duplicate id {}", t.id);
        assert!(t.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'), "not kebab-case: {}", t.id);
        assert!(!t.label.is_empty());
        assert_ne!(t.id, omarchy::ID);
    }
}

#[test]
fn families_are_adjacent() {
    let themes = builtin();
    let family = |t: &Theme| t.label.split(' ').next().unwrap().to_string();
    let mut done = HashSet::new();
    let mut last = String::new();
    for t in &themes {
        let f = family(t);
        if f != last {
            assert!(done.insert(f.clone()), "family {f} is split up");
            last = f;
        }
    }
}

#[test]
fn by_id_works() {
    for t in builtin() {
        assert_eq!(by_id(&t.id).unwrap().id, t.id);
    }
    assert!(by_id(DEFAULT_ID).is_some());
    assert!(by_id("no-such-theme").is_none());
}

#[test]
fn dark_flag_matches_colors() {
    for t in builtin() {
        assert_eq!(t.dark, t.bg.luminance() < t.fg.luminance(), "{}", t.id);
    }
}

#[test]
fn mocha_matches_mockup() {
    let t = by_id("catppuccin-mocha").unwrap();
    let hex = |s| Rgb::hex(s).unwrap();
    assert_eq!(t.bg, hex("#1e1e2e"));
    assert_eq!(t.fg, hex("#cdd6f4"));
    assert_eq!(t.accent, hex("#89b4fa"));
    assert_eq!(t.track, hex("#45475a"));
    assert_eq!(t.viz, [hex("#89b4fa"), hex("#cba6f7"), hex("#f5c2e7")]);
    // Muted starts at overlay1 and is lifted only as far as the selection
    // tint needs; it stays clearly dimmer than fg.
    assert!(t.muted.luminance() > hex("#7f849c").luminance());
    assert!(musinator_themes::contrast(t.muted, t.fg) > 1.4);
    // Fills are the mockup blends.
    assert_eq!(t.title_bar, t.bg.mix(hex("#45475a"), 0.45));
    assert_eq!(t.selection, t.bg.mix(t.accent, 0.20));
    assert_eq!(t.selection_dim, t.bg.mix(hex("#45475a"), 0.50));
    assert_eq!(t.status_bar, t.bg.mix(t.accent, 0.18));
}

#[test]
fn print_worst_pairs() {
    let mut themes = builtin();
    themes.extend(omarchy::current());
    println!("{:<24} {:<28} {:>6}", "theme", "worst pair", "ratio");
    for t in &themes {
        let (pair, ratio, min) = contrast_report(t).into_iter().min_by(|a, b| (a.1 / a.2).total_cmp(&(b.1 / b.2))).unwrap();
        let id = if t.id == omarchy::ID { format!("{} [{}]", t.id, t.label) } else { t.id.clone() };
        println!("{id:<24} {pair:<28} {ratio:>6.2} (needs {min})");
    }
}
