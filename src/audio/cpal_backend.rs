//! WASAPI and ASIO on Windows, ALSA and PipeWire devices on Linux, all via
//! cpal: an input stream and an output stream joined by a ring buffer.

use super::{BackendInfo, DeviceInfo, DeviceLists, Processor, Running, StartRequest};
use crate::shared::{AnalyzerTap, Shared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, HostId, SampleFormat, SizedSample, StreamConfig};
use std::str::FromStr;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{mpsc, Arc};

fn host_label(id: HostId) -> String {
    match id.name() {
        "PipeWire" => "PipeWire devices".into(),
        other => other.to_string(),
    }
}

pub fn hosts() -> Vec<BackendInfo> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| BackendInfo {
            id: id.name().to_string(),
            label: host_label(id),
            fixed_input: false,
        })
        .collect()
}

pub(super) fn host(name: &str) -> Result<cpal::Host, String> {
    let id = HostId::from_str(name).map_err(|_| format!("Unknown audio system \"{name}\""))?;
    cpal::host_from_id(id).map_err(|e| format!("{name} isn't available: {e}"))
}

pub(super) fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Unnamed device".into())
}

pub fn devices(backend: &str) -> Result<DeviceLists, String> {
    let host = host(backend)?;
    let mut lists = DeviceLists::default();
    let all = host.devices().map_err(|e| e.to_string())?;
    let is_wasapi = backend.eq_ignore_ascii_case("wasapi");
    for d in all {
        let Ok(id) = d.id() else { continue };
        let name = device_name(&d);
        // The lists already start with "Default input/output", and the Equaliser's
        // own PipeWire node can't feed itself.
        let raw = id.id();
        if raw.starts_with("default_")
            || raw.ends_with("_default")
            || name.starts_with("default_")
            || raw.starts_with("taw_equaliser")
            || raw.starts_with("eq_box")
            || raw.starts_with("eq-box")
        {
            continue;
        }
        // No loopback: on WASAPI an output device opened as an input captures
        // what it's playing, which feeds straight back. Only real inputs.
        if d.supports_input() && !(is_wasapi && d.supports_output()) {
            lists.inputs.push(DeviceInfo {
                id: id.to_string(),
                label: name.clone(),
            });
        }
        if d.supports_output() {
            lists.outputs.push(DeviceInfo {
                id: id.to_string(),
                label: name,
            });
        }
    }
    Ok(lists)
}

pub(super) fn find_device(
    host: &cpal::Host,
    id: Option<&str>,
    output: bool,
) -> Result<cpal::Device, String> {
    match id {
        Some(id) => {
            let parsed = cpal::DeviceId::from_str(id).map_err(|e| e.to_string())?;
            host.device_by_id(&parsed)
                .ok_or_else(|| "The chosen device isn't connected any more".to_string())
        }
        None => if output {
            host.default_output_device()
        } else {
            host.default_input_device()
        }
        .ok_or_else(|| {
            format!(
                "No default {} device",
                if output { "output" } else { "input" }
            )
        }),
    }
}

pub fn start(req: &StartRequest, shared: Arc<Shared>, tap: AnalyzerTap) -> Result<Running, String> {
    // Some hosts' streams can't move between threads, so a dedicated thread
    // owns them until the returned handle is dropped.
    let req = req.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("taw-equaliser-audio".into())
        .spawn(move || match open(&req, shared, tap) {
            Ok((streams, desc)) => {
                let _ = ready_tx.send(Ok(desc));
                let _ = stop_rx.recv();
                drop(streams);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    let desc = ready_rx
        .recv()
        .map_err(|_| "The audio thread stopped unexpectedly".to_string())??;
    Ok(Running::new(
        desc,
        StopOnDrop {
            stop: Some(stop_tx),
            thread: Some(thread),
        },
    ))
}

struct StopOnDrop {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

macro_rules! with_sample_type {
    ($fmt:expr, $T:ident => $body:expr) => {
        match $fmt {
            SampleFormat::F32 => {
                type $T = f32;
                $body
            }
            SampleFormat::F64 => {
                type $T = f64;
                $body
            }
            SampleFormat::I16 => {
                type $T = i16;
                $body
            }
            SampleFormat::I32 => {
                type $T = i32;
                $body
            }
            SampleFormat::U16 => {
                type $T = u16;
                $body
            }
            SampleFormat::U8 => {
                type $T = u8;
                $body
            }
            other => Err(format!("Sample format {other} isn't supported")),
        }
    };
}

fn open(
    req: &StartRequest,
    shared: Arc<Shared>,
    tap: AnalyzerTap,
) -> Result<(Vec<cpal::Stream>, String), String> {
    let host = host(&req.backend)?;
    let out_dev = find_device(&host, req.output.as_deref(), true)?;
    let in_dev = find_device(&host, req.input.as_deref(), false)?;
    // Never capture an output (WASAPI would turn it into loopback, which
    // feeds back), even if an old setting still names one.
    if req.backend.eq_ignore_ascii_case("wasapi") && in_dev.supports_output() {
        return Err("That input is an output device. The Equaliser doesn't capture outputs (loopback), because it can feed back. Choose a real input.".into());
    }

    let out_supported = out_dev.default_output_config().map_err(|e| e.to_string())?;
    let rate = out_supported.sample_rate();
    let out_cfg: StreamConfig = out_supported.config();

    let in_supported = in_dev.default_input_config().map_err(|e| e.to_string())?;
    let in_supported = if in_supported.sample_rate() == rate {
        in_supported
    } else {
        in_dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?
            .filter(|r| r.channels() == in_supported.channels())
            .find_map(|r| r.try_with_sample_rate(rate))
            .ok_or_else(|| {
                format!(
                    "The input runs at {} Hz and can't match the output's {rate} Hz. Set both devices to the same sample rate.",
                    in_supported.sample_rate()
                )
            })?
    };
    let in_cfg: StreamConfig = in_supported.config();
    let in_ch = in_cfg.channels as usize;
    let out_ch = out_cfg.channels as usize;

    let target = ((req.latency_ms.max(1.0) / 1000.0) * rate as f32) as usize * in_ch;
    let (mut ring_tx, ring_rx) = rtrb::RingBuffer::<f32>::new(target * 4 + 16384 * in_ch);
    for _ in 0..target {
        let _ = ring_tx.push(0.0);
    }

    let proc = Processor::new(rate, out_ch, shared.clone(), tap);
    let err_shared = shared.clone();
    let on_error = move |e: cpal::Error| {
        err_shared.dropouts.fetch_add(1, Relaxed);
        eprintln!("audio stream error: {e}");
    };

    let input = with_sample_type!(in_supported.sample_format(), T => {
        build_input::<T>(&in_dev, &in_cfg, ring_tx, shared.clone(), on_error.clone())
    })?;
    let output = with_sample_type!(out_supported.sample_format(), T => {
        build_output::<T>(&out_dev, &out_cfg, ring_rx, in_ch, target, proc, shared.clone(), on_error.clone())
    })?;
    input.play().map_err(|e| e.to_string())?;
    output.play().map_err(|e| e.to_string())?;

    let desc = format!(
        "{} · {} → {}",
        req.backend,
        device_name(&in_dev),
        device_name(&out_dev)
    );
    Ok((vec![input, output], desc))
}

fn build_input<T>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    mut ring: rtrb::Producer<f32>,
    shared: Arc<Shared>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    dev.build_input_stream(
        *cfg,
        move |data: &[T], _| {
            let free = ring.slots();
            if free < data.len() {
                shared.dropouts.fetch_add(1, Relaxed);
            }
            for s in &data[..data.len().min(free)] {
                let _ = ring.push(f32::from_sample_(*s));
            }
        },
        on_error,
        None,
    )
    .map_err(|e| format!("Couldn't open the input: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn build_output<T>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    mut ring: rtrb::Consumer<f32>,
    in_ch: usize,
    target: usize,
    mut proc: Processor,
    shared: Arc<Shared>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let out_ch = proc.channels();
    let mut scratch: Vec<f32> = vec![0.0; 8192 * out_ch];
    dev.build_output_stream(
        *cfg,
        move |data: &mut [T], _| {
            // Different clocks drift apart: if the input has run ahead,
            // skip back to the target latency.
            let queued = ring.slots();
            if queued > target * 3 + data.len() {
                let skip = (queued - target) / in_ch * in_ch;
                if let Ok(chunk) = ring.read_chunk(skip) {
                    chunk.commit_all();
                }
            }
            if scratch.len() < data.len() {
                scratch.resize(data.len(), 0.0); // only if the host's buffer grew
            }
            let buf = &mut scratch[..data.len()];
            let frames = data.len() / out_ch;
            let mut short = false;
            for f in 0..frames {
                let frame = &mut buf[f * out_ch..(f + 1) * out_ch];
                if ring.slots() < in_ch {
                    frame.fill(0.0);
                    short = true;
                    continue;
                }
                let mut first = 0.0;
                for c in 0..in_ch {
                    let v = ring.pop().unwrap_or(0.0);
                    if c == 0 {
                        first = v;
                    }
                    if c < out_ch {
                        frame[c] = v;
                    }
                }
                for c in in_ch..out_ch {
                    frame[c] = if in_ch == 1 { first } else { 0.0 };
                }
            }
            if short {
                shared.dropouts.fetch_add(1, Relaxed);
            }
            proc.run(buf);
            for (o, s) in data.iter_mut().zip(buf.iter()) {
                *o = T::from_sample_(*s);
            }
        },
        on_error,
        None,
    )
    .map_err(|e| format!("Couldn't open the output: {e}"))
}
