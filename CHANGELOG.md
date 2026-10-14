# Changelog

## Unreleased

The new TAW look (first working build, for review).

- Spool's look by default (Light, Amber), with Spool's seven colour themes tinting the whole panel, and Porcelain as the alternative; each in Light, Dark or Automatic. Chosen in Settings (the gear).
- The band cards offer Peak, Low shelf and High shelf. High-pass, low-pass and notch are no longer offered (they're rarely needed at home); presets that use them still load and play, and those bands show their type.
- New layout: an On switch with where the Equaliser is working, the preset, preamp and meters in one row, the response graph in the display window, and ten band cards (type menu, gain fader, Hz and Q knobs). The badge, logo and model line stay.
- Simple view: five tone sliders (Bass to Treble) and the preset. Detailed has all ten bands.
- Audio setup (system, input, output, buffer, the Windows engine) has moved into Settings. Clicking the status under the On switch opens it.
- The Equaliser draws its own title strip: drag it to move the window, double-click to maximise, and use its own minimise, maximise and close buttons. Settings has "Use the system title bar".
- The family switcher starts the Library, Player or Analyser when they're installed.
- The spectrum behind the curve draws in the display's own ink by default; the colour schemes are still in Settings.

## 0.3.0 – 2026-10-13

EQ Box is now **Timster's Audio Works Equaliser** (EQ-001), one of the family's hi-fi separates with the Library, Player and Analyser.

- New name on the panel, window and program (`taw-equaliser`). On Linux the output is called "TAW Equaliser".
- Windows engine: now `taw_equaliser_apo.dll` in `Program Files\Timster's Audio Works\Equaliser`, settings in `ProgramData\Timster's Audio Works\Equaliser`, event log source "TAW Equaliser".
- **Coming from EQ Box:** save your EQ as a preset first (panel settings start fresh). Installing the engine sees an EQ Box engine and offers to remove it, putting every device back, then installs in its place. Remove also cleans up an EQ Box engine.
- The SP-10 Spectrum Analyser is no longer included: it's now its own download, the TAW Analyser (TimsterAudio/TAW-Analyser).
- The panel, spectrum drawing and analyser now come from the shared TAW-Panel crate. Nothing looks or behaves differently.

## 0.2.0 – 2026-10-11

- Windows: a system-wide engine (test build). EQ Box's EQ runs inside Windows' own audio engine and equalises every app on the outputs you choose, with no virtual cable. Choose **Windows (system-wide)** under System and press **Install**.
- WASAPI no longer offers "Loopback:" inputs, and never captures an output device, because looping an output back into the EQ can feed back.
- The preset format is now called the APO preset format in dialogs and docs.
- Windows engine: install it from EQ Box (Install / Add / Remove under System) or by double-clicking the `.cmd` files, with no PowerShell typing. The display shows whether the engine is installed and running.
- Windows engine: an output's own driver effect (speaker tuning, "enhancements") is kept and runs after EQ Box's EQ, instead of having to be switched off.
- Windows engine: the event log only gets start-up, stream starts and problems.
- Windows engine: EQ Box tells you when the engine installed in Windows is older than the window (the display shows UPDATE ENGINE and the key becomes **Update**). An older engine ignores settings it doesn't understand, such as the new HPF, LPF and notch filters.
- New filter types: high-pass (HPF), low-pass (LPF) and notch. They have a fixed shape (12 dB per octave for HPF and LPF), so a band's gain doesn't apply to them, and they fade in and out when switched. Presets with `HP`, `HPQ`, `LP`, `LPQ` and `NO` filters now load, and EQ Box saves them in that format.
- Each band's filter type is chosen with a stepped **Type** knob (amber) above Freq and Q, with the type shown in a red readout, instead of a row of keys. Clockwise it goes LPF, LO SH, PEAK, HI SH, HPF, NOTCH. The gain fader now runs the strip's full height, and its readout sits level with the On key.
- The bell filter is now labelled **PEAK**.
- **Auto** preamp: a key next to Preamp that keeps the overall response's highest point at 0 dB, so boosts can't clip. It turns the preamp down for boosts and up for cuts, and follows every change. Turning the Preamp knob switches it off.
- Each band's On key lights up green, instead of having an LED.
- Analyser colours: the **Colour** tab on the Response frame picks red, amber, green, ice, white, rainbow (bass to treble) or heat (by level), and the **Style** tab picks fill and line, line only, or fill only.
- **SP-10 Spectrum Analyser**, a separate program (`sp-10`) that doesn't need EQ Box: the spectrum analyser on its own, listening to what an output plays or to an input. Curve, bars or LED views at 1/3, 1/6 or 1/12 octave; the same colour schemes; adjustable rise, fall and peak hold; range and slope; and for any screen, a resizable window with hideable controls, full screen and always-on-top.

## 0.1.0 – 2026-10-10

First version.

- Ten bands, each a bell, low shelf or high shelf, with gain from +12 to −24 dB, a frequency from 20 Hz to 20 kHz, and a Q from 0.1 to 20.
- Preamp, bypass, Flat and Reset.
- Loads and saves APO-format EQ presets (such as AutoEQ's `ParametricEQ.txt`). You can use the Load key, drop a file on the window, or give a path on the command line.
- Response graph with per-band curves, the total response, a live spectrum analyser and draggable band points.
- Audio on Linux: a native PipeWire sink called "EQ Box", plus PipeWire and ALSA device modes.
- Audio on Windows: WASAPI (with loopback capture), and ASIO with the `asio` build feature.
- Drone Box panel: black or silver, colour or pointer knobs, LED display, level meters and red readouts.
- Remembers its settings between runs.
- Downloads for Linux, Windows and Windows + ASIO on the GitHub Releases page.
