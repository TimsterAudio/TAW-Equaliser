# Roadmap

Ideas and planned work for the Equaliser. Tick items off (`[x]`) when they ship, and note the version.

## In progress

- [x] **System-wide EQ on Windows, with no virtual cable.** the Equaliser's own engine plugs into the Windows audio engine as an Audio Processing Object (APO), the same way other EQ software does it. See [docs/WINDOWS_APO_PLAN.md](docs/WINDOWS_APO_PLAN.md). *(0.2.0, as a test build installed from the Equaliser. Still to come: a full Windows installer.)*

## Planned

- [x] **Analyser colour options:** choose the colours of the spectrum analyser (fill, line, and scheme). *(0.2.0)*
- [x] **More filter types:** notch, high-pass (HPF) and low-pass (LPF), alongside bell and the shelves. Presets that use `NO`, `HP`/`HPQ` and `LP`/`LPQ` would then load instead of being skipped. *(0.2.0)*
- [x] **Band On/Off key without an LED:** drop the LED above each band's On key, and make the key itself light up when the band is on. *(0.2.0)*
- [x] **A separate analyser app:** just the spectrum analyser, as its own program, with options for different displays (sizes, layouts and screens), colours, and rise/decay (attack and release) times. *(0.2.0: SP-10 Spectrum Analyser.)*
