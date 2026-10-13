# Plan: system-wide EQ on Windows (EQ Box APO)

> Since 0.3.0 the app is the TAW Equaliser: the DLL is `taw_equaliser_apo.dll`, files live under `Program Files\Timster's Audio Works\Equaliser` and `ProgramData\Timster's Audio Works\Equaliser`, and the event log source is "TAW Equaliser". This plan keeps the names it was written with.

**Status:** steps 1 and 2 are built, along with the core of step 3: the app mode and live settings. Confirmed working on Windows 11 (build 26200) with a USB DAC: the engine runs inside `audiodg.exe` and the EQ is heard on every app. Note that the audio engine creates APOs through COM aggregation, which `com.rs` supports. Since then: setup runs from the app (Install / Add / Remove) or a double-click, and step 5's mechanism is built: a driver's own endpoint effect is kept and run after the EQ (checked in the harness; waiting on a test with a device that has one). The full installer (step 4) is still to come.

## Goal

EQ Box equalises everything Windows plays on the outputs you choose, with no virtual cable and no third-party software. This is how other EQ software does it.

## How it works

Windows mixes every app's sound in its audio engine (the `audiodg.exe` process), then sends the result to the device. Each output can carry **Audio Processing Objects (APOs)**: small DLLs the engine loads and runs on the sound before the device gets it. Sound card drivers use them for their "enhancements". Other EQ software installs its own the same way.

```
 apps ──► Windows audio engine (audiodg.exe) ──────────────────► speakers
                │
                └─ EQ Box APO (eq-box-apo.dll)
                     runs the same EQ code as the app
                     reads its settings from a settings file
                             ▲
 EQ Box window ──────────────┘  writes settings; shows meters and analyser
```

There are three parts:

| Part | What it is |
|---|---|
| **The engine** (`eq-box-apo.dll`) | The EQ, built as a Windows APO. It runs inside the audio engine all the time, even when the EQ Box window is closed. It uses the same filter code as the Linux version, so it sounds identical. |
| **The settings link** | EQ Box writes the current bands to a file under `C:\ProgramData\EQ Box\`. The engine watches that file and glides to the new settings without clicks, the same way it does now. Because it's a file, the EQ stays set after a restart, with or without the window open. |
| **Setup** | An installer (needs admin once). It copies the DLL, registers it with Windows, and lets you tick which outputs get EQ Box. Windows' audio service restarts once so it picks up the change. Uninstalling restores each device exactly as it was. |

Inside EQ Box, **System** gets a new choice: **Windows (system-wide)**. It has no Input and Output lists, just the outputs EQ Box is installed on, plus **Add / remove devices** and **Repair** buttons. The existing WASAPI and ASIO modes stay as they are.

## What it will and won't cover

**It covers** everything that plays through the normal Windows mixer: browsers, music and video players, games and calls.

**It won't cover**, the same as other EQ software:
- Apps that take **exclusive control** of a device, such as some music players in exclusive or "bit-perfect" mode, and anything using ASIO. EQ Box's ASIO mode remains for those cases.
- Devices with **"Audio enhancements" turned off** in Windows sound settings. Setup will check this and tell you.
- After some **Windows or driver updates**, Windows can reset a device's effects. EQ Box will detect that its engine isn't running on a device it should be on, and offer **Repair**.

Sound card enhancements that are already on a device are kept. EQ Box's engine runs alongside the driver's own effects rather than replacing them. This is the trickiest part to get right on every machine, so it gets its own test step.

## Steps

Each step ends with something you can test on Windows. I can compile everything here, but I can't run Windows, so you're the tester for each step.

1. **Restructure (no change you'll notice).** Split the code so the EQ engine can be built both into the app and into the DLL. Linux keeps working exactly as now.
2. **First sound.** Build the engine DLL, plus a small PowerShell script to install it on one device and remove it again (run as admin). The EQ is fixed, read from the settings file at start-up.
   *Test:* run the script, play music in any app, and hear the EQ. Then run the remove script and confirm the device is back to normal.
3. **Live control.** Add the Windows (system-wide) mode to EQ Box. Moving a knob changes the sound in every app within a moment. Meters and the analyser show the output, either from Windows' loopback capture or, if that turns out to miss the EQ, from the engine itself.
   *Test:* tweak bands and load presets while music plays in other apps.
4. **Installer.** A proper Windows installer (`EQ-Box-Setup-x.y.z.exe`) with a device picker, Repair and Uninstall, built by GitHub Actions and attached to each release next to the zip files.
   *Test:* install on a clean user account, pick devices, reboot, uninstall.
5. **Keeping driver enhancements.** Test on devices that already have sound card effects (laptops, Realtek, USB headsets).
   *Test:* run on as many different machines as you can.

## Risks and safety

- **The worst case is no sound from a device.** Every step ships with a way back: the remove script (step 2), and later Uninstall and Repair. Windows' own "Audio enhancements: Off" switch for a device also bypasses EQ Box immediately.
- **Device keys belong to Windows.** On Windows 11 a device's effect settings are owned by TrustedInstaller, so setup takes ownership of just that one key, as other EQ software does. It saves the original owner and permissions first, and removal puts them back exactly.
- **Windows security prompts.** The installer isn't signed with a paid certificate at first, so SmartScreen will warn on first run, as it does for the current zip files.
- **Unknowns we'll find out by testing:**
  - Whether Windows' loopback capture includes the EQ (this decides how the analyser gets its feed).
  - How individual drivers behave with an extra APO.

## Not changing

Linux (PipeWire), WASAPI and ASIO modes, presets, and the panel all stay as they are. The roadmap items (analyser colours, notch and HPF/LPF filters, lit On keys, and the analyser app) come after this, as you asked.
