//! The Equaliser panel, in the TAW hi-fi look (TAW-Panel's `hifi`): the
//! nameplate, family switcher and window buttons along the top; the On
//! switch, preset, preamp and meters; the response graph in the display
//! window; and the ten band cards (Detailed) or five tone sliders (Simple).
//! Audio setup lives in the settings drawer.

use super::graph::{self, fmt_freq, kind_name, BAND_COLOURS};
use super::spectrum::{self, Look, Scheme};
use super::theme::{KnobStyle, PanelKind};
use super::widgets::Spec;
use crate::analyzer::Analyzer;
use crate::audio::engine_setup::{self, EngineState, Setup};
use crate::audio::{self, BackendInfo, DeviceLists, Running, StartRequest};
use crate::dsp::*;
use crate::family::{self, App};
use crate::shared::{analyzer_channel, Shared};
use egui::{Align2, Color32, Id, Key, Pos2, Rect, Stroke, Ui, Vec2, ViewportCommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use taw_panel::hifi::{self, Seg, Tokens};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The panel is laid out at this size and scaled to fit the window.
pub const DESIGN: Vec2 = Vec2::new(1280.0, 812.0);
/// The nameplate strip (drag it to move the window), as in the Player.
const PLATE_H: f32 = 66.0;
/// The middle of the nameplate's row of keys, just under the window buttons.
const ROW_Y: f32 = 46.0;
/// The middle of the On switch, preset, preamp and meters row.
const CONTROLS_Y: f32 = 100.0;
/// The top of the response graph.
const GRAPH_TOP: f32 = 134.0;
const STORAGE_KEY: &str = "taw-equaliser";
/// The filter types the band cards offer.
const PANEL_KINDS: [FilterKind; 3] = [
    FilterKind::Peak,
    FilterKind::LowShelf,
    FilterKind::HighShelf,
];
/// Simple view's tone sliders, and the band each one moves.
const TONE: [(&str, usize); 5] = [
    ("Bass", 1),
    ("Low mid", 3),
    ("Mid", 5),
    ("High mid", 7),
    ("Treble", 9),
];

/// Everything remembered between runs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    bands: [BandParams; NUM_BANDS],
    preamp_db: f32,
    /// The preamp follows the bands, keeping the response's peak at 0 dB.
    auto_preamp: bool,
    bypass: bool,
    preset_name: String,
    backend: Option<String>,
    input: Option<String>,
    output: Option<String>,
    latency_ms: f32,
    /// The Jupiter panel before 0.3.0. Kept so older settings still load.
    #[allow(dead_code)]
    panel: PanelKind,
    #[allow(dead_code)]
    knobs: KnobStyle,
    analyser: bool,
    analyser_scheme: Scheme,
    analyser_fill: Look,
    running: bool,
    selected: usize,
    last_dir: Option<PathBuf>,
    /// Finish, light or dark, and accent.
    look: hifi::Look,
    /// Simple view: five tone sliders instead of the ten bands.
    simple: bool,
    /// Use the system's title bar instead of the Equaliser's own.
    system_title_bar: bool,
    /// The spectrum behind the curve in the display's own ink, rather than
    /// a colour scheme.
    analyser_ink: bool,
    /// Where the user pointed the Equaliser to family apps it couldn't find.
    family: std::collections::HashMap<String, PathBuf>,
    /// The In and Out meters: classic LED colours or the display's ink.
    meter_colours: hifi::MeterColours,
    /// Bumped when a default changes for everyone: 1 (0.3.1) turns Auto
    /// preamp on once for settings saved by 0.3.0.
    settings_version: u32,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            bands: default_bands(),
            preamp_db: 0.0,
            auto_preamp: true,
            bypass: false,
            preset_name: "Flat".into(),
            backend: None,
            input: None,
            output: None,
            latency_ms: 30.0,
            panel: PanelKind::Black,
            knobs: KnobStyle::Colour,
            analyser: true,
            analyser_scheme: Scheme::default(),
            analyser_fill: Look::default(),
            running: false,
            selected: 5,
            last_dir: None,
            look: hifi::Look::default(),
            simple: false,
            system_title_bar: false,
            analyser_ink: true,
            family: Default::default(),
            meter_colours: hifi::MeterColours::Classic,
            settings_version: 0,
        }
    }
}

const GAIN: Spec = Spec {
    min: GAIN_MIN_DB,
    max: GAIN_MAX_DB,
    default: 0.0,
    log: false,
    step: 0.1,
};
/// Simple view's tone sliders: ±12 dB.
const TONE_GAIN: Spec = Spec {
    min: -12.0,
    max: 12.0,
    default: 0.0,
    log: false,
    step: 0.1,
};
const PREAMP: Spec = Spec {
    min: PREAMP_MIN_DB,
    max: PREAMP_MAX_DB,
    default: 0.0,
    log: false,
    step: 0.1,
};
const LATENCY: Spec = Spec {
    min: 5.0,
    max: 200.0,
    default: 30.0,
    log: true,
    step: 1.0,
};

fn freq_spec(default: f32) -> Spec {
    Spec {
        min: FREQ_MIN,
        max: FREQ_MAX,
        default,
        log: true,
        step: 0.0,
    }
}

fn q_spec(kind: FilterKind) -> Spec {
    let default = kind.default_q();
    Spec {
        min: Q_MIN,
        max: Q_MAX,
        default,
        log: true,
        step: 0.0,
    }
}

fn fmt_db(v: f32) -> String {
    format!("{v:+.1} dB")
}

fn fmt_hz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{} kHz", fmt_freq(v).trim_end_matches('K'))
    } else {
        format!("{} Hz", fmt_freq(v))
    }
}

/// Brings settings saved by an older version up to date.
fn upgrade(s: &mut Saved) {
    if s.settings_version < 1 {
        // Auto preamp is on by default from 0.3.1, so nothing clips.
        s.auto_preamp = true;
        s.settings_version = 1;
    }
}

/// A frequency short enough for a knob: "105", "3.2k".
fn short_hz(v: f32) -> String {
    fmt_freq(v).replace('K', "k")
}

pub struct EqApp {
    s: Saved,
    shared: Arc<Shared>,
    running: Option<Running>,
    analyzer: Analyzer,
    /// When the analyser last had samples (see `now`).
    analyser_fed: f64,
    /// Bands whose boost takes the response past the headroom (HOT).
    hot: Vec<bool>,
    /// Until when each band's CLIP light stays lit (see `now`).
    clip_until: [f64; NUM_BANDS],
    backends: Vec<BackendInfo>,
    devices: DeviceLists,
    device_error: Option<String>,
    audio_error: Option<String>,
    touched: Option<(String, String, f64)>,
    notice: Option<(String, f64)>,
    peaks_db: [f32; 4],
    /// The Windows system-wide engine, checked once a second while
    /// "Windows (system-wide)" is chosen.
    engine: EngineState,
    engine_checked: f64,
    /// A setup script waiting on Windows' permission prompt.
    setup: Option<std::process::Child>,
    settings_open: bool,
    installed: [bool; 4],
}

impl EqApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        hifi::install_fonts(&cc.egui_ctx);
        let mut s: Saved = cc
            .storage
            .and_then(|st| eframe::get_value(st, STORAGE_KEY))
            .unwrap_or_default();
        s.bands = s.bands.map(|b| b.clamped());
        s.selected = s.selected.min(NUM_BANDS - 1);
        upgrade(&mut s);
        let settings = EqSettings {
            bands: s.bands,
            preamp_db: s.preamp_db,
            bypass: s.bypass,
        };
        let backends = audio::backends();
        if !s
            .backend
            .as_ref()
            .is_some_and(|b| backends.iter().any(|x| &x.id == b))
        {
            s.backend = backends.first().map(|b| b.id.clone());
        }
        if s.system_title_bar {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Decorations(true));
        }
        let mut app = EqApp {
            shared: Arc::new(Shared::new(&settings)),
            s,
            running: None,
            analyzer: Analyzer::default(),
            analyser_fed: f64::NEG_INFINITY,
            hot: vec![false; NUM_BANDS],
            clip_until: [f64::NEG_INFINITY; NUM_BANDS],
            backends,
            devices: DeviceLists::default(),
            device_error: None,
            audio_error: None,
            touched: None,
            notice: None,
            peaks_db: [-90.0; 4],
            engine: EngineState::NotInstalled,
            engine_checked: f64::NEG_INFINITY,
            setup: None,
            settings_open: false,
            installed: [false, false, true, false],
        };
        app.refresh_family();
        app.refresh_devices();
        if let Some(path) = std::env::args_os().nth(1) {
            app.load_preset_file(std::path::Path::new(&path));
        }
        if app.s.running {
            app.start_audio();
        }
        app
    }

    fn refresh_family(&mut self) {
        for (i, a) in App::ALL.into_iter().enumerate() {
            if a != App::Equaliser {
                self.installed[i] =
                    family::find(a.program(), self.s.family.get(a.name())).is_some();
            }
        }
    }

    fn settings(&self) -> EqSettings {
        EqSettings {
            bands: self.s.bands,
            preamp_db: self.s.preamp_db,
            bypass: self.s.bypass,
        }
    }

    fn backend(&self) -> Option<&BackendInfo> {
        let id = self.s.backend.as_ref()?;
        self.backends.iter().find(|b| &b.id == id)
    }

    fn refresh_devices(&mut self) {
        self.device_error = None;
        self.devices = match self.s.backend.clone() {
            Some(b) => audio::devices(&b).unwrap_or_else(|e| {
                self.device_error = Some(e);
                DeviceLists::default()
            }),
            None => DeviceLists::default(),
        };
        if self
            .s
            .input
            .as_ref()
            .is_some_and(|i| !self.devices.inputs.iter().any(|d| &d.id == i))
        {
            self.s.input = None;
        }
        if self
            .s
            .output
            .as_ref()
            .is_some_and(|o| !self.devices.outputs.iter().any(|d| &d.id == o))
        {
            self.s.output = None;
        }
    }

    fn start_audio(&mut self) {
        self.running = None;
        let Some(backend) = self.s.backend.clone() else {
            self.audio_error = Some("No audio system available".into());
            return;
        };
        let (tap, rx) = analyzer_channel();
        let req = StartRequest {
            backend,
            input: self.s.input.clone(),
            output: self.s.output.clone(),
            latency_ms: self.s.latency_ms,
        };
        self.shared.store(&self.settings());
        self.shared
            .dropouts
            .store(0, std::sync::atomic::Ordering::Relaxed);
        match audio::start(&req, self.shared.clone(), tap) {
            Ok(r) => {
                self.analyzer.connect(rx);
                self.audio_error = None;
                self.touch("Audio", "running");
                self.running = Some(r);
                self.s.running = true;
            }
            Err(e) => {
                self.audio_error = Some(e);
                self.s.running = false;
            }
        }
    }

    fn stop_audio(&mut self) {
        self.running = None;
        self.s.running = false;
        self.touch("Audio", "stopped");
    }

    fn restart_if_running(&mut self) {
        if self.running.is_some() {
            self.start_audio();
        }
    }

    fn touch(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.touched = Some((name.into(), value.into(), now()));
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), now()));
    }

    fn is_win_engine(&self) -> bool {
        self.s.backend.as_deref() == Some(audio::WINDOWS_ENGINE)
    }

    /// Keeps `engine` current while the Windows engine is chosen, and
    /// reports on a setup script once Windows' permission prompt closes.
    fn watch_engine(&mut self, ctx: &egui::Context) {
        if let Some(child) = self.setup.as_mut() {
            if let Some(result) = engine_setup::launch_result(child) {
                self.setup = None;
                match result {
                    Ok(()) => self.notify("Setup is running in its own window"),
                    Err(e) => self.notify(e),
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if !self.is_win_engine() {
            return;
        }
        let t = now();
        if t - self.engine_checked >= 1.0 {
            self.engine_checked = t;
            self.engine = engine_setup::engine_state();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn run_setup(&mut self, what: Setup) {
        if self.setup.is_some() {
            return;
        }
        match engine_setup::launch(what) {
            Ok(child) => {
                self.setup = Some(child);
                self.notify("Windows will ask for permission");
            }
            Err(e) => self.notify(e),
        }
    }

    // ------------------------------------------------------------ presets

    fn load_preset(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Load an APO-format EQ preset")
            .add_filter("APO-format EQ preset", &["txt", "cfg"])
            .add_filter("All files", &["*"]);
        if let Some(d) = &self.s.last_dir {
            dlg = dlg.set_directory(d);
        }
        let Some(path) = dlg.pick_file() else { return };
        self.load_preset_file(&path);
    }

    pub fn load_preset_file(&mut self, path: &std::path::Path) {
        self.s.last_dir = path.parent().map(|p| p.to_path_buf());
        let text = match std::fs::read(path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => return self.notify(format!("Couldn't read {}: {e}", path.display())),
        };
        let preset = crate::preset::parse(&text);
        if preset.bands.is_empty() && preset.preamp_db == 0.0 {
            return self.notify("That file has no Preamp or Filter lines the Equaliser can use");
        }
        let (bands, preamp) = preset.to_panel();
        self.s.bands = bands;
        self.s.preamp_db = preamp;
        self.s.preset_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.touch("Loaded", format!("{} bands", preset.bands.len()));
        if preset.warnings.is_empty() {
            self.notice = None;
        } else {
            self.notify(preset.warnings.join(" · "));
        }
    }

    fn save_preset(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Save as an APO-format EQ preset")
            .add_filter("APO-format EQ preset", &["txt"])
            .set_file_name(format!("{}.txt", self.s.preset_name));
        if let Some(d) = &self.s.last_dir {
            dlg = dlg.set_directory(d);
        }
        let Some(path) = dlg.save_file() else { return };
        self.s.last_dir = path.parent().map(|p| p.to_path_buf());
        match std::fs::write(
            &path,
            crate::preset::export(&self.s.bands, self.s.preamp_db),
        ) {
            Ok(()) => {
                self.s.preset_name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.touch("Saved", self.s.preset_name.clone());
            }
            Err(e) => self.notify(format!("Couldn't save: {e}")),
        }
    }

    // ------------------------------------------------------------ frame

    fn update_meters(&mut self, dt: f32) {
        let peaks = self.shared.take_peaks();
        // The output went over full scale: light CLIP on the band most
        // likely to blame, from where the music is loudest right now.
        if peaks[2].max(peaks[3]) > 1.0 && !self.s.bypass && self.running.is_some() {
            let rate = self.sample_rate();
            let an = &self.analyzer;
            let third = 2f64.powf(1.0 / 6.0);
            let level =
                |f: f64| an.level((f / third) as f32, (f * third) as f32, rate as f32) as f64;
            if let Some(i) = clip_band(&self.s.bands, rate, level) {
                self.clip_until[i] = now() + 2.0;
            }
        }
        for (shown, p) in self.peaks_db.iter_mut().zip(peaks) {
            let db = gain_to_db(p).max(-90.0);
            *shown = if db > *shown {
                db
            } else {
                (*shown - 30.0 * dt).max(db).max(-90.0)
            };
        }
    }

    /// The running stream's sample rate, or 48 kHz.
    fn sample_rate(&self) -> f64 {
        match self
            .shared
            .sample_rate
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => 48_000.0,
            r => r as f64,
        }
    }

    /// The light above a band's gain: CLIP (red) while the output is
    /// clipping from it, HOT (amber) while its boost takes the response past
    /// the headroom, otherwise a faint CLIP.
    fn clip_light(&self, ui: &Ui, i: usize, centre: Pos2, t: &Tokens) {
        let clipping = now() < self.clip_until[i];
        let hot = self.hot[i];
        let r = Rect::from_center_size(centre, Vec2::new(38.0, 14.0));
        let p = ui.painter();
        let (fill, text, word) = if clipping {
            (Color32::from_rgb(0xff, 0x4a, 0x30), Color32::WHITE, "CLIP")
        } else if hot {
            (
                Color32::from_rgb(0xef, 0xc2, 0x3b),
                Color32::from_rgb(0x14, 0x10, 0x0e),
                "HOT",
            )
        } else {
            (Color32::TRANSPARENT, t.dim.gamma_multiply(0.45), "CLIP")
        };
        p.rect_filled(r, egui::CornerRadius::same(3), fill);
        if fill == Color32::TRANSPARENT {
            p.rect_stroke(
                r,
                egui::CornerRadius::same(3),
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
        }
        hifi::label(
            p,
            r.center(),
            Align2::CENTER_CENTER,
            word,
            hifi::bold(9.5),
            text,
        );
        let tip = if clipping {
            "Clipping: the sound went over full scale, most likely from this band's boost. Turn it down, or lower the preamp."
        } else if hot {
            "This band's boost takes the sound past the headroom, so loud music can clip. Turn it down, lower the preamp or turn on Auto."
        } else {
            "Lights HOT when this band's boost leaves too little headroom, and CLIP when the sound clips because of it."
        };
        ui.interact(r, Id::new(("clip", i)), egui::Sense::hover())
            .on_hover_text(tip);
    }

    fn flat(&mut self) {
        for b in &mut self.s.bands {
            b.gain_db = 0.0;
        }
        self.s.preamp_db = 0.0;
        self.touch("All gains", "0.0 dB");
    }

    fn reset(&mut self) {
        self.s.bands = default_bands();
        self.s.preamp_db = 0.0;
        self.s.preset_name = "Flat".into();
        self.touch("Reset", "default bands");
    }

    fn toggle_run(&mut self) {
        if self.running.is_some() {
            self.stop_audio();
        } else {
            self.start_audio();
        }
    }

    /// What the status line under the On switch says, and whether it's a
    /// problem.
    fn status(&self) -> (String, bool) {
        let rate = self
            .shared
            .sample_rate
            .load(std::sync::atomic::Ordering::Relaxed);
        match (&self.running, &self.audio_error) {
            (Some(_), _) if self.is_win_engine() => match self.engine {
                EngineState::NotInstalled => ("Engine not installed (see Settings)".into(), true),
                EngineState::Idle => ("Whole computer · engine idle".into(), false),
                EngineState::Active => ("Whole computer · all apps".into(), false),
                EngineState::Outdated => ("Engine needs updating (see Settings)".into(), true),
            },
            (Some(r), _) => {
                let d = r
                    .description
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if rate > 0 && !self.s.simple {
                    (format!("{d} · {:.1} kHz", rate as f32 / 1000.0), false)
                } else {
                    (d, false)
                }
            }
            (None, Some(e)) => (e.clone(), true),
            (None, None) => ("Audio isn't going through the Equaliser".into(), false),
        }
    }

    // ------------------------------------------------------------ top

    fn plate(&mut self, ui: &mut Ui, screen: Rect, o: Vec2, t: &Tokens) {
        let plate = Rect::from_min_size(screen.min, Vec2::new(screen.width(), PLATE_H + o.y));
        if !self.s.system_title_bar {
            hifi::drag_to_move(ui, plate);
        }
        hifi::nameplate(
            ui,
            Pos2::new(24.0, 10.0) + o,
            t,
            "Equaliser",
            &format!("EQ-001 · Ten band parametric equaliser · V{VERSION}"),
        );
        if !self.s.system_title_bar {
            hifi::window_buttons(ui, Pos2::new(screen.right() - 10.0, screen.top() + 6.0), t);
        }
        let right = o.x + DESIGN.x - 24.0;
        let row = o.y + ROW_Y;
        let gear = Rect::from_min_size(Pos2::new(right - 30.0, row - 15.0), Vec2::splat(30.0));
        if hifi::gear_button(ui, gear, Id::new("settings"), t, self.settings_open).clicked() {
            self.settings_open = !self.settings_open;
        }
        let (seg, picked) = hifi::segmented(
            ui,
            Pos2::new(gear.left() - 12.0, row),
            true,
            Id::new("simple"),
            t,
            &["Simple", "Detailed"],
            if self.s.simple { 0 } else { 1 },
            Seg::Plain,
            &[],
            &[],
        );
        if let Some(i) = picked {
            self.s.simple = i == 0;
        }
        let (_, picked) =
            hifi::family_switch(ui, Pos2::new(seg.left() - 14.0, row), t, 2, self.installed);
        if let Some(i) = picked {
            let a = App::ALL[i];
            match family::find(a.program(), self.s.family.get(a.name())) {
                Some(exe) => match family::launch(&exe) {
                    Ok(()) => self.notify(format!("Starting the {}…", a.name())),
                    Err(e) => self.notify(format!("The {} wouldn't start: {e}", a.name())),
                },
                None => self.notify(format!(
                    "The {} isn't installed here (looked next to the Equaliser and on the PATH).",
                    a.name()
                )),
            }
            self.refresh_family();
        }
    }

    /// The On switch and status, the preset, the preamp and the meters.
    fn controls_row(&mut self, ui: &mut Ui, o: Vec2, t: &Tokens) {
        let y = o.y + CONTROLS_Y;
        let x0 = o.x + 24.0;
        let lab = |ui: &Ui, x: f32, s: &str| {
            hifi::label(
                ui.painter(),
                Pos2::new(x, y),
                Align2::LEFT_CENTER,
                s,
                hifi::ui_font(12.0),
                t.dim,
            )
        };
        let sep = |ui: &Ui, x: f32| {
            ui.painter().line_segment(
                [Pos2::new(x, y - 18.0), Pos2::new(x, y + 18.0)],
                Stroke::new(1.0, t.line),
            );
        };

        // On / off (runs the audio) and where it's working.
        let on = self.running.is_some();
        let sw = Rect::from_min_size(Pos2::new(x0, y - 15.0), Vec2::new(52.0, 30.0));
        if hifi::switch(ui, sw, Id::new("run"), t, on)
            .on_hover_text("Start or stop the Equaliser")
            .clicked()
        {
            self.toggle_run();
        }
        let (status, problem) = self.status();
        let p = ui.painter();
        hifi::label(
            p,
            Pos2::new(sw.right() + 12.0, y - 9.0),
            Align2::LEFT_CENTER,
            if on { "On" } else { "Off" },
            hifi::bold(15.0),
            t.text,
        );
        let light = if problem {
            t.warn
        } else if on {
            t.ok
        } else {
            t.dim.gamma_multiply(0.6)
        };
        hifi::status_light(p, Pos2::new(sw.right() + 17.0, y + 9.0), light);
        let status_w = if self.s.simple { 420.0 } else { 200.0 };
        let sr = hifi::label_fit(
            p,
            Pos2::new(sw.right() + 26.0, y + 9.0),
            Align2::LEFT_CENTER,
            &status,
            hifi::ui_font(12.0),
            if problem { t.warn } else { t.dim },
            status_w,
        );
        // Clicking the status opens the audio settings.
        let hit = Rect::from_min_max(
            Pos2::new(sw.right() + 8.0, y - 18.0),
            Pos2::new(sr.right().max(sw.right() + 60.0), y + 18.0),
        );
        if ui
            .interact(hit, Id::new("status"), egui::Sense::click())
            .on_hover_text("Audio settings")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.settings_open = true;
        }
        let mut x = sw.right() + 26.0 + status_w + 14.0;
        sep(ui, x);
        x += 14.0;

        // Preset: its name, load and save, flat and reset.
        x = lab(ui, x, "Preset").right() + 10.0;
        let name_r = Rect::from_min_size(
            Pos2::new(x, y - 13.0),
            Vec2::new(if self.s.simple { 200.0 } else { 130.0 }, 26.0),
        );
        hifi::well(ui.painter(), name_r, t);
        hifi::label_fit(
            ui.painter(),
            Pos2::new(name_r.left() + 10.0, y),
            Align2::LEFT_CENTER,
            &self.s.preset_name,
            hifi::ui_font(12.5),
            t.text,
            name_r.width() - 20.0,
        );
        x = name_r.right() + 6.0;
        let buttons: &[(&str, u8)] = if self.s.simple {
            &[("Load…", 0), ("Flat", 2)]
        } else {
            &[
                ("Load…", 0),
                ("Save…", 1),
                ("Flat", 2),
                ("Reset", 3),
                ("Bypass", 4),
            ]
        };
        for (label, what) in buttons {
            let w = hifi::button_width(ui, label);
            let r = Rect::from_min_size(Pos2::new(x, y - 13.0), Vec2::new(w, 26.0));
            let primary = *what == 4 && self.s.bypass;
            if hifi::button(ui, r, Id::new(("preset", *what)), t, label, primary).clicked() {
                match what {
                    0 => self.load_preset(),
                    1 => self.save_preset(),
                    2 => self.flat(),
                    3 => self.reset(),
                    _ => {
                        self.s.bypass = !self.s.bypass;
                        self.touch("Bypass", if self.s.bypass { "on" } else { "off" });
                    }
                }
            }
            x = r.right() + 6.0;
        }
        if self.s.simple {
            return;
        }
        x += 8.0;
        sep(ui, x);
        x += 14.0;

        // Preamp, and Auto, which keeps the response's peak at 0 dB.
        x = lab(ui, x, "Preamp").right() + 10.0;
        let mut pre = self.s.preamp_db;
        let sr = Rect::from_min_size(Pos2::new(x + 8.0, y - 8.0), Vec2::new(70.0, 16.0));
        if hifi::hslider(ui, sr, Id::new("preamp"), t, &mut pre, &PREAMP).changed() {
            // Moving the slider takes over from Auto.
            self.s.auto_preamp = false;
            self.s.preamp_db = pre;
            self.touch("Preamp", fmt_db(pre));
        }
        let vr = hifi::label(
            ui.painter(),
            Pos2::new(sr.right() + 14.0, y),
            Align2::LEFT_CENTER,
            &fmt_db(self.s.preamp_db),
            hifi::num(12.0),
            t.text,
        );
        let mut auto = self.s.auto_preamp;
        hifi::labelled_switch(
            ui,
            Pos2::new(vr.left() + 66.0, y - 9.0),
            Id::new("auto-preamp"),
            t,
            &mut auto,
            "Auto",
        );
        if auto != self.s.auto_preamp {
            self.s.auto_preamp = auto;
            if auto {
                self.s.preamp_db = auto_preamp_db(&self.s.bands, 48_000.0);
            }
            self.touch(
                "Auto preamp",
                if auto {
                    fmt_db(self.s.preamp_db)
                } else {
                    "off".into()
                },
            );
        }

        // In and Out meters, left and right.
        let right = o.x + DESIGN.x - 24.0;
        let mw = 70.0;
        let p = ui.painter();
        for (k, name) in ["In", "Out"].iter().enumerate() {
            let top = y - 15.0 + k as f32 * 17.0;
            hifi::label(
                p,
                Pos2::new(right - mw - 8.0, top + 6.0),
                Align2::RIGHT_CENTER,
                name,
                hifi::ui_font(11.0),
                t.dim,
            );
            for side in 0..2 {
                let r = Rect::from_min_size(
                    Pos2::new(right - mw, top + side as f32 * 7.0),
                    Vec2::new(mw, 5.0),
                );
                hifi::meter_in(p, r, t, self.peaks_db[k * 2 + side], self.s.meter_colours);
            }
        }
    }

    fn graph_area(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let rate = match self
            .shared
            .sample_rate
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => 48_000.0,
            r => r as f32,
        };
        let mut s = self.settings();
        let style = (!self.s.analyser_ink).then_some(spectrum::Style {
            scheme: self.s.analyser_scheme,
            fill: self.s.analyser_fill,
            strength: 0.4,
        });
        let res = graph::graph(
            ui,
            rect,
            &mut s,
            &mut self.s.selected,
            rate,
            (self.s.analyser && self.running.is_some()).then_some(&self.analyzer),
            style,
            self.s.simple.then_some(&TONE[..]),
            t,
        );
        if let Some(i) = res.changed {
            let b = s.bands[i];
            self.touch(
                format!("Band {} {}", i + 1, kind_name(b.kind)),
                if b.enabled {
                    format!("{} {:+.1} dB", fmt_hz(b.freq), b.gain_db)
                } else {
                    "off".into()
                },
            );
        }
        self.s.bands = s.bands;
    }

    // ------------------------------------------------------------ bands

    fn bands(&mut self, ui: &mut Ui, area: Rect, t: &Tokens) {
        let gap = 8.0;
        let w = (area.width() - gap * (NUM_BANDS - 1) as f32) / NUM_BANDS as f32;
        for i in 0..NUM_BANDS {
            let r = Rect::from_min_size(
                Pos2::new(area.left() + i as f32 * (w + gap), area.top()),
                Vec2::new(w, area.height()),
            );
            self.band_card(ui, i, r, t);
        }
    }

    fn band_card(&mut self, ui: &mut Ui, i: usize, r: Rect, t: &Tokens) {
        let sel = self.s.selected == i;
        let mut b = self.s.bands[i];
        let before = b;
        let mut touched: Option<(&str, String)> = None;
        // Clicking anywhere on the card selects its band. Registered before
        // the card's controls, so they still get their own clicks.
        let mut interacted = ui
            .interact(r, Id::new(("card", i)), egui::Sense::click())
            .clicked();
        {
            let p = ui.painter();
            hifi::well(p, r, t);
            if sel {
                p.rect_stroke(
                    r,
                    egui::CornerRadius::same(8),
                    Stroke::new(2.0, t.accent),
                    egui::StrokeKind::Inside,
                );
            }
            p.circle_filled(
                Pos2::new(r.left() + 16.0, r.top() + 18.0),
                5.0,
                BAND_COLOURS[i],
            );
            hifi::label(
                p,
                Pos2::new(r.left() + 27.0, r.top() + 18.0),
                Align2::LEFT_CENTER,
                &(i + 1).to_string(),
                hifi::bold(13.0),
                if b.enabled { t.text } else { t.dim },
            );
        }
        // On / off
        let sw = Rect::from_min_size(
            Pos2::new(r.right() - 40.0, r.top() + 10.0),
            Vec2::new(30.0, 17.0),
        );
        if hifi::switch(ui, sw, Id::new(("on", i)), t, b.enabled).clicked() {
            b.enabled = !b.enabled;
            touched = Some(("", if b.enabled { "on".into() } else { "off".into() }));
            interacted = true;
        }

        // Filter type
        let tr = Rect::from_min_size(
            Pos2::new(r.left() + 8.0, r.top() + 36.0),
            Vec2::new(r.width() - 16.0, 26.0),
        );
        // Peak and shelves only: high-pass, low-pass and notch aren't needed
        // in most home setups. A band loaded from a preset with one of
        // those keeps it, and it's listed so the menu shows it.
        let mut kinds = PANEL_KINDS.to_vec();
        if !kinds.contains(&b.kind) {
            kinds.push(b.kind);
        }
        let names: Vec<String> = kinds.iter().map(|k| kind_name(*k).to_string()).collect();
        let cur = kinds.iter().position(|k| *k == b.kind).unwrap_or(0);
        if let Some(k) = hifi::select(ui, tr, &format!("type-{i}"), t, &names, cur) {
            b.kind = kinds[k];
            touched = Some(("Type", kind_name(b.kind).to_string()));
            interacted = true;
        }

        if b.kind.has_gain() {
            self.clip_light(ui, i, Pos2::new(r.center().x, r.top() + 72.0), t);
        }

        // Gain fader, or a note for the types without gain.
        let fr = Rect::from_min_size(
            Pos2::new(r.center().x - 14.0, r.top() + 82.0),
            Vec2::new(28.0, 150.0),
        );
        if b.kind.has_gain() {
            let resp = hifi::vfader(ui, fr, Id::new(("gain", i)), t, &mut b.gain_db, &GAIN, 0.0);
            interacted |= resp.is_pointer_button_down_on();
            if resp.changed() {
                touched = Some(("Gain", fmt_db(b.gain_db)));
            }
        } else {
            let p = ui.painter();
            hifi::label(
                p,
                fr.center() - Vec2::new(0.0, 9.0),
                Align2::CENTER_CENTER,
                "No gain",
                hifi::ui_font(11.5),
                t.dim,
            );
            hifi::label(
                p,
                fr.center() + Vec2::new(0.0, 9.0),
                Align2::CENTER_CENTER,
                "fixed slope",
                hifi::ui_font(11.5),
                t.dim,
            );
        }
        hifi::label(
            ui.painter(),
            Pos2::new(r.center().x, fr.bottom() + 16.0),
            Align2::CENTER_CENTER,
            &if b.kind.has_gain() {
                fmt_db(b.gain_db)
            } else {
                "—".into()
            },
            hifi::num(12.0),
            t.text,
        );

        // Frequency and Q
        let ky = fr.bottom() + 54.0;
        let (f_s, q_s) = (short_hz(b.freq), format!("{:.2}", b.q));
        let resp = hifi::knob(
            ui,
            Pos2::new(r.center().x - 24.0, ky),
            Id::new(("freq", i)),
            t,
            &mut b.freq,
            &freq_spec(default_bands()[i].freq),
            &f_s,
            "Hz",
        );
        interacted |= resp.is_pointer_button_down_on();
        if resp.changed() {
            touched = Some(("Freq", fmt_hz(b.freq)));
        }
        let resp = hifi::knob(
            ui,
            Pos2::new(r.center().x + 24.0, ky),
            Id::new(("q", i)),
            t,
            &mut b.q,
            &q_spec(b.kind),
            &q_s,
            "Q",
        );
        interacted |= resp.is_pointer_button_down_on();
        if resp.changed() {
            touched = Some(("Q", format!("{:.2}", b.q)));
        }

        if interacted || touched.is_some() {
            self.s.selected = i;
        }
        if b != before {
            self.s.bands[i] = b;
        }
        if let Some((what, val)) = touched {
            self.touch(format!("Band {} {what}", i + 1).trim().to_string(), val);
        }
    }

    /// Simple view: five tone sliders, each moving one band's gain.
    fn tone(&mut self, ui: &mut Ui, area: Rect, t: &Tokens) {
        let gap = 18.0;
        let w = (area.width() - gap * (TONE.len() - 1) as f32) / TONE.len() as f32;
        for (k, (name, band)) in TONE.iter().enumerate() {
            let r = Rect::from_min_size(
                Pos2::new(area.left() + k as f32 * (w + gap), area.top()),
                Vec2::new(w, area.height()),
            );
            hifi::well(ui.painter(), r, t);
            hifi::label(
                ui.painter(),
                Pos2::new(r.center().x, r.top() + 22.0),
                Align2::CENTER_CENTER,
                name,
                hifi::bold(15.0),
                t.text,
            );
            self.clip_light(ui, *band, Pos2::new(r.right() - 32.0, r.top() + 22.0), t);
            let fr = Rect::from_min_size(
                Pos2::new(r.center().x - 18.0, r.top() + 48.0),
                Vec2::new(36.0, r.height() - 120.0),
            );
            let mut b = self.s.bands[*band];
            let mut g = b.gain_db.clamp(TONE_GAIN.min, TONE_GAIN.max);
            if hifi::vfader(ui, fr, Id::new(("tone", k)), t, &mut g, &TONE_GAIN, 0.0).changed() {
                b.gain_db = g;
                b.enabled = true;
                self.s.bands[*band] = b;
                self.touch(*name, fmt_db(g));
            }
            hifi::label(
                ui.painter(),
                Pos2::new(r.center().x, fr.bottom() + 22.0),
                Align2::CENTER_CENTER,
                &fmt_db(b.gain_db),
                hifi::num(14.0),
                t.text,
            );
            hifi::label(
                ui.painter(),
                Pos2::new(r.center().x, fr.bottom() + 44.0),
                Align2::CENTER_CENTER,
                &fmt_hz(b.freq),
                hifi::ui_font(12.0),
                t.dim,
            );
        }
    }

    fn footer(&mut self, ui: &mut Ui, o: Vec2, t: &Tokens) {
        let p = ui.painter();
        let y = o.y + DESIGN.y - 20.0;
        let n = now();
        let msg = match (&self.notice, &self.touched) {
            (Some((m, at)), _) if n - at < 12.0 => Some((m.clone(), t.warn)),
            (_, Some((name, v, at))) if n - at < 2.5 => Some((format!("{name}: {v}"), t.text)),
            _ => None,
        };
        if !self.s.simple {
            hifi::label_fit(
                p,
                Pos2::new(o.x + 24.0, y),
                Align2::LEFT_CENTER,
                "Drag the points on the graph · scroll over one for Q · right-click to switch a band off · double-click to reset · Space: bypass · Ctrl+O / Ctrl+S or drop a file: load / save",
                hifi::ui_font(11.5),
                t.dim,
                if msg.is_some() { 760.0 } else { DESIGN.x - 48.0 },
            );
        }
        if let Some((m, c)) = msg {
            hifi::label_fit(
                p,
                Pos2::new(o.x + DESIGN.x - 24.0, y),
                Align2::RIGHT_CENTER,
                &m,
                hifi::ui_font(12.0),
                c,
                if self.s.simple { 1100.0 } else { 440.0 },
            );
        }
    }

    // ------------------------------------------------------------ settings

    fn settings_drawer(&mut self, ctx: &egui::Context, t: &Tokens) {
        let mut open = self.settings_open;
        let mut look = self.s.look;
        let mut system_bar = self.s.system_title_bar;
        let mut actions: Vec<Action> = Vec::new();
        let backend_names: Vec<String> = self.backends.iter().map(|b| b.label.clone()).collect();
        let backend_cur = self
            .backends
            .iter()
            .position(|b| Some(&b.id) == self.s.backend.as_ref())
            .unwrap_or(0);
        let fixed_input = self.backend().is_some_and(|b| b.fixed_input);
        let is_win_engine = self.is_win_engine();
        let mut inputs = vec!["Default input".to_string()];
        inputs.extend(self.devices.inputs.iter().map(|d| d.label.clone()));
        let in_cur = self
            .s
            .input
            .as_ref()
            .and_then(|id| self.devices.inputs.iter().position(|d| &d.id == id))
            .map_or(0, |i| i + 1);
        let mut outputs = vec!["Default output".to_string()];
        outputs.extend(self.devices.outputs.iter().map(|d| d.label.clone()));
        let out_cur = self
            .s
            .output
            .as_ref()
            .and_then(|id| self.devices.outputs.iter().position(|d| &d.id == id))
            .map_or(0, |i| i + 1);
        let mut latency = self.s.latency_ms;
        let dropouts = self
            .shared
            .dropouts
            .load(std::sync::atomic::Ordering::Relaxed);
        let help = self.help_text();
        let engine = self.engine;
        let mut analyser = self.s.analyser;
        let mut meter_colours = self.s.meter_colours;
        let mut colours = vec!["Display".to_string()];
        colours.extend(Scheme::ALL.iter().map(|s| title_case(s.name())));
        let colour_cur = if self.s.analyser_ink {
            0
        } else {
            1 + Scheme::ALL
                .iter()
                .position(|s| *s == self.s.analyser_scheme)
                .unwrap_or(0)
        };
        let styles: Vec<String> = ["Fill and line", "Line", "Fill"].map(String::from).to_vec();
        let style_cur = Look::ALL
            .iter()
            .position(|l| *l == self.s.analyser_fill)
            .unwrap_or(0);

        hifi::drawer(ctx, t, "Settings", &mut open, |ui| {
            hifi::drawer_heading(ui, t, "Audio");
            let r = hifi::drawer_row(ui, t, "System", 200.0);
            if let Some(i) = hifi::select(ui, r, "backend", t, &backend_names, backend_cur) {
                actions.push(Action::Backend(i));
            }
            let r = hifi::drawer_row(ui, t, "Input", 200.0);
            if fixed_input {
                hifi::label_fit(
                    ui.painter(),
                    Pos2::new(r.right(), r.center().y),
                    Align2::RIGHT_CENTER,
                    if is_win_engine {
                        "Windows audio (all apps)"
                    } else {
                        "TAW Equaliser (apps play here)"
                    },
                    hifi::ui_font(12.5),
                    t.dim,
                    200.0,
                );
            } else if let Some(i) = hifi::select(ui, r, "input", t, &inputs, in_cur) {
                actions.push(Action::Input(i));
            }
            let r = hifi::drawer_row(
                ui,
                t,
                if is_win_engine {
                    "Output (meters)"
                } else {
                    "Output"
                },
                200.0,
            );
            if let Some(i) = hifi::select(ui, r, "output", t, &outputs, out_cur) {
                actions.push(Action::Output(i));
            }
            if !fixed_input {
                let r = hifi::drawer_row(ui, t, "Buffer", 200.0);
                let sr = Rect::from_min_size(r.min + Vec2::new(8.0, 5.0), Vec2::new(130.0, 16.0));
                let resp = hifi::hslider(ui, sr, Id::new("latency"), t, &mut latency, &LATENCY);
                hifi::label(
                    ui.painter(),
                    Pos2::new(r.right(), r.center().y),
                    Align2::RIGHT_CENTER,
                    &format!("{latency:.0} ms"),
                    hifi::num(12.0),
                    t.text,
                );
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    actions.push(Action::Restart);
                }
            }
            ui.horizontal(|ui| {
                let mut add = |ui: &mut Ui, label: &str, a: Action| {
                    let w = hifi::button_width(ui, label);
                    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), egui::Sense::hover());
                    if hifi::button(ui, r, Id::new(("drawer", label)), t, label, false).clicked() {
                        actions.push(a);
                    }
                };
                add(ui, "Rescan devices", Action::Rescan);
                if is_win_engine {
                    let install = match engine {
                        EngineState::NotInstalled => "Install engine",
                        EngineState::Outdated => "Update engine",
                        _ => "Add to an output",
                    };
                    add(ui, install, Action::Install);
                    if engine != EngineState::NotInstalled {
                        add(ui, "Remove", Action::Remove);
                    }
                }
            });
            if !is_win_engine {
                ui.label(
                    egui::RichText::new(format!("Dropouts: {dropouts}"))
                        .font(hifi::ui_font(12.0))
                        .color(t.dim),
                );
            }
            ui.label(
                egui::RichText::new(&help.0)
                    .font(hifi::ui_font(12.5))
                    .color(if help.1 { t.warn } else { t.dim }),
            );

            hifi::drawer_heading(ui, t, "Display");
            let r = hifi::drawer_row(ui, t, "Spectrum behind the curve", 32.0);
            let sw = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
            if hifi::switch(ui, sw, Id::new("analyser"), t, analyser).clicked() {
                analyser = !analyser;
            }
            let r = hifi::drawer_row(ui, t, "Spectrum colour", 160.0);
            if let Some(i) = hifi::select(ui, r, "an-colour", t, &colours, colour_cur) {
                actions.push(Action::Colour(i));
            }
            let r = hifi::drawer_row(ui, t, "Spectrum style", 160.0);
            if let Some(i) = hifi::select(ui, r, "an-style", t, &styles, style_cur) {
                actions.push(Action::Style(i));
            }
            let r = hifi::drawer_row(ui, t, "Meter colours", 150.0);
            let names = hifi::MeterColours::ALL.map(|m| m.name());
            let sel = hifi::MeterColours::ALL
                .iter()
                .position(|m| *m == meter_colours)
                .unwrap_or(0);
            if let (_, Some(i)) = hifi::segmented(
                ui,
                Pos2::new(r.right(), r.center().y),
                true,
                Id::new("meter-colours"),
                t,
                &names,
                sel,
                Seg::Plain,
                &[],
                &[],
            ) {
                meter_colours = hifi::MeterColours::ALL[i];
            }

            hifi::look_settings(ui, t, &mut look);
            hifi::drawer_heading(ui, t, "Window");
            let r = hifi::drawer_row(ui, t, "Use the system title bar", 32.0);
            let sw = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
            if hifi::switch(ui, sw, Id::new("system-bar"), t, system_bar).clicked() {
                system_bar = !system_bar;
            }
        });

        self.settings_open = open;
        self.s.look = look;
        self.s.analyser = analyser;
        self.s.meter_colours = meter_colours;
        if (latency - self.s.latency_ms).abs() > f32::EPSILON {
            self.s.latency_ms = latency;
            self.touch("Buffer", format!("{latency:.0} ms"));
        }
        if system_bar != self.s.system_title_bar {
            self.s.system_title_bar = system_bar;
            ctx.send_viewport_cmd(ViewportCommand::Decorations(system_bar));
        }
        for a in actions {
            match a {
                Action::Backend(i) => {
                    self.s.backend = Some(self.backends[i].id.clone());
                    self.s.input = None;
                    self.s.output = None;
                    self.refresh_devices();
                    self.restart_if_running();
                }
                Action::Input(i) => {
                    self.s.input = (i > 0).then(|| self.devices.inputs[i - 1].id.clone());
                    self.restart_if_running();
                }
                Action::Output(i) => {
                    self.s.output = (i > 0).then(|| self.devices.outputs[i - 1].id.clone());
                    self.restart_if_running();
                }
                Action::Restart => self.restart_if_running(),
                Action::Rescan => {
                    self.refresh_devices();
                    self.touch(
                        "Devices",
                        format!(
                            "{} in / {} out",
                            self.devices.inputs.len(),
                            self.devices.outputs.len()
                        ),
                    );
                }
                Action::Install => self.run_setup(Setup::Install),
                Action::Remove => self.run_setup(Setup::Remove),
                Action::Colour(i) => {
                    if i == 0 {
                        self.s.analyser_ink = true;
                    } else {
                        self.s.analyser_ink = false;
                        self.s.analyser_scheme = Scheme::ALL[i - 1];
                    }
                }
                Action::Style(i) => self.s.analyser_fill = Look::ALL[i],
            }
        }
    }

    /// What to do with the audio choice in hand, or what went wrong.
    fn help_text(&self) -> (String, bool) {
        let is_win_engine = self.is_win_engine();
        let is_pw_sink = self.s.backend.as_deref() == Some(audio::PIPEWIRE_SINK);
        if let Some(e) = self.audio_error.as_ref().or(self.device_error.as_ref()) {
            (e.clone(), true)
        } else if is_win_engine && self.engine == EngineState::NotInstalled {
            ("The Equaliser's engine isn't installed yet. Press Install engine, allow Windows' permission prompt, and pick your output in the window that opens. Every app on that output then goes through the Equaliser, with no virtual cable.".to_string(), false)
        } else if is_win_engine && self.engine == EngineState::Outdated {
            ("The engine installed in Windows is older than this Equaliser, so some settings (such as the high-pass, low-pass and notch filters) have no effect. Press Update engine, allow Windows' permission prompt, and pick the same output to install the new engine.".to_string(), true)
        } else if is_win_engine {
            ("The Equaliser's engine runs inside Windows and equalises every app on the outputs it's installed on (Add puts it on another, Remove takes it off them all). The On switch sends your settings to it, and they stay set after the Equaliser closes. Use Bypass to switch the EQ off.".to_string(), false)
        } else if is_pw_sink {
            ("The Equaliser adds an output called \"TAW Equaliser\". Choose it in your sound settings (or route apps to it with qpwgraph or Helvum) and the Equaliser plays the result to the Output above.".to_string(), false)
        } else if self
            .s
            .backend
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case("asio"))
        {
            ("Pick your interface for Input and Output. Send your playback into its inputs (a virtual channel in its mixer), and the Equaliser plays the result to its outputs.".to_string(), false)
        } else if self
            .s
            .backend
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case("wasapi"))
        {
            ("Set Windows' default output to a virtual cable (such as VB-CABLE), choose the cable's output (such as \"CABLE Output\") as Input and your speakers as Output.".to_string(), false)
        } else {
            (
                "Audio from Input goes through the EQ to Output.".to_string(),
                false,
            )
        }
    }
}

/// Changes asked for in the settings drawer, applied after it's drawn.
enum Action {
    Backend(usize),
    Input(usize),
    Output(usize),
    Restart,
    Rescan,
    Install,
    Remove,
    Colour(usize),
    Style(usize),
}

fn title_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut c = lower.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl eframe::App for EqApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Lay out at the design size and scale the whole panel to fit.
        taw_panel::frame::fit_to_window(ui.ctx(), DESIGN, 2.0);
        let ctx = ui.ctx().clone();
        let t = Tokens::from_ctx(&ctx, &self.s.look);
        hifi::apply_visuals(&ctx, &t);

        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.update_meters(dt);
        // The analyser also tells the CLIP lights where the music is loud,
        // so it runs while audio does, shown or not.
        if self.s.analyser || self.running.is_some() {
            // Simple's spectrum moves more calmly.
            self.analyzer.ballistics.rise_ms = if self.s.simple { 120.0 } else { 0.0 };
            self.analyzer.ballistics.fall_ms = if self.s.simple { 700.0 } else { 215.0 };
            self.analyzer.update();
            // `active` only says whether samples came in since the last
            // frame, and audio comes in blocks that can be longer than a
            // frame (and not at all from Windows' loopback while the output
            // is silent). Keep the spectrum up while audio runs, and let it
            // fall away when the sound stops rather than freeze or blink.
            let t = now();
            if self.analyzer.active {
                self.analyser_fed = t;
            } else if t - self.analyser_fed > 0.25 {
                for b in &mut self.analyzer.bins_db {
                    *b = (*b - 60.0 * dt).max(-140.0);
                }
            }
        }
        if self.running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.watch_engine(&ctx);

        // Keyboard shortcuts (not while a control has focus).
        let free = ui.memory(|m| m.focused().is_none()) && !self.settings_open;
        let (space, ctrl_o, ctrl_s) = ui.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.modifiers.command && i.key_pressed(Key::O),
                i.modifiers.command && i.key_pressed(Key::S),
            )
        });
        if space && free {
            self.s.bypass = !self.s.bypass;
            self.touch("Bypass", if self.s.bypass { "on" } else { "off" });
        }
        if ctrl_o {
            self.load_preset();
        }
        let dropped = ui.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.load_preset_file(&path);
        }
        if ctrl_s {
            self.save_preset();
        }

        self.hot = if self.s.bypass {
            vec![false; NUM_BANDS]
        } else {
            hot_bands(&self.s.bands, self.s.preamp_db, self.sample_rate())
        };

        let screen = ui.max_rect();
        hifi::faceplate(ui.painter(), screen, &t);
        let origin = (screen.center() - DESIGN / 2.0).max(screen.min);
        let o = origin.to_vec2();

        self.plate(ui, screen, o, &t);
        self.controls_row(ui, o, &t);
        if self.s.simple {
            let g = Rect::from_min_max(
                Pos2::new(24.0, GRAPH_TOP) + o,
                Pos2::new(DESIGN.x - 24.0, 372.0) + o,
            );
            self.graph_area(ui, g, &t);
            let area = Rect::from_min_max(
                Pos2::new(24.0, 390.0) + o,
                Pos2::new(DESIGN.x - 24.0, DESIGN.y - 40.0) + o,
            );
            self.tone(ui, area, &t);
        } else {
            let g = Rect::from_min_max(
                Pos2::new(24.0, GRAPH_TOP) + o,
                Pos2::new(DESIGN.x - 24.0, 402.0) + o,
            );
            self.graph_area(ui, g, &t);
            let area = Rect::from_min_max(
                Pos2::new(24.0, 418.0) + o,
                Pos2::new(DESIGN.x - 24.0, DESIGN.y - 40.0) + o,
            );
            self.bands(ui, area, &t);
        }
        self.footer(ui, o, &t);
        self.settings_drawer(&ctx, &t);
        if !self.s.system_title_bar {
            hifi::resize_edges(ui, screen);
        }

        // Simple has no preamp control, so Auto always looks after it there;
        // only Detailed can turn Auto off. It settles when the mouse button
        // is let go, so a boost is heard while it's dragged rather than
        // cancelled out step by step, and isn't worked out on every frame
        // of a drag.
        if self.s.simple {
            self.s.auto_preamp = true;
        }
        if self.s.auto_preamp && !ui.input(|i| i.pointer.any_down()) {
            self.s.preamp_db = auto_preamp_db(&self.s.bands, 48_000.0);
        }
        self.shared.store(&self.settings());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Tokens::new(&self.s.look, false)
            .face_bottom
            .to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_still_load() {
        // 0.3.0 settings have the Jupiter panel and no look fields.
        let s: Saved = serde_json::from_str(
            r#"{"preamp_db": -3.0, "panel": "Silver", "knobs": "Pointer", "preset_name": "Warm"}"#,
        )
        .unwrap();
        assert_eq!(s.preamp_db, -3.0);
        assert_eq!(s.preset_name, "Warm");
        assert_eq!(s.look, hifi::Look::default());
        assert!(s.analyser_ink);
        assert!(!s.simple);
    }

    #[test]
    fn auto_preamp_comes_on_once() {
        // 0.3.0 saved Auto off by default: 0.3.1 turns it on once...
        let mut s: Saved =
            serde_json::from_str(r#"{"auto_preamp": false, "preamp_db": -3.0}"#).unwrap();
        upgrade(&mut s);
        assert!(s.auto_preamp);
        assert_eq!(s.meter_colours, hifi::MeterColours::Classic);
        // ...and leaves it off after that, if the user turns it off.
        s.auto_preamp = false;
        let mut again: Saved = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        upgrade(&mut again);
        assert!(!again.auto_preamp);
        // New settings start with it on.
        assert!(Saved::default().auto_preamp);
    }

    #[test]
    fn tone_sliders_move_distinct_bands() {
        let mut seen = std::collections::HashSet::new();
        for (_, b) in TONE {
            assert!(b < NUM_BANDS);
            assert!(seen.insert(b));
        }
    }
}
