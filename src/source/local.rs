//! Local files. Catalog comes from the library database.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use super::{Caps, Item, Lyrics, Page, SearchHits, Source, Stream};
use crate::decode::FileStream;
use crate::model::{ArtRef, SourceId, Track, TrackRef};

pub const ID: &str = "local";

pub struct LocalSource {
    id: SourceId,
}

impl LocalSource {
    pub fn new() -> Self {
        Self { id: Arc::from(ID) }
    }
}

impl Source for LocalSource {
    fn id(&self) -> &SourceId {
        &self.id
    }
    fn caps(&self) -> Caps {
        Caps::BROWSE | Caps::SEARCH | Caps::SEEK | Caps::PLAYLISTS | Caps::LYRICS | Caps::ART
    }
    fn children(&self, _node: &str, _page: Page) -> Result<Vec<Item>> {
        Ok(Vec::new())
    }
    fn search(&self, _query: &str, _limit: usize) -> Result<SearchHits> {
        Ok(SearchHits::default())
    }
    fn track(&self, r: &TrackRef) -> Result<Track> {
        Ok(Track { r: Some(r.clone()), ..Default::default() })
    }
    fn open(&self, r: &TrackRef) -> Result<Box<dyn Stream>> {
        Ok(Box::new(FileStream::open(Path::new(&r.id))?))
    }
    fn art(&self, _a: &ArtRef) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn lyrics(&self, _r: &TrackRef) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}
