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
    /// Installed, but on no output that's connected: a Windows update, a
    /// new driver or a device plugged in elsewhere can drop it.
    NoOutput,
    /// Installed on a connected output, but that output has Windows' Audio
    /// enhancements turned off, which skips the engine.
    EnhancementsOff,
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
        return idle_state(&engine_outputs());
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

/// An output the engine is installed on, as the registry has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineOutput {
    /// Connected and enabled (DEVICE_STATE_ACTIVE).
    pub active: bool,
    /// Audio enhancements turned off for it.
    pub enhancements_off: bool,
}

/// Why an installed engine isn't running: on no connected output, only on
/// outputs with Audio enhancements off, or just waiting for sound.
pub fn idle_state(outputs: &[EngineOutput]) -> EngineState {
    let active: Vec<_> = outputs.iter().filter(|o| o.active).collect();
    if active.is_empty() {
        EngineState::NoOutput
    } else if active.iter().all(|o| o.enhancements_off) {
        EngineState::EnhancementsOff
    } else {
        EngineState::Idle
    }
}

/// The outputs the engine is installed on: playback devices whose endpoint
/// effect (PKEY_FX_EndpointEffectClsid) is the engine's CLSID, as
/// `engine-common.ps1` sets it.
#[cfg(windows)]
pub fn engine_outputs() -> Vec<EngineOutput> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::*;

    const RENDER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";
    // Never changes: installed systems point at it.
    const CLSID: &str = "{0FEBC01A-A301-491F-BBCD-17284A13CB61}";
    const PKEY_EFX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},7";
    const PKEY_SYSFX_OFF: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn get_dword(key: &str, value: &str) -> Option<u32> {
        let (k, v) = (wide(key), wide(value));
        let mut data = 0u32;
        let mut size = 4u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                k.as_ptr(),
                v.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (r == ERROR_SUCCESS).then_some(data)
    }
    fn get_string(key: &str, value: &str) -> Option<String> {
        let (k, v) = (wide(key), wide(value));
        let mut buf = [0u16; 128];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                k.as_ptr(),
                v.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (r == ERROR_SUCCESS).then(|| {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..n])
        })
    }

    let mut out = Vec::new();
    let mut root: HKEY = std::ptr::null_mut();
    let path = wide(RENDER);
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut root) }
        != ERROR_SUCCESS
    {
        return out;
    }
    for i in 0.. {
        let mut name = [0u16; 64];
        let mut len = name.len() as u32;
        let r = unsafe {
            RegEnumKeyExW(
                root,
                i,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if r != ERROR_SUCCESS {
            break;
        }
        let guid = String::from_utf16_lossy(&name[..len as usize]);
        let dev = format!("{RENDER}\\{guid}");
        let ours = get_string(&format!("{dev}\\FxProperties"), PKEY_EFX)
            .is_some_and(|c| c.eq_ignore_ascii_case(CLSID));
        if ours {
            out.push(EngineOutput {
                active: get_dword(&dev, "DeviceState").unwrap_or(0) & 1 != 0,
                enhancements_off: get_dword(&format!("{dev}\\Properties"), PKEY_SYSFX_OFF)
                    == Some(1),
            });
        }
    }
    unsafe { RegCloseKey(root) };
    out
}

#[cfg(not(windows))]
pub fn engine_outputs() -> Vec<EngineOutput> {
    Vec::new()
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

#[cfg(test)]
mod idle_tests {
    use super::*;

    fn out(active: bool, enhancements_off: bool) -> EngineOutput {
        EngineOutput {
            active,
            enhancements_off,
        }
    }

    #[test]
    fn says_why_the_engine_is_idle() {
        assert_eq!(idle_state(&[]), EngineState::NoOutput);
        // Only on a device that isn't connected (an old USB port, say).
        assert_eq!(idle_state(&[out(false, false)]), EngineState::NoOutput);
        assert_eq!(idle_state(&[out(true, true)]), EngineState::EnhancementsOff);
        // One usable output is enough: it's just waiting for sound.
        assert_eq!(
            idle_state(&[out(true, true), out(true, false)]),
            EngineState::Idle
        );
        assert_eq!(
            idle_state(&[out(false, false), out(true, false)]),
            EngineState::Idle
        );
    }
}
