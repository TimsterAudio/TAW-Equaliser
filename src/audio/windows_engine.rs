//! "Windows (system-wide)": the Equaliser's engine runs inside the Windows audio
//! engine (see `crates/taw-equaliser-apo`), so nothing here touches the audio path.
//! This side writes the panel to the engine's settings file whenever it
//! changes, and listens to the output through WASAPI loopback (read-only)
//! for the meters and analyser.

use super::cpal_backend::{device_name, find_device, host};
use super::Running;
use crate::shared::{AnalyzerTap, Shared};
use cpal::traits::{DeviceTrait, StreamTrait};
use eq_core::engine_settings;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// How often the panel is checked for changes to send to the engine.
const TICK: Duration = Duration::from_millis(40);

pub fn start(
    output: Option<String>,
    shared: Arc<Shared>,
    tap: AnalyzerTap,
) -> Result<Running, String> {
    let path = engine_settings::default_path();
    engine_settings::write(&path, &shared.load()).map_err(|e| {
        format!(
            "Couldn't write the engine's settings to {}: {e}. Is the system-wide engine installed?",
            path.display()
        )
    })?;

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<String>();
    let thread = std::thread::Builder::new()
        .name("taw-equaliser-engine-link".into())
        .spawn(move || {
            // cpal streams may not move between threads, so this thread
            // owns the loopback stream as well as writing the settings.
            let monitor = open_loopback(output.as_deref(), shared.clone(), tap);
            let desc = match &monitor {
                Ok((_, name)) => format!("WINDOWS ENGINE  {name}"),
                Err(e) => {
                    eprintln!("loopback meter unavailable: {e}");
                    "WINDOWS ENGINE  (NO METERS)".to_string()
                }
            };
            let _ = ready_tx.send(desc);
            let mut written = shared.load();
            while let Err(mpsc::RecvTimeoutError::Timeout) = stop_rx.recv_timeout(TICK) {
                let now = shared.load();
                // On failure, keep `written` stale so it's retried.
                if now != written && engine_settings::write(&path, &now).is_ok() {
                    written = now;
                }
            }
            drop(monitor);
        })
        .map_err(|e| e.to_string())?;
    let desc = ready_rx
        .recv()
        .map_err(|_| "The engine link stopped unexpectedly".to_string())?;
    Ok(Running::new(
        desc,
        Stop {
            stop: Some(stop_tx),
            thread: Some(thread),
        },
    ))
}

struct Stop {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Stop {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Capture what an output is playing (WASAPI loopback) to drive the meters
/// and analyser.
fn open_loopback(
    output: Option<&str>,
    shared: Arc<Shared>,
    mut tap: AnalyzerTap,
) -> Result<(cpal::Stream, String), String> {
    let host = host("WASAPI")?;
    let dev = find_device(&host, output, true)?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    if cfg.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!(
            "loopback format {} isn't supported",
            cfg.sample_format()
        ));
    }
    let ch = cfg.channels() as usize;
    shared.sample_rate.store(cfg.sample_rate(), Relaxed);
    let stream = dev
        .build_input_stream(
            cfg.config(),
            move |data: &[f32], _| {
                // Windows' loopback is the output itself, so it feeds both
                // the In and Out meters.
                shared.meter(data, ch, false);
                shared.meter(data, ch, true);
                tap.push(data, ch);
            },
            |e| eprintln!("loopback error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, device_name(&dev)))
}
