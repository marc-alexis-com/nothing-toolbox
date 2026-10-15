//! Terminal 3D view of the head pose: a shaded model of the headset, with an orbit camera.
//!
//! The model (`model.rs`) is rasterised with a depth buffer into a pixel grid of two pixels
//! per character cell, printed as `▀` half blocks in 24-bit colour.

use crate::headtrack::{euler_deg, mat_from_quat, matmul, Mat3, Quat};
use crate::kitty::{self, Kitty, Route};
use crate::model::{self, Edition, Rgb, Tri, V3};
use crate::ps2;
use std::sync::OnceLock;

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

fn apply(m: &Mat3, p: V3) -> V3 {
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2])
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn mesh(edition: Edition) -> &'static [Tri] {
    static WHITE: OnceLock<Vec<Tri>> = OnceLock::new();
    static BLACK: OnceLock<Vec<Tri>> = OnceLock::new();
    match edition {
        Edition::White => WHITE.get_or_init(|| model::headset(Edition::White)),
        Edition::Black => BLACK.get_or_init(|| model::headset(Edition::Black)),
    }
}

/// Light from the upper left, in front of the head (view space: x right, y into the screen,
/// z up).
const LIGHT: V3 = [-0.45, -0.55, 0.70];
/// Camera distance for the (mild) perspective, in model units (about 8 cm).
const DISTANCE: f64 = 8.0;
/// Height of the picture's centre: the headband rises well above the cups.
const LIFT: f64 = 0.3;
/// The floor, a little under the cups, and its two shades.
const FLOOR_Z: f64 = -1.15;
const FLOOR: [(f64, Rgb); 1] = [(1.35, [0.085, 0.085, 0.095])];

/// A `pw` x `ph` picture (square pixels), row by row; `None` is background.
pub struct Picture {
    pub pw: usize,
    pub ph: usize,
    pub px: Vec<Option<Rgb>>,
}

/// Draws at twice the resolution and averages each 2x2 block: smoother edges and shading. A
/// pixel is background when most of its block is.
pub fn rasterize(q: Quat, edition: Edition, pw: usize, ph: usize, az: f64, el: f64) -> Picture {
    let big = rasterize_at(q, edition, 2 * pw, 2 * ph, az, el);
    let px = (0..pw * ph)
        .map(|i| {
            let (x, y) = (i % pw, i / pw);
            let block: Vec<Rgb> = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .filter_map(|(dx, dy)| big.px[(2 * y + dy) * big.pw + 2 * x + dx])
                .collect();
            (block.len() >= 2).then(|| {
                let n = block.len() as f64;
                [0, 1, 2].map(|k| block.iter().map(|c| c[k]).sum::<f64>() / n)
            })
        })
        .collect();
    Picture { pw, ph, px }
}

fn rasterize_at(q: Quat, edition: Edition, pw: usize, ph: usize, az: f64, el: f64) -> Picture {
    let r = mat_from_quat(q);
    let view = view_matrix(az, el);
    let m = matmul(&view, &r);
    // The scene spans about 4.6 model units across.
    let scale = (pw as f64).min(ph as f64) / 4.9;
    let (cx, cy) = (pw as f64 / 2.0, ph as f64 / 2.0);
    let project = |v: V3| -> (f64, f64, f64) {
        let k = DISTANCE / (DISTANCE + v[1]);
        (cx + v[0] * k * scale, cy - (v[2] - LIFT) * k * scale, v[1])
    };
    let mut px: Vec<Option<Rgb>> = vec![None; pw * ph];
    let mut depth = vec![f64::INFINITY; pw * ph];
    let mut fill = |s: [(f64, f64, f64); 3], rgb: Rgb, px: &mut Vec<Option<Rgb>>| {
        let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
        if area.abs() < 1e-9 {
            return;
        }
        let x0 = s.iter().map(|p| p.0).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let x1 = s.iter().map(|p| p.0).fold(f64::MIN, f64::max).ceil().min(pw as f64) as usize;
        let y0 = s.iter().map(|p| p.1).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let y1 = s.iter().map(|p| p.1).fold(f64::MIN, f64::max).ceil().min(ph as f64) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                let e = |a: (f64, f64, f64), b: (f64, f64, f64)| {
                    (b.0 - a.0) * (fy - a.1) - (fx - a.0) * (b.1 - a.1)
                };
                let (w0, w1, w2) = (e(s[1], s[2]) / area, e(s[2], s[0]) / area, e(s[0], s[1]) / area);
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
                let i = y * pw + x;
                if z < depth[i] {
                    depth[i] = z;
                    px[i] = Some(rgb);
                }
            }
        }
    };

    // The floor stays put (only the camera moves it): two discs, the darker one on top.
    for (k, (radius, rgb)) in FLOOR.iter().enumerate() {
        let n = 48;
        let z = FLOOR_Z + 0.002 * k as f64;
        let rim = |i: usize| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            project(apply(&view, [radius * t.cos(), radius * t.sin(), z]))
        };
        let centre = project(apply(&view, [0.0, 0.0, z]));
        for i in 0..n {
            fill([centre, rim(i), rim(i + 1)], *rgb, &mut px);
        }
    }

    let light = unit(LIGHT);
    let half = unit([light[0], light[1] - 1.0, light[2]]);
    for t in mesh(edition) {
        let v = t.v.map(|p| apply(&m, p));
        let mut n = unit(cross(sub(v[1], v[0]), sub(v[2], v[0])));
        if n[1] > 0.0 {
            n = n.map(|c| -c); // two-sided: the face turned towards the camera
        }
        let diffuse = dot(n, light).max(0.0);
        let spec = t.gloss * dot(n, half).max(0.0).powi(24);
        let rgb = if t.glow {
            t.color
        } else {
            t.color.map(|c| (c * (0.30 + 0.70 * diffuse) + 0.45 * spec).min(1.0))
        };
        fill(v.map(project), rgb, &mut px);
    }

    Picture { pw, ph, px }
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
    let picture = rasterize(q, edition, w, 2 * h, az, el);

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

    const ID: Quat = [0.0, 0.0, 0.0, 1.0];

    fn covered(p: &Picture) -> usize {
        p.px.iter().filter(|c| c.is_some()).count()
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
    fn the_headset_stays_in_frame() {
        // Whatever the pose and the camera, the headset is drawn whole: nothing but the floor
        // touches the border of the picture.
        let floor = |c: &Option<Rgb>| c.is_none_or(|c| FLOOR.iter().any(|f| f.1 == c));
        for (q, az, el) in [
            (ID, DEFAULT_AZIMUTH, DEFAULT_ELEVATION),
            (ID, -60.0, 45.0),
            ([0.259, 0.0, 0.0, 0.966], 120.0, -40.0),
            ([0.0, 0.5, 0.0, 0.866], 0.0, 85.0),
            ([0.383, 0.0, 0.383, 0.84], 200.0, 10.0),
        ] {
            let p = rasterize(q, Edition::Black, 73, 54, az, el);
            assert!(covered(&p) > 300, "too little drawn at az {az} el {el}");
            for x in 0..p.pw {
                assert!(
                    floor(&p.px[x]) && floor(&p.px[(p.ph - 1) * p.pw + x]),
                    "top/bottom at az {az} el {el}"
                );
            }
            for y in 0..p.ph {
                assert!(floor(&p.px[y * p.pw + p.pw - 1]), "right edge at az {az} el {el}");
            }
        }
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
    fn nearer_parts_hide_farther_ones() {
        // From the wearer's right the red dot on the right cup shows; from the left, the left
        // cup hides it.
        let red = |p: &Picture| p.px.iter().flatten().filter(|c| c[0] > 0.5 && c[1] < 0.2).count();
        assert!(red(&rasterize(ID, Edition::White, 80, 60, -90.0, 0.0)) > 0);
        assert_eq!(red(&rasterize(ID, Edition::White, 80, 60, 90.0, 0.0)), 0);
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
    fn the_right_cup_marker_shows_from_behind() {
        let p = rasterize(ID, Edition::White, 80, 60, DEFAULT_AZIMUTH, DEFAULT_ELEVATION);
        let red: Vec<usize> = (0..p.px.len())
            .filter(|&i| p.px[i].is_some_and(|c| c[0] > 0.5 && c[1] < 0.2))
            .map(|i| i % p.pw)
            .collect();
        assert!(!red.is_empty() && red.iter().all(|&x| x > p.pw / 2), "{red:?}");
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
