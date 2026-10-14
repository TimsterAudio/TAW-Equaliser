//! The Equaliser as a PipeWire node: a virtual "TAW Equaliser" sink that any app can play
//! into, and a playback stream that sends the equalised audio on to a real
//! output. This is the same two-stream shape as `pw-loopback`.

use super::{DeviceInfo, Processor, Running};
use crate::shared::{AnalyzerTap, Shared};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::Pod;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc};
use std::time::Duration;

const CHANNELS: usize = 2;
const STRIDE: usize = CHANNELS * std::mem::size_of::<f32>();
pub const NODE_NAME: &str = "taw_equaliser";

/// Real outputs the Equaliser can send to, by PipeWire node name.
pub fn outputs() -> Vec<DeviceInfo> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let Ok(host) = cpal::host_from_id(cpal::HostId::PipeWire) else {
        return Vec::new();
    };
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| {
            let id = d.id().ok()?;
            let node = id.id().to_string();
            if node.ends_with("_default")
                || node.starts_with(NODE_NAME)
                || node.starts_with("taw-equaliser")
                || node.starts_with("eq_box")
                || node.starts_with("eq-box")
            {
                return None;
            }
            let label = d
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| node.clone());
            Some(DeviceInfo { id: node, label })
        })
        .collect()
}

pub fn start(
    target: Option<String>,
    shared: Arc<Shared>,
    tap: AnalyzerTap,
) -> Result<Running, String> {
    let (quit_tx, quit_rx) = pw::channel::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let thread = std::thread::Builder::new()
        .name("taw-equaliser-pipewire".into())
        .spawn(move || {
            if let Err(e) = run(target, shared, tap, quit_rx, &ready_tx) {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    let stop = Stop {
        quit: Some(quit_tx),
        thread: Some(thread),
    };
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(Running::new(
            "PipeWire · the TAW Equaliser output".into(),
            stop,
        )),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("PipeWire didn't answer. Is it running?".into()),
    }
}

struct Stop {
    quit: Option<pw::channel::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Stop {
    fn drop(&mut self) {
        if let Some(q) = self.quit.take() {
            let _ = q.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn format_pod() -> Vec<u8> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_channels(CHANNELS as u32);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .expect("audio format pod")
    .0
    .into_inner()
}

struct SinkData {
    proc: Processor,
    ring: rtrb::Producer<f32>,
    scratch: Vec<f32>,
    shared: Arc<Shared>,
    graph_rate: Arc<AtomicU32>,
}

struct PlaybackData {
    ring: rtrb::Consumer<f32>,
    filling: bool,
    shared: Arc<Shared>,
}

fn run(
    target: Option<String>,
    shared: Arc<Shared>,
    tap: AnalyzerTap,
    quit_rx: pw::channel::Receiver<()>,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();
    let pw_err = |what: &str| {
        let what = what.to_string();
        move |e: pw::Error| format!("{what}: {e}")
    };
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err("PipeWire main loop"))?;
    let context =
        pw::context::ContextRc::new(&mainloop, None).map_err(pw_err("PipeWire context"))?;
    let core = context
        .connect_rc(None)
        .map_err(pw_err("Couldn't connect to PipeWire (is it running?)"))?;

    let weak = mainloop.downgrade();
    let _quit = quit_rx.attach(mainloop.loop_(), move |_| {
        if let Some(ml) = weak.upgrade() {
            ml.quit();
        }
    });

    // The link group stops the session manager from wiring our own output
    // back into our own sink, even when the Equaliser is the default output.
    let group = format!("taw-equaliser-{}", std::process::id());

    let sink = pw::stream::StreamBox::new(
        &core,
        "TAW Equaliser",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CLASS => "Audio/Sink",
            *pw::keys::NODE_NAME => NODE_NAME,
            *pw::keys::NODE_DESCRIPTION => "TAW Equaliser",
            *pw::keys::NODE_VIRTUAL => "true",
            *pw::keys::NODE_LINK_GROUP => group.as_str(),
            "audio.position" => "FL,FR",
        },
    )
    .map_err(pw_err("Couldn't create the TAW Equaliser sink"))?;

    let mut playback_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "DSP",
        *pw::keys::NODE_NAME => "taw_equaliser.output",
        *pw::keys::NODE_DESCRIPTION => "TAW Equaliser output",
        *pw::keys::NODE_PASSIVE => "true",
        *pw::keys::NODE_LINK_GROUP => group.as_str(),
        "audio.position" => "FL,FR",
    };
    if let Some(t) = &target {
        playback_props.insert(*pw::keys::TARGET_OBJECT, t.as_str());
    }
    let playback = pw::stream::StreamBox::new(&core, "TAW Equaliser output", playback_props)
        .map_err(pw_err("Couldn't create the TAW Equaliser output"))?;

    // 8192 frames is far more than any graph quantum.
    let (ring_tx, ring_rx) = rtrb::RingBuffer::<f32>::new(8192 * CHANNELS);

    // The format arrives on the main-loop thread while `process` runs on the
    // real-time thread, so they get separate listeners and share only this.
    let graph_rate = Arc::new(AtomicU32::new(0));
    let rate_for_format = graph_rate.clone();
    let _format_listener = sink
        .add_local_listener_with_user_data(())
        .param_changed(move |_, _, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut info = AudioInfoRaw::new();
            if info.parse(param).is_ok() && info.rate() > 0 {
                rate_for_format.store(info.rate(), Relaxed);
            }
        })
        .register()
        .map_err(pw_err("PipeWire listener"))?;

    let sink_data = SinkData {
        proc: Processor::new(48_000, CHANNELS, shared.clone(), tap),
        ring: ring_tx,
        scratch: vec![0.0; 8192 * CHANNELS],
        shared: shared.clone(),
        graph_rate,
    };
    let _sink_listener = sink
        .add_local_listener_with_user_data(sink_data)
        .process(|stream, data| {
            let rate = data.graph_rate.load(Relaxed);
            if rate > 0 {
                data.proc.reconfigure(rate, CHANNELS);
            }
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(d) = datas.first_mut() else { return };
            let offset = d.chunk().offset() as usize;
            let size = d.chunk().size() as usize;
            let Some(bytes) = d.data() else { return };
            let end = (offset + size).min(bytes.len());
            let bytes = &bytes[offset.min(end)..end];
            let n = (bytes.len() / STRIDE * CHANNELS).min(data.scratch.len());
            let buf = &mut data.scratch[..n];
            for (s, b) in buf.iter_mut().zip(bytes.as_chunks::<4>().0) {
                *s = f32::from_le_bytes(*b);
            }
            data.proc.run(buf);
            if data.ring.slots() < n {
                data.shared.dropouts.fetch_add(1, Relaxed);
            }
            for s in buf.iter() {
                if data.ring.push(*s).is_err() {
                    break;
                }
            }
        })
        .register()
        .map_err(pw_err("PipeWire listener"))?;

    let _playback_listener = playback
        .add_local_listener_with_user_data(PlaybackData {
            ring: ring_rx,
            filling: true,
            shared: shared.clone(),
        })
        .process(|stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let requested = buffer.requested() as usize;
            let datas = buffer.datas_mut();
            let Some(d) = datas.first_mut() else { return };
            let Some(bytes) = d.data() else { return };
            let mut frames = bytes.len() / STRIDE;
            if requested > 0 {
                frames = frames.min(requested);
            }
            let want = frames * CHANNELS;
            let avail = data.ring.slots();
            // Wait for one buffer's worth before starting, so the sink and
            // the output can run in either order within a graph cycle.
            if data.filling && avail >= want {
                data.filling = false;
            }
            // Never let the queue grow beyond a couple of buffers.
            if avail > want * 3 {
                if let Ok(c) = data.ring.read_chunk((avail - want) / CHANNELS * CHANNELS) {
                    c.commit_all();
                }
            }
            for (i, b) in bytes[..want * 4]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let v = if data.filling {
                    0.0
                } else {
                    match data.ring.pop() {
                        Ok(v) => v,
                        Err(_) => {
                            if !data.filling {
                                data.filling = true;
                                // A wholly empty queue just means nothing is
                                // playing into the sink; running dry midway is
                                // a real dropout.
                                if i > 0 {
                                    data.shared.dropouts.fetch_add(1, Relaxed);
                                }
                            }
                            0.0
                        }
                    }
                };
                b.copy_from_slice(&v.to_le_bytes());
            }
            let chunk = d.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = STRIDE as i32;
            *chunk.size_mut() = (want * 4) as u32;
        })
        .register()
        .map_err(pw_err("PipeWire listener"))?;

    let flags = pw::stream::StreamFlags::AUTOCONNECT
        | pw::stream::StreamFlags::MAP_BUFFERS
        | pw::stream::StreamFlags::RT_PROCESS;
    let pod = format_pod();
    let mut params = [Pod::from_bytes(&pod).expect("format pod")];
    sink.connect(spa::utils::Direction::Input, None, flags, &mut params)
        .map_err(pw_err("Couldn't start the TAW Equaliser sink"))?;
    let mut params = [Pod::from_bytes(&pod).expect("format pod")];
    playback
        .connect(spa::utils::Direction::Output, None, flags, &mut params)
        .map_err(pw_err("Couldn't start the TAW Equaliser output"))?;

    let _ = ready.send(Ok(()));
    mainloop.run();
    let _ = sink.disconnect();
    let _ = playback.disconnect();
    Ok(())
}
