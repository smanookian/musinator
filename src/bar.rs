//! "Show in Omarchy bar": add or remove our command module in
//! `~/.config/omarchy/shell.json` (backed up first). The bar reloads on save.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value, json};

const ID: &str = "musinator";

fn shell_json() -> PathBuf {
    crate::config::home().join(".config/omarchy/shell.json")
}

/// True when this looks like Omarchy with the shell bar.
pub fn available() -> bool {
    shell_json().exists()
}

fn module() -> Value {
    json!({
        "id": ID,
        "type": "command",
        "exec": "musinator --bar",
        "interval": 2,
        "tooltip": "Musinator",
        "onClick": "omarchy-launch-or-focus-tui musinator"
    })
}

fn has_module(v: &Value) -> bool {
    ["left", "center", "right"].iter().any(|s| v["bar"]["layout"][s].as_array().is_some_and(|a| a.iter().any(|m| m["id"] == ID)))
}

/// Make shell.json match the setting. No-op when nothing needs to change.
pub fn sync(on: bool) {
    if available() {
        let _ = set(on);
    }
}

pub fn set(on: bool) -> Result<()> {
    let p = shell_json();
    let text = std::fs::read_to_string(&p).context("read shell.json")?;
    let mut v: Value = serde_json::from_str(&text).context("parse shell.json")?;
    if has_module(&v) == on {
        return Ok(());
    }
    if on {
        // Keep the user's file as it was before Musinator first touched it.
        let bak = p.with_file_name("shell.json.bak-musinator");
        if !bak.exists() {
            std::fs::write(&bak, &text)?;
        }
        let layout = &mut v["bar"]["layout"];
        if !layout["right"].is_array() {
            layout["right"] = json!([]);
        }
        let right = layout["right"].as_array_mut().unwrap();
        // Left of the tray, like other media widgets.
        let at = right.iter().position(|m| m["id"] == "omarchy.tray").unwrap_or(0);
        right.insert(at, module());
    } else {
        for s in ["left", "center", "right"] {
            if let Some(a) = v["bar"]["layout"][s].as_array_mut() {
                a.retain(|m| m["id"] != ID);
            }
        }
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v)? + "\n")?;
    std::fs::rename(tmp, p)?;
    Ok(())
}
