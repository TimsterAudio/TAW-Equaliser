# Handoff: TAW Equaliser (EQ-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- **0.3.0 released on 2026-10-14** (merged to `main`; release.yml publishes `v0.3.0`). It carries both the rename (EQ Box → TAW Equaliser; engine moves to `Timster's Audio Works\Equaliser`, install removes an EQ Box engine first) and the new TAW look (Spool Light Amber default, Porcelain alternative; own window buttons; Simple view with five tone sliders; ten band cards; audio setup in Settings; family switcher; Peak / Low shelf / High shelf offered, HP/LP/notch still load and play).
- TAW-Panel pinned at its 0.3.0 commit `1b8162d`.
- Checked here: all checks, the panel under Xvfb, and the APO harness under Wine (PASS). **Not yet tested on real Windows**, in particular the install script's EQ Box → TAW Equaliser switch. Fixes from the user's Windows test go in a 0.3.x.

## User feedback still open

- Anodised was dropped (looked fake); Spool replaced it. Check the user is happy with Spool Light Amber as the default.
- Band on/off and type menus were broken in the first look build; fixed in `2089e28` and checked in the running app.

## How to test here

- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings`
- The panel under Xvfb (`Xvfb :99 &`, `DISPLAY=:99`), with PipeWire and a null sink for audio: see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card". `xdotool`: move the pointer, pause about 0.3 s, then click, or egui misses it.
- The APO harness under Wine, as in CLAUDE.md.

## Next

1. The user's Windows test of 0.3.0 (engine install, the switch from an EQ Box engine, removal).
2. Feedback on the new look.

## EQ Box lives on separately

On 2026-10-14 EQ Box got its own repository again (TimsterAudio/EQ-Box, restored from 0.2.0) as the Box family's equaliser. It registers the **same Windows engine CLSID** as TAW Equaliser, so only one engine can be installed at a time. This repo's install script offers to remove an EQ Box engine first; keep that. EQ Box 0.2.1 in turn refuses to install over, or remove, TAW Equaliser's engine.
