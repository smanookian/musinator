//! Optional: let PipeWire switch its rate to the file's rate (bit-perfect).
//! Only runs when the user turns it on in Settings → Audio.

use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

const RATES: &str = "[ 44100 48000 88200 96000 176400 192000 352800 384000 ]";

fn path() -> PathBuf {
    crate::config::home().join(".config/pipewire/pipewire.conf.d/50-musinator.conf")
}

pub fn enabled() -> bool {
    path().exists()
}

/// Write or remove the drop-in, and apply it live (no restart).
pub fn set(on: bool) -> Result<()> {
    let p = path();
    if on {
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(
            &p,
            format!("# Written by Musinator (Settings → Audio → PipeWire rate switching).\ncontext.properties = {{\n    default.clock.allowed-rates = {RATES}\n}}\n"),
        )?;
    } else if p.exists() {
        std::fs::remove_file(&p)?;
    }
    let rates = if on { RATES.to_string() } else { default_rates() };
    let _ = Command::new("pw-metadata").args(["-n", "settings", "0", "clock.allowed-rates", &rates]).output();
    Ok(())
}

/// Rates from the user's other PipeWire config, else 48000.
fn default_rates() -> String {
    let dirs = [crate::config::home().join(".config/pipewire"), PathBuf::from("/etc/pipewire"), PathBuf::from("/usr/share/pipewire")];
    for d in dirs {
        for f in std::iter::once(d.join("pipewire.conf")).chain(std::fs::read_dir(d.join("pipewire.conf.d")).into_iter().flatten().flatten().map(|e| e.path())) {
            if f.file_name().is_some_and(|n| n == "50-musinator.conf") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            for line in text.lines() {
                let l = line.trim();
                if l.starts_with("default.clock.allowed-rates") {
                    if let (Some(a), Some(b)) = (l.find('['), l.find(']')) {
                        return l[a..=b].to_string();
                    }
                }
            }
        }
    }
    "[ 48000 ]".into()
}
