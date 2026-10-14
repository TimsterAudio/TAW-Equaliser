# Handoff: TAW Equaliser (EQ-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- `main` is 0.2.0 (still named EQ Box in places, Jupiter panel).
- Branch `claude/focused-ritchie-veg4m9`, unmerged, carries two things:
  1. **The 0.3.0 rename** (EQ Box → TAW Equaliser): commits `135eb21` and `cf8becc`. The Windows engine moves to `Timster's Audio Works\Equaliser`; the install script removes an EQ Box engine first. Checked under Wine and pwsh here, **not yet tested on real Windows**: the user is to test the EQ Box → TAW Equaliser switch.
  2. **The new TAW look** (`949d087`, `2089e28`): Spool finish by default (Light, Amber), Porcelain alternative; no system title bar (own window buttons, drag, resize; "Use the system title bar" in Settings); Simple view with five tone sliders; ten band cards; audio setup in the Settings drawer; family switcher. Band types offered: Peak, Low shelf, High shelf (HP/LP/notch still load from presets and play).
- Both are in CHANGELOG under Unreleased / 0.3.0. Merging the branch ships both together. If the user wants 0.3.0 released on its own first, split the look onto another branch (ask before pushing to a new branch name).
- TAW-Panel is pinned at `ce561c2` (an unreleased commit on TAW-Panel's branch, with the `hifi` module). Before a release, TAW-Panel needs its 0.3.0 version recorded and this pin moved to that commit.

## User feedback still open

- Anodised was dropped (looked fake); Spool replaced it. Check the user is happy with Spool Light Amber as the default.
- Band on/off and type menus were broken in the first look build; fixed in `2089e28` and checked in the running app.

## How to test here

- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings`
- The panel under Xvfb (`Xvfb :99 &`, `DISPLAY=:99`), with PipeWire and a null sink for audio: see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card". `xdotool`: move the pointer, pause about 0.3 s, then click, or egui misses it.
- The APO harness under Wine, as in CLAUDE.md.

## Next

1. The user's Windows test of 0.3.0; then merge and release (release.yml publishes on merge to `main`).
2. Feedback on the new look; screenshots in Spool and Porcelain, light and dark.

## EQ Box lives on separately

On 2026-10-14 EQ Box got its own repository again (TimsterAudio/EQ-Box, restored from 0.2.0) as the Box family's equaliser. It registers the **same Windows engine CLSID** as TAW Equaliser, so only one engine can be installed at a time. This repo's install script already offers to remove an EQ Box engine first; keep that.
