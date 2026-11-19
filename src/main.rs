mod app;
mod bar;
mod config;
mod decode;
mod engine;
mod ipc;
mod keys;
mod lastfm;
mod library;
mod model;
mod mpris;
mod pwrate;
mod source;
mod update;

const HELP: &str = "Musinator — a terminal music player

Usage:
  musinator                 open the player
  musinator FILE|FOLDER...  play these songs (in the open player, if one runs)
  musinator play            resume playing
  musinator pause           pause
  musinator toggle          play / pause
  musinator next            next song
  musinator prev            previous song
  musinator stop            stop
  musinator status [--json] what is playing
  musinator --bar           one line of JSON for the Omarchy bar or Waybar
  musinator update          install the newest version
  musinator --version
  musinator --help

Settings are inside the app (press S). Config file: ~/.config/musinator/config.toml";

/// Words that are commands, never file names.
const COMMANDS: &[&str] = &["play", "pause", "toggle", "next", "prev", "stop", "status", "update", "help"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let cmd = args.iter().find(|a| *a != "--json").map(String::as_str);
    // `musinator song.flac folder/ …`: play them (in the running Musinator if there is one).
    let open: Vec<std::path::PathBuf> = match cmd {
        Some(c) if !COMMANDS.contains(&c) && std::path::Path::new(c).exists() => {
            args.iter().map(std::path::PathBuf::from).filter(|p| p.exists()).map(|p| std::path::absolute(&p).unwrap_or(p)).collect()
        }
        _ => Vec::new(),
    };
    let code = match cmd {
        Some("update") => update::cli(),
        _ if !open.is_empty() && ipc::open(&open).is_ok() => 0,
        _ if cmd.is_none() || !open.is_empty() => match app::run(config::Config::load(), open) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("musinator: {e:#}");
                1
            }
        },
        Some("--bar") => {
            println!("{}", ipc::bar_line());
            0
        }
        Some("--help" | "-h" | "help") => {
            println!("{HELP}");
            0
        }
        Some("--version" | "-V") => {
            println!("musinator {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(c @ ("play" | "pause" | "toggle" | "next" | "prev" | "stop" | "status")) => match ipc::call(c) {
            Ok(v) => {
                if let Some(e) = v["error"].as_str() {
                    eprintln!("musinator: {e}");
                    1
                } else {
                    if json {
                        println!("{v}");
                    } else if c == "status" {
                        println!("{}", ipc::status_text(&v));
                    }
                    0
                }
            }
            Err(e) => {
                if json && c == "status" {
                    println!("{}", serde_json::json!({ "running": false, "state": "stopped" }));
                    0
                } else {
                    eprintln!("musinator: {e}");
                    1
                }
            }
        },
        None => 0, // handled by the guard above
        Some(other) if other.contains('/') || other.contains('.') => {
            eprintln!("musinator: can't find \"{other}\"");
            1
        }
        Some(other) => {
            eprintln!("musinator: unknown command \"{other}\"\n\n{HELP}");
            2
        }
    };
    std::process::exit(code);
}
