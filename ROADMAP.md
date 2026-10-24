# Roadmap

Ideas and planned work for the Equaliser. Tick items off (`[x]`) when they ship, and note the version.

## Pipeline (proposed 2026-10-14)

Carried over from the EQ Box plan on branch `claude/roadmap-pipeline` (2026-10-12), renamed and renumbered after 0.3.0. Sizes: S = one session, M = two or three, L = a release of its own. Briefs from the family session go in `docs/briefs/`; TAW-Panel's `docs/TRACKER.md` tracks the family as a whole.

| Release | What | Size |
|---|---|---|
| 0.3.x | **Prove the system-wide engine on Windows**, and fix what turns up | S–M |
| 0.4.0 | **Easy enough for anyone, part 1:** the setup window, sensible first run, Update and Repair, Copy report | L |
| 0.5.0 | **Easy enough for anyone, part 2:** the installer, Linux one-click, named starting points | L |
| later | The EQ inside the Player; band counts | — |

### 0.3.x: prove the engine on Windows

- [ ] Install over an EQ Box engine: it's offered for removal first, every device comes back, then the TAW engine installs
- [ ] Install and Remove on at least three machines (a laptop with Realtek effects, a USB headset or DAC, a desktop); note what each driver does
- [ ] The engine survives a reboot and a Windows update; Remove always puts the device back
- [ ] Settle whether Windows' loopback capture includes the EQ (it decides where the analyser behind the curve gets its feed)

- [x] From the first test (2026-10-16): the spectrum no longer flickers or vanishes; Auto preamp on by default, always on in Simple, and gentler on treble boosts; Simple's graph shows only its five controls; corner resizing (TAW-Panel 0.3.2); the family's 0.4.0 look and meter colours *(0.3.1)*
- [ ] Make connecting to the right output obvious (user feedback, 2026-10-16): the first steps of 0.4.0's setup window, or Linux's **Make it my output** from 0.5.0, brought forward if the user chooses

- [x] A clip light on every band: HOT from the settings, CLIP from the sound *(0.3.2, the user's idea)*
- [ ] **Simple's tone controls at 40 Hz, 160 Hz, 625 Hz, 2.5 kHz and 10 kHz** (user request, 2026-10-16), two octaves apart, instead of bands 2, 4, 6, 8 and 10 at their current frequencies (150 Hz to 12.5 kHz with the preset shown). To settle first: Simple drives five of the same ten bands, so moving them changes what Detailed and a loaded preset have there. Either Simple retunes those bands when it's used (and says so), or it keeps five bands of its own on top of the ten (the engine would need fifteen); Bass and Treble may suit shelves rather than peaks.

- [ ] **A wider analyser range, as an option** (user request, 2026-10-24): the spectrum behind the curve from 10 Hz to 50 kHz, chosen in Settings › Display (20 Hz – 20 kHz stays the default). Only the display widens: the bands keep their 20 Hz – 20 kHz range (CLAUDE.md). Above 20 kHz needs a high sample rate (50 kHz needs 100 kHz or more; at 48 kHz the spectrum stops at 24 kHz, at 96 kHz at 48 kHz), so the graph should show where the sound actually ends. Below 20 Hz, the 8192-point FFT gives about 6 Hz per bin at 48 kHz, coarse but readable. Worth offering the same option in the TAW Analyser, through TAW-Panel.

*Exit check:* no machine ends up without sound, and Remove restores every device exactly.

### 0.4.0: easy enough for anyone, part 1

- [ ] **A setup window instead of PowerShell.** A small setup helper (in Rust, doing what the scripts do now) runs with the one Windows permission prompt, and the Equaliser shows its own window: which speakers to work on (connected outputs by their friendly names, the one playing now ticked), a **Play test sound** button by each, plain progress with ticks, then "Done". The same window for Remove and Update.
- [ ] **Sensible first run.** With the engine installed, the Equaliser starts system-wide, on, with Auto preamp.
- [ ] **One-click Update and Repair.** Update when the app is newer than the engine; Repair when a Windows update resets a device's effects.
- [ ] **Fix it for me.** A **Turn on** button when "Audio enhancements" are off for the output.
- [ ] **Copy report**: what `diagnose-engine.ps1` shows, to paste into a message.

### 0.5.0: easy enough for anyone, part 2

- [ ] **A real installer** instead of a zip, built by the release workflow: Program Files, Start menu, uninstall from Settings › Apps, the speaker page in the same pass.
- [ ] **Code signing**: a cost decision for the user, not a build task. Until then the download page shows the "More info › Run anyway" step.
- [ ] **Linux:** a **Make it my output** button, **Start with my computer**, and a single-file download (AppImage or Flatpak).
- [ ] **Named starting points** in Simple: "More bass", "Clearer speech", "Softer treble".

### Later

- [ ] The EQ inside the Player: `eq-core` runs in the Player for its own playback, with no administrator install; the system-wide engine stays for other apps' sound.
- [ ] Band counts (EQ-3, EQ-5, EQ-10) in one window that resizes with the count; the engine keeps ten bands with the unused ones off.
- [ ] Share `eq-core` with Drone Box when Drone Box moves its DSP to Rust.

## In progress

- [x] **System-wide EQ on Windows, with no virtual cable.** the Equaliser's own engine plugs into the Windows audio engine as an Audio Processing Object (APO), the same way other EQ software does it. See [docs/WINDOWS_APO_PLAN.md](docs/WINDOWS_APO_PLAN.md). *(0.2.0, as a test build installed from the Equaliser. Still to come: a full Windows installer.)*

## Planned

- [x] **Analyser colour options:** choose the colours of the spectrum analyser (fill, line, and scheme). *(0.2.0)*
- [x] **More filter types:** notch, high-pass (HPF) and low-pass (LPF), alongside bell and the shelves. Presets that use `NO`, `HP`/`HPQ` and `LP`/`LPQ` would then load instead of being skipped. *(0.2.0)*
- [x] **Band On/Off key without an LED:** drop the LED above each band's On key, and make the key itself light up when the band is on. *(0.2.0)*
- [x] **A separate analyser app:** just the spectrum analyser, as its own program, with options for different displays (sizes, layouts and screens), colours, and rise/decay (attack and release) times. *(0.2.0: SP-10 Spectrum Analyser.)*
