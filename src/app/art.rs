//! Album art in the background: load, decode, resize and encode for the
//! terminal off the UI thread. Also keeps a JPEG copy on disk for MPRIS.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel as cb;
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

use crate::model::ArtRef;
use crate::source::Sources;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Lower-right panel on screen A.
    Panel,
    /// Big cover on screen B.
    Big,
    /// Album grid cell.
    Grid,
}

pub struct ArtReq {
    pub art: ArtRef,
    pub cells: Size,
    pub slot: Slot,
}

pub struct ArtReady {
    pub art: ArtRef,
    pub cells: Size,
    pub slot: Slot,
    pub proto: Option<Protocol>,
    /// JPEG on disk (for MPRIS `mpris:artUrl`).
    pub file: Option<PathBuf>,
}

pub struct ArtWorker {
    tx: cb::Sender<ArtReq>,
}

pub fn cache_file(a: &ArtRef) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    crate::config::cache_dir().join("art").join(format!("{:016x}.jpg", h.finish()))
}

impl ArtWorker {
    pub fn start(sources: Arc<Sources>, picker: Picker, out: cb::Sender<ArtReady>) -> Self {
        let (tx, rx) = cb::unbounded::<ArtReq>();
        std::thread::Builder::new()
            .name("art".into())
            .spawn(move || {
                let mut mem: HashMap<ArtRef, Option<Arc<DynamicImage>>> = HashMap::new();
                let mut order: VecDeque<ArtRef> = VecDeque::new();
                while let Ok(first) = rx.recv() {
                    // Newest request per slot wins (skip stale ones while scrolling).
                    let mut reqs = vec![first];
                    reqs.extend(rx.try_iter());
                    let mut seen = std::collections::HashSet::new();
                    let mut todo = Vec::new();
                    for r in reqs.into_iter().rev() {
                        if r.slot == Slot::Grid || seen.insert(r.slot) {
                            todo.push(r);
                        }
                    }
                    todo.reverse();
                    for r in todo {
                        let img = match mem.get(&r.art) {
                            Some(i) => i.clone(),
                            None => {
                                let img = load(&sources, &r.art).map(Arc::new);
                                mem.insert(r.art.clone(), img.clone());
                                order.push_back(r.art.clone());
                                if order.len() > 96
                                    && let Some(old) = order.pop_front()
                                {
                                    mem.remove(&old);
                                }
                                img
                            }
                        };
                        let file = img.as_ref().and_then(|i| save_jpeg(&r.art, i));
                        let proto = img.as_ref().and_then(|i| encode(&picker, i, r.cells));
                        let _ = out.send(ArtReady { art: r.art, cells: r.cells, slot: r.slot, proto, file });
                    }
                }
            })
            .ok();
        Self { tx }
    }

    pub fn request(&self, art: ArtRef, cells: Size, slot: Slot) {
        let _ = self.tx.send(ArtReq { art, cells, slot });
    }
}

fn load(sources: &Sources, a: &ArtRef) -> Option<DynamicImage> {
    let bytes = sources.get(&a.source).ok()?.art(a).ok()??;
    image::load_from_memory(&bytes).ok()
}

fn save_jpeg(a: &ArtRef, img: &DynamicImage) -> Option<PathBuf> {
    let p = cache_file(a);
    if p.exists() {
        return Some(p);
    }
    std::fs::create_dir_all(p.parent()?).ok()?;
    let small = img.thumbnail(512, 512).to_rgb8();
    let tmp = p.with_extension("tmp");
    small.save_with_format(&tmp, image::ImageFormat::Jpeg).ok()?;
    std::fs::rename(&tmp, &p).ok()?;
    Some(p)
}

/// Resize to the exact pixel size of the cell area (keeping the square-ish cover centred) and encode.
fn encode(picker: &Picker, img: &DynamicImage, cells: Size) -> Option<Protocol> {
    if cells.width == 0 || cells.height == 0 {
        return None;
    }
    let fs = picker.font_size();
    let (pw, ph) = (cells.width as u32 * fs.width as u32, cells.height as u32 * fs.height as u32);
    let side = pw.min(ph);
    let resized = img.resize_to_fill(side, side, image::imageops::FilterType::Triangle);
    picker.new_protocol(resized, cells, Resize::Fit(None)).ok()
}
