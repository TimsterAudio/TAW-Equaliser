//! Milestones and problems, sent to the Windows Event Log (Application,
//! source "TAW Equaliser") and to the debugger output. The Windows audio engine
//! runs with a restricted token that may not write files, but the
//! Application log accepts write-restricted processes.
//!
//! Only start-up, stream starts and problems go to the event log, so it
//! stays quiet in normal use; `Debug` messages go to the debugger output
//! only (DebugView shows them).
//!
//! Never call this from `APOProcess`: it does system calls and allocates.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::EventLog::{
    DeregisterEventSource, RegisterEventSourceW, ReportEventW, EVENTLOG_ERROR_TYPE,
    EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE,
};
use windows_core::{w, HSTRING, PCWSTR};

#[derive(Clone, Copy)]
pub enum Level {
    /// Debugger output only.
    Debug,
    Info,
    Warning,
    Error,
}

pub fn log(level: Level, msg: &str) {
    let text = HSTRING::from(format!(
        "TAW Equaliser engine (process {}): {msg}",
        std::process::id()
    ));
    let kind = match level {
        Level::Debug => {
            unsafe { OutputDebugStringW(&text) };
            return;
        }
        Level::Info => EVENTLOG_INFORMATION_TYPE,
        Level::Warning => EVENTLOG_WARNING_TYPE,
        Level::Error => EVENTLOG_ERROR_TYPE,
    };
    unsafe {
        OutputDebugStringW(&text);
        if let Ok(source) = RegisterEventSourceW(PCWSTR::null(), w!("TAW Equaliser")) {
            let strings = [PCWSTR(text.as_ptr())];
            let _ = ReportEventW(source, kind, 0, 1000, None, 0, Some(&strings), None);
            let _ = DeregisterEventSource(source);
        }
    }
}

/// Log at most `max` times over the life of the process (for things that
/// can repeat, like format queries).
pub fn log_limited(count: &AtomicU32, max: u32, level: Level, msg: &str) {
    if count.fetch_add(1, Relaxed) < max {
        log(level, msg);
    }
}
