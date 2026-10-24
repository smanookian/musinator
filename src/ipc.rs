//! Control socket: `musinator play|pause|toggle|next|prev|stop|status [--json]`
//! and `musinator --bar` talk to the running app over
//! `$XDG_RUNTIME_DIR/musinator.sock` (one JSON line each way).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use crossbeam_channel as cb;
use serde_json::{Value, json};

use crate::engine::{Engine, EngineView};
use crate::model::{fmt_time, khz};

pub struct Request {
    pub cmd: String,
    pub reply: cb::Sender<Value>,
}

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("musinator.sock")
}

/// Listen for commands. Fails if another Musinator is running.
pub fn serve() -> Result<cb::Receiver<Request>> {
    let p = socket_path();
    if UnixStream::connect(&p).is_ok() {
        bail!("Musinator is already running");
    }
    let _ = std::fs::remove_file(&p);
    let listener = UnixListener::bind(&p)?;
    let (tx, rx) = cb::unbounded();
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                if BufReader::new(&conn).read_line(&mut line).is_err() {
                    return;
                }
                let cmd = serde_json::from_str::<Value>(&line).ok().and_then(|v| v["cmd"].as_str().map(str::to_string)).unwrap_or_default();
                let (rtx, rrx) = cb::bounded(1);
                if tx.send(Request { cmd, reply: rtx }).is_err() {
                    return;
                }
                if let Ok(v) = rrx.recv_timeout(Duration::from_secs(2)) {
                    let mut w = &conn;
                    let _ = writeln!(w, "{v}");
                }
            });
        }
    })?;
    Ok(rx)
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

/// Send one command to the running app.
pub fn call(cmd: &str) -> Result<Value> {
    let mut s = UnixStream::connect(socket_path()).map_err(|_| anyhow::anyhow!("Musinator is not running"))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    writeln!(s, "{}", json!({ "cmd": cmd }))?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line)?;
    if line.trim().is_empty() {
        anyhow::bail!("Musinator did not answer");
    }
    Ok(serde_json::from_str(&line)?)
}

pub fn status(e: &Engine) -> Value {
    status_view(&e.view())
}

pub fn status_view(v: &EngineView) -> Value {
    let s = v.snapshot();
    let (shuffle, repeat, queue_len) = {
        let q = v.queue().lock();
        (q.shuffle, q.repeat.label(), q.up_next().len())
    };
    let out_rate = v.output_rate();
    let mut j = json!({
        "running": true,
        "state": s.state.label(),
        "volume": (s.volume * 100.0).round() as u32,
        "shuffle": shuffle,
        "repeat": repeat,
        "up_next": queue_len,
    });
    if let Some(t) = &s.track {
        let pos = v.position().as_secs_f64();
        let f = s.format.clone().unwrap_or_default();
        let resampled = out_rate != 0 && f.sample_rate != 0 && out_rate != f.sample_rate;
        j["title"] = json!(t.display_title());
        j["artist"] = json!(t.artist);
        j["album"] = json!(t.album);
        j["file"] = json!(t.r.as_ref().map(|r| r.id.clone()));
        j["position"] = json!((pos * 10.0).round() / 10.0);
        j["duration"] = json!(t.duration_ms.map(|d| d as f64 / 1000.0));
        j["format"] = json!({
            "codec": f.codec, "sample_rate": f.sample_rate, "bits": f.bits, "channels": f.channels,
            "output_rate": out_rate, "resampled": resampled,
            "bit_perfect": !resampled && out_rate != 0 && s.volume >= 1.0 && s.gain_db.is_none(),
        });
        j["gain_db"] = json!(s.gain_db.map(|g| (g * 10.0).round() / 10.0));
    }
    j
}

/// Text for `musinator status`.
pub fn status_text(v: &Value) -> String {
    let state = v["state"].as_str().unwrap_or("stopped");
    let Some(title) = v["title"].as_str() else { return format!("{state} · nothing playing") };
    let icon = match state {
        "playing" => "▶",
        "paused" => "⏸",
        _ => "■",
    };
    let pos = fmt_time((v["position"].as_f64().unwrap_or(0.0) * 1000.0) as u64);
    let dur = v["duration"].as_f64().map(|d| fmt_time((d * 1000.0) as u64)).unwrap_or_else(|| "--:--".into());
    let f = &v["format"];
    let mut fmt = format!("{} {}", f["codec"].as_str().unwrap_or(""), khz(f["sample_rate"].as_u64().unwrap_or(0) as u32));
    if f["resampled"].as_bool() == Some(true) {
        fmt.push_str(&format!(" → {} kHz", khz(f["output_rate"].as_u64().unwrap_or(0) as u32)));
    }
    if f["resampled"].as_bool() != Some(true) {
        fmt.push_str(" kHz");
    }
    format!("{icon} {title} — {}  {pos} / {dur}  ({fmt}, vol {}%)", v["artist"].as_str().unwrap_or(""), v["volume"])
}

/// One line of Waybar-style JSON for the Omarchy bar. Empty line when idle.
pub fn bar_line() -> String {
    let Ok(v) = call("status") else { return String::new() };
    let state = v["state"].as_str().unwrap_or("stopped");
    let Some(title) = v["title"].as_str() else { return String::new() };
    if state == "stopped" {
        return String::new();
    }
    let artist = v["artist"].as_str().unwrap_or("");
    let pos = fmt_time((v["position"].as_f64().unwrap_or(0.0) * 1000.0) as u64);
    let icon = if state == "playing" { "♪" } else { "⏸" };
    let short = |s: &str, n: usize| if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() };
    let text = if artist.is_empty() { format!("{icon} {}  {pos}", short(title, 40)) } else { format!("{icon} {} · {}  {pos}", short(title, 32), short(artist, 24)) };
    let tooltip = format!("{}\n{} — {}\n{}", title, artist, v["album"].as_str().unwrap_or(""), status_text(&v));
    json!({ "text": text, "tooltip": tooltip, "class": state, "alt": state }).to_string()
}

