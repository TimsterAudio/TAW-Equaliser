# Handoff: TAW Equaliser (EQ-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- **0.3.0 released on 2026-10-14** (merged to `main`; release.yml publishes `v0.3.0`). It carries both the rename (EQ Box → TAW Equaliser; engine moves to `Timster's Audio Works\Equaliser`, install removes an EQ Box engine first) and the new TAW look (Spool Light Amber default, Porcelain alternative; own window buttons; Simple view with five tone sliders; ten band cards; audio setup in Settings; family switcher; Peak / Low shelf / High shelf offered, HP/LP/notch still load and play).
- TAW-Panel pinned at its 0.4.0 commit `d030ffd` (the Player's pin) from 2026-10-16, on branch `claude/taw-equaliser-0-3-0-s93tgl`, not yet merged or released.
- Checked here: all checks, the panel under Xvfb, and the APO harness under Wine (PASS). **Not yet tested on real Windows**, in particular the install script's EQ Box → TAW Equaliser switch. Fixes from the user's Windows test go in a 0.3.x.

## 2026-10-16: the user's first test of 0.3.0, and what was done

Passed: Bypass, settings kept after a restart, the Simple sliders. Feedback, and what the branch does about it (all in CHANGELOG under Unreleased):

- **Spectrum and its right-hand scale flicker, or aren't there at all.** TAW-Panel's `Analyzer::active` only means "samples arrived since the last frame"; audio blocks longer than a frame (and Windows' loopback, which sends nothing while the output is silent) made the graph drop the spectrum and its scale on those frames. Reproduced here with `pw-metadata -n settings 0 clock.force-quantum 8192`: the scale was missing in about two frames in three; with the fix, in none. The app now shows them whenever audio is on and lets the bins fall (60 dB/s) after 0.25 s without samples. **For the family session:** `active` behaves the same in every app that uses TAW-Panel's analyser; worth fixing there (hold it for a moment, or decay the bins) and then removing the workaround in `ui/app.rs`.
- **README out of date (LPF, HPF, notch).** Already fixed on `main` by PR #3; the test used the old README.
- **Auto preamp on by default, and always on in Simple.** Done: `Saved::auto_preamp` defaults to true, and the new `settings_version` turns it on once for 0.3.0 settings. Simple forces Auto on (only Detailed can turn it off); Auto is now worked out each frame just before the settings reach the audio, which also fixes Simple, where it was never worked out at all.
- **Bottom corners don't resize.** Fixed by TAW-Panel 0.3.2 (in the 0.4.0 pin).
- **Connecting to the right outputs isn't obvious.** Not built yet: options put to the user (see ROADMAP 0.4.0 / 0.5.0); waiting on their choice.

Also with the 0.4.0 pin: the shorter nameplate (no EQUALISER), the top band at the Player's height (plate 66, key row 46; the controls row and graph move up 28 and the graph gets the room), Meter colours in Settings (a new `Display` section), and new README screenshots.

## User feedback still open

- Anodised was dropped (looked fake); Spool replaced it. Check the user is happy with Spool Light Amber as the default.
- Band on/off and type menus were broken in the first look build; fixed in `2089e28` and checked in the running app.

## How to test here

- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings`
- The panel under Xvfb (`Xvfb :99 &`, `DISPLAY=:99`), with PipeWire and a null sink for audio: see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card". `xdotool`: move the pointer, pause about 0.3 s, then click, or egui misses it.
- The APO harness under Wine, as in CLAUDE.md.

## Next

1. The user's review of the branch, then 0.3.1 (merge to release).
2. The user's Windows test of the engine (install, the switch from an EQ Box engine, removal), still to come.
3. Their choice on making the output connection obvious.

## EQ Box lives on separately

On 2026-10-14 EQ Box got its own repository again (TimsterAudio/EQ-Box, restored from 0.2.0) as the Box family's equaliser. It registers the **same Windows engine CLSID** as TAW Equaliser, so only one engine can be installed at a time. This repo's install script offers to remove an EQ Box engine first; keep that. EQ Box 0.2.1 in turn refuses to install over, or remove, TAW Equaliser's engine.
