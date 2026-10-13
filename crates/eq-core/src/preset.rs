//! APO-format EQ preset files: the `Preamp:` / `Filter N:` text format that
//! AutoEQ's `ParametricEQ.txt` and other EQ software use.

use crate::dsp::{BandParams, FilterKind, NUM_BANDS, PREAMP_MAX_DB, PREAMP_MIN_DB};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ApoPreset {
    pub preamp_db: f32,
    pub bands: Vec<BandParams>,
    /// Things the file asked for that the Equaliser changed or couldn't do.
    pub warnings: Vec<String>,
}

impl ApoPreset {
    /// Fit the preset to the panel: at most ten bands, with the unused ones
    /// switched off at flat gain.
    pub fn to_panel(&self) -> ([BandParams; NUM_BANDS], f32) {
        let mut out = crate::dsp::default_bands();
        for b in out.iter_mut() {
            b.enabled = false;
            b.gain_db = 0.0;
        }
        for (slot, b) in out.iter_mut().zip(&self.bands) {
            *slot = b.clamped();
        }
        (out, self.preamp_db.clamp(PREAMP_MIN_DB, PREAMP_MAX_DB))
    }
}

pub fn parse(text: &str) -> ApoPreset {
    let mut preset = ApoPreset::default();
    let mut unsupported: Vec<String> = Vec::new();
    let mut skipped = 0usize;

    for (line_no, raw) in text.lines().enumerate() {
        let line = raw
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let Some((cmd, rest)) = line.split_once(':') else {
            continue;
        };
        let cmd = cmd.trim().to_ascii_lowercase();
        if cmd == "preamp" {
            match parse_db(rest) {
                Some(db) => preset.preamp_db += db,
                None => preset
                    .warnings
                    .push(format!("Line {}: couldn't read the preamp", line_no + 1)),
            }
        } else if cmd == "filter"
            || (cmd.starts_with("filter") && cmd[6..].trim().parse::<u32>().is_ok())
        {
            match parse_filter(rest) {
                FilterLine::Band(b) => {
                    if preset.bands.len() < NUM_BANDS {
                        preset.bands.push(b);
                    } else {
                        skipped += 1;
                    }
                }
                FilterLine::Unsupported(t) => {
                    if !unsupported.contains(&t) {
                        unsupported.push(t);
                    }
                }
                FilterLine::Invalid => preset
                    .warnings
                    .push(format!("Line {}: couldn't read this filter", line_no + 1)),
            }
        }
        // Device:, Channel:, Include:, Convolution: and the rest don't apply here.
    }

    if !unsupported.is_empty() {
        preset.warnings.push(format!(
            "Skipped filter types the Equaliser doesn't have: {}",
            unsupported.join(", ")
        ));
    }
    if skipped > 0 {
        preset.warnings.push(format!(
            "Only the first {NUM_BANDS} filters were loaded ({skipped} more ignored)"
        ));
    }
    let clamped = preset.bands.iter().filter(|b| b.clamped() != **b).count();
    if clamped > 0 {
        preset.warnings.push(format!(
            "{clamped} filter(s) were outside the panel's range (gain +12/−24 dB, Q 0.1–20, 20 Hz–20 kHz) and were limited"
        ));
    }
    if !(PREAMP_MIN_DB..=PREAMP_MAX_DB).contains(&preset.preamp_db) {
        preset
            .warnings
            .push("The preamp was outside +12/−24 dB and was limited".into());
    }
    preset
}

enum FilterLine {
    Band(BandParams),
    Unsupported(String),
    Invalid,
}

fn parse_filter(rest: &str) -> FilterLine {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    let mut enabled = true;
    if let Some(t) = toks.first() {
        match t.to_ascii_uppercase().as_str() {
            "ON" => i = 1,
            "OFF" => {
                enabled = false;
                i = 1
            }
            _ => {}
        }
    }
    let Some(type_tok) = toks.get(i) else {
        return FilterLine::Invalid;
    };
    let type_up = type_tok.to_ascii_uppercase();
    i += 1;
    let kind = match type_up.as_str() {
        "PK" | "PEQ" | "MODAL" => FilterKind::Peak,
        "LS" | "LSC" | "LSQ" => FilterKind::LowShelf,
        "HS" | "HSC" | "HSQ" => FilterKind::HighShelf,
        "HP" | "HPQ" => FilterKind::HighPass,
        "LP" | "LPQ" => FilterKind::LowPass,
        "NO" => FilterKind::Notch,
        _ => return FilterLine::Unsupported(type_up),
    };
    // "LS 6dB", "LSC 12 dB" etc. give a slope instead of a Q.
    let mut slope_db: Option<f32> = None;
    if let Some(t) = toks.get(i) {
        let lower = t.to_ascii_lowercase();
        if let Some(num) = lower.strip_suffix("db") {
            if let Ok(v) = num.parse::<f32>() {
                slope_db = Some(v);
                i += 1;
            }
        } else if let Ok(v) = lower.parse::<f32>() {
            if toks
                .get(i + 1)
                .is_some_and(|u| u.eq_ignore_ascii_case("db"))
            {
                slope_db = Some(v);
                i += 2;
            }
        }
    }

    let mut freq: Option<f32> = None;
    let mut gain: f32 = 0.0;
    let mut q: Option<f32> = None;
    while i < toks.len() {
        let key = toks[i].to_ascii_lowercase();
        match key.as_str() {
            "fc" => {
                let Some(v) = toks.get(i + 1).and_then(|v| parse_num(v)) else {
                    return FilterLine::Invalid;
                };
                let unit = toks.get(i + 2).map(|u| u.to_ascii_lowercase());
                let (mult, used) = match unit.as_deref() {
                    Some("khz") => (1000.0, 3),
                    Some("hz") => (1.0, 3),
                    _ => (1.0, 2),
                };
                freq = Some(v * mult);
                i += used;
            }
            "gain" => {
                let Some(v) = toks.get(i + 1).and_then(|v| parse_num(v)) else {
                    return FilterLine::Invalid;
                };
                gain = v;
                i += if toks
                    .get(i + 2)
                    .is_some_and(|u| u.eq_ignore_ascii_case("db"))
                {
                    3
                } else {
                    2
                };
            }
            "q" => {
                let Some(v) = toks.get(i + 1).and_then(|v| parse_num(v)) else {
                    return FilterLine::Invalid;
                };
                q = Some(v);
                i += 2;
            }
            "bw" => {
                // "BW Oct 1.0"
                let (val_idx, used) = if toks
                    .get(i + 1)
                    .is_some_and(|t| t.eq_ignore_ascii_case("oct"))
                {
                    (i + 2, 3)
                } else {
                    (i + 1, 2)
                };
                let Some(bw) = toks.get(val_idx).and_then(|v| parse_num(v)) else {
                    return FilterLine::Invalid;
                };
                q = Some(bw_to_q(bw));
                i += used;
            }
            _ => i += 1,
        }
    }
    let Some(freq) = freq else {
        return FilterLine::Invalid;
    };
    let q = q.unwrap_or(match (kind, slope_db) {
        // The format's fixed-slope shelves (6/12 dB per octave) are close
        // to these Qs at moderate gain.
        (FilterKind::Peak, _) => 1.0,
        // A narrow notch; the format's own default is narrower than the
        // panel allows.
        (FilterKind::Notch, _) => crate::dsp::Q_MAX,
        (FilterKind::HighPass | FilterKind::LowPass, _) => 0.707,
        (_, Some(s)) if s <= 6.0 => 0.5,
        _ => 0.707,
    });
    FilterLine::Band(BandParams {
        enabled,
        kind,
        freq,
        gain_db: gain,
        q,
    })
}

fn parse_num(s: &str) -> Option<f32> {
    s.trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .replace(',', ".")
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}

fn parse_db(s: &str) -> Option<f32> {
    s.split_whitespace().next().and_then(parse_num)
}

/// Bandwidth in octaves to Q (ignoring the small bilinear warping term).
pub fn bw_to_q(bw: f32) -> f32 {
    let x = (std::f32::consts::LN_2 / 2.0 * bw).sinh();
    if x > 0.0 {
        1.0 / (2.0 * x)
    } else {
        1.0
    }
}

/// Write the panel as an APO-format preset.
pub fn export(bands: &[BandParams], preamp_db: f32) -> String {
    let mut s = String::from("# TAW Equaliser preset (APO format)\n");
    s += &format!("Preamp: {preamp_db:.1} dB\n");
    for (i, b) in bands.iter().enumerate() {
        let t = match b.kind {
            FilterKind::Peak => "PK",
            FilterKind::LowShelf => "LSC",
            FilterKind::HighShelf => "HSC",
            FilterKind::HighPass => "HPQ",
            FilterKind::LowPass => "LPQ",
            FilterKind::Notch => "NO",
        };
        let gain = if b.kind.has_gain() {
            format!(" Gain {:.1} dB", b.gain_db)
        } else {
            String::new()
        };
        s += &format!(
            "Filter {}: {} {} Fc {} Hz{} Q {:.3}\n",
            i + 1,
            if b.enabled { "ON" } else { "OFF" },
            t,
            fmt_freq(b.freq),
            gain,
            b.q
        );
    }
    s
}

fn fmt_freq(f: f32) -> String {
    if f >= 100.0 {
        format!("{f:.0}")
    } else {
        format!("{f:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTOEQ: &str = "Preamp: -6.4 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
Filter 2: ON PK Fc 3460 Hz Gain 4.2 dB Q 2.53
Filter 3: ON PK Fc 1.2 kHz Gain -2.0 dB Q 1.00
Filter 4: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70
Filter 5: OFF PK Fc 500 Hz Gain 3 dB Q 1
Filter 6: ON LP Fc 18000 Hz
Filter 7: ON BP Fc 1000 Hz
";

    #[test]
    fn reads_autoeq_style_presets() {
        let p = parse(AUTOEQ);
        assert_eq!(p.preamp_db, -6.4);
        assert_eq!(p.bands.len(), 6);
        assert!(!p.bands[4].enabled);
        assert_eq!(p.bands[0].kind, FilterKind::LowShelf);
        assert_eq!(
            (p.bands[0].freq, p.bands[0].gain_db, p.bands[0].q),
            (105.0, 5.5, 0.70)
        );
        assert_eq!(p.bands[1].kind, FilterKind::Peak);
        assert_eq!(p.bands[2].freq, 1200.0);
        assert_eq!(p.bands[3].kind, FilterKind::HighShelf);
        assert_eq!(p.bands[5].kind, FilterKind::LowPass);
        assert_eq!((p.bands[5].freq, p.bands[5].q), (18000.0, 0.707));
        assert!(p.warnings.iter().any(|w| w.contains("BP")));
        assert!(!p.warnings.iter().any(|w| w.contains("LP")));
    }

    #[test]
    fn reads_cut_filters() {
        let p = parse(
            "Filter 1: ON HP Fc 30 Hz\n\
             Filter 2: ON HPQ Fc 40 Hz Q 0.5\n\
             Filter 3: ON LPQ Fc 16 kHz Q 1.2\n\
             Filter 4: ON NO Fc 60 Hz\n\
             Filter 5: OFF NO Fc 50 Hz Q 8\n",
        );
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        let k: Vec<_> = p.bands.iter().map(|b| (b.kind, b.freq, b.q)).collect();
        assert_eq!(
            k,
            vec![
                (FilterKind::HighPass, 30.0, 0.707),
                (FilterKind::HighPass, 40.0, 0.5),
                (FilterKind::LowPass, 16000.0, 1.2),
                (FilterKind::Notch, 60.0, crate::dsp::Q_MAX),
                (FilterKind::Notch, 50.0, 8.0),
            ]
        );
        assert!(!p.bands[4].enabled);
    }

    #[test]
    fn handles_slopes_bandwidth_and_comments() {
        let p = parse(
            "# comment\nDevice: Speakers\nPreamp: -3 dB\nPreamp: -1 dB\n\
             Filter: ON LS 6dB Fc 80 Hz Gain 4 dB\n\
             Filter: ON PK Fc 2000 Hz Gain -3 dB BW Oct 1.0\n\
             Filter: ON HSC 12 dB Fc 8000 Hz Gain 2 dB # tail\n",
        );
        assert_eq!(p.preamp_db, -4.0);
        assert_eq!(p.bands.len(), 3);
        assert_eq!(p.bands[0].q, 0.5);
        assert!((p.bands[1].q - 1.414).abs() < 0.01);
        assert_eq!(p.bands[2].q, 0.707);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    #[test]
    fn keeps_ten_bands_and_limits_ranges() {
        let mut text = String::new();
        for i in 0..14 {
            text += &format!(
                "Filter {}: ON PK Fc {} Hz Gain 20 dB Q 1\n",
                i + 1,
                100 * (i + 1)
            );
        }
        let p = parse(&text);
        assert_eq!(p.bands.len(), 10);
        let (bands, _) = p.to_panel();
        assert!(bands.iter().all(|b| b.gain_db == 12.0));
        assert_eq!(p.warnings.len(), 2);
    }

    #[test]
    fn export_round_trips() {
        let mut bands = crate::dsp::default_bands();
        bands[2].gain_db = -7.5;
        bands[2].q = 2.25;
        bands[6].enabled = false;
        bands[0].kind = FilterKind::HighPass;
        bands[0].q = 0.5;
        bands[7].kind = FilterKind::Notch;
        bands[7].q = 6.0;
        bands[9].kind = FilterKind::LowPass;
        let text = export(&bands, -3.5);
        assert!(text.contains("HPQ Fc 31.0 Hz Q 0.500"), "{text}");
        let p = parse(&text);
        assert_eq!(p.preamp_db, -3.5);
        let (back, preamp) = p.to_panel();
        assert_eq!(preamp, -3.5);
        assert_eq!(back, bands);
    }
}
