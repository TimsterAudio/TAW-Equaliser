//! What the engine is doing, for diagnosing it on a user's machine.
//!
//! The audio thread only bumps atomic counters. The settings thread writes
//! them once a second to `engine-status.txt`, next to the settings file. If
//! that file never appears, Windows never loaded the engine.

use eq_core::shared::Shared;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

pub const FILE_NAME: &str = "engine-status.txt";

pub static CREATED: AtomicU32 = AtomicU32::new(0);
pub static INITIALIZED: AtomicU32 = AtomicU32::new(0);
pub static LOCKED: AtomicU32 = AtomicU32::new(0);
pub static RATE: AtomicU32 = AtomicU32::new(0);
pub static CHANNELS: AtomicU32 = AtomicU32::new(0);
pub static BUFFERS: AtomicU64 = AtomicU64::new(0);
pub static SILENT_BUFFERS: AtomicU64 = AtomicU64::new(0);
pub static SETTINGS_READS: AtomicU64 = AtomicU64::new(0);
pub static SETTINGS_UNREADABLE: AtomicU64 = AtomicU64::new(0);
pub static FORMAT_QUERIES: AtomicU32 = AtomicU32::new(0);
pub static LOCK_FAILURES: AtomicU32 = AtomicU32::new(0);
pub static FORMATS_REFUSED: AtomicU32 = AtomicU32::new(0);
pub static FORMATS_REFUSED_LOGGED: AtomicU32 = AtomicU32::new(0);
pub static FORMATS_ACCEPTED_LOGGED: AtomicU32 = AtomicU32::new(0);
pub static STREAMS_LOGGED: AtomicU32 = AtomicU32::new(0);
/// Why the settings file last failed to read (settings thread only).
pub static LAST_READ_ERROR: Mutex<String> = Mutex::new(String::new());

pub fn set_read_error(e: &str) {
    if let Ok(mut s) = LAST_READ_ERROR.lock() {
        s.clear();
        s.push_str(e);
    }
}
/// The last refused format: (bits << 16) | (container bytes << 8) | float.
pub static LAST_REFUSED: AtomicU32 = AtomicU32::new(0);

pub fn refused(valid_bits: u32, container_bytes: u32, is_float: bool) {
    FORMATS_REFUSED.fetch_add(1, Relaxed);
    LAST_REFUSED.store(
        ((valid_bits & 0xff) << 16) | ((container_bytes & 0xff) << 8) | is_float as u32,
        Relaxed,
    );
}

pub fn report(shared: &Shared, settings_path: &std::path::Path) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = shared.load();
    let mut bands = String::new();
    for (i, b) in s.bands.iter().enumerate() {
        if b.enabled && b.gain_db != 0.0 {
            let _ = write!(bands, " {}:{:+.1}dB@{:.0}Hz", i + 1, b.gain_db, b.freq);
        }
    }
    if bands.is_empty() {
        bands = " flat".into();
    }
    let refused = LAST_REFUSED.load(Relaxed);
    format!(
        "TAW Equaliser engine status (written by the Windows audio engine)\r\n\
         updated (unix time): {now}\r\n\
         process id: {pid}\r\n\
         engine instances created: {created}, initialised: {init}\r\n\
         streams running: {locked}   format: {rate} Hz, {ch} ch\r\n\
         buffers processed: {bufs}   silent buffers passed through: {silent}\r\n\
         settings file: {path}\r\n\
         settings reads: {reads}   unreadable: {bad}\r\n\
         current EQ:{bands}   preamp {pre:+.1} dB   bypass {bypass}\r\n\
         last settings error: {err}\r\n\
         format queries: {queries}   formats refused: {nref} (last: {bits}-bit in {cont}-byte container, float {float})\r\n\
         lock failures: {lockfail}\r\n\
         running as: {user}   PROGRAMDATA: {pd}   temp: {temp}\r\n\
         {level_line}{level}\r\n",
        level_line = eq_core::engine_settings::LEVEL_LINE,
        level = eq_core::engine_settings::ENGINE_LEVEL,
        pid = std::process::id(),
        err = LAST_READ_ERROR.lock().map(|s| if s.is_empty() { "none".to_string() } else { s.clone() }).unwrap_or_default(),
        queries = FORMAT_QUERIES.load(Relaxed),
        lockfail = LOCK_FAILURES.load(Relaxed),
        user = std::env::var("USERNAME").unwrap_or_else(|_| "?".into()),
        pd = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "(not set)".into()),
        temp = std::env::temp_dir().display(),
        created = CREATED.load(Relaxed),
        init = INITIALIZED.load(Relaxed),
        locked = LOCKED.load(Relaxed),
        rate = RATE.load(Relaxed),
        ch = CHANNELS.load(Relaxed),
        bufs = BUFFERS.load(Relaxed),
        silent = SILENT_BUFFERS.load(Relaxed),
        path = settings_path.display(),
        reads = SETTINGS_READS.load(Relaxed),
        bad = SETTINGS_UNREADABLE.load(Relaxed),
        pre = s.preamp_db,
        bypass = s.bypass,
        nref = FORMATS_REFUSED.load(Relaxed),
        bits = refused >> 16,
        cont = (refused >> 8) & 0xff,
        float = refused & 1 == 1,
    )
}
