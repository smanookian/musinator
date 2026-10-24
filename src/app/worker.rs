//! Background jobs for the UI: catalog queries, search, lyrics, play counts,
//! playlist edits. The UI thread never touches the database.

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel as cb;

use crate::library::{db, m3u8};
use crate::model::{Track, TrackRef};
use crate::source::local::LocalSource;
use crate::source::{Item, Lyrics, Page, SearchHits, Source};

/// What to do with resolved tracks.
#[derive(Clone, Debug)]
pub enum Then {
    Play { from: String },
    Queue { from: String },
    PlayNext { from: String },
    AddToPlaylist(i64),
}

pub enum Job {
    Children {
        seq: u64,
        node: String,
    },
    Search {
        seq: u64,
        query: String,
    },
    Lyrics {
        track: TrackRef,
    },
    Tracks {
        nodes: Vec<String>,
        tracks: Vec<Arc<Track>>,
        then: Then,
    },
    RecordPlay(TrackRef),
    FetchLyrics {
        track: Arc<Track>,
        next_to_song: bool,
    },
    Rate {
        track: TrackRef,
        rating: Option<u8>,
    },
    LyricsOffset {
        track: TrackRef,
        ms: i64,
    },
    /// Resolve saved queue paths back to tracks (resume).
    Restore(crate::app::session::Session),
    Like(TrackRef),
    Playlists,
    NewPlaylist {
        name: String,
        nodes: Vec<String>,
        tracks: Vec<Arc<Track>>,
    },
    RenamePlaylist {
        id: i64,
        name: String,
    },
    DeletePlaylist(i64),
    RemoveFromPlaylist {
        id: i64,
        pos: i64,
    },
    MoveInPlaylist {
        id: i64,
        a: i64,
        b: i64,
    },
    Export {
        id: i64,
        path: PathBuf,
    },
    Import {
        path: PathBuf,
    },
}

pub enum Msg {
    Children {
        seq: u64,
        items: Result<Vec<Item>, String>,
    },
    Search {
        seq: u64,
        hits: SearchHits,
    },
    Lyrics {
        track: TrackRef,
        lyrics: Option<Lyrics>,
        offset: i64,
    },
    Restored {
        session: crate::app::session::Session,
        tracks: Vec<Arc<Track>>,
    },
    Rated,
    /// An online lyrics lookup finished (found or not).
    LookupDone,
    Tracks {
        tracks: Vec<Arc<Track>>,
        then: Then,
    },
    Playlists(Vec<db::Playlist>),
    Liked(bool),
    Info(String),
    Error(String),
    /// A playlist changed: reload views showing it.
    PlaylistChanged(i64),
}

pub struct Worker {
    tx: cb::Sender<Job>,
}

impl Worker {
    pub fn start(local: Arc<LocalSource>, out: cb::Sender<Msg>) -> Self {
        let (tx, rx) = cb::unbounded::<Job>();
        std::thread::Builder::new()
            .name("ui-worker".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Only the newest search/children request matters.
                    let mut jobs = vec![first];
                    jobs.extend(rx.try_iter());
                    let last_search = jobs.iter().rposition(|j| matches!(j, Job::Search { .. }));
                    for (i, j) in jobs.into_iter().enumerate() {
                        if matches!(j, Job::Search { .. }) && Some(i) != last_search {
                            continue;
                        }
                        run(&local, j, &out);
                    }
                }
            })
            .ok();
        Self { tx }
    }

    pub fn send(&self, j: Job) {
        let _ = self.tx.send(j);
    }
}

fn tracks_of(local: &LocalSource, node: &str, depth: u32, out: &mut Vec<Arc<Track>>) {
    let Ok(items) = local.children(node, Page::ALL) else { return };
    for it in items {
        match it {
            Item::Track(t) => out.push(t),
            Item::Node(n) if depth > 0 => tracks_of(local, &n.id, depth - 1, out),
            Item::Node(_) => {}
        }
    }
}

fn run(local: &LocalSource, job: Job, out: &cb::Sender<Msg>) {
    let send = |m: Msg| {
        let _ = out.send(m);
    };
    let err = |e: anyhow::Error| Msg::Error(format!("{e:#}"));
    match job {
        Job::Children { seq, node } => send(Msg::Children { seq, items: local.children(&node, Page::ALL).map_err(|e| format!("{e:#}")) }),
        Job::Search { seq, query } => send(Msg::Search { seq, hits: local.search(&query, 50).unwrap_or_default() }),
        Job::Lyrics { track } => {
            let lyrics = local.lyrics(&track).ok().flatten();
            let offset = db::lyrics_offset(&local.conn(), &track.source, &track.id);
            send(Msg::Lyrics { track, lyrics, offset });
        }
        Job::Tracks { nodes, mut tracks, then } => {
            for n in nodes {
                tracks_of(local, &n, 3, &mut tracks);
            }
            match then {
                Then::AddToPlaylist(id) => {
                    let n = tracks.len();
                    match db::add_to_playlist(&mut local.conn(), id, &tracks) {
                        Ok(()) => {
                            send(Msg::Info(format!("Added {n} to playlist")));
                            send(Msg::PlaylistChanged(id));
                            send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
                        }
                        Err(e) => send(err(e)),
                    }
                }
                then => send(Msg::Tracks { tracks, then }),
            }
        }
        Job::RecordPlay(r) => {
            if let Err(e) = db::record_play(&local.conn(), &r.source, &r.id) {
                send(err(e));
            }
        }
        Job::FetchLyrics { track, next_to_song } => {
            let r = track.tref().clone();
            match crate::library::lrclib::fetch(&track) {
                Ok(Some(text)) => {
                    if let Err(e) = crate::library::lrclib::store(&r.id, &text, next_to_song) {
                        send(err(e));
                    }
                    let lyrics = Some(crate::library::lrc::parse(&text)).filter(|l| !l.is_empty());
                    let offset = db::lyrics_offset(&local.conn(), &r.source, &r.id);
                    let synced = lyrics.as_ref().is_some_and(|l| !l.synced.is_empty());
                    send(Msg::Lyrics { track: r, lyrics, offset });
                    send(Msg::Info(if synced { "Lyrics found (synced)".into() } else { "Lyrics found (not synced)".into() }));
                }
                Ok(None) => send(Msg::Info("No lyrics found online for this song".into())),
                Err(e) => send(Msg::Error(format!("Lyrics lookup failed: {e:#}"))),
            }
            send(Msg::LookupDone);
        }
        Job::Rate { track, rating } => match db::set_rating(&local.conn(), &track.source, &track.id, rating) {
            Ok(()) => send(Msg::Rated),
            Err(e) => send(err(e)),
        },
        Job::LyricsOffset { track, ms } => {
            if let Err(e) = db::set_lyrics_offset(&local.conn(), &track.source, &track.id, ms) {
                send(err(e));
            }
        }
        Job::Restore(session) => {
            let src = local.id().clone();
            let tracks: Vec<Arc<Track>> = session
                .items
                .iter()
                .filter(|i| std::path::Path::new(&i.path).exists())
                .map(|i| {
                    let r = TrackRef { source: src.clone(), id: i.path.clone() };
                    Arc::new(local.track(&r).unwrap_or_else(|_| Track { r: Some(r), ..Default::default() }))
                })
                .collect();
            send(Msg::Restored { session, tracks });
        }
        Job::Like(r) => match db::toggle_like(&local.conn(), &r.source, &r.id) {
            Ok(on) => send(Msg::Liked(on)),
            Err(e) => send(err(e)),
        },
        Job::Playlists => send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default())),
        Job::NewPlaylist { name, nodes, mut tracks } => {
            for n in nodes {
                tracks_of(local, &n, 3, &mut tracks);
            }
            let res = (|| {
                let mut c = local.conn();
                let id = db::create_playlist(&c, &name)?;
                db::add_to_playlist(&mut c, id, &tracks)?;
                anyhow::Ok(id)
            })();
            match res {
                Ok(_) => {
                    send(Msg::Info(format!("Playlist \"{name}\" created")));
                    send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
                }
                Err(e) => send(err(e)),
            }
        }
        Job::RenamePlaylist { id, name } => {
            let _ = db::rename_playlist(&local.conn(), id, &name);
            send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
        }
        Job::DeletePlaylist(id) => {
            let _ = db::delete_playlist(&local.conn(), id);
            send(Msg::Info("Playlist deleted".into()));
            send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
        }
        Job::RemoveFromPlaylist { id, pos } => {
            let _ = db::remove_from_playlist(&mut local.conn(), id, pos);
            send(Msg::PlaylistChanged(id));
            send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
        }
        Job::MoveInPlaylist { id, a, b } => {
            let _ = db::move_in_playlist(&mut local.conn(), id, a, b);
            send(Msg::PlaylistChanged(id));
        }
        Job::Export { id, path } => {
            let tracks = db::playlist_tracks(&local.conn(), local.id(), id).unwrap_or_default();
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            match m3u8::export(&path, &tracks) {
                Ok(()) => send(Msg::Info(format!("Saved {}", path.display()))),
                Err(e) => send(err(e)),
            }
        }
        Job::Import { path } => {
            let res = (|| {
                let entries = m3u8::import(&path)?;
                let src = local.id().clone();
                let tracks: Vec<Arc<Track>> = entries
                    .into_iter()
                    .map(|p| {
                        let r = TrackRef { source: src.clone(), id: p };
                        Arc::new(local.track(&r).unwrap_or_else(|_| Track { r: Some(r), ..Default::default() }))
                    })
                    .collect();
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into());
                let mut c = local.conn();
                let id = db::create_playlist(&c, &name)?;
                db::add_to_playlist(&mut c, id, &tracks)?;
                anyhow::Ok((name, tracks.len()))
            })();
            match res {
                Ok((name, n)) => {
                    send(Msg::Info(format!("Imported \"{name}\" ({n} songs)")));
                    send(Msg::Playlists(db::playlists(&local.conn()).unwrap_or_default()));
                }
                Err(e) => send(err(e)),
            }
        }
    }
}
