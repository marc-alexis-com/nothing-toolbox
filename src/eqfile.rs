//! Advanced EQ curves: limits, total gain, and the text formats used by `eq load` / `eq save`.
//!
//! Two input formats are accepted:
//! - the native one, written by `eq save`: one band per line, `FREQ_HZ GAIN_DB Q`, `#` comments;
//! - AutoEQ's "ParametricEQ.txt": `Filter N: ON PK Fc 105 Hz Gain -2.3 dB Q 0.70` lines (only
//!   peaking filters: the headset's advanced EQ has no shelves). Its `Preamp` line is ignored,
//!   the total gain being computed here as the vendor app does.

use crate::decode::{Eq, EqBand};
use std::f64::consts::PI;

/// Limits enforced by the vendor app's advanced EQ page (B170).
pub const MAX_BANDS: usize = 8;
pub const GAIN_RANGE: (f32, f32) = (-6.0, 6.0);
pub const GAIN_STEP: f32 = 0.5;
pub const FREQ_RANGE: (f32, f32) = (20.0, 20_000.0);
pub const Q_RANGE: (f32, f32) = (0.1, 10.0);
/// The only filter type the app writes: a peaking (bell) filter.
pub const FILTER_PEAK: u8 = 1;

/// The app's factory curve: three flat bands.
pub fn factory() -> Vec<EqBand> {
    [50.0, 250.0, 4000.0]
        .iter()
        .map(|&f| EqBand { filter: FILTER_PEAK, gain: 0.0, freq: f, q: 1.0 })
        .collect()
}

/// Checks the limits and snaps gains to the app's 0.5 dB step. Returns notes for the user.
pub fn validate(bands: &mut [EqBand]) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    if bands.is_empty() {
        return Err("a curve needs at least one band".into());
    }
    if bands.len() > MAX_BANDS {
        return Err(format!("{} bands, the headset takes at most {MAX_BANDS}", bands.len()));
    }
    for b in bands.iter_mut() {
        if !(FREQ_RANGE.0..=FREQ_RANGE.1).contains(&b.freq) {
            return Err(format!("{} Hz is outside {}-{} Hz", b.freq, FREQ_RANGE.0, FREQ_RANGE.1));
        }
        if !(GAIN_RANGE.0..=GAIN_RANGE.1).contains(&b.gain) {
            return Err(format!(
                "{:+} dB at {} Hz is outside {:+}..{:+} dB",
                b.gain, b.freq, GAIN_RANGE.0, GAIN_RANGE.1
            ));
        }
        if !(Q_RANGE.0..=Q_RANGE.1).contains(&b.q) {
            return Err(format!("Q {} at {} Hz is outside {}-{}", b.q, b.freq, Q_RANGE.0, Q_RANGE.1));
        }
        let snapped = (b.gain / GAIN_STEP).round() * GAIN_STEP;
        if snapped != b.gain {
            notes.push(format!("{} Hz: {:+} dB rounded to {:+} dB", b.freq, b.gain, snapped));
            b.gain = snapped;
        }
        b.filter = FILTER_PEAK;
    }
    bands.sort_by(|a, b| a.freq.total_cmp(&b.freq));
    Ok(notes)
}

/// Gain in dB of an RBJ peaking filter at frequency `f`, for a 48 kHz sample rate.
fn peak_db(band: &EqBand, f: f64) -> f64 {
    let fs = 48_000.0;
    let a = 10f64.powf(band.gain as f64 / 40.0);
    let w0 = 2.0 * PI * band.freq as f64 / fs;
    let alpha = w0.sin() / (2.0 * band.q as f64);
    let (b0, b1, b2) = (1.0 + alpha * a, -2.0 * w0.cos(), 1.0 - alpha * a);
    let (a0, a1, a2) = (1.0 + alpha / a, -2.0 * w0.cos(), 1.0 - alpha / a);
    let w = 2.0 * PI * f / fs;
    // |H(e^jw)|^2 for a biquad, from the real and imaginary parts.
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let num = (b0 + b1 * c1 + b2 * c2).powi(2) + (b1 * s1 + b2 * s2).powi(2);
    let den = (a0 + a1 * c1 + a2 * c2).powi(2) + (a1 * s1 + a2 * s2).powi(2);
    10.0 * (num / den).log10()
}

/// Highest point of the combined response, floored at 0 dB (sampled 20 Hz - 20 kHz).
/// The vendor app draws the curve with its own algorithm; this RBJ biquad model is an
/// approximation of it, close enough to decide when headroom is needed.
pub fn response_peak(bands: &[EqBand]) -> f64 {
    (0..=400)
        .map(|i| 20.0 * 1000f64.powf(i as f64 / 400.0))
        .map(|f| bands.iter().map(|b| peak_db(b, f)).sum::<f64>())
        .fold(0.0, f64::max)
}

/// Total gain the vendor app sends with a curve, for firmware 1.0.1.46 and later:
/// `min(0, 6 - peak)`, i.e. headroom only once the curve rises above +6 dB.
pub fn total_gain(bands: &[EqBand]) -> f32 {
    (6.0 - response_peak(bands)).min(0.0) as f32
}

/// 0xF050 payload: `[profile 0, n, total f32, (type, gain f32, freq f32, q f32) × n]`.
pub fn payload(eq: &Eq) -> Vec<u8> {
    let mut out = vec![eq.profile.unwrap_or(0), eq.bands.len() as u8];
    out.extend_from_slice(&eq.total_gain.to_le_bytes());
    for b in &eq.bands {
        out.push(b.filter);
        out.extend_from_slice(&b.gain.to_le_bytes());
        out.extend_from_slice(&b.freq.to_le_bytes());
        out.extend_from_slice(&b.q.to_le_bytes());
    }
    out
}

/// Builds the curve to send: validated bands and the app's total gain.
pub fn curve(mut bands: Vec<EqBand>) -> Result<(Eq, Vec<String>), String> {
    let notes = validate(&mut bands)?;
    let total_gain = total_gain(&bands);
    Ok((Eq { profile: Some(0), total_gain, bands }, notes))
}

pub fn format(eq: &Eq) -> String {
    let mut s =
        String::from("# nothing-toolbox advanced EQ (Nothing Headphone (1))\n# freq_hz  gain_db  q\n");
    for b in &eq.bands {
        s.push_str(&format!("{:<9} {:+.1}  {:.2}\n", b.freq, b.gain, b.q));
    }
    s
}

fn num(s: &str, what: &str, line: usize) -> Result<f32, String> {
    s.parse::<f32>().map_err(|_| format!("line {line}: {what} {s:?} is not a number"))
}

pub fn parse(text: &str) -> Result<Vec<EqBand>, String> {
    let mut bands = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with("Preamp") {
            continue;
        }
        if line.starts_with("Filter") {
            let w: Vec<&str> = line.split_whitespace().collect();
            let field = |key: &str| w.iter().position(|x| *x == key).and_then(|p| w.get(p + 1)).copied();
            if w.contains(&"OFF") {
                continue;
            }
            let kind = w.get(3).copied().unwrap_or("");
            if kind != "PK" {
                return Err(format!(
                    "line {n}: {kind} filters are not supported, the headset only has peaking bands (PK)"
                ));
            }
            let get = |key: &str| field(key).ok_or(format!("line {n}: no {key} value"));
            bands.push(EqBand {
                filter: FILTER_PEAK,
                freq: num(get("Fc")?, "frequency", n)?,
                gain: num(get("Gain")?, "gain", n)?,
                q: num(get("Q")?, "Q", n)?,
            });
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.len() != 3 {
            return Err(format!("line {n}: expected `FREQ_HZ GAIN_DB Q`, got {line:?}"));
        }
        bands.push(EqBand {
            filter: FILTER_PEAK,
            freq: num(w[0], "frequency", n)?,
            gain: num(w[1], "gain", n)?,
            q: num(w[2], "Q", n)?,
        });
    }
    Ok(bands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::advanced_eq;
    use crate::frame::parse_hex;

    #[test]
    fn payload_round_trips_a_real_curve() {
        // The flat 8-band curve read from a B170.
        let p = parse_hex(
            "00 08 00000000 \
            01 00000000 00005c42 0000803f 01 00000000 0000dc42 0000803f \
            01 00000000 00005c43 0000803f 01 00000000 0000dc43 0000803f \
            01 00000000 0000a544 0000803f 01 00000000 00404e45 0000803f \
            01 00000000 0040ce45 0000803f 01 00000000 00404e46 0000803f",
        )
        .unwrap();
        let eq = advanced_eq(&p).unwrap();
        assert_eq!(payload(&eq), p);
        assert_eq!(p.len(), 2 + 4 + 13 * 8);
    }

    #[test]
    fn factory_curve_matches_the_app() {
        // 45 bytes, as built from the vendor app's code for firmware >= 1.0.1.46.
        let (eq, _) = curve(factory()).unwrap();
        let expected = parse_hex(
            "00 03 00000000 01 00000000 00004842 0000803f 01 00000000 00007a43 0000803f \
             01 00000000 00007a45 0000803f",
        )
        .unwrap();
        assert_eq!(payload(&eq), expected);
    }

    #[test]
    fn total_gain_only_when_the_curve_exceeds_six_db() {
        let band = |freq, gain| EqBand { filter: 1, freq, gain, q: 1.0 };
        assert_eq!(total_gain(&[band(100.0, 6.0)]), 0.0);
        let g = total_gain(&[band(100.0, 6.0), band(150.0, 6.0)]);
        assert!(g < -3.0 && g > -6.5, "{g}");
        assert!((response_peak(&[band(1000.0, 4.0)]) - 4.0).abs() < 0.05);
    }

    #[test]
    fn limits_and_snapping() {
        let band = |freq, gain, q| EqBand { filter: 0, freq, gain, q };
        let mut ok = vec![band(1000.0, 2.3, 1.0), band(100.0, -1.0, 0.7)];
        let notes = validate(&mut ok).unwrap();
        assert_eq!(ok[0].freq, 100.0, "sorted by frequency");
        assert_eq!(ok[1].gain, 2.5);
        assert_eq!(ok[1].filter, FILTER_PEAK);
        assert_eq!(notes.len(), 1);
        assert!(validate(&mut [band(1000.0, 7.0, 1.0)]).is_err());
        assert!(validate(&mut [band(10.0, 0.0, 1.0)]).is_err());
        assert!(validate(&mut [band(1000.0, 0.0, 20.0)]).is_err());
        assert!(validate(&mut vec![band(1000.0, 0.0, 1.0); 9]).is_err());
        assert!(validate(&mut []).is_err());
    }

    #[test]
    fn parses_native_and_autoeq_files() {
        let native = "# comment\n55 +3.0 1.00\n\n13200  -1.5 0.7  # air\n";
        let b = parse(native).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!((b[1].freq, b[1].gain, b[1].q), (13200.0, -1.5, 0.7));

        let autoeq = "Preamp: -6.2 dB\n\
                      Filter 1: ON PK Fc 105 Hz Gain -2.3 dB Q 0.70\n\
                      Filter 2: ON PK Fc 2900 Hz Gain 4.0 dB Q 2.10\n\
                      Filter 3: OFF PK Fc 8000 Hz Gain 1.0 dB Q 1.00\n";
        let b = parse(autoeq).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].freq, b[0].gain, b[0].q), (105.0, -2.3, 0.7));

        assert!(parse("Filter 1: ON LSC Fc 105 Hz Gain 2.0 dB Q 0.70").unwrap_err().contains("LSC"));
        assert!(parse("100 2").is_err());
    }

    #[test]
    fn saved_files_load_back() {
        let (eq, _) = curve(vec![EqBand { filter: 1, freq: 55.0, gain: 3.0, q: 1.0 }]).unwrap();
        assert_eq!(parse(&format(&eq)).unwrap(), eq.bands);
    }
}
