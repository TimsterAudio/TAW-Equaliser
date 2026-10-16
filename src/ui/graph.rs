//! The response display: the display window with each band's curve, the
//! total response, the analyser behind them, and draggable band points.

use super::spectrum;
use super::widgets::wheel;
use crate::analyzer::Analyzer;
use crate::dsp::{
    Coeffs, EqSettings, FREQ_MAX, FREQ_MIN, GAIN_MAX_DB, GAIN_MIN_DB, NUM_BANDS, Q_MAX, Q_MIN,
};
use egui::epaint::Mesh;
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use taw_panel::hifi::{self, Tokens};

/// The bands' colours on the graph, on their cards and in their tags.
pub const BAND_COLOURS: [Color32; NUM_BANDS] = [
    Color32::from_rgb(0xe5, 0x57, 0x4a),
    Color32::from_rgb(0xec, 0x8a, 0x3a),
    Color32::from_rgb(0xe8, 0xc5, 0x47),
    Color32::from_rgb(0xa6, 0xd2, 0x4f),
    Color32::from_rgb(0x4f, 0xbf, 0x7a),
    Color32::from_rgb(0x3b, 0xb3, 0xb0),
    Color32::from_rgb(0x4f, 0x8f, 0xe0),
    Color32::from_rgb(0x8a, 0x6f, 0xe0),
    Color32::from_rgb(0xd8, 0x73, 0xb8),
    Color32::from_rgb(0xb8, 0xb0, 0x9e),
];

const DB_TOP: f32 = 15.0;
const DB_BOTTOM: f32 = -27.0;
const SPEC_TOP: f32 = 0.0;
const SPEC_BOTTOM: f32 = -96.0;
const NODE_R: f32 = 9.0;

pub struct GraphResult {
    /// The band whose settings changed this frame, if any.
    pub changed: Option<usize>,
}

struct Axes {
    plot: Rect,
}

impl Axes {
    fn x(&self, f: f32) -> f32 {
        let t = (f.ln() - FREQ_MIN.ln()) / (FREQ_MAX.ln() - FREQ_MIN.ln());
        self.plot.left() + t * self.plot.width()
    }
    fn f(&self, x: f32) -> f32 {
        let t = ((x - self.plot.left()) / self.plot.width()).clamp(0.0, 1.0);
        (FREQ_MIN.ln() + t * (FREQ_MAX.ln() - FREQ_MIN.ln())).exp()
    }
    fn y(&self, db: f32) -> f32 {
        self.plot.top() + (DB_TOP - db) / (DB_TOP - DB_BOTTOM) * self.plot.height()
    }
    fn db(&self, y: f32) -> f32 {
        DB_TOP - (y - self.plot.top()) / self.plot.height() * (DB_TOP - DB_BOTTOM)
    }
    fn spec_y(&self, db: f32) -> f32 {
        let t = ((db - SPEC_BOTTOM) / (SPEC_TOP - SPEC_BOTTOM)).clamp(0.0, 1.0);
        self.plot.bottom() - t * self.plot.height()
    }
    /// Where a band's point sits: at its gain, or on the 0 dB line for the
    /// types without gain (high-pass, low-pass, notch).
    fn node(&self, b: &crate::dsp::BandParams) -> Pos2 {
        let db = if b.kind.has_gain() { b.gain_db } else { 0.0 };
        Pos2::new(self.x(b.freq), self.y(db))
    }
}

pub fn fmt_freq(f: f32) -> String {
    if f >= 10_000.0 {
        format!("{:.1}K", f / 1000.0)
    } else if f >= 1000.0 {
        format!("{:.2}K", f / 1000.0)
    } else if f >= 100.0 {
        format!("{f:.0}")
    } else {
        format!("{f:.1}")
    }
}

#[allow(clippy::too_many_arguments)]
pub fn graph(
    ui: &mut Ui,
    rect: Rect,
    s: &mut EqSettings,
    selected: &mut usize,
    sample_rate: f32,
    analyzer: Option<&Analyzer>,
    analyser_style: Option<spectrum::Style>,
    t: &Tokens,
) -> GraphResult {
    let mut result = GraphResult { changed: None };
    hifi::display(ui.painter(), rect, t);
    let p = ui.painter().with_clip_rect(rect);
    let text = |p: &egui::Painter,
                pos: Pos2,
                align: Align2,
                s: &str,
                f: egui::FontId,
                c: Color32,
                _sp: f32| { hifi::label(p, pos, align, s, f, c) };

    let plot = Rect::from_min_max(
        rect.min + Vec2::new(40.0, 12.0),
        rect.max - Vec2::new(36.0, 24.0),
    );
    let ax = Axes { plot };
    let fs = sample_rate.max(8000.0) as f64;
    let grid = t.ink_faint(0.07);
    let label_col = t.ink_faint(0.6);
    let small = hifi::num(10.5);

    // Grid: decades and their 2s and 5s, dB every 6.
    for &f in &[
        20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0,
        700.0, 800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0, 8000.0,
        9000.0, 10_000.0, 20_000.0,
    ] {
        let x = ax.x(f);
        let major = [100.0, 1000.0, 10_000.0].contains(&f);
        let col = if major { t.ink_faint(0.16) } else { grid };
        p.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0, col),
        );
    }
    for &(f, l) in &[
        (20.0, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (5000.0, "5k"),
        (10_000.0, "10k"),
        (20_000.0, "20k"),
    ] {
        text(
            &p,
            Pos2::new(ax.x(f), plot.bottom() + 5.0),
            Align2::CENTER_TOP,
            l,
            small.clone(),
            label_col,
            0.5,
        );
    }
    let mut db = -24;
    while db <= 12 {
        let y = ax.y(db as f32);
        let col = if db == 0 {
            t.ink_faint(0.32)
        } else {
            t.ink_faint(0.10)
        };
        p.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, col),
        );
        let l = if db > 0 {
            format!("+{db}")
        } else {
            db.to_string()
        };
        text(
            &p,
            Pos2::new(plot.left() - 7.0, y),
            Align2::RIGHT_CENTER,
            &l,
            small.clone(),
            label_col,
            0.3,
        );
        db += 6;
    }

    // Analyser, behind everything else.
    if let Some(an) = analyzer {
        let mut x = plot.left();
        let mut pts = Vec::new();
        let mut levels = Vec::new();
        while x <= plot.right() {
            let f0 = ax.f(x - 1.0);
            let f1 = ax.f(x + 1.0);
            let db = an.level(f0, f1, sample_rate) + tilt(ax.f(x));
            pts.push(Pos2::new(x, ax.spec_y(db)));
            levels.push(((db - SPEC_BOTTOM) / (SPEC_TOP - SPEC_BOTTOM)).clamp(0.0, 1.0));
            x += 2.0;
        }
        // Nothing to draw in silence but a line along the floor.
        let heard = levels.iter().any(|&l| l > 0.0);
        match analyser_style {
            _ if !heard => {}
            Some(style) => spectrum::draw(
                &p,
                &pts,
                &levels,
                plot.left(),
                plot.right(),
                plot.bottom(),
                style,
            ),
            // In the display's own ink, faint behind the curves.
            None => hifi::ink_curve(&p, &pts, plot.bottom(), t.ink, 0.10, 1.0),
        }
        for &(d, l) in &[(-24.0, "-24"), (-48.0, "-48"), (-72.0, "-72")] {
            text(
                &p,
                Pos2::new(plot.right() + 6.0, ax.spec_y(d)),
                Align2::LEFT_CENTER,
                l,
                small.clone(),
                label_col.gamma_multiply(0.7),
                0.2,
            );
        }
    }

    // Curves, sampled across the width.
    let n = (plot.width() / 2.0) as usize;
    let freqs: Vec<f64> = (0..=n)
        .map(|i| ax.f(plot.left() + i as f32 * 2.0) as f64)
        .collect();
    let coeffs: Vec<Coeffs> = s
        .bands
        .iter()
        .map(|b| Coeffs::for_band(&b.clamped(), fs))
        .collect();
    let mut total = vec![s.preamp_db as f64; freqs.len()];
    for (bi, c) in coeffs.iter().enumerate() {
        let b = &s.bands[bi];
        if !b.enabled || (b.kind.has_gain() && b.gain_db.abs() < 0.05) {
            continue;
        }
        let col = BAND_COLOURS[bi];
        let pts: Vec<Pos2> = freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                let m = c.magnitude_db(f, fs);
                total[i] += m;
                // Cut filters fall away to nothing: stop just below the plot.
                Pos2::new(
                    plot.left() + i as f32 * 2.0,
                    ax.y((m as f32).max(DB_BOTTOM - 4.0)),
                )
            })
            .collect();
        let sel = bi == *selected;
        fill_to(
            &p,
            &pts,
            ax.y(0.0),
            col.gamma_multiply(if sel { 0.22 } else { 0.10 }),
        );
        p.add(Shape::line(
            pts,
            Stroke::new(
                if sel { 1.6 } else { 1.0 },
                col.gamma_multiply(if sel { 0.95 } else { 0.6 }),
            ),
        ));
    }
    let total_pts: Vec<Pos2> = total
        .iter()
        .enumerate()
        .map(|(i, &m)| {
            Pos2::new(
                plot.left() + i as f32 * 2.0,
                ax.y((m as f32).clamp(DB_BOTTOM - 4.0, DB_TOP + 4.0)),
            )
        })
        .collect();
    let total_col = if s.bypass {
        t.ink.gamma_multiply(0.35)
    } else {
        t.ink
    };
    if t.glow != Color32::TRANSPARENT {
        p.add(Shape::line(
            total_pts.clone(),
            Stroke::new(6.0, t.glow.gamma_multiply(0.35)),
        ));
    }
    p.add(Shape::line(total_pts, Stroke::new(2.4, total_col)));
    if s.bypass {
        hifi::ink_text(
            &p,
            Pos2::new(plot.right() - 8.0, plot.top() + 6.0),
            Align2::RIGHT_TOP,
            "BYPASSED",
            hifi::num(13.0),
            t,
        );
    }

    // Interaction with the band points.
    let id = Id::new("graph");
    let resp = ui.interact(plot.expand(NODE_R), id, Sense::click_and_drag());
    let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
    let drag_id = id.with("drag");
    if resp.drag_started() {
        // Hit-test where the button went down, not where the drag began.
        if let Some(pos) = ui
            .input(|i| i.pointer.press_origin())
            .or(resp.interact_pointer_pos())
        {
            if let Some(i) = nearest(&ax, s, *selected, pos) {
                *selected = i;
                let grab = ax.node(&s.bands[i]) - pos;
                ui.data_mut(|d| d.insert_temp(drag_id, (i, grab)));
            }
        }
    }
    if resp.dragged() {
        if let (Some((i, grab)), Some(pos)) = (
            ui.data(|d| d.get_temp::<(usize, Vec2)>(drag_id)),
            resp.interact_pointer_pos(),
        ) {
            let fine = ui.input(|i| i.modifiers.shift);
            let target = pos + grab;
            let b = &mut s.bands[i];
            let (nf, ng) = if fine {
                let d = resp.drag_delta() * 0.15;
                (
                    ax.f(ax.x(b.freq) + d.x),
                    b.gain_db - d.y * (DB_TOP - DB_BOTTOM) / plot.height(),
                )
            } else {
                (ax.f(target.x), ax.db(target.y))
            };
            let nf = snap3(nf.clamp(FREQ_MIN, FREQ_MAX));
            let ng = if b.kind.has_gain() {
                ((ng * 10.0).round() / 10.0).clamp(GAIN_MIN_DB, GAIN_MAX_DB)
            } else {
                // Moving a cut filter's point up or down changes nothing.
                b.gain_db
            };
            if nf != b.freq || ng != b.gain_db {
                b.freq = nf;
                b.gain_db = ng;
                b.enabled = true;
                result.changed = Some(i);
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<(usize, Vec2)>(drag_id));
    }
    let hovered_node = pointer.and_then(|pos| nearest(&ax, s, *selected, pos));
    if resp.clicked() {
        if let Some(i) = hovered_node {
            *selected = i;
        }
    }
    if resp.double_clicked() {
        if let Some(i) = hovered_node {
            s.bands[i].gain_db = 0.0;
            result.changed = Some(i);
        }
    }
    if resp.secondary_clicked() {
        if let Some(i) = hovered_node {
            s.bands[i].enabled = !s.bands[i].enabled;
            *selected = i;
            result.changed = Some(i);
        }
    }
    if resp.hovered() {
        let scroll = wheel(ui);
        if scroll != 0.0 {
            let i = hovered_node.unwrap_or(*selected);
            let b = &mut s.bands[i];
            let k: f32 = if ui.input(|i| i.modifiers.shift) {
                1.02
            } else {
                1.1
            };
            let q = if scroll > 0.0 { b.q * k } else { b.q / k };
            b.q = snap3(q.clamp(Q_MIN, Q_MAX));
            *selected = i;
            result.changed = Some(i);
        }
    }
    if hovered_node.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if ui.data(|d| d.get_temp::<(usize, Vec2)>(drag_id)).is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    // Points, selected one last so it sits on top.
    let mut order: Vec<usize> = (0..NUM_BANDS).filter(|&i| i != *selected).collect();
    order.push(*selected);
    for &i in &order {
        let b = &s.bands[i];
        let c = ax.node(b);
        let col = BAND_COLOURS[i];
        let sel = i == *selected;
        if sel {
            p.circle_filled(c, NODE_R + 6.0, col.gamma_multiply(0.15));
            p.circle_stroke(c, NODE_R + 3.0, Stroke::new(1.8, t.ink));
        }
        if b.enabled {
            p.circle_filled(
                c + Vec2::new(0.0, 1.5),
                NODE_R,
                Color32::from_black_alpha(160),
            );
            p.circle_filled(c, NODE_R, col);
            p.circle_stroke(c, NODE_R, Stroke::new(1.0, Color32::from_black_alpha(120)));
            text(
                &p,
                c,
                Align2::CENTER_CENTER,
                &(i + 1).to_string(),
                hifi::bold(11.0),
                Color32::from_rgb(0x14, 0x10, 0x0e),
                0.0,
            );
        } else {
            p.circle_filled(c, NODE_R, t.display_bg);
            p.circle_stroke(c, NODE_R, Stroke::new(1.5, col.gamma_multiply(0.6)));
            text(
                &p,
                c,
                Align2::CENTER_CENTER,
                &(i + 1).to_string(),
                hifi::bold(11.0),
                col,
                0.0,
            );
        }
    }

    // A tag for the point under the mouse (or being dragged).
    let tag_band = ui
        .data(|d| d.get_temp::<(usize, Vec2)>(drag_id))
        .map(|(i, _)| i)
        .or(hovered_node);
    if let Some(i) = tag_band {
        let b = &s.bands[i];
        let gain = if b.kind.has_gain() {
            format!(" · {:+.1} dB", b.gain_db)
        } else {
            String::new()
        };
        let label = format!(
            "{} {} · {} Hz{} · Q {:.2}{}",
            i + 1,
            kind_name(b.kind),
            fmt_freq(b.freq).replace('K', "k"),
            gain,
            b.q,
            if b.enabled { "" } else { " · off" }
        );
        let f = hifi::ui_font(12.5);
        let w = p.layout_no_wrap(label.clone(), f.clone(), t.text).size().x + 18.0;
        let c = ax.node(b);
        let mut r = Rect::from_min_size(Pos2::new(c.x + 14.0, c.y - 32.0), Vec2::new(w, 24.0));
        if r.right() > plot.right() {
            r = r.translate(Vec2::new(-(w + 28.0), 0.0));
        }
        if r.top() < plot.top() {
            r = r.translate(Vec2::new(0.0, 44.0));
        }
        p.rect_filled(
            r.translate(Vec2::new(0.0, 1.5)),
            CornerRadius::same(6),
            Color32::from_black_alpha(60),
        );
        p.rect_filled(r, CornerRadius::same(6), t.raised);
        p.rect_filled(
            Rect::from_min_size(r.min, Vec2::new(4.0, r.height())),
            CornerRadius {
                nw: 6,
                sw: 6,
                ne: 0,
                se: 0,
            },
            BAND_COLOURS[i],
        );
        hifi::label(
            &p,
            r.center() + Vec2::new(2.0, 0.0),
            Align2::CENTER_CENTER,
            &label,
            f,
            t.text,
        );
    }

    result
}

/// The filter type in words, for tags and menus.
pub fn kind_name(k: crate::dsp::FilterKind) -> &'static str {
    use crate::dsp::FilterKind::*;
    match k {
        Peak => "Peak",
        LowShelf => "Low shelf",
        HighShelf => "High shelf",
        HighPass => "High-pass",
        LowPass => "Low-pass",
        Notch => "Notch",
    }
}

/// The band point under `pos`; the selected band wins ties so stacked
/// points stay reachable.
fn nearest(ax: &Axes, s: &EqSettings, selected: usize, pos: Pos2) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for i in 0..NUM_BANDS {
        let d = ax.node(&s.bands[i]).distance(pos) - if i == selected { 2.0 } else { 0.0 };
        if d <= NODE_R + 4.0 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// A gentle tilt (+3 dB/octave around 1 kHz) so music reads roughly level.
fn tilt(f: f32) -> f32 {
    3.0 * (f / 1000.0).log2()
}

fn snap3(v: f32) -> f32 {
    let mag = 10f32.powf(v.log10().floor() - 2.0);
    (v / mag).round() * mag
}

/// Fill between a polyline and a horizontal baseline (any shape).
fn fill_to(p: &egui::Painter, pts: &[Pos2], base_y: f32, col: Color32) {
    let mut mesh = Mesh::default();
    for w in pts.windows(2) {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(w[0], col);
        mesh.colored_vertex(w[1], col);
        mesh.colored_vertex(Pos2::new(w[1].x, base_y), col);
        mesh.colored_vertex(Pos2::new(w[0].x, base_y), col);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    p.add(Shape::mesh(mesh));
}
