//! Advanced EQ curves: limits, total gain, and the text formats used by `eq load` / `eq save`.
//!
//! Two input formats are accepted:
//! - the native one, written by `eq save`: one band per line, `FREQ_HZ GAIN_DB Q`, `#` comments;
//! - AutoEQ's "ParametricEQ.txt": `Filter N: ON PK Fc 105 Hz Gain -2.3 dB Q 0.70` lines (only
//!   peaking filters: the headset's advanced EQ has no shelves). Its `Preamp` line is ignored,
//!   the total gain being computed here as the vendor app does.

use crate::decode::{Eq, EqBand};

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

/// Dart's `pow`, which short-cuts a few exponents (bit-exactness with the vendor app).
fn dart_pow(b: f64, e: f64) -> f64 {
    match e {
        0.0 => 1.0,
        1.0 => b,
        2.0 => b * b,
        3.0 => b * b * b,
        0.5 if b != f64::NEG_INFINITY => {
            if b == 0.0 {
                0.0
            } else {
                b.sqrt()
            }
        }
        _ => b.powf(e),
    }
}

/// Dart's `round`: halves away from zero.
fn dart_round(x: f64) -> f64 {
    x.round()
}

type Complex = (f64, f64);

fn cmul(a: Complex, b: Complex) -> Complex {
    (a.0 * b.0 - a.1 * b.1, a.1 * b.0 + a.0 * b.1)
}

const SAMPLE_RATE: f64 = 44_100.0;
const TWO_PI: f64 = std::f64::consts::TAU;

/// RBJ peaking biquad `(b0, b1, b2, a0, a1, a2)`, not normalised, as the app builds it.
fn coefficients(fc: f64, gain: f64, q: f64) -> [f64; 6] {
    let w0 = fc * TWO_PI / SAMPLE_RATE;
    let a = dart_pow(10.0, gain / 40.0);
    let (s, c) = (w0.sin(), w0.cos());
    let alpha = s / (q * 2.0);
    let (t, u) = (alpha * a, alpha / a);
    [t + 1.0, c * -2.0, 1.0 - t, u + 1.0, c * -2.0, 1.0 - u]
}

fn response(cf: &[f64; 6], w: f64) -> Complex {
    let [b0, b1, b2, a0, a1, a2] = *cf;
    let z = (w.cos(), -w.sin());
    let zz = cmul(z, z);
    let n = (zz.0 * b2 + z.0 * b1 + b0, zz.1 * b2 + z.1 * b1);
    let d = (zz.0 * a2 + z.0 * a1 + a0, zz.1 * a2 + z.1 * a1);
    let den = d.0 * d.0 + d.1 * d.1;
    ((n.0 * d.0 + n.1 * d.1) / den, (n.1 * d.0 - n.0 * d.1) / den)
}

/// Combined response in dB at `freq`, with the values as sent (no rounding): what `eq edit`
/// draws. It assumes the headset runs the same filters as the app models, which is inferred.
pub fn response_db(bands: &[EqBand], freq: f64) -> f64 {
    let w = freq * TWO_PI / SAMPLE_RATE;
    let h = bands
        .iter()
        .map(|b| coefficients(b.freq as f64, b.gain as f64, b.q as f64))
        .fold((1.0, 0.0), |h, cf| cmul(h, response(&cf, w)));
    (h.0 * h.0 + h.1 * h.1).sqrt().max(1e-12).log10() * 20.0
}

/// Highest point of the combined response, floored at 0 dB, computed exactly as the vendor
/// app does (`EQAlgorithm.getCoordinate`): frequencies and gains rounded to whole numbers,
/// RBJ peaking filters at 44.1 kHz cascaded as complex products, evaluated on a 300-point
/// log grid from 20 Hz (0.01 decade steps) whose first N points are replaced by the N band
/// centres.
pub fn response_peak(bands: &[EqBand]) -> f64 {
    if bands.is_empty() {
        return 0.0;
    }
    let params: Vec<(f64, f64, f64)> =
        bands.iter().map(|b| (dart_round(b.freq as f64), dart_round(b.gain as f64), b.q as f64)).collect();
    let n = params.len();
    let ln10 = std::f64::consts::LN_10;
    let step = (20_000f64.ln() / ln10 - 20f64.ln() / ln10) / 300.0;
    let ratio = dart_pow(10.0, step);
    let mut freqs = vec![0.0f64; n + 300];
    for (i, p) in params.iter().enumerate() {
        freqs[i] = p.0;
    }
    for (i, f) in freqs.iter_mut().enumerate().take(300).skip(n) {
        *f = dart_pow(ratio, i as f64) * 20.0;
    }
    freqs.sort_by(f64::total_cmp);
    let cfs: Vec<[f64; 6]> = params.iter().map(|&(f, g, q)| coefficients(f, g, q)).collect();
    let mut peak = 0.0f64;
    for f in freqs {
        let w = f * TWO_PI / SAMPLE_RATE;
        let h = cfs[1..].iter().fold(response(&cfs[0], w), |h, cf| cmul(h, response(cf, w)));
        let mut mag = (h.0 * h.0 + h.1 * h.1).sqrt();
        if mag.is_nan() {
            continue;
        }
        if mag <= 1e-12 {
            mag = 1e-12;
        }
        let db = (mag.ln() / ln10) * 20.0;
        if db > peak {
            peak = db;
        }
    }
    peak
}

/// Firmware from which the vendor app switches to the headroom formula below.
const HEADROOM_FIRMWARE: [u32; 4] = [1, 0, 1, 46];

/// True when `version` ("1.0.1.81") is at least 1.0.1.46. The app compares the third and
/// fourth components numerically; an unreadable version counts as old, as in the app.
fn uses_headroom_formula(version: Option<&str>) -> bool {
    let Some(v) = version else { return false };
    let parts: Vec<u32> =
        v.split('-').next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.len() >= 4 && [parts[2], parts[3]] >= [HEADROOM_FIRMWARE[2], HEADROOM_FIRMWARE[3]]
}

/// Total gain the vendor app sends with a curve. Firmware 1.0.1.46 and later:
/// `min(0, 6 - peak)`, headroom only once the curve rises above +6 dB. Older firmware (or an
/// unknown version): `-(trunc(peak) + 1)`, always at least 1 dB down.
pub fn total_gain(bands: &[EqBand], firmware: Option<&str>) -> f32 {
    let peak = response_peak(bands);
    if uses_headroom_formula(firmware) {
        (6.0 - peak).min(0.0) as f32
    } else {
        -(peak.max(0.0).trunc() + 1.0) as f32
    }
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

/// Builds the curve to send: validated bands and the app's total gain for that firmware.
pub fn curve(mut bands: Vec<EqBand>, firmware: Option<&str>) -> Result<(Eq, Vec<String>), String> {
    let notes = validate(&mut bands)?;
    let total_gain = total_gain(&bands, firmware);
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
        let (eq, _) = curve(factory(), Some("1.0.1.81")).unwrap();
        let expected = parse_hex(
            "00 03 00000000 01 00000000 00004842 0000803f 01 00000000 00007a43 0000803f \
             01 00000000 00007a45 0000803f",
        )
        .unwrap();
        assert_eq!(payload(&eq), expected);
    }

    #[test]
    fn peak_matches_the_vendor_app() {
        // Reference values computed by a straight transcription of the app's algorithm.
        let band = |freq, gain, q| EqBand { filter: 1, freq, gain, q };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(response_peak(&[band(100.0, 6.0, 1.0)]), 5.999999999999972));
        // Q is a float32 on the wire: 0.7 becomes 0.699999988..., hence ...239 and not the
        // ...215 the same transcription gives with Q = 0.7 exactly.
        let three = [band(100.0, 5.0, 0.7), band(150.0, 5.0, 0.7), band(3000.0, -2.5, 2.0)];
        assert!(close(response_peak(&three), 9.21025523982672), "{}", response_peak(&three));
        assert!(close(response_peak(&[band(55.0, 3.0, 1.0)]), 2.999999999999651));
        // The app rounds gains to whole dB for the curve: +2.4 -> 2, +2.5 -> 3, -2.5 -> 0 peak.
        assert!(close(response_peak(&[band(1000.0, 2.4, 1.0)]), 2.0));
        assert!(close(response_peak(&[band(1000.0, 2.5, 1.0)]), 3.0));
        assert!(response_peak(&[band(1000.0, -2.5, 1.0)]) < 1e-6);
        assert_eq!(response_peak(&[]), 0.0);
    }

    #[test]
    fn drawn_response() {
        let band = |freq, gain, q| EqBand { filter: 1, freq, gain, q };
        assert!(response_db(&[band(1000.0, 0.0, 3.0)], 50.0).abs() < 1e-9);
        assert!((response_db(&[band(1000.0, 4.5, 1.0)], 1000.0) - 4.5).abs() < 1e-6);
        assert!((response_db(&[band(1000.0, -2.5, 1.0)], 1000.0) + 2.5).abs() < 1e-6);
        assert!(response_db(&[band(1000.0, 6.0, 1.0)], 20.0) < 0.1);
        assert_eq!(response_db(&[], 1000.0), 0.0);
    }

    #[test]
    fn total_gain_only_when_the_curve_exceeds_six_db() {
        let band = |freq, gain| EqBand { filter: 1, freq, gain, q: 1.0 };
        let fw = Some("1.0.1.81");
        assert_eq!(total_gain(&[band(100.0, 6.0)], fw), 0.0);
        assert!((total_gain(&[band(100.0, 5.0), band(150.0, 5.0)], fw) as f64) < -2.0);
        // Older firmware: always at least 1 dB down, -(trunc(peak) + 1).
        assert_eq!(total_gain(&[band(100.0, 0.0)], Some("1.0.1.45")), -1.0);
        assert_eq!(total_gain(&[band(100.0, 3.0)], Some("1.0.1.45")), -3.0);
        assert_eq!(total_gain(&[band(100.0, 3.0)], None), -3.0);
        assert!(uses_headroom_formula(Some("1.0.1.46")));
        assert!(uses_headroom_formula(Some("2.1.1.100-beta")));
        assert!(!uses_headroom_formula(Some("1.0.0.99")));
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
        assert!(validate(&mut [band(1000.0, 0.0, 1.0); 9]).is_err());
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
        let (eq, _) =
            curve(vec![EqBand { filter: 1, freq: 55.0, gain: 3.0, q: 1.0 }], Some("1.0.1.81")).unwrap();
        assert_eq!(parse(&format(&eq)).unwrap(), eq.bands);
    }
}
