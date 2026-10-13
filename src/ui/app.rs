//! The EQ Box panel.

use super::graph::{self, fmt_freq};
use super::spectrum::{self, Look, Scheme};
use super::theme::*;
use super::widgets::*;
use crate::analyzer::Analyzer;
use crate::audio::engine_setup::{self, EngineState, Setup};
use crate::audio::{self, BackendInfo, DeviceLists, Running, StartRequest};
use crate::dsp::*;
use crate::shared::{analyzer_channel, Shared};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, Id, Key, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The panel is laid out at this size and scaled to fit the window.
pub const DESIGN: Vec2 = Vec2::new(1640.0, 884.0);
const STORAGE_KEY: &str = "eq-box";

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
    panel: PanelKind,
    knobs: KnobStyle,
    analyser: bool,
    analyser_scheme: Scheme,
    analyser_fill: Look,
    running: bool,
    selected: usize,
    last_dir: Option<PathBuf>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            bands: default_bands(),
            preamp_db: 0.0,
            auto_preamp: false,
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
/// The filter type knob: one step per type, clockwise in `FilterKind::ALL`
/// order.
const TYPE: Spec = Spec {
    min: 0.0,
    max: (FilterKind::ALL.len() - 1) as f32,
    default: 2.0, // peak
    log: false,
    step: 1.0,
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

pub struct EqApp {
    s: Saved,
    shared: Arc<Shared>,
    running: Option<Running>,
    analyzer: Analyzer,
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
}

impl EqApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let mut s: Saved = cc
            .storage
            .and_then(|st| eframe::get_value(st, STORAGE_KEY))
            .unwrap_or_default();
        s.bands = s.bands.map(|b| b.clamped());
        s.selected = s.selected.min(NUM_BANDS - 1);
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
        let mut app = EqApp {
            shared: Arc::new(Shared::new(&settings)),
            s,
            running: None,
            analyzer: Analyzer::default(),
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
        };
        app.refresh_devices();
        if let Some(path) = std::env::args_os().nth(1) {
            app.load_preset_file(std::path::Path::new(&path));
        }
        if app.s.running {
            app.start_audio();
        }
        app
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
            return self.notify("That file has no Preamp or Filter lines EQ Box can use");
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

    fn fit_to_window(&self, ui: &Ui) {
        // Lay out at the design size and scale the whole panel to fit.
        taw_panel::frame::fit_to_window(ui.ctx(), DESIGN, 4.0);
    }

    fn update_meters(&mut self, dt: f32) {
        let peaks = self.shared.take_peaks();
        for (shown, p) in self.peaks_db.iter_mut().zip(peaks) {
            let db = gain_to_db(p).max(-90.0);
            *shown = if db > *shown {
                db
            } else {
                (*shown - 30.0 * dt).max(db).max(-90.0)
            };
        }
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
        self.fit_to_window(ui);
        let th = Theme::new(self.s.panel, self.s.knobs);
        set_visuals(ui.ctx(), &th);

        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.update_meters(dt);
        if self.s.analyser {
            self.analyzer.update();
        }
        if self.running.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.watch_engine(ui.ctx());

        // Keyboard shortcuts (not while a control has focus).
        let free = ui.memory(|m| m.focused().is_none());
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

        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0, th.page);
        let origin = (screen.center() - DESIGN / 2.0).max(screen.min);
        let o = origin.to_vec2();
        let panel = Rect::from_min_size(origin, DESIGN);
        draw_panel_bg(ui.painter(), panel, &th);

        self.top_strip(ui, o, &th);
        stripe(
            ui.painter(),
            Rect::from_min_size(Pos2::new(22.0, 110.0) + o, Vec2::new(DESIGN.x - 44.0, 6.0)),
        );
        self.response_section(ui, o, &th);
        self.audio_section(ui, o, &th);
        self.band_strips(ui, o, &th);
        self.footer(ui, o, &th);

        self.shared.store(&self.settings());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Theme::new(self.s.panel, self.s.knobs)
            .page
            .to_normalized_gamma_f32()
    }
}

pub(crate) fn set_visuals(ctx: &egui::Context, th: &Theme) {
    let mut v = egui::Visuals::dark();
    v.window_fill = Color32::from_rgb(0x1f, 0x1f, 0x1f);
    v.panel_fill = th.panel;
    v.extreme_bg_color = LCD_BG;
    v.selection.bg_fill = LCD_DIM;
    v.selection.stroke = Stroke::new(1.0, LCD_INK);
    v.override_text_color = Some(Color32::from_rgb(0xec, 0xe8, 0xdd));
    v.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xec, 0xe8, 0xdd, 60));
    v.widgets.hovered.weak_bg_fill = LCD_DIM;
    if ctx.global_style().visuals != v {
        ctx.set_visuals(v);
    }
}

pub(crate) use taw_panel::frame::draw_panel_bg;

impl EqApp {
    fn top_strip(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let at = |x: f32, y: f32| Pos2::new(x, y) + o;

        // Maker wordmark, logo and model line.
        {
            let p = ui.painter();
            let mut job = LayoutJob::default();
            job.append(
                "Timster's Audio Works - ",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER, 13.0),
                    color: th.legend_dim,
                    ..Default::default()
                },
            );
            job.append(
                "EQ Box",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER_BOLD, 13.0),
                    color: th.legend,
                    ..Default::default()
                },
            );
            let g = p.layout_job(job);
            let r = Rect::from_min_size(at(22.0, 14.0), g.size() + Vec2::new(20.0, 10.0));
            p.rect_stroke(
                r,
                CornerRadius::same(3),
                Stroke::new(1.0, th.frame),
                StrokeKind::Inside,
            );
            p.galley(r.min + Vec2::new(10.0, 5.0), g, th.legend);
        }
        slanted_text(
            ui,
            at(24.0, 50.0),
            "EQ BOX",
            font(F_LOGO, 30.0),
            th.legend,
            3.0,
            12.0,
        );
        text(
            ui.painter(),
            at(22.0, 92.0),
            Align2::LEFT_TOP,
            &format!("EQ-10 · TEN BAND PARAMETRIC EQUALISER · V{VERSION}"),
            font(F_LEGEND, 10.0),
            th.legend_dim,
            2.4,
        );

        // LED display
        let t = now();
        let line1 = match &self.touched {
            Some((n, v, when)) if t - when < 2.5 => (n.clone(), v.clone()),
            _ => (
                self.s.preset_name.clone(),
                if self.s.bypass {
                    "BYPASS".into()
                } else {
                    "EQ IN".into()
                },
            ),
        };
        let rate = self
            .shared
            .sample_rate
            .load(std::sync::atomic::Ordering::Relaxed);
        let line2 = match (&self.running, &self.audio_error) {
            (Some(r), _) if self.is_win_engine() => (
                r.description.clone(),
                match self.engine {
                    EngineState::NotInstalled => "NO ENGINE".into(),
                    EngineState::Idle => "ENGINE IDLE".into(),
                    EngineState::Active if rate > 0 => format!("{rate} HZ"),
                    EngineState::Active => "ENGINE ON".into(),
                    EngineState::Outdated => "UPDATE ENGINE".into(),
                },
            ),
            (Some(r), _) => (
                r.description.clone(),
                if rate > 0 {
                    format!("{rate} HZ")
                } else {
                    "RUNNING".into()
                },
            ),
            (None, Some(_)) => ("AUDIO ERROR".into(), "STOP".into()),
            (None, None) => ("AUDIO STOPPED".into(), "STOP".into()),
        };
        lcd(
            ui.painter(),
            Rect::from_min_size(at(340.0, 24.0), Vec2::new(316.0, 52.0)),
            &[line1, line2],
        );

        // Preset keys
        let keys: [(&str, KeyColour); 4] = [
            ("Load", K_CREAM),
            ("Save", K_CREAM),
            ("Flat", K_GREY),
            ("Reset", K_GREY),
        ];
        for (i, (label, col)) in keys.iter().enumerate() {
            let r = Rect::from_min_size(at(680.0 + i as f32 * 60.0, 36.0), Vec2::new(52.0, 22.0));
            if key(ui, r, Id::new(("preset", i)), *col, None, label, th).clicked() {
                match i {
                    0 => self.load_preset(),
                    1 => self.save_preset(),
                    2 => {
                        for b in &mut self.s.bands {
                            b.gain_db = 0.0;
                        }
                        self.s.preamp_db = 0.0;
                        self.touch("All gains", "0.0 dB");
                    }
                    _ => {
                        self.s.bands = default_bands();
                        self.s.preamp_db = 0.0;
                        self.s.preset_name = "Flat".into();
                        self.touch("Reset", "default bands");
                    }
                }
            }
        }
        text(
            ui.painter(),
            at(680.0, 18.0),
            Align2::LEFT_TOP,
            "APO PRESET",
            font(F_LEGEND_BOLD, 10.5),
            th.legend_dim,
            2.0,
        );

        // Meters
        {
            let p = ui.painter();
            let lr = |a: f32, b: f32| [a, b];
            meter(
                p,
                Rect::from_min_size(at(948.0, 26.0), Vec2::new(160.0, 16.0)),
                lr(self.peaks_db[0], self.peaks_db[1]),
            );
            meter(
                p,
                Rect::from_min_size(at(948.0, 50.0), Vec2::new(160.0, 16.0)),
                lr(self.peaks_db[2], self.peaks_db[3]),
            );
            legend(p, at(940.0, 34.0), Align2::RIGHT_CENTER, "In", th);
            legend(p, at(940.0, 58.0), Align2::RIGHT_CENTER, "Out", th);
            legend(p, at(1028.0, 74.0), Align2::CENTER_TOP, "Level L / R", th);
        }

        // Preamp, and Auto, which keeps the response's peak at 0 dB
        if self.s.auto_preamp {
            self.s.preamp_db = auto_preamp_db(&self.s.bands, 48_000.0);
        }
        let mut pre = self.s.preamp_db;
        if knob(
            ui,
            at(1150.0, 44.0),
            20.0,
            Id::new("preamp"),
            &mut pre,
            &PREAMP,
            None,
            th,
        )
        .changed()
        {
            // Turning the knob takes over from Auto.
            self.s.auto_preamp = false;
            self.s.preamp_db = pre;
            self.touch("Preamp", fmt_db(pre));
        }
        legend(
            ui.painter(),
            at(1150.0, 74.0),
            Align2::CENTER_TOP,
            "Preamp",
            th,
        );
        readout(
            ui.painter(),
            at(1150.0, 90.0),
            &fmt_db(self.s.preamp_db),
            64.0,
        );
        let r = Rect::from_min_size(at(1180.0, 46.0), Vec2::new(36.0, 18.0));
        if key(
            ui,
            r,
            Id::new("auto-preamp"),
            K_GREY,
            Some(self.s.auto_preamp),
            "Auto",
            th,
        )
        .clicked()
        {
            self.s.auto_preamp = !self.s.auto_preamp;
            if self.s.auto_preamp {
                self.s.preamp_db = auto_preamp_db(&self.s.bands, 48_000.0);
            }
            self.touch(
                "Auto preamp",
                if self.s.auto_preamp {
                    fmt_db(self.s.preamp_db)
                } else {
                    "off".into()
                },
            );
        }

        // Panel and knob switches
        let group = |ui: &Ui, x: f32, label: &str| {
            text(
                ui.painter(),
                at(x, 18.0),
                Align2::CENTER_TOP,
                label,
                font(F_LEGEND_BOLD, 10.5),
                th.legend_dim,
                2.0,
            );
        };
        group(ui, 1284.0, "PANEL");
        group(ui, 1392.0, "KNOBS");
        let sw: [(&str, bool, f32); 4] = [
            ("Black", self.s.panel == PanelKind::Black, 1238.0),
            ("Silver", self.s.panel == PanelKind::Silver, 1286.0),
            ("Colour", self.s.knobs == KnobStyle::Colour, 1346.0),
            ("Pointer", self.s.knobs == KnobStyle::Pointer, 1394.0),
        ];
        for (i, (label, on, x)) in sw.iter().enumerate() {
            let r = Rect::from_min_size(at(*x, 46.0), Vec2::new(42.0, 18.0));
            if key(ui, r, Id::new(("switch", i)), K_GREY, Some(*on), label, th).clicked() {
                match i {
                    0 => self.s.panel = PanelKind::Black,
                    1 => self.s.panel = PanelKind::Silver,
                    2 => self.s.knobs = KnobStyle::Colour,
                    _ => self.s.knobs = KnobStyle::Pointer,
                }
            }
        }

        // Bypass and Run / Stop
        let r = Rect::from_min_size(at(1450.0, 46.0), Vec2::new(56.0, 18.0));
        if key(
            ui,
            r,
            Id::new("bypass"),
            K_YELLOW,
            Some(self.s.bypass),
            "Bypass",
            th,
        )
        .clicked()
        {
            self.s.bypass = !self.s.bypass;
            self.touch("Bypass", if self.s.bypass { "on" } else { "off" });
        }
        let r = Rect::from_min_size(at(1524.0, 34.0), Vec2::new(94.0, 30.0));
        if key(
            ui,
            r,
            Id::new("run"),
            K_RED,
            Some(self.running.is_some()),
            "Run / Stop",
            th,
        )
        .clicked()
        {
            if self.running.is_some() {
                self.stop_audio();
            } else {
                self.start_audio();
            }
        }
    }

    fn response_section(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let rect = Rect::from_min_max(Pos2::new(22.0, 136.0) + o, Pos2::new(1290.0, 500.0) + o);
        section(ui.painter(), rect, "Response", th, false);
        let rate = match self
            .shared
            .sample_rate
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => 48_000.0,
            r => r as f32,
        };
        let mut s = self.settings();
        let res = graph::graph(
            ui,
            Rect::from_min_max(
                rect.min + Vec2::new(14.0, 20.0),
                rect.max - Vec2::new(14.0, 14.0),
            ),
            &mut s,
            &mut self.s.selected,
            rate,
            self.s.analyser.then_some(&self.analyzer),
            spectrum::Style {
                scheme: self.s.analyser_scheme,
                fill: self.s.analyser_fill,
                strength: 0.4,
            },
        );
        if let Some(i) = res.changed {
            let b = s.bands[i];
            self.touch(
                format!("Band {} {}", i + 1, b.kind.short()),
                if b.enabled {
                    format!("{} {:+.1}", fmt_freq(b.freq), b.gain_db)
                } else {
                    "off".into()
                },
            );
        }
        self.s.bands = s.bands;

        // Analyser tabs on the top edge, at the right: its colours, its
        // style, and on / off.
        let y = rect.top() - 9.0;
        let r = Rect::from_min_size(Pos2::new(rect.right() - 128.0, y), Vec2::new(112.0, 18.0));
        if frame_tab(
            ui,
            r,
            "analyser-tab",
            Some(self.s.analyser),
            "ANALYSER",
            None,
            th,
        ) {
            self.s.analyser = !self.s.analyser;
        }
        let r = Rect::from_min_size(Pos2::new(rect.right() - 276.0, y), Vec2::new(140.0, 18.0));
        let fill = self.s.analyser_fill.name();
        if frame_tab(ui, r, "analyser-fill", None, "STYLE", Some(fill), th) {
            self.s.analyser_fill = self.s.analyser_fill.next();
            self.touch("Analyser style", self.s.analyser_fill.name());
        }
        let r = Rect::from_min_size(Pos2::new(rect.right() - 410.0, y), Vec2::new(126.0, 18.0));
        let scheme = self.s.analyser_scheme.name();
        if frame_tab(ui, r, "analyser-colour", None, "COLOUR", Some(scheme), th) {
            self.s.analyser_scheme = self.s.analyser_scheme.next();
            self.touch("Analyser colour", self.s.analyser_scheme.name());
        }
    }

    fn audio_section(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let rect = Rect::from_min_max(Pos2::new(1306.0, 136.0) + o, Pos2::new(1618.0, 500.0) + o);
        section(ui.painter(), rect, "Audio", th, false);
        let x0 = rect.left() + 14.0;
        let w = rect.width() - 28.0;
        let row = |y: f32| Rect::from_min_size(Pos2::new(x0, rect.top() + y), Vec2::new(w, 24.0));
        let label = |ui: &Ui, y: f32, s: &str| {
            legend(
                ui.painter(),
                Pos2::new(x0, rect.top() + y),
                Align2::LEFT_TOP,
                s,
                th,
            );
        };

        // Audio system
        label(ui, 18.0, "System");
        let names: Vec<String> = self.backends.iter().map(|b| b.label.clone()).collect();
        let cur = self
            .backends
            .iter()
            .position(|b| Some(&b.id) == self.s.backend.as_ref())
            .unwrap_or(0);
        if let Some(i) = lcd_combo(ui, row(34.0), "backend", &names, cur) {
            self.s.backend = Some(self.backends[i].id.clone());
            self.s.input = None;
            self.s.output = None;
            self.refresh_devices();
            self.restart_if_running();
        }

        let fixed_input = self.backend().is_some_and(|b| b.fixed_input);
        let is_pw_sink = self.s.backend.as_deref() == Some(audio::PIPEWIRE_SINK);
        let is_win_engine = self.s.backend.as_deref() == Some(audio::WINDOWS_ENGINE);

        // Input
        label(ui, 70.0, "Input");
        if fixed_input {
            let r = row(86.0);
            ui.painter().rect_filled(r, CornerRadius::same(2), LCD_BG);
            glow_text(
                ui.painter(),
                r.left_center() + Vec2::new(8.0, 0.0),
                Align2::LEFT_CENTER,
                if is_win_engine {
                    "WINDOWS AUDIO (ALL APPS)"
                } else {
                    "EQ BOX SINK (APPS PLAY HERE)"
                },
                font(F_MONO, 13.0),
                LCD_INK,
                0.3,
            );
        } else {
            let mut items = vec!["Default input".to_string()];
            items.extend(self.devices.inputs.iter().map(|d| d.label.clone()));
            let cur = self
                .s
                .input
                .as_ref()
                .and_then(|id| self.devices.inputs.iter().position(|d| &d.id == id))
                .map_or(0, |i| i + 1);
            if let Some(i) = lcd_combo(ui, row(86.0), "input", &items, cur) {
                self.s.input = (i > 0).then(|| self.devices.inputs[i - 1].id.clone());
                self.restart_if_running();
            }
        }

        // Output
        // With the Windows engine, Output only picks what the meters hear.
        label(
            ui,
            122.0,
            if is_win_engine {
                "Output (meters)"
            } else {
                "Output"
            },
        );
        let mut items = vec!["Default output".to_string()];
        items.extend(self.devices.outputs.iter().map(|d| d.label.clone()));
        let cur = self
            .s
            .output
            .as_ref()
            .and_then(|id| self.devices.outputs.iter().position(|d| &d.id == id))
            .map_or(0, |i| i + 1);
        if let Some(i) = lcd_combo(ui, row(138.0), "output", &items, cur) {
            self.s.output = (i > 0).then(|| self.devices.outputs[i - 1].id.clone());
            self.restart_if_running();
        }

        // Buffer knob (two-device backends only) and rescan
        let ky = rect.top() + 200.0;
        if !fixed_input {
            let mut lat = self.s.latency_ms;
            let resp = knob(
                ui,
                Pos2::new(x0 + 26.0, ky),
                18.0,
                Id::new("latency"),
                &mut lat,
                &LATENCY,
                Some(FAM_PITCH),
                th,
            );
            if resp.changed() {
                self.s.latency_ms = lat;
                self.touch("Buffer", format!("{lat:.0} ms"));
            }
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.restart_if_running();
            }
            legend(
                ui.painter(),
                Pos2::new(x0 + 26.0, ky + 26.0),
                Align2::CENTER_TOP,
                "Buffer",
                th,
            );
            readout(
                ui.painter(),
                Pos2::new(x0 + 26.0, ky + 42.0),
                &format!("{:.0} ms", self.s.latency_ms),
                56.0,
            );
        }
        if is_win_engine {
            // Setup for the system-wide engine, where the Buffer knob would be.
            let install = match self.engine {
                EngineState::NotInstalled => "Install",
                EngineState::Outdated => "Update",
                _ => "Add",
            };
            let r = Rect::from_min_size(Pos2::new(x0, ky - 6.0), Vec2::new(52.0, 18.0));
            if key(ui, r, Id::new("engine-install"), K_CREAM, None, install, th).clicked() {
                self.run_setup(Setup::Install);
            }
            if self.engine != EngineState::NotInstalled {
                // Where Dropouts would be (the engine has none to count).
                let r = Rect::from_min_size(Pos2::new(x0 + 174.0, ky - 6.0), Vec2::new(52.0, 18.0));
                if key(ui, r, Id::new("engine-remove"), K_GREY, None, "Remove", th).clicked() {
                    self.run_setup(Setup::Remove);
                }
            }
        }
        let kr = Rect::from_min_size(Pos2::new(x0 + 78.0, ky - 6.0), Vec2::new(52.0, 18.0));
        if key(ui, kr, Id::new("rescan"), K_GREY, None, "Rescan", th).clicked() {
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
        if !is_win_engine {
            let p = ui.painter();
            let drops = self
                .shared
                .dropouts
                .load(std::sync::atomic::Ordering::Relaxed);
            legend(
                p,
                Pos2::new(x0 + 200.0, ky - 18.0),
                Align2::CENTER_TOP,
                "Dropouts",
                th,
            );
            readout(p, Pos2::new(x0 + 200.0, ky - 2.0), &drops.to_string(), 64.0);
        }

        // Help or error text
        let help = if let Some(e) = self.audio_error.as_ref().or(self.device_error.as_ref()) {
            (e.clone(), LED)
        } else if is_win_engine && self.engine == EngineState::NotInstalled {
            (
                "EQ Box's engine isn't installed yet. Press Install, allow Windows' permission prompt, and pick your output in the window that opens. Every app on that output then goes through EQ Box, with no virtual cable.".to_string(),
                th.legend_dim,
            )
        } else if is_win_engine && self.engine == EngineState::Outdated {
            (
                "The engine installed in Windows is older than this EQ Box, so some settings (such as the HPF, LPF and notch filters) have no effect. Press Update, allow Windows' permission prompt, and pick the same output to install the new engine.".to_string(),
                LED,
            )
        } else if is_win_engine {
            (
                "EQ Box's engine runs inside Windows and equalises every app on the outputs it's installed on (Add puts it on another, Remove takes it off them all). Run sends your settings to it, and they stay set after EQ Box closes. Use Bypass to switch the EQ off.".to_string(),
                th.legend_dim,
            )
        } else if is_pw_sink {
            (
                "EQ Box adds an output called \"EQ Box\". Choose it in your sound settings (or route apps to it with qpwgraph / Helvum) and EQ Box plays the result to the Output above.".to_string(),
                th.legend_dim,
            )
        } else if self
            .s
            .backend
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case("asio"))
        {
            (
                "Pick your interface for Input and Output. Send your playback into its inputs (a virtual channel in its mixer), and EQ Box plays the result to its outputs.".to_string(),
                th.legend_dim,
            )
        } else if self
            .s
            .backend
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case("wasapi"))
        {
            (
                "Set Windows' default output to a virtual cable (such as VB-CABLE), choose the cable's output (such as \"CABLE Output\") as Input and your speakers as Output.".to_string(),
                th.legend_dim,
            )
        } else {
            (
                "Audio from Input goes through the EQ to Output.".to_string(),
                th.legend_dim,
            )
        };
        let galley = ui.painter().layout(help.0, font(F_LEGEND, 12.5), help.1, w);
        ui.painter()
            .galley(Pos2::new(x0, rect.top() + 262.0), galley, help.1);
    }

    fn band_strips(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let gap = 8.0;
        let total = DESIGN.x - 44.0;
        let w = (total - gap * (NUM_BANDS - 1) as f32) / NUM_BANDS as f32;
        let top = 518.0;
        let h = 330.0;
        for i in 0..NUM_BANDS {
            let rect = Rect::from_min_size(
                Pos2::new(22.0 + i as f32 * (w + gap), top) + o,
                Vec2::new(w, h),
            );
            self.band_strip(ui, i, rect, th);
        }
    }

    fn band_strip(&mut self, ui: &mut Ui, i: usize, rect: Rect, th: &Theme) {
        let sel = self.s.selected == i;
        section(ui.painter(), rect, &format!("Band {}", i + 1), th, sel);
        let chip = Rect::from_min_size(
            Pos2::new(rect.right() - 30.0, rect.top() - 4.0),
            Vec2::new(18.0, 8.0),
        );
        ui.painter().rect_filled(chip.expand(3.0), 0, th.panel);
        ui.painter()
            .rect_filled(chip, CornerRadius::same(1), BAND_COLOURS[i]);
        let mut b = self.s.bands[i];
        let before = b;
        let mut touched: Option<(&str, String)> = None;
        let mut interacted = false;

        // Filter type: a stepped knob, with the type in its display
        let kinds = FilterKind::ALL;
        let mut t = kinds.iter().position(|k| *k == b.kind).unwrap_or(1) as f32;
        // In the knob column, over Freq and Q.
        let tx = rect.right() - 46.0;
        let resp = knob(
            ui,
            Pos2::new(tx, rect.top() + 46.0),
            17.0,
            Id::new(("type", i)),
            &mut t,
            &TYPE,
            Some(FAM_TYPE),
            th,
        );
        interacted |= resp.is_pointer_button_down_on();
        let kind = kinds[(t.round() as usize).min(kinds.len() - 1)];
        if kind != b.kind {
            b.kind = kind;
            touched = Some(("Type", kind.short().to_string()));
        }
        legend(
            ui.painter(),
            Pos2::new(tx, rect.top() + 68.0),
            Align2::CENTER_TOP,
            "Type",
            th,
        );
        readout(
            ui.painter(),
            Pos2::new(tx, rect.top() + 84.0),
            b.kind.short(),
            60.0,
        );

        // Gain fader
        let fr = Rect::from_min_size(
            Pos2::new(rect.left() + 14.0, rect.top() + 26.0),
            Vec2::new(36.0, 244.0),
        );
        let resp = fader(
            ui,
            fr,
            Id::new(("gain", i)),
            &mut b.gain_db,
            &GAIN,
            None,
            th,
        );
        interacted |= resp.is_pointer_button_down_on();
        if resp.changed() {
            touched = Some(("Gain", fmt_db(b.gain_db)));
        }
        // The gain readout lines up with the On key, its legend with "On".
        let cx_f = fr.center().x;
        legend(
            ui.painter(),
            Pos2::new(cx_f, rect.top() + 301.0),
            Align2::CENTER_TOP,
            "Gain",
            th,
        );
        readout(
            ui.painter(),
            Pos2::new(cx_f, rect.top() + 279.0),
            &if b.kind.has_gain() {
                format!("{:+.1}", b.gain_db)
            } else {
                "--".to_string()
            },
            50.0,
        );

        // Frequency and Q knobs
        let cx = rect.right() - 46.0;
        let resp = knob(
            ui,
            Pos2::new(cx, rect.top() + 133.0),
            19.0,
            Id::new(("freq", i)),
            &mut b.freq,
            &freq_spec(default_bands()[i].freq),
            Some(FAM_TONE),
            th,
        );
        interacted |= resp.is_pointer_button_down_on();
        if resp.changed() {
            touched = Some(("Freq", fmt_hz(b.freq)));
        }
        legend(
            ui.painter(),
            Pos2::new(cx, rect.top() + 157.0),
            Align2::CENTER_TOP,
            "Freq",
            th,
        );
        readout(
            ui.painter(),
            Pos2::new(cx, rect.top() + 173.0),
            &fmt_hz(b.freq),
            72.0,
        );

        let resp = knob(
            ui,
            Pos2::new(cx, rect.top() + 217.0),
            19.0,
            Id::new(("q", i)),
            &mut b.q,
            &q_spec(b.kind),
            Some(FAM_TONE),
            th,
        );
        interacted |= resp.is_pointer_button_down_on();
        if resp.changed() {
            touched = Some(("Q", format!("{:.2}", b.q)));
        }
        legend(
            ui.painter(),
            Pos2::new(cx, rect.top() + 241.0),
            Align2::CENTER_TOP,
            "Q",
            th,
        );
        readout(
            ui.painter(),
            Pos2::new(cx, rect.top() + 256.0),
            &format!("{:.2}", b.q),
            56.0,
        );

        // On / off
        let r = Rect::from_min_size(
            Pos2::new(cx - 20.0, rect.top() + 280.0),
            Vec2::new(40.0, 16.0),
        );
        if lit_key(ui, r, Id::new(("on", i)), ON_GREEN, b.enabled, "On", th).clicked() {
            b.enabled = !b.enabled;
            touched = Some(("", if b.enabled { "on".into() } else { "off".into() }));
            interacted = true;
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

    fn footer(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let p = ui.painter();
        let y = 860.0;
        text(
            p,
            Pos2::new(22.0, y) + o,
            Align2::LEFT_TOP,
            "Drag knobs and faders up or down (Shift for fine) · drag the points on the graph, scroll for Q, right-click to switch a band off · double-click to reset · Space: bypass · Ctrl+O / Ctrl+S or drop a file: load / save",
            font(F_LEGEND, 12.0),
            th.legend_dim,
            0.4,
        );
        if let Some((msg, when)) = &self.notice {
            if now() - when < 12.0 {
                let r = Rect::from_min_size(Pos2::new(900.0, y - 2.0) + o, Vec2::new(718.0, 18.0));
                let clip = p.with_clip_rect(r);
                text(
                    &clip,
                    r.right_top(),
                    Align2::RIGHT_TOP,
                    msg,
                    font(F_LEGEND, 12.5),
                    FOCUS,
                    0.3,
                );
            } else {
                self.notice = None;
            }
        }
    }
}
