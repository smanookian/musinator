//! The local library: background scan, tags, covers, loudness, file watching.
//! Everything here runs on worker threads at low priority.

pub mod db;
pub mod lrc;
pub mod m3u8;
pub mod tags;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel as cb;
use parking_lot::Mutex;
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub enum LibEvent {
    Progress { done: usize, total: usize },
    Done { added: usize, removed: usize, total: usize },
    Loudness { done: usize, total: usize },
    Error(String),
}

const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const COVER_EXT: &[&str] = &["jpg", "jpeg", "png", "webp"];

pub struct Library {
    pub db_path: PathBuf,
    folders: Arc<Mutex<Vec<PathBuf>>>,
    tx: cb::Sender<LibEvent>,
    busy: Arc<AtomicBool>,
    again: Arc<AtomicBool>,
    pub measure_loudness: Arc<AtomicBool>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

/// Lower the priority of the calling thread (Linux: per-thread nice).
fn lower_priority() {
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

impl Library {
    pub fn new(db_path: PathBuf, folders: Vec<PathBuf>, tx: cb::Sender<LibEvent>) -> Arc<Self> {
        Arc::new(Self {
            db_path,
            folders: Arc::new(Mutex::new(folders)),
            tx,
            busy: Arc::new(AtomicBool::new(false)),
            again: Arc::new(AtomicBool::new(false)),
            measure_loudness: Arc::new(AtomicBool::new(true)),
            watcher: Mutex::new(None),
        })
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.folders.lock().clone()
    }

    pub fn set_folders(self: &Arc<Self>, f: Vec<PathBuf>) {
        *self.folders.lock() = f;
        self.watch();
        self.scan();
    }

    /// Start a background scan (or queue one if a scan is running).
    pub fn scan(self: &Arc<Self>) {
        if self.busy.swap(true, AcqRel) {
            self.again.store(true, Release);
            return;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                lower_priority();
                loop {
                    me.again.store(false, Release);
                    if let Err(e) = me.scan_once() {
                        let _ = me.tx.send(LibEvent::Error(format!("Scan failed: {e:#}")));
                    }
                    if me.measure_loudness.load(Relaxed) && !me.again.load(Acquire) {
                        if let Err(e) = me.measure() {
                            let _ = me.tx.send(LibEvent::Error(format!("Loudness: {e:#}")));
                        }
                    }
                    if !me.again.load(Acquire) {
                        break;
                    }
                }
                me.busy.store(false, Release);
            })
            .ok();
    }

    fn pool() -> rayon::ThreadPool {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).saturating_sub(1).max(1);
        rayon::ThreadPoolBuilder::new().num_threads(n).start_handler(|_| lower_priority()).build().expect("thread pool")
    }

    fn scan_once(&self) -> Result<()> {
        let mut conn = db::open(&self.db_path)?;
        let folders = self.folders();
        // 1. Walk.
        let mut files: Vec<(PathBuf, i64, i64)> = Vec::new();
        let mut covers: HashMap<PathBuf, PathBuf> = HashMap::new();
        for root in &folders {
            let walk = walkdir::WalkDir::new(root).follow_links(true).into_iter();
            // Skip hidden files and folders (.git, caches, ...).
            let walk = walk.filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
            for e in walk.filter_map(|e| e.ok()) {
                if !e.file_type().is_file() {
                    continue;
                }
                let p = e.path();
                if tags::is_audio(p) {
                    if let Ok(m) = e.metadata() {
                        let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                        files.push((p.to_path_buf(), mtime, m.len() as i64));
                    }
                } else if is_cover(p) {
                    let dir = p.parent().unwrap_or(Path::new("")).to_path_buf();
                    let better = covers.get(&dir).is_none_or(|old| cover_rank(p) < cover_rank(old));
                    if better {
                        covers.insert(dir, p.to_path_buf());
                    }
                }
            }
        }
        // 2. Diff with the database.
        let known: HashMap<String, (i64, i64)> = {
            let mut st = conn.prepare("SELECT path, mtime, size FROM tracks")?;
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?.filter_map(|r| r.ok()).collect()
        };
        let seen: HashSet<String> = files.iter().map(|f| f.0.to_string_lossy().into_owned()).collect();
        let todo: Vec<&(PathBuf, i64, i64)> = files
            .iter()
            .filter(|(p, m, s)| known.get(p.to_string_lossy().as_ref()) != Some(&(*m, *s)))
            .collect();
        let total = todo.len();
        let mut added = 0;
        let _ = self.tx.send(LibEvent::Progress { done: 0, total });
        // 3. Read tags in parallel, write in batches.
        let pool = Self::pool();
        let now = db::now();
        for chunk in todo.chunks(256) {
            let rows: Vec<_> = pool.install(|| chunk.par_iter().map(|(p, m, s)| (p, tags::read(p, *m, *s))).collect());
            let tx = conn.transaction()?;
            for (p, r) in rows {
                match r {
                    Ok(row) => {
                        let dir = p.parent().unwrap_or(Path::new(""));
                        let art = if row.embedded_art { Some("embedded".to_string()) } else { covers.get(dir).map(|c| c.to_string_lossy().into_owned()) };
                        let lyrics = if p.with_extension("lrc").exists() {
                            1
                        } else if row.embedded_lyrics {
                            2
                        } else {
                            0
                        };
                        if known.contains_key(row.path.as_str()) {
                            // updated in place
                        } else {
                            added += 1;
                        }
                        db::upsert(&tx, &row, art.as_deref(), lyrics, now)?;
                    }
                    Err(_) => {
                        // Unreadable tags: keep the file with its name as title.
                        let row = tags::TrackRow {
                            path: p.to_string_lossy().into_owned(),
                            folder: p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
                            title: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                            codec: "?".into(),
                            ..Default::default()
                        };
                        db::upsert(&tx, &row, None, 0, now)?;
                        added += 1;
                    }
                }
            }
            tx.commit()?;
            let _ = self.tx.send(LibEvent::Progress { done: added.min(total), total });
        }
        // 4. Remove files that are gone.
        let gone: Vec<&String> = known.keys().filter(|k| !seen.contains(*k)).collect();
        {
            let tx = conn.transaction()?;
            for g in &gone {
                tx.execute("DELETE FROM tracks WHERE path=?1", [g])?;
            }
            tx.commit()?;
        }
        if total > 0 || !gone.is_empty() || known.is_empty() {
            db::rebuild_albums(&conn)?;
        }
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        let _ = self.tx.send(LibEvent::Done { added, removed: gone.len(), total: count as usize });
        Ok(())
    }

    /// Measure EBU R128 loudness for files without ReplayGain tags.
    fn measure(&self) -> Result<()> {
        let conn = db::open(&self.db_path)?;
        let todo = db::unmeasured(&conn)?;
        if todo.is_empty() {
            return Ok(());
        }
        let total = todo.len();
        let pool = Self::pool();
        let mut done = 0;
        for chunk in todo.chunks(64) {
            if self.again.load(Acquire) || !self.measure_loudness.load(Relaxed) {
                return Ok(());
            }
            let res: Vec<_> = pool.install(|| chunk.par_iter().map(|(id, path, key)| (*id, key.clone(), measure_file(Path::new(path)))).collect());
            let mut albums = HashSet::new();
            conn.execute_batch("BEGIN")?;
            for (id, key, r) in res {
                // Unreadable files get 0 dB so they are not retried forever.
                let (g, p) = r.unwrap_or((0.0, 1.0));
                db::set_measured(&conn, id, g, p)?;
                albums.insert(key);
            }
            for k in albums {
                db::set_measured_album(&conn, &k)?;
            }
            conn.execute_batch("COMMIT")?;
            done += chunk.len();
            let _ = self.tx.send(LibEvent::Loudness { done, total });
        }
        Ok(())
    }

    pub fn unwatch(&self) {
        *self.watcher.lock() = None;
    }

    /// Watch music folders; rescan 2 s after changes stop.
    pub fn watch(self: &Arc<Self>) {
        use notify::{RecursiveMode, Watcher};
        let (tx, rx) = cb::unbounded::<()>();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(e) = res {
                if !matches!(e.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(());
                }
            }
        });
        let Ok(mut w) = w else { return };
        for f in self.folders() {
            let _ = w.watch(&f, RecursiveMode::Recursive);
        }
        *self.watcher.lock() = Some(w);
        let me = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("watch".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Debounce.
                    while rx.recv_timeout(Duration::from_secs(2)).is_ok() {}
                    match me.upgrade() {
                        Some(lib) => lib.scan(),
                        None => return,
                    }
                }
            })
            .ok();
    }
}

fn is_cover(p: &Path) -> bool {
    cover_rank(p) < usize::MAX
}

fn cover_rank(p: &Path) -> usize {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !COVER_EXT.contains(&ext.as_str()) {
        return usize::MAX;
    }
    COVER_NAMES.iter().position(|n| *n == stem).unwrap_or(usize::MAX)
}

/// Returns (ReplayGain 2 gain dB, sample peak).
pub fn measure_file(path: &Path) -> Result<(f32, f32)> {
    use crate::source::Stream;
    let mut s = crate::decode::FileStream::open(path)?;
    let f = s.info().format.clone();
    let mut m = ebur128::EbuR128::new(f.channels as u32, f.sample_rate, ebur128::Mode::I | ebur128::Mode::SAMPLE_PEAK)?;
    let mut buf = Vec::with_capacity(16384);
    loop {
        buf.clear();
        if !s.read(&mut buf)? {
            break;
        }
        m.add_frames_f32(&buf)?;
    }
    let lufs = m.loudness_global()?;
    let peak = (0..f.channels as u32).filter_map(|c| m.sample_peak(c).ok()).fold(0.0f64, f64::max);
    let gain = if lufs.is_finite() { -18.0 - lufs } else { 0.0 };
    Ok((gain as f32, peak as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_loudness_of_known_tone() {
        // 1 kHz sine in both channels.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.flac");
        let st = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000:duration=5", "-af", "volume=0.8", "-ac", "2", "-c:a", "flac"])
            .arg(&p)
            .status()
            .unwrap();
        assert!(st.success());
        let (g, peak) = measure_file(&p).unwrap();
        // A 1 kHz sine in both channels measures its peak level in LUFS (ffmpeg ebur128 agrees).
        assert!(peak > 0.01 && peak < 1.0, "peak {peak}");
        let want = -18.0 - 20.0 * peak.log10();
        assert!((g - want).abs() < 0.1, "gain {g}, want {want}");
    }
}
