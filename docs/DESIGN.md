# Panel design

The Equaliser uses the TAW hi-fi look shared by the Library, Player and Analyser: TAW-Panel's `hifi` module. (Until 0.3.0 it used Drone Box's Jupiter-style panel, which stays with the Box instruments.) The look's colours, type and controls are defined there; this page covers how the Equaliser lays them out.

## The look

- **Finishes:** Spool (the default: Spool's warm panels, each of its seven colour themes tinting the whole panel, with the display readout in the theme's colour) and Porcelain (matte white, paper display; Graphite in the dark build). Each has Light, Dark and Automatic (following the computer). The default theme is Amber.
- **Type:** Hanken Grotesk for labels and text, Chivo Mono for numbers and the display, Space Grotesk in the maker's badge.
- **Nameplate,** top left, as on every Timster's Audio Works hi-fi unit: the badge "Timster's Audio Works - Equaliser" with the model line "EQ-001 · Ten band parametric equaliser · V…" under it. No big slanted name since TAW-Panel 0.4.0; that stays with the Box instruments.
- **No system title bar** by default. The nameplate strip moves the window (double-click maximises); minimise, maximise and close are drawn top right; the edges and corners resize. Settings has "Use the system title bar".

## Layout (design units, 1280 × 812, scaled to fit the window)

- **Top, 0–66 (the Player's height):** nameplate (left); window buttons (top right); on a row centred at 46, the family switcher (Library, Player, Equaliser, Analyser; apps that aren't installed are dimmed), Simple / Detailed, and the settings gear.
- **Controls row, about 82–118:**
  - the **On switch** (starts and stops the audio) with "On" or "Off" and a status line: where it's working, or what's wrong (amber). Clicking the status opens the audio settings.
  - **Preset:** the preset's name, Load…, Save…, Flat, Reset and **Bypass** (filled with the accent while bypassed; Space toggles it). Simple shows Load… and Flat only.
  - **Preamp:** a slider, its value and an **Auto** switch (Detailed only). Auto is on by default. It cuts for boosts less the treble's headroom (`eq_core::dsp::headroom_allowance_db`: none to 2 kHz, then 3 dB an octave to 9 dB at 16 kHz), lifts to exactly 0 dB when the bands only cut, follows the bands and never the sound, and settles when the mouse button is let go. Simple has no preamp control, so Auto is always on there: only Detailed can turn it off. Moving the slider takes over from Auto.
  - **In** and **Out** meters, left and right (Detailed only), in the classic LED colours or the display's ink (Settings).
- **Response graph, 134–402 (Simple: 134–372):** the display window. The total response in the display's ink, each active band's curve and fill in its colour, the spectrum faintly behind (optional; shown, with its dB scale on the right, the whole time audio is on, and falling away when the sound stops), and the numbered band points. The selected point has a ring in the ink; hovering or dragging a point shows a tag with its type, frequency, gain and Q.
- **Detailed: ten band cards, 418 to the footer.** Each card: the band's colour dot and number, an on switch, the filter type menu (Peak, Low shelf, High shelf; a band loaded with high-pass, low-pass or notch shows that too), the clip light (faint CLIP outline when all's well; yellow HOT from `eq_core::dsp::hot_bands` when the band takes the response, with the preamp, past the headroom; red CLIP for 2 s when the output goes over full scale and `clip_band` blames this band from the analyser's levels; a tooltip says what to do; none on bands without gain), a vertical gain fader (0 dB tick; the fill runs from 0 dB to the gain in the accent), the gain value, and small Hz and Q knobs. High-pass, low-pass and notch have no gain, so their card says "No gain, fixed slope" instead of a fader. The selected band's card is outlined in the accent.
- **Simple: five tone sliders,** Bass, Low mid, Mid, High mid and Treble (bands 2, 4, 6, 8 and 10 of the same ten, ±12 dB), each with its gain and frequency and the band's clip light beside its name. The graph shows only those five, as points named B, LM, M, HM and T that move up and down (gain only, ±12 dB; no Q or on/off), while the total curve still includes every band; the spectrum is smoothed over a third of an octave and moves more calmly (120 ms rise, 700 ms fall). Switching back to Detailed shows exactly what they changed.
- **Footer:** the mouse and key hints (Detailed) on the left; the last change or a message on the right.

## Settings drawer

Opened by the gear, it slides over the right of the window:

- **Audio:** System, Input (or what it is, for the PipeWire sink and the Windows engine), Output, Buffer (two-device systems), Rescan devices, the Windows engine's Install / Update / Add and Remove, Dropouts, and a paragraph on how the chosen system works (or the error, in amber).
- **Display:** the spectrum behind the curve (on or off), its colour (Display ink, or a colour scheme) and style, and the meter colours (Classic or Display).
- **Look:** finish, light or dark, accent.
- **Window:** use the system title bar.
