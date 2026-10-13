//! The parts of the Equaliser with no UI or audio-API code: the filters and the
//! real-time equaliser, presets, the lock-free shared state, and the
//! settings file the Windows engine reads. Both the app and the Windows
//! engine DLL are built on this crate.

// Index loops read best in the per-sample DSP code.
#![allow(clippy::needless_range_loop)]

pub mod dsp;
pub mod engine_settings;
pub mod preset;
pub mod shared;
