//! Audio backends. Every backend feeds interleaved f32 buffers through one
//! [`Processor`]; the UI only ever talks to [`Shared`].

mod cpal_backend;
pub mod engine_setup;
#[cfg(target_os = "linux")]
mod pipewire_sink;
#[cfg(windows)]
mod windows_engine;

use crate::dsp::Equalizer;
use crate::shared::{AnalyzerTap, Shared};
use std::sync::Arc;

/// Backend id for EQ Box's own PipeWire node.
pub const PIPEWIRE_SINK: &str = "pipewire-sink";
/// Backend id for the system-wide engine inside the Windows audio engine.
pub const WINDOWS_ENGINE: &str = "windows-engine";

/// EQ, meters and analyser feed for one running stream.
pub struct Processor {
    eq: Equalizer,
    shared: Arc<Shared>,
    tap: AnalyzerTap,
}

impl Processor {
    pub fn new(sample_rate: u32, channels: usize, shared: Arc<Shared>, tap: AnalyzerTap) -> Self {
        shared
            .sample_rate
            .store(sample_rate, std::sync::atomic::Ordering::Relaxed);
        Processor {
            eq: Equalizer::new(sample_rate as f64, channels),
            shared,
            tap,
        }
    }

    pub fn channels(&self) -> usize {
        self.eq.channels()
    }

    /// Start again at a new sample rate or channel count.
    #[allow(dead_code)] // only the PipeWire backend renegotiates formats
    pub fn reconfigure(&mut self, sample_rate: u32, channels: usize) {
        if sample_rate as f64 != self.eq.sample_rate() || channels != self.eq.channels() {
            self.shared
                .sample_rate
                .store(sample_rate, std::sync::atomic::Ordering::Relaxed);
            self.eq = Equalizer::new(sample_rate as f64, channels);
        }
    }

    /// Process interleaved audio in place.
    pub fn run(&mut self, buf: &mut [f32]) {
        let ch = self.eq.channels();
        self.shared.meter(buf, ch, false);
        let settings = self.shared.load();
        self.eq.process(buf, &settings);
        self.shared.meter(buf, ch, true);
        self.tap.push(buf, ch);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendInfo {
    pub id: String,
    pub label: String,
    /// The backend picks its own input (the PipeWire sink is the input).
    pub fixed_input: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceLists {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

pub fn backends() -> Vec<BackendInfo> {
    let mut v = Vec::new();
    #[cfg(target_os = "linux")]
    v.push(BackendInfo {
        id: PIPEWIRE_SINK.into(),
        label: "PipeWire sink".into(),
        fixed_input: true,
    });
    #[cfg(windows)]
    v.push(BackendInfo {
        id: WINDOWS_ENGINE.into(),
        label: "Windows (system-wide)".into(),
        fixed_input: true,
    });
    v.extend(cpal_backend::hosts());
    v
}

pub fn devices(backend: &str) -> Result<DeviceLists, String> {
    #[cfg(target_os = "linux")]
    if backend == PIPEWIRE_SINK {
        return Ok(DeviceLists {
            inputs: Vec::new(),
            outputs: pipewire_sink::outputs(),
        });
    }
    #[cfg(windows)]
    if backend == WINDOWS_ENGINE {
        // The engine runs inside Windows; "Output" picks which device the
        // meters and analyser listen to.
        return Ok(DeviceLists {
            inputs: Vec::new(),
            outputs: cpal_backend::devices("WASAPI")?.outputs,
        });
    }
    cpal_backend::devices(backend)
}

#[derive(Clone, Debug)]
pub struct StartRequest {
    pub backend: String,
    pub input: Option<String>,
    pub output: Option<String>,
    /// Buffer between input and output, for backends that need one.
    pub latency_ms: f32,
}

/// A running stream. Dropping it stops the audio.
pub struct Running {
    pub description: String,
    _stop: Box<dyn Send>,
}

impl Running {
    fn new(description: String, stop: impl Send + 'static) -> Self {
        Running {
            description,
            _stop: Box::new(stop),
        }
    }
}

pub fn start(req: &StartRequest, shared: Arc<Shared>, tap: AnalyzerTap) -> Result<Running, String> {
    #[cfg(target_os = "linux")]
    if req.backend == PIPEWIRE_SINK {
        return pipewire_sink::start(req.output.clone(), shared, tap);
    }
    #[cfg(windows)]
    if req.backend == WINDOWS_ENGINE {
        return windows_engine::start(req.output.clone(), shared, tap);
    }
    cpal_backend::start(req, shared, tap)
}
