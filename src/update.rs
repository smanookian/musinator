//! Updates from GitHub releases: find the newest version, download it, check its
//! SHA-256, and swap the program file. Installs made by a package manager are not
//! touched; the user gets the command for their package manager instead.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::source::{http_error, net_error};

const API: &str = "https://api.github.com/repos/smanookian/musinator/releases/latest";
const ASSET: &str = "musinator-x86_64-linux.tar.gz";
/// The install line from the README: also the way to update when Musinator can't.
pub const INSTALL_CMD: &str = "curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh";
const DAY: u64 = 24 * 60 * 60;

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Clone, Debug)]
pub struct Release {
    /// Without the leading "v".
    pub version: String,
    /// Release notes as plain lines.
    pub notes: Vec<String>,
    tar_url: String,
    sha_url: String,
}

/// Why Musinator can't replace itself, and what to run instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Manual {
    pub why: String,
    pub cmd: String,
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(secs))).http_status_as_error(false).build().into()
}

/// The newest release on GitHub.
pub fn latest() -> Result<Release> {
    let mut resp = agent(15)
        .get(API)
        .header("User-Agent", &format!("musinator/{}", current()))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| net_error(e, "GitHub"))?;
    if resp.status() != 200 {
        return Err(http_error("GitHub", resp.status()));
    }
    let v: serde_json::Value = resp
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| anyhow!("GitHub sent an answer Musinator can't read"))?;
    parse_release(&v)
}

fn parse_release(v: &serde_json::Value) -> Result<Release> {
    let version = v["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    if version.is_empty() {
        bail!("GitHub sent a release without a version");
    }
    let asset = |name: &str| v["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"].as_str().map(String::from);
    let (Some(tar_url), Some(sha_url)) = (asset(ASSET), asset(&format!("{ASSET}.sha256"))) else {
        bail!("version {version} has no download for this computer yet");
    };
    Ok(Release { notes: plain_notes(v["body"].as_str().unwrap_or_default()), version, tar_url, sha_url })
}

/// Markdown release notes as plain lines: no `#`, `**` or backticks, "•" bullets,
/// one empty line between paragraphs at most.
fn plain_notes(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in body.lines() {
        let l = l.trim_end().trim_start_matches('#').trim_start().replace("**", "").replace('`', "");
        let l = match l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            Some(rest) => format!("• {rest}"),
            None => l,
        };
        if l.is_empty() && out.last().is_none_or(String::is_empty) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// True when `latest` is a newer stable version than `cur` ("0.1.10" > "0.1.9").
pub fn newer(latest: &str, cur: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        if v.contains('-') {
            return None; // pre-release: never offered
        }
        v.split('.').map(|p| p.parse().ok()).collect()
    };
    match (parse(latest), parse(cur.split('-').next().unwrap_or(cur))) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Where the running program lives, if Musinator may replace it.
pub fn target() -> Result<PathBuf, Manual> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| Manual { why: "Musinator can't find its own program file.".into(), cmd: INSTALL_CMD.into() })?;
    if let Some(pkg) = Command::new("pacman")
        .args(["-Qoq", &exe.to_string_lossy()])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| !p.is_empty())
    {
        return Err(Manual {
            why: format!("It was installed with pacman as {pkg}, so your package manager updates it."),
            cmd: format!("yay -S {pkg}"),
        });
    }
    let dir = exe.parent().unwrap_or(Path::new("/"));
    if !writable(dir) {
        return Err(Manual { why: format!("Musinator can't write to {}.", dir.display()), cmd: INSTALL_CMD.into() });
    }
    Ok(exe)
}

fn writable(dir: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else { return false };
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Download `rel`, check it, and put it in place of the running program.
/// The running Musinator keeps working; the new version starts next time.
pub fn install(rel: &Release) -> Result<PathBuf> {
    let exe = target().map_err(|m| anyhow!("{} Run: {}", m.why, m.cmd))?;
    let sums = download(&rel.sha_url, 4096).context("the checksum file")?;
    let want = String::from_utf8_lossy(&sums).split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let tar = download(&rel.tar_url, 200 << 20).context("the download")?;
    let got: String = Sha256::digest(&tar).iter().map(|b| format!("{b:02x}")).collect();
    if want.len() != 64 || got != want {
        bail!("the download is damaged (its checksum does not match): try again");
    }

    let tmp = std::env::temp_dir().join(format!("musinator-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let res = unpack_and_swap(&tmp, &tar, &rel.version, &exe);
    let _ = std::fs::remove_dir_all(&tmp);
    res.map(|()| exe)
}

fn download(url: &str, max: u64) -> Result<Vec<u8>> {
    let mut resp =
        agent(120).get(url).header("User-Agent", &format!("musinator/{}", current())).call().map_err(|e| net_error(e, "GitHub"))?;
    if resp.status() != 200 {
        return Err(http_error("GitHub", resp.status()));
    }
    let mut out = Vec::new();
    resp.body_mut().as_reader().take(max).read_to_end(&mut out)?;
    Ok(out)
}

fn unpack_and_swap(tmp: &Path, tar: &[u8], version: &str, exe: &Path) -> Result<()> {
    let archive = tmp.join(ASSET);
    std::fs::write(&archive, tar)?;
    let ok = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(tmp).status().map(|s| s.success()).unwrap_or(false);
    let new = tmp.join("musinator");
    if !ok || !new.is_file() {
        bail!("the download could not be unpacked");
    }
    // The new program must run here and say it is the version we asked for.
    let says = Command::new(&new).arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if says.as_deref() != Some(&format!("musinator {version}")) {
        bail!("the new version does not start on this computer (it says: {})", says.unwrap_or_default());
    }
    swap(&new, exe)?;
    refresh_desktop_files(tmp);
    Ok(())
}

/// Replace `exe` with `new` so that a failure at any step leaves a working program.
fn swap(new: &Path, exe: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let next = exe.with_file_name("musinator.new");
    let bak = exe.with_file_name("musinator.bak");
    std::fs::copy(new, &next).with_context(|| format!("can't write {}", next.display()))?;
    std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(exe, &bak).with_context(|| format!("can't move {}", exe.display()))?;
    if let Err(e) = std::fs::rename(&next, exe) {
        let _ = std::fs::rename(&bak, exe);
        let _ = std::fs::remove_file(&next);
        return Err(e).with_context(|| format!("can't replace {}", exe.display()));
    }
    let _ = std::fs::remove_file(&bak);
    Ok(())
}

/// Update the launcher entry and icon where the install script put them.
fn refresh_desktop_files(tmp: &Path) {
    let share = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| crate::config::home().join(".local/share"));
    for (file, dest) in [
        ("musinator.desktop", share.join("applications/musinator.desktop")),
        ("musinator.svg", share.join("icons/hicolor/scalable/apps/musinator.svg")),
    ] {
        if dest.exists() {
            let _ = std::fs::copy(tmp.join(file), dest);
        }
    }
}

// ---------------- when to check ----------------

/// Kept apart from config.toml, so the config file does not change on every check.
#[derive(Default, Serialize, Deserialize)]
struct State {
    last_check: u64,
    skipped: String,
}

fn state_path() -> PathBuf {
    crate::config::data_dir().join("update.json")
}

fn load() -> State {
    std::fs::read_to_string(state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn store(s: &State) {
    let _ = std::fs::create_dir_all(crate::config::data_dir());
    let _ = std::fs::write(state_path(), serde_json::to_string(s).unwrap_or_default());
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The daily check is due (and this is noted, so it runs once a day).
pub fn take_due() -> bool {
    let mut s = load();
    let due = now().saturating_sub(s.last_check) >= DAY;
    if due {
        s.last_check = now();
        store(&s);
    }
    due
}

pub fn skip(version: &str) {
    let mut s = load();
    s.skipped = version.into();
    store(&s);
}

pub fn skipped(version: &str) -> bool {
    load().skipped == version
}

// ---------------- command line ----------------

/// `musinator update`
pub fn cli() -> i32 {
    println!("Checking for updates…");
    let rel = match latest() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("musinator: {e:#}");
            return 1;
        }
    };
    if !newer(&rel.version, current()) {
        println!("You have the newest version (v{}).", current());
        return 0;
    }
    println!("Musinator v{} is out. You have v{}.\n", rel.version, current());
    for l in &rel.notes {
        println!("  {l}");
    }
    println!();
    if let Err(m) = target() {
        println!("{}\nRun: {}", m.why, m.cmd);
        return 1;
    }
    println!("Downloading and checking…");
    match install(&rel) {
        Ok(p) => {
            println!("Updated {} to v{}. Restart Musinator to use it.", p.display(), rel.version);
            0
        }
        Err(e) => {
            eprintln!("musinator: update failed: {e:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(newer("0.1.2", "0.1.1"));
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.1.1", "0.1.1"));
        assert!(!newer("0.1.0", "0.1.1"));
        assert!(!newer("0.2.0-beta", "0.1.1"), "pre-releases are not offered");
        assert!(!newer("garbage", "0.1.1"));
    }

    #[test]
    fn reads_a_github_release() {
        let v = serde_json::json!({
            "tag_name": "v0.1.2",
            "body": "### Fixed\n\n- Notifications show the **album cover**.\n- `U` installs updates.\n",
            "assets": [
                { "name": ASSET, "browser_download_url": "https://x/t.tar.gz" },
                { "name": format!("{ASSET}.sha256"), "browser_download_url": "https://x/t.sha256" },
            ],
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.version, "0.1.2");
        assert_eq!(r.notes, ["Fixed", "", "• Notifications show the album cover.", "• U installs updates."]);
        assert_eq!((r.tar_url.as_str(), r.sha_url.as_str()), ("https://x/t.tar.gz", "https://x/t.sha256"));
    }

    #[test]
    fn a_release_without_our_download_is_refused() {
        let v = serde_json::json!({ "tag_name": "v0.2.0", "assets": [{ "name": ASSET, "browser_download_url": "u" }] });
        assert!(parse_release(&v).unwrap_err().to_string().contains("no download"));
    }

    #[test]
    fn swap_replaces_the_program_and_leaves_nothing_behind() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("musinator");
        let new = d.path().join("downloaded");
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        swap(&new, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert!(!d.path().join("musinator.bak").exists() && !d.path().join("musinator.new").exists());
    }
}
