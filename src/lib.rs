//! The code behind EQ Box, the equaliser. (The SP-10 Spectrum Analyser is
//! now its own program: TimsterAudio/TAW-Analyser.)

// Index loops read best in the per-sample DSP code.
#![allow(clippy::needless_range_loop)]

pub mod audio;
pub mod icon;
pub mod ui;

pub use eq_core::{dsp, preset, shared};
pub use taw_panel::analyzer;
