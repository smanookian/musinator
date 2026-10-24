//! The play queue. Items are kept in play order; shuffle reorders the items
//! after the current one and can be undone.

use std::sync::Arc;

use crate::model::{Repeat, Track};

#[derive(Clone, Debug)]
pub struct QueueItem {
    /// Unique within this queue (stable while items move).
    pub id: u64,
    pub track: Arc<Track>,
    /// Where it came from, e.g. "Late night" (shown as "from: Late night").
    pub from: Option<String>,
}

#[derive(Default)]
pub struct Queue {
    items: Vec<QueueItem>,
    current: Option<u64>,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Order of ids before shuffle was turned on.
    unshuffled: Vec<u64>,
    next_id: u64,
    /// Bumped on every change, so views know to refresh.
    pub version: u64,
    rng: u64,
}

impl Queue {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Self { rng: seed | 1, ..Default::default() }
    }

    fn make(&mut self, tracks: Vec<Arc<Track>>, from: Option<String>) -> Vec<QueueItem> {
        tracks
            .into_iter()
            .map(|track| {
                self.next_id += 1;
                QueueItem { id: self.next_id, track, from: from.clone() }
            })
            .collect()
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    /// Swap in new info for songs (`find` returns it for changed songs).
    pub fn refresh(&mut self, find: &dyn Fn(&Arc<Track>) -> Option<Arc<Track>>) {
        for it in &mut self.items {
            if let Some(t) = find(&it.track) {
                it.track = t;
            }
        }
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn get(&self, id: u64) -> Option<&QueueItem> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }
    pub fn current_id(&self) -> Option<u64> {
        self.current
    }
    pub fn current(&self) -> Option<&QueueItem> {
        self.current.and_then(|id| self.get(id))
    }
    pub fn set_current(&mut self, id: Option<u64>) {
        if self.current != id {
            self.current = id;
            self.changed();
        }
    }

    /// Items after the current one, in play order.
    pub fn up_next(&self) -> &[QueueItem] {
        match self.current.and_then(|id| self.index_of(id)) {
            Some(i) => &self.items[i + 1..],
            None => &self.items,
        }
    }

    /// Replace everything; returns the id of the item at `start`.
    pub fn replace(&mut self, tracks: Vec<Arc<Track>>, from: Option<String>, start: usize) -> Option<u64> {
        self.items = self.make(tracks, from);
        self.current = None;
        self.unshuffled.clear();
        let start_id = self.items.get(start).map(|i| i.id);
        if self.shuffle {
            // Start item first, the rest shuffled.
            if let Some(i) = self.items.iter().position(|x| Some(x.id) == start_id) {
                let it = self.items.remove(i);
                self.items.insert(0, it);
            }
            self.unshuffled = self.items.iter().map(|i| i.id).collect();
            let n = self.items.len();
            self.shuffle_range(1, n);
        }
        self.changed();
        start_id
    }

    /// Put back a saved queue as it was (play order kept, no reshuffle).
    pub fn restore(
        &mut self,
        items: Vec<(Arc<Track>, Option<String>)>,
        current: Option<usize>,
        shuffle: bool,
        repeat: Repeat,
    ) -> Option<u64> {
        self.items = items
            .into_iter()
            .map(|(track, from)| {
                self.next_id += 1;
                QueueItem { id: self.next_id, track, from }
            })
            .collect();
        self.unshuffled.clear();
        self.shuffle = shuffle;
        self.repeat = repeat;
        self.current = None;
        self.changed();
        current.and_then(|i| self.items.get(i)).map(|i| i.id)
    }

    /// Add to the end (`a`).
    pub fn append(&mut self, tracks: Vec<Arc<Track>>, from: Option<String>) {
        let new = self.make(tracks, from);
        if self.shuffle {
            self.unshuffled.extend(new.iter().map(|i| i.id));
        }
        self.items.extend(new);
        self.changed();
    }

    /// Insert right after the current item (`A`, play next).
    pub fn insert_next(&mut self, tracks: Vec<Arc<Track>>, from: Option<String>) {
        let new = self.make(tracks, from);
        if self.shuffle {
            self.unshuffled.extend(new.iter().map(|i| i.id));
        }
        let at = self.current.and_then(|id| self.index_of(id)).map(|i| i + 1).unwrap_or(0);
        self.items.splice(at..at, new);
        self.changed();
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(i) = self.index_of(id) {
            self.items.remove(i);
            self.unshuffled.retain(|x| *x != id);
            if self.current == Some(id) {
                self.current = None;
            }
            self.changed();
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.unshuffled.clear();
        self.current = None;
        self.changed();
    }

    /// Move an item one step earlier (`K`) or later (`J`). Returns false if it can't move.
    pub fn move_by(&mut self, id: u64, delta: isize) -> bool {
        let Some(i) = self.index_of(id) else { return false };
        let j = i as isize + delta;
        if j < 0 || j as usize >= self.items.len() {
            return false;
        }
        self.items.swap(i, j as usize);
        self.changed();
        true
    }

    /// The item to play after `id`. `auto` = reached the end on its own (repeat-one applies).
    pub fn next_after(&self, id: Option<u64>, auto: bool) -> Option<&QueueItem> {
        if auto
            && self.repeat == Repeat::One
            && let Some(it) = id.and_then(|id| self.get(id))
        {
            return Some(it);
        }
        let next = match id.and_then(|id| self.index_of(id)) {
            Some(i) => i + 1,
            None => 0,
        };
        match self.items.get(next) {
            Some(it) => Some(it),
            None if self.repeat != Repeat::Off => self.items.first(),
            None => None,
        }
    }

    pub fn prev_before(&self, id: Option<u64>) -> Option<&QueueItem> {
        let i = id.and_then(|id| self.index_of(id))?;
        if i > 0 {
            self.items.get(i - 1)
        } else if self.repeat != Repeat::Off {
            self.items.last()
        } else {
            None
        }
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.unshuffled = self.items.iter().map(|i| i.id).collect();
            let start = self.current.and_then(|id| self.index_of(id)).map(|i| i + 1).unwrap_or(0);
            let n = self.items.len();
            self.shuffle_range(start, n);
        } else {
            let order = std::mem::take(&mut self.unshuffled);
            let mut by_id: Vec<Option<QueueItem>> = Vec::new();
            let mut rest = std::mem::take(&mut self.items);
            for id in &order {
                if let Some(p) = rest.iter().position(|i| i.id == *id) {
                    by_id.push(Some(rest.remove(p)));
                }
            }
            self.items = by_id.into_iter().flatten().chain(rest).collect();
        }
        self.changed();
    }

    /// Whether the neighbours of `id` are from the same album (album gain in auto mode).
    pub fn album_context(&self, id: u64) -> bool {
        let Some(i) = self.index_of(id) else { return false };
        let same = |j: usize| {
            self.items.get(j).is_some_and(|o| {
                let (a, b) = (&o.track, &self.items[i].track);
                !a.album.is_empty() && a.album == b.album && a.album_artist == b.album_artist
            })
        };
        (i > 0 && same(i - 1)) || same(i + 1)
    }

    fn shuffle_range(&mut self, start: usize, end: usize) {
        if end <= start + 1 {
            return;
        }
        for i in (start + 1..end).rev() {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            let j = start + (self.rng % (i - start + 1) as u64) as usize;
            self.items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: &str) -> Arc<Track> {
        Arc::new(Track { title: n.into(), album: "A".into(), ..Default::default() })
    }
    fn titles(q: &Queue) -> Vec<String> {
        q.items().iter().map(|i| i.track.title.clone()).collect()
    }

    #[test]
    fn play_next_goes_after_current_and_repeat_wraps() {
        let mut q = Queue::new();
        let first = q.replace(vec![t("1"), t("2"), t("3")], None, 0);
        q.set_current(first);
        q.insert_next(vec![t("x")], None);
        assert_eq!(titles(&q), ["1", "x", "2", "3"]);
        let last = q.items()[3].id;
        assert!(q.next_after(Some(last), false).is_none());
        q.repeat = Repeat::All;
        assert_eq!(q.next_after(Some(last), false).unwrap().track.title, "1");
        q.repeat = Repeat::One;
        assert_eq!(q.next_after(Some(last), true).unwrap().id, last);
        assert_eq!(q.next_after(Some(last), false).unwrap().track.title, "1");
    }

    #[test]
    fn restore_keeps_order_labels_and_modes() {
        let mut q = Queue::new();
        let items = vec![(t("b"), Some("Mix".into())), (t("a"), None), (t("c"), Some("Mix".into()))];
        let id = q.restore(items, Some(1), true, Repeat::All).unwrap();
        assert_eq!(titles(&q), ["b", "a", "c"]);
        assert_eq!(q.get(id).unwrap().track.title, "a");
        assert_eq!(q.items()[2].from.as_deref(), Some("Mix"));
        assert!(q.shuffle);
        assert_eq!(q.repeat, Repeat::All);
    }

    #[test]
    fn shuffle_keeps_current_and_restores_order() {
        let mut q = Queue::new();
        let tracks: Vec<_> = (0..30).map(|i| t(&i.to_string())).collect();
        let id = q.replace(tracks, None, 5);
        q.set_current(id);
        let before = titles(&q);
        q.set_shuffle(true);
        assert_eq!(q.current().unwrap().track.title, "5");
        assert_eq!(&titles(&q)[..6], &before[..6]);
        assert_ne!(titles(&q), before);
        q.set_shuffle(false);
        assert_eq!(titles(&q), before);
    }
}
