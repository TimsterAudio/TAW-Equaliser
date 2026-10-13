//! The settings file that links the Equaliser window to the Windows engine.
//!
//! The window writes the panel to this file; the engine (running inside the
//! Windows audio engine) watches it and glides to the new settings. Because
//! it's a file, the EQ stays set across restarts with the window closed.

use crate::dsp::{BandParams, EqSettings, NUM_BANDS, PREAMP_MAX_DB, PREAMP_MIN_DB};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// Bump only for changes old engines can't read; new fields just need a
/// default.
pub const FORMAT: u32 = 1;
/// What the engine can do. It reports this in its status file, so the window
/// can tell when the installed engine is older than the window and needs
/// installing again. Bump it whenever the window starts sending something
/// older engines can't play (2: high-pass, low-pass and notch filters).
pub const ENGINE_LEVEL: u32 = 2;
/// The status file's line that carries [`ENGINE_LEVEL`].
pub const LEVEL_LINE: &str = "engine level: ";
/// The settings folder under `%ProgramData%`, shared by the family.
pub const DIR: [&str; 2] = ["Timster's Audio Works", "Equaliser"];
pub const FILE_NAME: &str = "engine.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct EngineFile {
    format: u32,
    bands: [BandParams; NUM_BANDS],
    preamp_db: f32,
    bypass: bool,
}

impl Default for EngineFile {
    fn default() -> Self {
        let s = EqSettings::default();
        EngineFile {
            format: FORMAT,
            bands: s.bands,
            preamp_db: s.preamp_db,
            bypass: s.bypass,
        }
    }
}

/// `%ProgramData%\Timster's Audio Works\Equaliser\engine.json`: a place
/// both the user's Equaliser window and the audio engine's service account
/// can reach.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join(DIR[0]).join(DIR[1]).join(FILE_NAME)
}

pub fn to_json(s: &EqSettings) -> String {
    let file = EngineFile {
        format: FORMAT,
        bands: s.bands,
        preamp_db: s.preamp_db,
        bypass: s.bypass,
    };
    serde_json::to_string_pretty(&file).expect("settings serialise")
}

/// Read settings, limited to the panel's ranges. `None` if the text isn't a
/// settings file (or is half-written), so the caller keeps what it has.
pub fn parse(text: &str) -> Option<EqSettings> {
    let file: EngineFile = serde_json::from_str(text).ok()?;
    if file.format > FORMAT {
        return None;
    }
    Some(EqSettings {
        bands: file.bands.map(|b| b.clamped()),
        preamp_db: if file.preamp_db.is_finite() {
            file.preamp_db.clamp(PREAMP_MIN_DB, PREAMP_MAX_DB)
        } else {
            0.0
        },
        bypass: file.bypass,
    })
}

pub fn read(path: &Path) -> Option<EqSettings> {
    read_checked(path).ok()
}

/// Like [`read`], but says why it failed (for the engine's status report).
pub fn read_checked(path: &Path) -> Result<EqSettings, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&text).ok_or_else(|| "not a valid settings file (or half-written)".to_string())
}

/// Write via a temporary file and a rename, so the engine never reads a
/// half-written file.
pub fn write(path: &Path, s: &EqSettings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, to_json(s))?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::FilterKind;

    #[test]
    fn round_trips() {
        let mut s = EqSettings::default();
        s.bands[3].gain_db = -7.5;
        s.bands[3].kind = FilterKind::HighShelf;
        s.bands[8].enabled = false;
        s.preamp_db = -4.0;
        s.bypass = true;
        assert_eq!(parse(&to_json(&s)), Some(s));
    }

    #[test]
    fn rejects_garbage_and_half_written_files() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("{\"format\": 1, \"bands\": ["), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse("{\"format\": 99}"), None);
    }

    #[test]
    fn fills_missing_fields_and_limits_ranges() {
        let s = parse("{\"preamp_db\": 40}").unwrap();
        assert_eq!(s.preamp_db, PREAMP_MAX_DB);
        assert_eq!(s.bands, EqSettings::default().bands);
    }

    #[test]
    fn writes_atomically() {
        let dir = std::env::temp_dir().join(format!("taw-equaliser-test-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let mut s = EqSettings::default();
        s.bands[0].gain_db = 6.0;
        write(&path, &s).unwrap();
        assert_eq!(read(&path), Some(s));
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
