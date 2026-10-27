//! MPRIS on D-Bus (`org.mpris.MediaPlayer2.musinator`): media keys, `playerctl`
//! and the Omarchy media controls. Also desktop notifications.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel as cb;
use mpris_server::zbus::{Result as ZResult, fdo};
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface, Server, Signal, Time, TrackId, Volume,
};
use parking_lot::Mutex;

use crate::engine::{Engine, PlayState};
use crate::model::{Repeat, Track};

pub enum MprisCmd {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Prev,
    /// Relative, microseconds.
    SeekBy(i64),
    /// Absolute, microseconds.
    SetPosition(i64),
    Volume(f64),
    Shuffle(bool),
    Loop(Repeat),
}

/// What the MPRIS thread needs to answer property reads.
#[derive(Clone)]
pub struct EngineView {
    view: crate::engine::EngineView,
    art: Arc<Mutex<Option<PathBuf>>>,
}

impl EngineView {
    pub fn new(e: &Engine) -> Self {
        Self { view: e.view(), art: Arc::new(Mutex::new(None)) }
    }
}

struct Imp {
    v: EngineView,
    tx: cb::Sender<MprisCmd>,
}

enum Update {
    Changed,
    Seeked(Duration),
}

pub struct Mpris {
    pub commands: cb::Receiver<MprisCmd>,
    tx: async_channel::Sender<Update>,
    art: Arc<Mutex<Option<PathBuf>>>,
}

impl Mpris {
    pub fn start(v: EngineView) -> anyhow::Result<Self> {
        let (ctx, commands) = cb::unbounded();
        let (tx, rx) = async_channel::unbounded::<Update>();
        let art = v.art.clone();
        let (ready_tx, ready_rx) = cb::bounded::<Result<(), String>>(1);
        std::thread::Builder::new().name("mpris".into()).spawn(move || {
            async_io::block_on(async move {
                let imp = Imp { v, tx: ctx };
                let server = match Server::new("musinator", imp).await {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                while let Ok(u) = rx.recv().await {
                    match u {
                        Update::Seeked(p) => {
                            let _ = server.emit(Signal::Seeked { position: Time::from_micros(p.as_micros() as i64) }).await;
                        }
                        Update::Changed => {
                            // Coalesce bursts.
                            while rx.try_recv().is_ok() {}
                            let imp = server.imp();
                            // Read the queue once; never hold two guards in one statement.
                            let (shuffle, repeat) = {
                                let q = imp.v.view.queue().lock();
                                (q.shuffle, q.repeat)
                            };
                            let props = vec![
                                Property::PlaybackStatus(imp.status()),
                                Property::Metadata(imp.meta()),
                                Property::Volume(imp.v.view.snapshot().volume as f64),
                                Property::Shuffle(shuffle),
                                Property::LoopStatus(loop_of(repeat)),
                                Property::CanGoNext(true),
                                Property::CanGoPrevious(true),
                            ];
                            let _ = server.properties_changed(props).await;
                        }
                    }
                }
            });
        })?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { commands, tx, art }),
            Ok(Err(e)) => Err(anyhow::anyhow!("media keys can't be turned on (MPRIS): {e}")),
            Err(_) => Err(anyhow::anyhow!("media keys can't be turned on: the desktop message bus (D-Bus) did not answer")),
        }
    }

    pub fn update(&self, _e: &Engine, art: Option<PathBuf>) {
        *self.art.lock() = art;
        let _ = self.tx.try_send(Update::Changed);
    }

    pub fn seeked(&self, p: Duration) {
        let _ = self.tx.try_send(Update::Seeked(p));
    }
}

fn loop_of(r: Repeat) -> LoopStatus {
    match r {
        Repeat::Off => LoopStatus::None,
        Repeat::All => LoopStatus::Playlist,
        Repeat::One => LoopStatus::Track,
    }
}

impl Imp {
    fn status(&self) -> PlaybackStatus {
        match self.v.view.snapshot().state {
            PlayState::Playing => PlaybackStatus::Playing,
            PlayState::Paused => PlaybackStatus::Paused,
            PlayState::Stopped => PlaybackStatus::Stopped,
        }
    }

    fn meta(&self) -> Metadata {
        let s = self.v.view.snapshot();
        let mut m = Metadata::new();
        let Some(t) = s.track else {
            m.set_trackid(Some(TrackId::NO_TRACK));
            return m;
        };
        let id = TrackId::try_from(format!("/org/musinator/track/{}", s.item_id.unwrap_or(0))).unwrap_or(TrackId::NO_TRACK);
        m.set_trackid(Some(id));
        m.set_title(Some(t.display_title().to_string()));
        if !t.artist.is_empty() {
            m.set_artist(Some([t.artist.clone()]));
        }
        if !t.album.is_empty() {
            m.set_album(Some(t.album.clone()));
        }
        if !t.album_artist.is_empty() {
            m.set_album_artist(Some([t.album_artist.clone()]));
        }
        if let Some(n) = t.track_no {
            m.set_track_number(Some(n as i32));
        }
        if let Some(d) = t.duration_ms {
            m.set_length(Some(Time::from_millis(d as i64)));
        }
        if let Some(r) = &t.r {
            m.set_url(Some(format!("file://{}", r.id)));
        }
        if let Some(a) = self.v.art.lock().clone() {
            m.set_art_url(Some(format!("file://{}", a.display())));
        }
        m
    }

    fn send(&self, c: MprisCmd) -> fdo::Result<()> {
        let _ = self.tx.send(c);
        Ok(())
    }
}

impl RootInterface for Imp {
    async fn raise(&self) -> fdo::Result<()> {
        Ok(())
    }
    async fn quit(&self) -> fdo::Result<()> {
        Ok(())
    }
    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn set_fullscreen(&self, _: bool) -> ZResult<()> {
        Ok(())
    }
    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn identity(&self) -> fdo::Result<String> {
        Ok("Musinator".into())
    }
    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok("musinator".into())
    }
    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(vec!["file".into()])
    }
    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(["audio/flac", "audio/mpeg", "audio/mp4", "audio/ogg", "audio/opus", "audio/wav"].map(String::from).to_vec())
    }
}

impl PlayerInterface for Imp {
    async fn next(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Next)
    }
    async fn previous(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Prev)
    }
    async fn pause(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Pause)
    }
    async fn play_pause(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Toggle)
    }
    async fn stop(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Stop)
    }
    async fn play(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Play)
    }
    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        self.send(MprisCmd::SeekBy(offset.as_micros()))
    }
    async fn set_position(&self, _track_id: TrackId, position: Time) -> fdo::Result<()> {
        self.send(MprisCmd::SetPosition(position.as_micros()))
    }
    async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("OpenUri is not supported".into()))
    }
    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(self.status())
    }
    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(loop_of(self.v.view.queue().lock().repeat))
    }
    async fn set_loop_status(&self, l: LoopStatus) -> ZResult<()> {
        let r = match l {
            LoopStatus::None => Repeat::Off,
            LoopStatus::Playlist => Repeat::All,
            LoopStatus::Track => Repeat::One,
        };
        let _ = self.tx.send(MprisCmd::Loop(r));
        Ok(())
    }
    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn set_rate(&self, _: PlaybackRate) -> ZResult<()> {
        Ok(())
    }
    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(self.v.view.queue().lock().shuffle)
    }
    async fn set_shuffle(&self, on: bool) -> ZResult<()> {
        let _ = self.tx.send(MprisCmd::Shuffle(on));
        Ok(())
    }
    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(self.meta())
    }
    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.v.view.snapshot().volume as f64)
    }
    async fn set_volume(&self, v: Volume) -> ZResult<()> {
        let _ = self.tx.send(MprisCmd::Volume(v.clamp(0.0, 1.0)));
        Ok(())
    }
    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_micros(self.v.view.position().as_micros() as i64))
    }
    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

/// Desktop notification for a new track (off the UI thread).
pub fn notify(t: &Track) {
    let title = t.display_title().to_string();
    let body = if t.album.is_empty() { t.artist.clone() } else { format!("{} — {}", t.artist, t.album) };
    let icon = t.art.as_ref().map(crate::app::art::cache_file).filter(|p| p.exists()).map(|p| p.display().to_string()).unwrap_or_default();
    // Replace the previous song's notification instead of stacking a new one.
    static LAST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    use std::sync::atomic::Ordering::Relaxed;
    std::thread::spawn(move || {
        let Ok(c) = mpris_server::zbus::blocking::Connection::session() else { return };
        let hints: std::collections::HashMap<&str, mpris_server::zbus::zvariant::Value> = std::collections::HashMap::new();
        let reply = c.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Musinator", LAST.load(Relaxed), icon, title, body, Vec::<&str>::new(), hints, 4000i32),
        );
        if let Some(id) = reply.ok().and_then(|r| r.body().deserialize::<u32>().ok()) {
            LAST.store(id, Relaxed);
        }
    });
}
