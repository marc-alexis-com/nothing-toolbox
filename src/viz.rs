//! Terminal 3D view of the head pose: the low-poly headset of `ps2.rs`, with an orbit camera.
//!
//! In kitty it is a real picture (`KittyView`); elsewhere the same picture is drawn at two
//! pixels per character cell, printed as `▀` half blocks in 24-bit colour.

use crate::headtrack::{euler_deg, mat_from_quat, matmul, Mat3, Quat};
use crate::kitty::{self, Kitty, Route};
use crate::model::{Edition, Rgb};
use crate::ps2;

const RESET: &str = "\x1b[0m";

/// Width of the text window in the top right corner, over the picture.
const WINDOW: usize = 30;
/// Its background, a dark slate.
const WINDOW_BG: &str = "\x1b[48;2;14;16;22m";

/// What the window shows besides the pose.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hud {
    pub hz: f64,
    pub az: f64,
    pub el: f64,
}

/// Straight behind the head, at its height.
pub const DEFAULT_AZIMUTH: f64 = 0.0;
pub const DEFAULT_ELEVATION: f64 = 0.0;

/// Orbit camera: azimuth about the vertical (Z), then elevation. The camera looks along +Y
/// from -Y: at 0/0 it is right behind the head, looking where the wearer looks, so the
/// wearer's right is on the right of the screen (not a mirror).
pub fn view_matrix(az_deg: f64, el_deg: f64) -> Mat3 {
    let (a, e) = (az_deg.to_radians(), el_deg.to_radians());
    let (ca, sa, ce, se) = (a.cos(), a.sin(), e.cos(), e.sin());
    let raz = [[ca, -sa, 0.0], [sa, ca, 0.0], [0.0, 0.0, 1.0]];
    let rel = [[1.0, 0.0, 0.0], [0.0, ce, -se], [0.0, se, ce]];
    matmul(&rel, &raz)
}

/// A `pw` x `ph` picture (square pixels), row by row; `None` is the terminal's background.
pub struct Picture {
    pub pw: usize,
    pub ph: usize,
    pub px: Vec<Option<Rgb>>,
}

fn rgb8(c: Rgb) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Prints a picture as half blocks: the upper pixel in the foreground of `▀`, the lower one in
/// its background, the terminal's own background where there is nothing.
fn encode(p: &Picture, row: usize, width: usize, out: &mut String) {
    let (mut fg, mut bg): (Option<[u8; 3]>, Option<[u8; 3]>) = (None, None);
    out.push_str(RESET);
    for x in 0..width.min(p.pw) {
        let top = p.px[2 * row * p.pw + x].map(rgb8);
        let bottom = if 2 * row + 1 < p.ph { p.px[(2 * row + 1) * p.pw + x].map(rgb8) } else { None };
        let (ch, want_fg, want_bg) = match (top, bottom) {
            (None, None) => (' ', fg, None),
            (Some(t), b) => ('▀', Some(t), b),
            (None, Some(b)) => ('▄', Some(b), None),
        };
        if want_bg != bg {
            match want_bg {
                Some([r, g, b]) => out.push_str(&format!("\x1b[48;2;{r};{g};{b}m")),
                None => out.push_str("\x1b[49m"),
            }
            bg = want_bg;
        }
        if want_fg != fg {
            if let Some([r, g, b]) = want_fg {
                out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            fg = want_fg;
        }
        out.push(ch);
    }
    out.push_str(RESET);
}

/// Renders one full frame (ANSI) for pose `q`, `cols` x `rows` cells: the picture over the
/// whole terminal, the window in its top right corner.
pub fn render(q: Quat, edition: Edition, cols: usize, rows: usize, az: f64, el: f64, hud: &Hud) -> String {
    let w = cols.max(20);
    let h = rows.max(10);
    // The same low-poly headset as the kitty picture, two pixels per cell.
    let img = ps2::render(q, edition, w, 2 * h, az, el);
    let px = img.px.chunks(3).map(|c| Some([c[0], c[1], c[2]].map(|v| v as f64 / 255.0))).collect();
    let picture = Picture { pw: img.w, ph: img.h, px };

    let mut out = String::from("\x1b[H");
    compose(h, w, &window(q, edition, hud), &mut out, |y, width, out| encode(&picture, y, width, out));
    out
}

/// The window's lines: pose in degrees, the raw quaternion, the colour, the keys.
fn window(q: Quat, edition: Edition, hud: &Hud) -> Vec<String> {
    let (yaw, pitch, roll) = euler_deg(&mat_from_quat(q));
    let colour = match edition {
        Edition::White => "white",
        Edition::Black => "black",
    };
    let dim = "\x1b[90m";
    vec![
        "\x1b[96mHead tracking\x1b[0m".to_string(),
        String::new(),
        format!("yaw   {yaw:+7.1} deg"),
        format!("pitch {pitch:+7.1} deg"),
        format!("roll  {roll:+7.1} deg"),
        String::new(),
        format!("q  x {:+.3}   y {:+.3}", q[0], q[1]),
        format!("   z {:+.3}   w {:+.3}", q[2], q[3]),
        String::new(),
        format!("\x1b[93m{:4.1} Hz\x1b[0m   view {:.0}/{:.0}", hud.hz, hud.az, hud.el),
        format!("colour {colour}"),
        String::new(),
        format!("{dim}arrows/hjkl  orbit{RESET}"),
        format!("{dim}0 reset camera{RESET}"),
        format!("{dim}r recentre pose{RESET}"),
        format!("{dim}c colour    q quit{RESET}"),
    ]
}

/// Length of `s` on screen: the text without its colour sequences.
fn visible_len(s: &str) -> usize {
    let (mut n, mut esc) = (0, false);
    for c in s.chars() {
        match (esc, c) {
            (true, 'm') => esc = false,
            (true, _) => {}
            (false, '\x1b') => esc = true,
            (false, _) => n += 1,
        }
    }
    n
}

/// The window's text framed in a box, every line `WINDOW` cells wide on its dark background.
fn framed(window: &[String]) -> Vec<String> {
    let frame = "\x1b[38;5;245m";
    let bar = "─".repeat(WINDOW - 2);
    let mut lines = vec![format!("{WINDOW_BG}{frame}┌{bar}┐{RESET}")];
    for line in window {
        // The colour sequences end with a reset: bring the background back after each.
        let line = line.replace(RESET, &format!("{RESET}{WINDOW_BG}"));
        let pad = (WINDOW - 4).saturating_sub(visible_len(&line));
        lines.push(format!(
            "{WINDOW_BG}{frame}│{RESET}{WINDOW_BG} {line}{}{frame}│{RESET}",
            " ".repeat(pad + 1)
        ));
    }
    lines.push(format!("{WINDOW_BG}{frame}└{bar}┘{RESET}"));
    lines
}

/// `h` lines over a picture `cols` wide (drawn by `row(y, width, out)`, `width` cells of it):
/// the window sits flush right, one line down, in front of the picture.
fn compose(
    h: usize,
    cols: usize,
    window: &[String],
    out: &mut String,
    mut row: impl FnMut(usize, usize, &mut String),
) {
    let lines = framed(window);
    for y in 0..h {
        match y.checked_sub(1).and_then(|i| lines.get(i)).filter(|_| cols > WINDOW + 8) {
            Some(line) => {
                row(y, cols - WINDOW, out);
                out.push_str(line);
            }
            None => row(y, cols, out),
        }
        // The last line has no newline: a full screen would scroll.
        out.push_str(if y + 1 < h { "\x1b[K\n" } else { "\x1b[K" });
    }
}

/// Pixels per cell when the terminal does not say (a common kitty font size).
const CELL: (f64, f64) = (10.0, 21.0);
/// Lines of the rendered picture, before it is scaled up to the cells: PS2 territory.
const LINES: f64 = 240.0;

/// The same view as `render`, drawn as a real picture through the kitty graphics protocol.
pub struct KittyView {
    kitty: Kitty,
    route: Route,
    /// Cell size in pixels, for the terminal size it was asked for.
    cell: Option<((usize, usize), (f64, f64))>,
}

impl KittyView {
    pub fn new(route: Route) -> KittyView {
        KittyView { kitty: Kitty::new(route), route, cell: None }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        q: Quat,
        edition: Edition,
        cols: usize,
        rows: usize,
        az: f64,
        el: f64,
        hud: &Hud,
    ) -> String {
        let w = cols.max(20);
        let h = rows.clamp(10, kitty::MAX_ROWS);
        let (cw, ch) = match self.cell {
            Some((size, cell)) if size == (cols, rows) => cell,
            _ => {
                let cell = kitty::cell_pixels(self.route, cols, rows).unwrap_or(CELL);
                self.cell = Some(((cols, rows), cell));
                cell
            }
        };
        // Render small, then scale up by a whole factor to about the size of the cells: the
        // terminal only has a few percent left to stretch.
        let (bw, bh) = (w as f64 * cw, h as f64 * ch);
        let k = (bh / LINES).round().max(1.0);
        let (pw, ph) = (((bw / k) as usize).max(16), ((bh / k) as usize).max(16));
        let picture = ps2::upscale(&ps2::render(q, edition, pw, ph, az, el), k as usize);

        let mut out = String::new();
        let id = self.kitty.upload(&picture, w, h, &mut out);
        out.push_str("\x1b[H");
        compose(h, w, &window(q, edition, hud), &mut out, |y, width, out| {
            Kitty::placeholder_row(id, y, width, out);
            out.push_str(RESET);
        });
        self.kitty.retire(id, &mut out);
        out
    }
}

/// Raw keyboard input for the viewer (cbreak, no echo), restored on drop.
pub struct RawTerminal {
    saved: Option<libc::termios>,
}

impl RawTerminal {
    pub fn enter() -> RawTerminal {
        // SAFETY: termios calls on stdin with a properly sized struct.
        unsafe {
            if libc::isatty(0) == 0 {
                return RawTerminal { saved: None };
            }
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return RawTerminal { saved: None };
            }
            let saved = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO);
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            print!("\x1b[2J\x1b[?25l");
            RawTerminal { saved: Some(saved) }
        }
    }

    /// False when standard input is not a terminal (no keys will ever be read).
    pub fn is_interactive(&self) -> bool {
        self.saved.is_some()
    }

    /// Non-blocking read of pending key bytes.
    pub fn keys(&self) -> Vec<u8> {
        if self.saved.is_none() {
            return Vec::new();
        }
        let mut buf = [0u8; 64];
        // SAFETY: reading into a local buffer of the given size; VMIN=0 makes it non-blocking.
        let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            buf[..n as usize].to_vec()
        } else {
            Vec::new()
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if let Some(t) = self.saved {
            // SAFETY: restoring the attributes saved in `enter`.
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &t);
            }
        }
        print!("\x1b[?25h\x1b[2J\x1b[H");
    }
}

pub fn terminal_size() -> (usize, usize) {
    // SAFETY: TIOCGWINSZ fills a winsize struct.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return (ws.ws_col as usize, ws.ws_row as usize);
        }
    }
    (100, 40)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::V3;

    const ID: Quat = [0.0, 0.0, 0.0, 1.0];

    fn apply(m: &Mat3, p: V3) -> V3 {
        [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2])
    }

    #[test]
    fn text_blocks_show_the_low_poly_picture() {
        // The upper pixel of each cell is the foreground of a half block, in 24-bit colour:
        // the top-left one is the renderer's own.
        let img = ps2::render(ID, Edition::White, 104, 56, DEFAULT_AZIMUTH, DEFAULT_ELEVATION);
        let f = render(ID, Edition::White, 104, 28, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, &Hud::default());
        let [r, g, b] = [img.px[0], img.px[1], img.px[2]];
        assert!(f.lines().next().unwrap().contains(&format!("38;2;{r};{g};{b}m▀")));
    }

    #[test]
    fn camera_and_pose_change_the_picture() {
        let a = render(ID, Edition::White, 100, 30, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, &Hud::default());
        let b = render(ID, Edition::White, 100, 30, 35.0, 20.0, &Hud::default());
        let c = render(
            [0.259, 0.0, 0.0, 0.966],
            Edition::White,
            100,
            30,
            DEFAULT_AZIMUTH,
            DEFAULT_ELEVATION,
            &Hud::default(),
        );
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn the_default_view_is_from_behind() {
        // Head at rest: the face looks away from the camera, the wearer's right ear is on the
        // right of the screen; a positive elevation looks from above.
        let m = view_matrix(DEFAULT_AZIMUTH, DEFAULT_ELEVATION);
        assert!(apply(&m, [0.0, 1.0, 0.0])[1] > 0.99);
        assert!(apply(&m, [1.0, 0.0, 0.0])[0] > 0.99);
        assert!(apply(&view_matrix(0.0, 20.0), [0.0, 0.0, 1.0])[1] < 0.0);
    }

    #[test]
    fn frames_have_one_line_per_row() {
        let f = render(ID, Edition::White, 104, 28, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, &Hud::default());
        assert_eq!(f.lines().count(), 28);
        assert!(f.contains('▀'));
    }

    #[test]
    fn the_window_floats_over_a_full_width_picture() {
        // Every line is as wide as the terminal, the window's lines included, and the window
        // names the colour.
        let f = render(ID, Edition::Black, 104, 28, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, &Hud::default());
        for line in f.trim_start_matches("\x1b[H").split('\n') {
            assert_eq!(visible_len(&line.replace("\x1b[K", "")), 104, "{line:?}");
        }
        assert!(f.contains("colour black") && f.contains("c colour"));
        assert!(!f.contains("right ear cup"));
    }

    #[test]
    fn tiny_terminal_does_not_panic() {
        let _ = render([0.259, 0.0, 0.0, 0.966], Edition::White, 5, 3, 0.0, 0.0, &Hud::default());
    }

    /// Writes what `headtrack --viz` prints in kitty through tmux, for three poses, to
    /// $NOTHING_PREVIEW_DIR/stream-N.bin (to `cat` in a real terminal).
    #[test]
    #[ignore]
    fn kitty_stream() {
        let dir = std::env::var("NOTHING_PREVIEW_DIR").expect("NOTHING_PREVIEW_DIR");
        let (cols, rows) = (
            std::env::var("COLS").map(|v| v.parse().unwrap()).unwrap_or(120),
            std::env::var("ROWS").map(|v| v.parse().unwrap()).unwrap_or(40),
        );
        let route = if std::env::var_os("DIRECT").is_some() { Route::Direct } else { Route::Tmux };
        let mut view = KittyView::new(route);
        let px = |k: &str, d: f64| std::env::var(k).map(|v| v.parse().unwrap()).unwrap_or(d);
        view.cell = Some(((cols, rows), (px("CW", 10.0), px("CH", 21.0))));
        for (n, q) in [ID, [0.259, 0.0, 0.0, 0.966], [0.0, 0.0, 0.383, 0.924]].iter().enumerate() {
            let f = view.render(
                *q,
                Edition::White,
                cols,
                rows,
                DEFAULT_AZIMUTH,
                DEFAULT_ELEVATION,
                &Hud::default(),
            );
            std::fs::write(format!("{dir}/stream-{n}.bin"), f).unwrap();
        }
        std::mem::forget(view);
    }
}
