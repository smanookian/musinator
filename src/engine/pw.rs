//! Native PipeWire output. Runs its own thread with a PipeWire main loop.
//!
//! The stream opens at the track's own rate and asks PipeWire for that graph
//! rate (`node.rate`). PipeWire resamples when the graph runs at another rate;
//! the real graph rate is read back every cycle into `Shared::graph_rate`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering::*;
use std::thread::JoinHandle;

use anyhow::{Result, anyhow};
use crossbeam_channel as cb;
use pipewire as pw;
use pw::spa;
use pw::spa::pod::Pod;

use super::render::{Renderer, Shared};
use super::{Output, OutputFormat};

enum Cmd {
    Configure(OutputFormat, cb::Sender<Result<(), String>>),
    Active(bool),
    Quit,
}

pub struct PwOutput {
    tx: pw::channel::Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

struct Live {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
}

impl PwOutput {
    pub fn start(renderer: Renderer, shared: Arc<Shared>) -> Result<Self> {
        let (tx, rx) = pw::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = cb::bounded::<Result<(), String>>(1);
        let thread = std::thread::Builder::new().name("pw-output".into()).spawn(move || {
            if let Err(e) = run(rx, renderer, shared, &ready_tx) {
                let _ = ready_tx.send(Err(e.to_string()));
            }
        })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx, thread: Some(thread) }),
            Ok(Err(e)) => Err(anyhow!("PipeWire: {e}")),
            Err(_) => Err(anyhow!("PipeWire output thread died")),
        }
    }
}

fn run(rx: pw::channel::Receiver<Cmd>, renderer: Renderer, shared: Arc<Shared>, ready: &cb::Sender<Result<(), String>>) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let renderer = Rc::new(RefCell::new(renderer));
    let live: Rc<RefCell<Option<Live>>> = Rc::new(RefCell::new(None));

    let ml = mainloop.clone();
    let _attached = rx.attach(mainloop.loop_(), move |cmd| match cmd {
        Cmd::Configure(fmt, ack) => {
            // Drop the old stream first so the device is free to change rate.
            if let Some(old) = live.borrow_mut().take() {
                let _ = old.stream.disconnect();
            }
            let res = connect(&core, fmt, renderer.clone(), shared.clone()).map_err(|e| e.to_string());
            let res = res.map(|l| {
                *live.borrow_mut() = Some(l);
            });
            let _ = ack.send(res);
        }
        Cmd::Active(on) => {
            if let Some(l) = live.borrow().as_ref() {
                let _ = l.stream.set_active(on);
            }
        }
        Cmd::Quit => {
            if let Some(old) = live.borrow_mut().take() {
                let _ = old.stream.disconnect();
            }
            ml.quit();
        }
    });
    let _ = ready.send(Ok(()));
    mainloop.run();
    Ok(())
}

fn connect(core: &pw::core::CoreRc, fmt: OutputFormat, renderer: Rc<RefCell<Renderer>>, shared: Arc<Shared>) -> Result<Live> {
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::APP_NAME => "Musinator",
        *pw::keys::APP_ID => "musinator",
        *pw::keys::APP_ICON_NAME => "audio-x-generic",
        *pw::keys::NODE_NAME => "musinator",
        *pw::keys::NODE_DESCRIPTION => "Musinator",
        *pw::keys::AUDIO_CHANNELS => fmt.channels.to_string(),
        // Ask the graph to run at our rate. Works when PipeWire allows the rate.
        "node.rate" => format!("1/{}", fmt.rate),
    };
    let stream = pw::stream::StreamRc::new(core.clone(), "Musinator", props)?;
    let channels = fmt.channels as usize;
    let rate = fmt.rate;
    shared.stream_rate.store(rate, Relaxed);
    shared.channels.store(fmt.channels as u32, Relaxed);
    let listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            if let Ok(t) = stream.time() {
                let r = t.rate();
                if r.num > 0 && r.denom > 0 {
                    shared.graph_rate.store(r.denom / r.num, Relaxed);
                }
            }
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let requested = buffer.requested() as usize;
            let datas = buffer.datas_mut();
            let data = &mut datas[0];
            let stride = 4 * channels;
            let frames = match data.data() {
                Some(bytes) => {
                    let mut frames = bytes.len() / stride;
                    if requested > 0 {
                        frames = frames.min(requested);
                    }
                    let n = frames * channels;
                    // PipeWire buffers are suitably aligned for f32.
                    let out: &mut [f32] = unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut f32, n) };
                    renderer.borrow_mut().render(out, channels, rate);
                    frames
                }
                None => 0,
            };
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = stride as _;
            *chunk.size_mut() = (stride * frames) as _;
        })
        .register()?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(fmt.rate);
    info.set_channels(fmt.channels as u32);
    info.set_position(positions(fmt.channels));
    let bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map_err(|e| anyhow!("format pod: {e:?}"))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("bad format pod"))?];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    Ok(Live { stream, _listener: listener })
}

fn positions(channels: u16) -> [u32; spa::param::audio::MAX_CHANNELS] {
    use spa::sys::*;
    let mut p = [0u32; spa::param::audio::MAX_CHANNELS];
    let layout: &[u32] = match channels {
        1 => &[SPA_AUDIO_CHANNEL_MONO],
        2 => &[SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR],
        3 => &[SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR, SPA_AUDIO_CHANNEL_LFE],
        4 => &[SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR, SPA_AUDIO_CHANNEL_RL, SPA_AUDIO_CHANNEL_RR],
        5 => &[SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR, SPA_AUDIO_CHANNEL_FC, SPA_AUDIO_CHANNEL_RL, SPA_AUDIO_CHANNEL_RR],
        6 => &[
            SPA_AUDIO_CHANNEL_FL,
            SPA_AUDIO_CHANNEL_FR,
            SPA_AUDIO_CHANNEL_FC,
            SPA_AUDIO_CHANNEL_LFE,
            SPA_AUDIO_CHANNEL_RL,
            SPA_AUDIO_CHANNEL_RR,
        ],
        _ => &[
            SPA_AUDIO_CHANNEL_FL,
            SPA_AUDIO_CHANNEL_FR,
            SPA_AUDIO_CHANNEL_FC,
            SPA_AUDIO_CHANNEL_LFE,
            SPA_AUDIO_CHANNEL_RL,
            SPA_AUDIO_CHANNEL_RR,
            SPA_AUDIO_CHANNEL_SL,
            SPA_AUDIO_CHANNEL_SR,
        ],
    };
    for (i, c) in layout.iter().enumerate().take(channels as usize) {
        p[i] = *c;
    }
    p
}

impl Output for PwOutput {
    fn configure(&self, fmt: OutputFormat) -> Result<()> {
        let (tx, rx) = cb::bounded(1);
        self.tx.send(Cmd::Configure(fmt, tx)).map_err(|_| anyhow!("PipeWire output stopped"))?;
        rx.recv().map_err(|_| anyhow!("PipeWire output stopped"))?.map_err(|e| anyhow!(e))
    }
    fn set_active(&self, on: bool) {
        let _ = self.tx.send(Cmd::Active(on));
    }
}

impl Drop for PwOutput {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
