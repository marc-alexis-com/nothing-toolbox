//! `eq edit`: an interactive editor for the advanced EQ, drawn in the terminal.
//!
//! This module holds the editor state, the key handling and the drawing; it does no I/O, so
//! that it can be tested without a terminal or a headset. `main.rs` owns the loop.

use crate::decode::EqBand;
use crate::eqfile::{self, FILTER_PEAK, FREQ_RANGE, GAIN_RANGE, GAIN_STEP, MAX_BANDS, Q_RANGE};

const RESET: &str = "\x1b[0m";
const GRID: &str = "\x1b[90m";
const CURVE: &str = "\x1b[97m";
const SHAPE: &str = "\x1b[31m";
const MARK: &str = "\x1b[1;37m";
const MARK_SELECTED: &str = "\x1b[1;97;41m";
const TITLE: &str = "\x1b[1;97m";

/// Width of the side panel, and of the dB scale on the left of the graph.
const PANEL: usize = 34;
const GUTTER: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    ShiftLeft,
    ShiftRight,
    Tab,
    BackTab,
    Delete,
    Char(char),
}

/// Decodes the bytes read from a terminal in cbreak mode. Unknown sequences are dropped.
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1b if matches!(bytes.get(i + 1), Some(b'[') | Some(b'O')) => {
                // CSI/SS3: parameters (digits and ';'), then one final byte.
                let mut j = i + 2;
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                    j += 1;
                }
                let params = std::str::from_utf8(&bytes[i + 2..j]).unwrap_or("");
                let shift = params.ends_with(";2");
                match (bytes.get(j), shift) {
                    (Some(b'A'), _) => keys.push(Key::Up),
                    (Some(b'B'), _) => keys.push(Key::Down),
                    (Some(b'C'), false) => keys.push(Key::Right),
                    (Some(b'D'), false) => keys.push(Key::Left),
                    (Some(b'C'), true) => keys.push(Key::ShiftRight),
                    (Some(b'D'), true) => keys.push(Key::ShiftLeft),
                    (Some(b'Z'), _) => keys.push(Key::BackTab),
                    (Some(b'~'), _) if params == "3" => keys.push(Key::Delete),
                    _ => {}
                }
                i = j + 1;
            }
            b'\t' => {
                keys.push(Key::Tab);
                i += 1;
            }
            0x7f => {
                keys.push(Key::Delete);
                i += 1;
            }
            b if b.is_ascii_graphic() || b == b' ' => {
                keys.push(Key::Char(b as char));
                i += 1;
            }
            _ => i += 1,
        }
    }
    keys
}

/// What the loop has to do after a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// The curve changed: send it (after a short pause, so that held keys send once).
    Changed,
    /// The curve was put back as it was at start: send it, and restore the mode too.
    Reverted,
    Save,
    Quit,
}

pub struct Editor {
    pub bands: Vec<EqBand>,
    pub selected: usize,
    original: Vec<EqBand>,
}

impl Editor {
    /// Starts from the headset's curve, brought within the editor's limits.
    pub fn new(bands: Vec<EqBand>) -> Editor {
        let mut bands: Vec<EqBand> = bands.into_iter().take(MAX_BANDS).map(sanitize).collect();
        if bands.is_empty() {
            bands = eqfile::factory();
        }
        bands.sort_by(|a, b| a.freq.total_cmp(&b.freq));
        Editor { original: bands.clone(), bands, selected: 0 }
    }

    pub fn is_original(&self) -> bool {
        self.bands == self.original
    }

    pub fn handle(&mut self, key: Key) -> Outcome {
        let n = self.bands.len();
        let changed = match key {
            Key::Up | Key::Char('k') => self.gain(GAIN_STEP),
            Key::Down | Key::Char('j') => self.gain(-GAIN_STEP),
            Key::Right | Key::Char('l') => self.freq(1.0 / 12.0),
            Key::Left | Key::Char('h') => self.freq(-1.0 / 12.0),
            Key::ShiftRight | Key::Char('L') => self.freq(1.0 / 3.0),
            Key::ShiftLeft | Key::Char('H') => self.freq(-1.0 / 3.0),
            Key::Char(']') => self.q(1),
            Key::Char('[') => self.q(-1),
            Key::Char('0') => {
                let b = &mut self.bands[self.selected];
                std::mem::replace(&mut b.gain, 0.0) != 0.0
            }
            Key::Char('a') => self.add(),
            Key::Char('x') | Key::Delete => self.remove(),
            Key::Tab | Key::Char('n') => {
                self.selected = (self.selected + 1) % n;
                false
            }
            Key::BackTab | Key::Char('p') => {
                self.selected = (self.selected + n - 1) % n;
                false
            }
            Key::Char(c @ '1'..='8') => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.selected = i;
                }
                false
            }
            Key::Char('u') => {
                if self.is_original() {
                    return Outcome::Nothing;
                }
                self.bands = self.original.clone();
                self.selected = self.selected.min(self.bands.len() - 1);
                return Outcome::Reverted;
            }
            Key::Char('r') => {
                // The vendor app's factory curve, as `eq reset` writes it.
                let factory = eqfile::factory();
                if self.bands == factory {
                    return Outcome::Nothing;
                }
                self.bands = factory;
                self.selected = 0;
                true
            }
            Key::Char('s') => return Outcome::Save,
            Key::Char('q') => return Outcome::Quit,
            _ => false,
        };
        if changed {
            Outcome::Changed
        } else {
            Outcome::Nothing
        }
    }

    fn gain(&mut self, step: f32) -> bool {
        let b = &mut self.bands[self.selected];
        let g = (b.gain + step).clamp(GAIN_RANGE.0, GAIN_RANGE.1);
        std::mem::replace(&mut b.gain, g) != g
    }

    /// Moves the selected band by `octaves`, to a whole number of hertz, and keeps the bands
    /// sorted (the headset gets them sorted) with the selection following its band.
    fn freq(&mut self, octaves: f32) -> bool {
        let b = self.bands[self.selected];
        let mut f = (b.freq * 2f32.powf(octaves)).round();
        if f == b.freq {
            f += octaves.signum();
        }
        let f = f.clamp(FREQ_RANGE.0, FREQ_RANGE.1);
        if f == b.freq {
            return false;
        }
        self.bands[self.selected].freq = f;
        self.resort(self.selected);
        true
    }

    /// Q on a grid of quarter-octave steps (1, 1.19, 1.41, 1.68, 2, ...), two decimals. A Q
    /// off the grid (from a file) snaps to the nearest step in the direction asked.
    fn q(&mut self, steps: i32) -> bool {
        let b = &mut self.bands[self.selected];
        let k = (b.q.log2() * 4.0).round() as i32 + steps;
        let q = ((2f32.powf(k as f32 / 4.0)) * 100.0).round() / 100.0;
        let q = q.clamp(Q_RANGE.0, Q_RANGE.1);
        std::mem::replace(&mut b.q, q) != q
    }

    /// Adds a flat band in the middle of the widest free stretch of the spectrum.
    fn add(&mut self) -> bool {
        if self.bands.len() >= MAX_BANDS {
            return false;
        }
        let mut edges = vec![FREQ_RANGE.0];
        edges.extend(self.bands.iter().map(|b| b.freq));
        edges.push(FREQ_RANGE.1);
        let (lo, hi) = edges
            .windows(2)
            .map(|w| (w[0], w[1]))
            .max_by(|a, b| (a.1 / a.0).total_cmp(&(b.1 / b.0)))
            .unwrap();
        let freq = (lo * hi).sqrt().round();
        self.bands.push(EqBand { filter: FILTER_PEAK, gain: 0.0, freq, q: 1.0 });
        self.resort(self.bands.len() - 1);
        true
    }

    fn remove(&mut self) -> bool {
        if self.bands.len() <= 1 {
            return false;
        }
        self.bands.remove(self.selected);
        self.selected = self.selected.min(self.bands.len() - 1);
        true
    }

    fn resort(&mut self, moved: usize) {
        let band = self.bands[moved];
        // Stable sort: among equal frequencies the moved band keeps its relative place.
        let mut tagged: Vec<(usize, EqBand)> = self.bands.iter().copied().enumerate().collect();
        tagged.sort_by(|a, b| a.1.freq.total_cmp(&b.1.freq));
        self.selected = tagged.iter().position(|(i, _)| *i == moved).unwrap_or(0);
        self.bands = tagged.into_iter().map(|(_, b)| b).collect();
        debug_assert_eq!(self.bands[self.selected], band);
    }
}

fn sanitize(b: EqBand) -> EqBand {
    EqBand {
        filter: FILTER_PEAK,
        gain: ((b.gain / GAIN_STEP).round() * GAIN_STEP).clamp(GAIN_RANGE.0, GAIN_RANGE.1),
        freq: b.freq.round().clamp(FREQ_RANGE.0, FREQ_RANGE.1),
        q: b.q.clamp(Q_RANGE.0, Q_RANGE.1),
    }
}

// --- Drawing ------------------------------------------------------------------------------

/// Braille dot bits, indexed by [column][row] inside a 2x4 cell.
const DOTS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

fn braille(bits: u8) -> char {
    char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
}

/// Horizontal position, 0..=1, of a frequency on the 20 Hz - 20 kHz log axis.
fn x_of(freq: f64) -> f64 {
    (freq / 20.0).log10() / 3.0
}

fn freq_of(x: f64) -> f64 {
    20.0 * 1000f64.powf(x)
}

fn freq_label(f: f32) -> String {
    format!("{:>5} Hz", f.round() as u32)
}

/// One full frame: the graph on the left, the band list and `info` lines on the right, and
/// `status` on the last line.
pub fn render(
    ed: &Editor,
    total_gain: f32,
    cols: usize,
    rows: usize,
    info: &[String],
    status: &str,
) -> String {
    // Scale, plot, two blank cells, panel: exactly `cols` wide, or the terminal would wrap
    // a line and scroll the whole frame.
    let w = cols.saturating_sub(GUTTER + 2 + PANEL).max(10);
    let h = rows.saturating_sub(2).max(4);
    let (pw, ph) = (2 * w, 4 * h);

    let curve: Vec<f64> =
        (0..pw).map(|px| eqfile::response_db(&ed.bands, freq_of(px as f64 / (pw - 1) as f64))).collect();
    let shape: Vec<f64> = (0..pw)
        .map(|px| {
            eqfile::response_db(&ed.bands[ed.selected..=ed.selected], freq_of(px as f64 / (pw - 1) as f64))
        })
        .collect();

    // Symmetric range in steps of 6 dB, at least +-12, wide enough for the whole curve.
    let extent = curve.iter().fold(0f64, |m, d| m.max(d.abs())).max(GAIN_RANGE.1 as f64);
    let range = ((extent / 6.0).ceil() * 6.0).max(12.0);
    // 0 dB on a whole dot row: halfway between two rows, a curve within a hair of 0 dB
    // would hop from one text row to the other.
    let zero = ((ph - 1) / 2) as f64;
    let y_of = |db: f64| -> usize { (zero - db * zero / range).round() as usize };

    let mut curve_bits = vec![vec![0u8; w]; h];
    let mut shape_bits = vec![vec![0u8; w]; h];
    for (samples, bits, is_shape) in [(&curve, &mut curve_bits, false), (&shape, &mut shape_bits, true)] {
        let mut prev: Option<usize> = None;
        for (px, &db) in samples.iter().enumerate() {
            let py = y_of(db.clamp(-range, range));
            // The selected band's own shape only where it leaves 0 dB: elsewhere it would
            // underline the whole graph.
            if is_shape && py == y_of(0.0) {
                prev = None;
                continue;
            }
            // Join to the previous column so that steep slopes stay connected.
            let (a, b) = match prev {
                Some(p) if p < py => (p + 1, py),
                Some(p) if p > py => (py, p - 1),
                _ => (py, py),
            };
            for y in a..=b {
                bits[y / 4][px / 2] |= DOTS[px % 2][y % 4];
            }
            prev = Some(py);
        }
    }

    let grid_rows: Vec<(usize, i32)> = {
        let mut v = Vec::new();
        let mut db = -range;
        while db <= range {
            v.push((y_of(db) / 4, db as i32));
            db += 6.0;
        }
        v
    };
    let grid_cols: Vec<usize> = [100.0, 1000.0, 10_000.0]
        .iter()
        .map(|&f| ((x_of(f) * (pw - 1) as f64).round() as usize) / 2)
        .collect();
    let mut marks = vec![vec![None; w]; h];
    for (i, b) in ed.bands.iter().enumerate() {
        let x = ((x_of(b.freq as f64) * (pw - 1) as f64).round() as usize / 2).min(w - 1);
        let y = (y_of(b.gain as f64) / 4).min(h - 1);
        marks[y][x] = Some(i);
    }

    let mut panel = vec![format!("{TITLE}Advanced EQ{RESET}"), String::new()];
    panel.push(format!("{GRID}  #      freq     gain      Q{RESET}"));
    for (i, b) in ed.bands.iter().enumerate() {
        let line = format!("{} {}  {}  {:+5.1} dB  {:5.2}", ' ', i + 1, freq_label(b.freq), b.gain, b.q);
        panel.push(if i == ed.selected { format!("{MARK_SELECTED}>{}{RESET}", &line[1..]) } else { line });
    }
    panel.push(String::new());
    panel.push(format!("total gain {total_gain:+6.1} dB"));
    // Plain text, cut to the panel: a wrapped line would shift the whole frame.
    panel.extend(info.iter().map(|l| l.chars().take(PANEL - 2).collect::<String>()));
    panel.push(String::new());
    for k in [
        "up/down  gain    left/right  freq",
        "H/L  freq x4     [ ]  width (Q)",
        "tab/1-8 band  a add  x delete",
        "0 flat  r factory  u undo all",
        "s save  q quit",
    ] {
        panel.push(format!("{GRID}{k}{RESET}"));
    }

    let mut out = String::from("\x1b[H");
    for y in 0..h {
        let label = grid_rows.iter().find(|(r, _)| *r == y).map(|(_, db)| format!("{db:>+4} "));
        match label {
            Some(l) if l.trim() == "+0" => out.push_str(&format!("{GRID}   0 ")),
            Some(l) => out.push_str(&format!("{GRID}{l}")),
            None => out.push_str(&" ".repeat(GUTTER)),
        }
        let mut current = "";
        let mut set = |out: &mut String, color: &'static str| {
            if current != color {
                out.push_str(RESET);
                out.push_str(color);
                current = color;
            }
        };
        for x in 0..w {
            if let Some(i) = marks[y][x] {
                set(&mut out, if i == ed.selected { MARK_SELECTED } else { MARK });
                out.push(char::from_digit(i as u32 + 1, 10).unwrap_or('?'));
            } else if curve_bits[y][x] != 0 {
                set(&mut out, CURVE);
                out.push(braille(curve_bits[y][x] | shape_bits[y][x]));
            } else if shape_bits[y][x] != 0 {
                set(&mut out, SHAPE);
                out.push(braille(shape_bits[y][x]));
            } else if grid_rows.iter().any(|(r, _)| *r == y) {
                set(&mut out, GRID);
                out.push('┈');
            } else if grid_cols.contains(&x) {
                set(&mut out, GRID);
                out.push('┊');
            } else {
                out.push(' ');
            }
        }
        out.push_str(RESET);
        if let Some(p) = panel.get(y) {
            out.push_str("  ");
            out.push_str(p);
        }
        out.push_str("\x1b[K\n");
    }

    // Decades first, the others where they fit, one blank cell between two labels.
    let mut axis = vec![' '; w];
    for (f, label) in [
        (20.0, "20"),
        (100.0, "100"),
        (1000.0, "1k"),
        (10_000.0, "10k"),
        (20_000.0, "20k"),
        (50.0, "50"),
        (200.0, "200"),
        (500.0, "500"),
        (2000.0, "2k"),
        (5000.0, "5k"),
    ] {
        let x = ((x_of(f) * (pw - 1) as f64).round() as usize) / 2;
        let start = x.saturating_sub(label.len() / 2).min(w.saturating_sub(label.len()));
        let around = start.saturating_sub(1)..(start + label.len() + 1).min(w);
        if start + label.len() <= w && axis[around].iter().all(|&c| c == ' ') {
            for (k, c) in label.chars().enumerate() {
                axis[start + k] = c;
            }
        }
    }
    out.push_str(&format!(
        "{GRID}{}{}  Hz{RESET}\x1b[K\n",
        " ".repeat(GUTTER),
        axis.iter().collect::<String>()
    ));
    out.push_str(&status.chars().take(cols.saturating_sub(1)).collect::<String>());
    out.push_str("\x1b[K");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(freq: f32, gain: f32, q: f32) -> EqBand {
        EqBand { filter: FILTER_PEAK, gain, freq, q }
    }

    #[test]
    fn keys_from_terminal_bytes() {
        assert_eq!(parse_keys(b"\x1b[A\x1b[Bjk"), [Key::Up, Key::Down, Key::Char('j'), Key::Char('k')]);
        assert_eq!(parse_keys(b"\x1b[1;2C\x1b[1;2D\x1b[C"), [Key::ShiftRight, Key::ShiftLeft, Key::Right]);
        assert_eq!(parse_keys(b"\t\x1b[Z\x1b[3~\x7f"), [Key::Tab, Key::BackTab, Key::Delete, Key::Delete]);
        // Application cursor mode (SS3) arrows, and an unknown sequence dropped.
        assert_eq!(parse_keys(b"\x1bOA\x1b[15~q"), [Key::Up, Key::Char('q')]);
    }

    #[test]
    fn edits_stay_within_the_headset_limits() {
        let mut ed = Editor::new(vec![band(20.0, 5.5, 9.0)]);
        for _ in 0..4 {
            ed.handle(Key::Up);
            ed.handle(Key::Left);
            ed.handle(Key::Char(']'));
        }
        assert_eq!(ed.bands[0], band(20.0, 6.0, 10.0));
        let mut eq = ed.bands.clone();
        assert!(eqfile::validate(&mut eq).unwrap().is_empty());
        assert_eq!(ed.handle(Key::Up), Outcome::Nothing, "already at the top");
    }

    #[test]
    fn frequency_and_q_steps() {
        let mut ed = Editor::new(vec![band(1000.0, 0.0, 1.0)]);
        ed.handle(Key::Right);
        assert_eq!(ed.bands[0].freq, 1059.0);
        ed.handle(Key::Char('H'));
        assert_eq!(ed.bands[0].freq, 841.0);
        for _ in 0..4 {
            ed.handle(Key::Char(']'));
        }
        assert_eq!(ed.bands[0].q, 2.0);
        // Low frequencies still move by at least 1 Hz.
        let mut low = Editor::new(vec![band(21.0, 0.0, 1.0)]);
        low.handle(Key::Right);
        assert_eq!(low.bands[0].freq, 22.0);
    }

    #[test]
    fn selection_follows_a_band_moved_past_another() {
        let mut ed = Editor::new(vec![band(100.0, 1.0, 1.0), band(120.0, 2.0, 1.0)]);
        for _ in 0..5 {
            ed.handle(Key::Right);
        }
        assert_eq!(ed.selected, 1);
        assert_eq!(ed.bands[1].gain, 1.0);
        assert!(ed.bands.windows(2).all(|w| w[0].freq <= w[1].freq));
    }

    #[test]
    fn add_remove_and_undo() {
        let mut ed = Editor::new(eqfile::factory());
        assert_eq!(ed.handle(Key::Char('a')), Outcome::Changed);
        // Widest free stretch (as a ratio): 250 to 4000 Hz, middle at 1 kHz.
        assert_eq!(ed.bands[ed.selected].freq, 1000.0);
        while ed.bands.len() < MAX_BANDS {
            ed.handle(Key::Char('a'));
        }
        assert_eq!(ed.handle(Key::Char('a')), Outcome::Nothing);
        while ed.bands.len() > 1 {
            ed.handle(Key::Char('x'));
        }
        assert_eq!(ed.handle(Key::Char('x')), Outcome::Nothing, "a curve keeps one band");
        assert_eq!(ed.handle(Key::Char('u')), Outcome::Reverted);
        assert!(ed.is_original());
        assert_eq!(ed.handle(Key::Char('u')), Outcome::Nothing);
    }

    #[test]
    fn factory_reset_then_undo() {
        let mut ed = Editor::new(vec![band(58.0, -1.0, 1.0), band(440.0, 6.0, 1.0), band(6600.0, -6.0, 1.0)]);
        ed.selected = 2;
        assert_eq!(ed.handle(Key::Char('r')), Outcome::Changed);
        assert_eq!(ed.bands, eqfile::factory());
        assert_eq!(ed.selected, 0);
        assert_eq!(ed.handle(Key::Char('r')), Outcome::Nothing, "already the factory curve");
        assert_eq!(ed.handle(Key::Char('u')), Outcome::Reverted);
        assert_eq!(ed.bands[1].gain, 6.0);
    }

    fn visible(s: &str) -> String {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            match (esc, c) {
                (false, '\x1b') => esc = true,
                (true, c) if c.is_ascii_alphabetic() => esc = false,
                (true, _) => {}
                (false, c) => out.push(c),
            }
        }
        out
    }

    #[test]
    fn a_nearly_flat_curve_stays_on_the_zero_row() {
        // A boost is above 0 dB everywhere, by a hair far from its centre: at 30 Hz, a band
        // at 15 kHz must not lift the curve by a text row.
        let row = |bands: Vec<EqBand>| {
            let frame = visible(&render(&Editor::new(bands), 0.0, 100, 30, &[], ""));
            frame
                .lines()
                .position(|l| {
                    l.chars().nth(GUTTER + 2).is_some_and(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                })
                .unwrap()
        };
        assert_eq!(row(vec![band(1000.0, 0.0, 1.0)]), row(vec![band(15_000.0, 0.5, 2.0)]));
    }

    #[test]
    fn frames_fit_the_terminal() {
        let mut ed = Editor::new(vec![
            band(60.0, 6.0, 0.5),
            band(70.0, 6.0, 0.5),
            band(80.0, 6.0, 0.5),
            band(9000.0, -6.0, 4.0),
        ]);
        ed.selected = 3;
        for (cols, rows) in [(120, 40), (80, 24), (10, 3)] {
            let frame = render(&ed, -4.5, cols, rows, &["headset   sent".into()], "status");
            let text = visible(&frame);
            // Each graph line: the scale plus the plot area, whatever the panel holds, and
            // never wider than the terminal once it is big enough for the panel.
            let plot = cols.saturating_sub(GUTTER + 2 + PANEL).max(10);
            for line in text.lines() {
                assert!(cols < GUTTER + 12 + PANEL || line.chars().count() <= cols, "{line:?}");
            }
            for line in text.lines().take(rows.saturating_sub(2).max(4)) {
                assert!(line.chars().count() >= GUTTER + plot, "{line:?}");
            }
            assert_eq!(text.lines().count(), rows.max(6), "one line per terminal row");
            if cols >= 80 {
                assert!(text.contains("+18"), "range widened for three stacked +6 dB bands");
                for digit in ['1', '2', '3', '4'] {
                    assert!(text.contains(digit));
                }
                assert!(text.contains("100") && text.contains("1k") && text.contains("10k"));
            }
        }
    }
}
