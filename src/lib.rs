//! The code behind the TAW Equaliser (EQ-001). (The spectrum analyser that
//! used to come with it is now the TAW Analyser: TimsterAudio/TAW-Analyser.)

// Index loops read best in the per-sample DSP code.
#![allow(clippy::needless_range_loop)]

pub mod audio;
pub mod icon;
pub mod ui;

pub use eq_core::{dsp, preset, shared};
pub use taw_panel::analyzer;
