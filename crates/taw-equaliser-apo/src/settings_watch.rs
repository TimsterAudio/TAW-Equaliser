//! Keeps the engine's settings in step with the file the the Equaliser window
//! writes. One thread per process, shared by every engine instance.

use crate::events::{self, Level};
use crate::status;
use eq_core::dsp::EqSettings;
use eq_core::engine_settings;
use eq_core::shared::Shared;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

const POLL: Duration = Duration::from_millis(100);
/// Write the status file every this many polls (once a second).
const STATUS_EVERY: u32 = 10;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

/// The settings every engine instance reads, starting the watcher the first
/// time it's asked for.
pub fn shared() -> Arc<Shared> {
    SHARED
        .get_or_init(|| {
            let path = engine_settings::default_path();
            let initial = match engine_settings::read_checked(&path) {
                Ok(s) => {
                    status::SETTINGS_READS.fetch_add(1, Relaxed);
                    events::log(
                        Level::Debug,
                        &format!("read settings from {}", path.display()),
                    );
                    s
                }
                Err(e) => {
                    status::SETTINGS_UNREADABLE.fetch_add(1, Relaxed);
                    status::set_read_error(&e);
                    events::log(
                        Level::Error,
                        &format!("cannot read settings {}: {e}", path.display()),
                    );
                    EqSettings::default()
                }
            };
            let shared = Arc::new(Shared::new(&initial));
            let watched = shared.clone();
            // If the thread can't start, the engine still runs on the
            // settings it read just now.
            let _ = std::thread::Builder::new()
                .name("taw-equaliser-settings".into())
                .spawn(move || watch(path, watched));
            shared
        })
        .clone()
}

fn watch(path: std::path::PathBuf, shared: Arc<Shared>) {
    let status_path = path.with_file_name(status::FILE_NAME);
    let mut seen: Option<SystemTime> = modified(&path);
    let mut ticks = 0u32;
    let mut status_write_failed = false;
    let mut audio_logged = false;
    let mut last_error = String::new();
    loop {
        std::thread::sleep(POLL);
        ticks += 1;
        if ticks.is_multiple_of(STATUS_EVERY) {
            // Best effort: diagnostics must never disturb the audio. If the
            // settings folder refuses the write, fall back to this
            // process's own temp folder.
            let report = status::report(&shared, &path);
            if let Err(e) = std::fs::write(&status_path, &report) {
                let fallback = std::env::temp_dir().join(status::FILE_NAME);
                let note = format!("could not write {}: {e}\r\n", status_path.display());
                if !status_write_failed {
                    status_write_failed = true;
                    events::log(
                        Level::Warning,
                        &format!("{}; trying {}", note.trim_end(), fallback.display()),
                    );
                }
                let _ = std::fs::write(fallback, report + &note);
            }
            // Say once when audio starts flowing through the engine.
            let buffers = status::BUFFERS.load(Relaxed);
            if !audio_logged && buffers > 0 {
                audio_logged = true;
                events::log(
                    Level::Info,
                    &format!("audio is flowing through the engine ({buffers} buffers so far)"),
                );
            }
        }
        let now = modified(&path);
        if now == seen {
            continue;
        }
        // A half-written or unreadable file is skipped and retried on the
        // next tick, keeping the current settings meanwhile.
        match engine_settings::read_checked(&path) {
            Ok(s) => {
                shared.store(&s);
                status::SETTINGS_READS.fetch_add(1, Relaxed);
                status::set_read_error("");
                if !last_error.is_empty() {
                    events::log(Level::Info, "settings readable again");
                } else {
                    events::log(Level::Debug, "settings updated from the Equaliser window");
                }
                last_error.clear();
                seen = now;
            }
            Err(_) if now.is_none() => {
                // The file was deleted: go flat rather than keep a stale EQ.
                shared.store(&EqSettings::default());
                seen = now;
            }
            Err(e) => {
                status::SETTINGS_UNREADABLE.fetch_add(1, Relaxed);
                status::set_read_error(&e);
                if e != last_error {
                    events::log(
                        Level::Error,
                        &format!("cannot read settings {}: {e}", path.display()),
                    );
                    last_error = e;
                }
            }
        }
    }
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
