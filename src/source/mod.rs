//! The `Source` interface: every place music comes from implements it.
//!
//! v1 has one source, [`local::LocalSource`] (files on disk, catalog in SQLite).
//! v2 (HTTP radio, Subsonic/Navidrome) and v3 (Spotify) add new implementations;
//! the engine, queue, playlists and UI only ever talk to these traits.
//!
//! All `Source` methods may block (disk, network). They are only called from
//! worker threads, never from the UI thread.

pub mod local;
pub mod radio;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bitflags::bitflags;

use crate::model::{ArtRef, AudioFormat, SourceId, Track, TrackRef};

bitflags! {
    /// What a source can do. The UI hides actions a source does not support.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Caps: u32 {
        const BROWSE    = 1 << 0;
        const SEARCH    = 1 << 1;
        /// Streams can seek.
        const SEEK      = 1 << 2;
        /// Endless streams (radio). No duration, no seek.
        const LIVE      = 1 << 3;
        /// Source has its own playlists.
        const PLAYLISTS = 1 << 4;
        const LYRICS    = 1 << 5;
        const ART       = 1 << 6;
    }
}

/// Opaque, source-specific id of a node in the browse tree ("albums", "album/17", ...).
pub type NodeId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Artist,
    Album,
    Genre,
    Folder,
    Playlist,
}

/// A browsable entry that is not a track.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub title: String,
    /// Second line / right-hand info, e.g. "Neon Harbor · 2024".
    pub subtitle: String,
    pub count: Option<u64>,
    pub art: Option<ArtRef>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Node(Node),
    Track(Arc<Track>),
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}

impl Page {
    pub const ALL: Page = Page { offset: 0, limit: usize::MAX };
}

/// Search hits, grouped.
#[derive(Clone, Debug, Default)]
pub struct SearchHits {
    pub artists: Vec<Node>,
    pub albums: Vec<Node>,
    pub tracks: Vec<Arc<Track>>,
    pub playlists: Vec<Node>,
}

/// Lyrics: synced lines (ms, text) when available, else plain text.
#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub synced: Vec<(u64, String)>,
    /// Word times per synced line (empty when the line has none).
    pub words: Vec<Vec<(u64, String)>>,
    pub plain: Option<String>,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.synced.is_empty() && self.plain.as_deref().is_none_or(|p| p.trim().is_empty())
    }
}

/// One place music comes from.
pub trait Source: Send + Sync {
    fn id(&self) -> &SourceId;
    fn caps(&self) -> Caps;

    /// Children of a browse node. `""` is the root.
    fn children(&self, node: &str, page: Page) -> Result<Vec<Item>>;
    fn search(&self, query: &str, limit: usize) -> Result<SearchHits>;
    /// Full metadata for one track.
    fn track(&self, r: &TrackRef) -> Result<Track>;

    /// Open a track for playback.
    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>>;
    /// Encoded image bytes (JPEG/PNG/...).
    fn art(&self, a: &ArtRef) -> Result<Option<Vec<u8>>>;
    fn lyrics(&self, r: &TrackRef) -> Result<Option<Lyrics>>;
}

/// Static facts about an open stream.
#[derive(Clone, Debug, Default)]
pub struct StreamInfo {
    pub format: AudioFormat,
    /// Total frames, when known. None for live streams.
    pub frames: Option<u64>,
}

impl StreamInfo {
    pub fn duration(&self) -> Option<Duration> {
        let rate = self.format.sample_rate.max(1) as f64;
        self.frames.map(|f| Duration::from_secs_f64(f as f64 / rate))
    }
}

/// Decoded PCM from any source: interleaved f32 at the stream's own rate.
pub trait Stream: Send {
    fn info(&self) -> &StreamInfo;
    /// Append the next block of interleaved samples to `out`.
    /// Returns `false` at the end of the stream.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool>;
    /// Seek; returns the frame position actually reached.
    fn seek(&mut self, to: Duration) -> Result<u64>;
    /// New song info inside a live stream (radio, v2).
    fn meta_update(&mut self) -> Option<Track> {
        None
    }
}

/// All sources, by id.
#[derive(Default)]
pub struct Sources {
    map: HashMap<SourceId, Arc<dyn Source>>,
}

impl Sources {
    pub fn add(&mut self, s: Arc<dyn Source>) {
        self.map.insert(s.id().clone(), s);
    }
    pub fn get(&self, id: &str) -> Result<&Arc<dyn Source>> {
        self.map.get(id).ok_or_else(|| anyhow::anyhow!("unknown source {id}"))
    }
    pub fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        self.get(&r.source)?.open(r)
    }
}
