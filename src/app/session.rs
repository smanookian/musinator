//! "Resume where you left off": the queue, current song and position,
//! saved to `~/.local/state/musinator/session.json` on quit.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub path: String,
    pub from: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Session {
    /// Queue in play order.
    pub items: Vec<Item>,
    pub current: Option<String>,
    pub position_ms: u64,
    pub shuffle: bool,
    pub repeat: String,
}

fn path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".local/state"))
        .join("musinator/session.json")
}

pub fn load() -> Option<Session> {
    serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()
}

pub fn save(s: &Session) -> anyhow::Result<()> {
    let p = path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(s)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

pub fn clear() {
    let _ = std::fs::remove_file(path());
}
