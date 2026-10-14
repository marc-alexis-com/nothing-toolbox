//! Terminal 3D view of the head pose: an axis gnomon and a cube, with an orbit camera.

use crate::headtrack::{euler_deg, mat_from_quat, matmul, Mat3, Quat};

const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Dim,
    Red,
    Green,
    Blue,
}

impl Color {
    fn code(self) -> &'static str {
        match self {
            Color::Dim => "\x1b[90m",
            Color::Red => "\x1b[91m",
            Color::Green => "\x1b[92m",
            Color::Blue => "\x1b[94m",
        }
    }
}

const CUBE: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];
const EDGES: [(usize, usize); 12] =
    [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
const AXES: [([f64; 3], Color, char); 3] = [
    ([1.7, 0.0, 0.0], Color::Red, 'X'),
    ([0.0, 1.7, 0.0], Color::Green, 'Y'),
    ([0.0, 0.0, 1.7], Color::Blue, 'Z'),
];

pub const DEFAULT_AZIMUTH: f64 = 35.0;
pub const DEFAULT_ELEVATION: f64 = 20.0;

/// Orbit camera: azimuth about the vertical (Z), then elevation.
fn view_matrix(az_deg: f64, el_deg: f64) -> Mat3 {
    let (a, e) = (az_deg.to_radians(), el_deg.to_radians());
    let (ca, sa, ce, se) = (a.cos(), a.sin(), e.cos(), e.sin());
    let raz = [[ca, -sa, 0.0], [sa, ca, 0.0], [0.0, 0.0, 1.0]];
    let rel = [[1.0, 0.0, 0.0], [0.0, ce, -se], [0.0, se, ce]];
    matmul(&rel, &raz)
}

/// Renders one full frame (ANSI) for pose `q`, `cols` x `rows` cells, plus side panel.
pub fn render(q: Quat, cols: usize, rows: usize, az: f64, el: f64, status: &str) -> String {
    let r = mat_from_quat(q);
    let m = matmul(&view_matrix(az, el), &r);
    let panel = 30;
    let w = cols.saturating_sub(panel + 1).max(20);
    let h = rows.saturating_sub(1).max(10);
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    // Terminal cells are about twice as tall as wide: x is scaled by 2.
    let scale = (w as f64 / 4.4).min(h as f64 / 2.4);
    let mut buf = vec![vec![(' ', Color::Dim); w]; h];

    let project = |p: [f64; 3]| -> (i64, i64) {
        let v: Vec<f64> = (0..3).map(|i| (0..3).map(|k| m[i][k] * p[k]).sum()).collect();
        ((cx + v[0] * scale * 2.0).round() as i64, (cy - v[2] * scale).round() as i64)
    };
    let mut plot = |x: i64, y: i64, ch: char, c: Color| {
        if (0..w as i64).contains(&x) && (0..h as i64).contains(&y) {
            buf[y as usize][x as usize] = (ch, c);
        }
    };
    let line = |a: [f64; 3], b: [f64; 3], ch: char, c: Color, plot: &mut dyn FnMut(i64, i64, char, Color)| {
        let ((mut x0, mut y0), (x1, y1)) = (project(a), project(b));
        let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx - dy;
        loop {
            plot(x0, y0, ch, c);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x0 += sx;
            }
            if e2 < dx {
                err += dx;
                y0 += sy;
            }
        }
    };
    for (a, b) in EDGES {
        line(CUBE[a], CUBE[b], '·', Color::Dim, &mut plot);
    }
    for (tip, c, label) in AXES {
        line([0.0; 3], tip, 'o', c, &mut plot);
        let (x, y) = project(tip);
        plot(x, y, label, c);
    }

    let (yaw, pitch, roll) = euler_deg(&r);
    let panel_lines = [
        "\x1b[96mHead tracking\x1b[0m".to_string(),
        String::new(),
        format!("{}X  {}Y  {}Z{RESET}  (Z = up)", Color::Red.code(), Color::Green.code(), Color::Blue.code()),
        String::new(),
        format!("yaw   {yaw:+7.1} deg"),
        format!("pitch {pitch:+7.1} deg"),
        format!("roll  {roll:+7.1} deg"),
        String::new(),
        format!("q  x {:+.3}", q[0]),
        format!("   y {:+.3}", q[1]),
        format!("   z {:+.3}", q[2]),
        format!("   w {:+.3}", q[3]),
    ];

    let mut out = String::from("\x1b[H");
    for (y, row) in buf.iter().enumerate() {
        let mut current = None;
        for &(ch, c) in row {
            if current != Some(c) {
                out.push_str(c.code());
                current = Some(c);
            }
            out.push(ch);
        }
        out.push_str(RESET);
        if let Some(p) = panel_lines.get(y) {
            out.push_str("  ");
            out.push_str(p);
        }
        out.push_str("\x1b[K\n");
    }
    out.push_str(status);
    out.push_str("\x1b[K");
    out
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

    #[test]
    fn camera_changes_the_picture() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let a = render(id, 100, 30, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, "");
        let b = render(id, 100, 30, 0.0, 0.0, "");
        assert_ne!(a, b);
        assert!(a.contains('X') && a.contains('Y') && a.contains('Z'));
    }

    #[test]
    fn tiny_terminal_does_not_panic() {
        let _ = render([0.259, 0.0, 0.0, 0.966], 5, 3, 0.0, 0.0, "");
    }
}
