# the Equaliser: working conventions

the Equaliser is a native ten-band parametric EQ by Timster's Audio Works, written in Rust with egui (via eframe) for the panel. Read `docs/ARCHITECTURE.md` before you change code, and `docs/DESIGN.md` before you change the panel. The panel follows Drone Box's design language; its theme, keys, knobs, faders and displays come from the shared `taw-panel` crate (TimsterAudio/TAW-Panel), pinned by commit in Cargo.toml. Change the shared look there, then move the pin. TAW-Panel's `FAMILY.md` covers how it fits with the Library, Player and Analyser.

## Hard rules

- **Native, not HTML.** No web views or browser shells.
- **No network requests at runtime.** Fonts and the icon are built into the binary.
- **Keep the audio callbacks real-time safe.** They must not lock, allocate, do file I/O or log. The UI and the audio thread share state only through `Shared` (atomics) and the analyser ring buffer.
- **DSP stays free of UI and audio-API code.** Everything in `crates/eq-core` is plain Rust and unit tested. Every app backend runs audio through `audio::Processor`, and the Windows engine runs the same `Equalizer`.
- **The Windows engine runs inside `audiodg.exe`: it must never panic, block or allocate in `APOProcess`.** A crash there silences the user's audio. Never change its CLSID.
- **The install and remove scripts handle both names.** The engine's files moved from `EQ Box` folders to `Timster's Audio Works\Equaliser` in 0.3.0; `engine-common.ps1` keeps the old (`Legacy…`) locations so removal cleans up either and install removes an EQ Box first. Keep that until no EQ Box installs are left.
- **Never break saved settings.** `Saved` in `ui/app.rs` uses `#[serde(default)]`. Add new fields with defaults, and never rename or repurpose existing ones.
- **Keep the APO preset format compatible** with other EQ software that uses it, both ways.
- **Keep the panel ranges:** gain +12 / −24 dB, Q 0.1 to 20, 20 Hz to 20 kHz, preamp +12 / −24 dB.

## Checks before pushing

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings   # Windows still compiles
```

The Windows engine can be checked here without Windows. Cross-build with MinGW and run the harness under Wine:

```sh
cargo build --release --target x86_64-pc-windows-gnu -p taw-equaliser-apo --example apo_harness
cd target/x86_64-pc-windows-gnu/release && cp examples/apo_harness.exe . && wine apo_harness.exe
```

## Workflow

- Work on a feature branch. `main` always holds a working version.
- Open a pull request when the user asks, or merge when they ask.
- For every user-visible change, add a line under **Unreleased** in `CHANGELOG.md`.
- For a release: move Unreleased into a section headed `## X.Y.Z – YYYY-MM-DD` (the release notes are taken from it), and bump `version` in `Cargo.toml` and the version in `README.md`. Merging that to `main` publishes the release: `.github/workflows/release.yml` creates the `vX.Y.Z` tag and release with the Linux and Windows packages. No manual tagging is needed.
