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
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::kitty::Kitty;

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

enum ArtMsg {
    Req(ArtReq),
    /// The picture changed on disk: drop it from the memory and disk caches.
    Forget(Vec<ArtRef>),
}

pub struct ArtWorker {
    tx: cb::Sender<ArtMsg>,
}

pub fn cache_file(a: &ArtRef) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    crate::config::cache_dir().join("art").join(format!("{:016x}.jpg", h.finish()))
}

impl ArtWorker {
    pub fn start(sources: Arc<Sources>, picker: Picker, out: cb::Sender<ArtReady>) -> Self {
        let (tx, rx) = cb::unbounded::<ArtMsg>();
        std::thread::Builder::new()
            .name("art".into())
            .spawn(move || {
                let mut mem: HashMap<ArtRef, Option<Arc<DynamicImage>>> = HashMap::new();
                let mut order: VecDeque<ArtRef> = VecDeque::new();
                while let Ok(first) = rx.recv() {
                    // Newest request per slot wins (skip stale ones while scrolling).
                    let mut reqs = Vec::new();
                    for m in std::iter::once(first).chain(rx.try_iter()) {
                        match m {
                            ArtMsg::Req(r) => reqs.push(r),
                            ArtMsg::Forget(arts) => {
                                for a in arts {
                                    mem.remove(&a);
                                    order.retain(|o| *o != a);
                                    let _ = std::fs::remove_file(cache_file(&a));
                                }
                            }
                        }
                    }
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
                        let id = image_id(r.slot, &r.art);
                        let proto = img.as_ref().and_then(|i| encode(&picker, i, r.cells, id));
                        let _ = out.send(ArtReady { art: r.art, cells: r.cells, slot: r.slot, proto, file });
                    }
                }
            })
            .ok();
        Self { tx }
    }

    pub fn request(&self, art: ArtRef, cells: Size, slot: Slot) {
        let _ = self.tx.send(ArtMsg::Req(ArtReq { art, cells, slot }));
    }

    pub fn forget(&self, arts: Vec<ArtRef>) {
        let _ = self.tx.send(ArtMsg::Forget(arts));
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
fn encode(picker: &Picker, img: &DynamicImage, cells: Size, id: u32) -> Option<Protocol> {
    if cells.width == 0 || cells.height == 0 {
        return None;
    }
    let fs = picker.font_size();
    let (pw, ph) = (cells.width as u32 * fs.width as u32, cells.height as u32 * fs.height as u32);
    let side = pw.min(ph);
    let resized = img.resize_to_fill(side, side, image::imageops::FilterType::Triangle);
    protocol(picker, resized, id)
}

/// Kitty image ids. Re-sending an image with the same id replaces the old one in the
/// terminal, so pictures do not pile up in its memory over a long session.
pub const ID_PANEL: u32 = 0x4d_0001;
pub const ID_BIG: u32 = 0x4d_0002;
pub const ID_LYRIC: u32 = 0x4d_0003;

fn image_id(slot: Slot, art: &ArtRef) -> u32 {
    match slot {
        Slot::Panel => ID_PANEL,
        Slot::Big => ID_BIG,
        // One id per album (bounded): the same cover reuses its slot.
        Slot::Grid => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            art.hash(&mut h);
            0x4d_1000 + (h.finish() % 4096) as u32
        }
    }
}

/// Encode an image that is already sized to whole cells. Kitty gets a fixed id.
pub fn protocol(picker: &Picker, img: DynamicImage, id: u32) -> Option<Protocol> {
    let fs = picker.font_size();
    let cells = Size::new(img.width().div_ceil(fs.width as u32) as u16, img.height().div_ceil(fs.height as u32) as u16);
    if picker.protocol_type() == ProtocolType::Kitty {
        // zlib (o=z): Kitty and Ghostty both read it; transparent areas shrink to almost nothing.
        let compress = true;
        Kitty::new(img, cells, id, picker.tmux_detected(), compress).ok().map(Protocol::Kitty)
    } else {
        picker.new_protocol(img, cells, Resize::Fit(None)).ok()
    }
}
