//! Installing and checking the Windows system-wide engine from the app.
//!
//! Setup itself is the PowerShell scripts in the `system-wide-engine`
//! folder next to `taw-equaliser.exe`. The app only starts them (Windows asks for
//! administrator permission) and watches for the engine.

use eq_core::engine_settings::{ENGINE_LEVEL, LEVEL_LINE};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    /// The engine DLL isn't installed.
    NotInstalled,
    /// Installed, but not running inside Windows audio right now (no app
    /// has played on its output since Windows audio started).
    Idle,
    /// Running inside Windows audio.
    Active,
    /// Running, but an older engine than this Equaliser: it can't play
    /// everything the panel sends (such as the cut filters). Installing
    /// again updates it.
    Outdated,
}

/// The engine's report (`engine-status.txt`) is rewritten every second
/// while it runs.
const ALIVE: Duration = Duration::from_secs(4);

pub fn engine_state() -> EngineState {
    if !cfg!(windows) {
        return EngineState::NotInstalled;
    }
    let dll = std::env::var_os("ProgramFiles").map(|p| {
        std::path::PathBuf::from(p)
            .join("Timster's Audio Works")
            .join("Equaliser")
            .join("taw_equaliser_apo.dll")
    });
    if !dll.is_some_and(|p| p.exists()) {
        return EngineState::NotInstalled;
    }
    let status = eq_core::engine_settings::default_path().with_file_name("engine-status.txt");
    let fresh = std::fs::metadata(&status)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < ALIVE);
    if !fresh {
        return EngineState::Idle;
    }
    // Engines from before levels were reported have no level line.
    let level = std::fs::read_to_string(&status)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.trim()
                    .strip_prefix(LEVEL_LINE)?
                    .trim()
                    .parse::<u32>()
                    .ok()
            })
        })
        .unwrap_or(0);
    if level < ENGINE_LEVEL {
        EngineState::Outdated
    } else {
        EngineState::Active
    }
}

/// Which setup script to start.
#[derive(Clone, Copy, Debug)]
pub enum Setup {
    Install,
    Remove,
}

/// Starts a setup script in its own administrator PowerShell window. The
/// returned process ends once Windows' permission prompt is answered (see
/// [`launch_result`]).
#[cfg(windows)]
pub fn launch(what: Setup) -> Result<std::process::Child, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let script = match what {
        Setup::Install => "install-engine.ps1",
        Setup::Remove => "remove-engine.ps1",
    };
    let dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("system-wide-engine"))
        .ok_or("Can't tell where the Equaliser is")?;
    let path = dir.join(script);
    if !path.exists() {
        return Err(format!(
            "Can't find {}. Keep the system-wide-engine folder next to taw-equaliser.exe.",
            path.display()
        ));
    }
    // The path goes through an environment variable, so no quoting can
    // break it. Windows shows its permission prompt for the new window.
    std::process::Command::new("powershell")
        .env("EQBOX_SCRIPT", &path)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Start-Process powershell -Verb RunAs -ArgumentList \
             ('-NoProfile -ExecutionPolicy Bypass -NoExit -File ' + [char]34 + $env:EQBOX_SCRIPT + [char]34)",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Couldn't start PowerShell: {e}"))
}

#[cfg(not(windows))]
pub fn launch(_what: Setup) -> Result<std::process::Child, String> {
    Err("The system-wide engine is for Windows.".into())
}

/// `None` while the permission prompt is up; then whether setup started.
pub fn launch_result(child: &mut std::process::Child) -> Option<Result<(), String>> {
    match child.try_wait() {
        Ok(None) => None,
        Ok(Some(s)) if s.success() => Some(Ok(())),
        Ok(Some(_)) => Some(Err(
            "Windows didn't give administrator permission, so nothing was changed.".into(),
        )),
        Err(e) => Some(Err(e.to_string())),
    }
}
