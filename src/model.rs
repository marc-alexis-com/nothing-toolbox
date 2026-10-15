//! The headset drawn by `headtrack --viz`: a triangle mesh of a Nothing Headphone (1), its
//! proportions taken from product photos (approximate), in the head-tracking frame.
//!
//! Frame: Z up, X along the ear-to-ear axis, Y front to back; the origin is the centre of the
//! head, so that rotations turn the headset about it. Which end of X is the left ear is
//! inferred, not measured: in the recording of `src/testdata/b170-gestures.txt` the first nod
//! goes to positive roll, and a nod starts downwards, which puts +X on the wearer's left and
//! +Y at the back (a right-handed frame).

pub type V3 = [f64; 3];

/// Linear RGB, 0..1.
pub type Rgb = [f64; 3];

pub struct Tri {
    pub v: [V3; 3],
    pub color: Rgb,
    /// Specular strength: 0 for matte padding, higher for the aluminium.
    pub gloss: f64,
    /// Drawn at full colour whatever the light (the red marker).
    pub glow: bool,
}

/// Colours of the two editions, from the headset's colour code (`C00C`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edition {
    White,
    Black,
}

struct Palette {
    shell: Rgb,
    window: Rgb,
    cushion: Rgb,
    band: Rgb,
    metal: Rgb,
}

fn palette(e: Edition) -> Palette {
    match e {
        Edition::White => Palette {
            shell: [0.86, 0.86, 0.84],
            window: [0.97, 0.97, 0.96],
            cushion: [0.55, 0.55, 0.56],
            band: [0.80, 0.80, 0.79],
            metal: [0.70, 0.71, 0.73],
        },
        Edition::Black => Palette {
            shell: [0.20, 0.20, 0.21],
            window: [0.32, 0.32, 0.34],
            cushion: [0.11, 0.11, 0.12],
            band: [0.16, 0.16, 0.17],
            metal: [0.45, 0.46, 0.48],
        },
    }
}

/// Marks the right ear cup, so that left and right can be told apart.
pub const ACCENT: Rgb = [0.84, 0.10, 0.13];

/// Rounded-rectangle outline ("squircle", |y/a|^4 + |z/b|^4 = 1) in the Y-Z plane.
fn squircle(a: f64, b: f64, n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            let (c, s) = (t.cos(), t.sin());
            (a * c.signum() * c.abs().sqrt(), b * s.signum() * s.abs().sqrt())
        })
        .collect()
}

fn circle(r: f64, n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            (r * t.cos(), r * t.sin())
        })
        .collect()
}

struct Builder {
    tris: Vec<Tri>,
}

impl Builder {
    fn tri(&mut self, a: V3, b: V3, c: V3, color: Rgb, gloss: f64) {
        self.tris.push(Tri { v: [a, b, c], color, gloss, glow: color == ACCENT });
    }

    fn quad(&mut self, a: V3, b: V3, c: V3, d: V3, color: Rgb, gloss: f64) {
        self.tri(a, b, c, color, gloss);
        self.tri(a, c, d, color, gloss);
    }

    /// A convex Y-Z outline, centred on (y0, z0), extruded along X from x0 to x1, with caps.
    #[allow(clippy::too_many_arguments)]
    fn prism(&mut self, outline: &[(f64, f64)], y0: f64, z0: f64, x0: f64, x1: f64, color: Rgb, gloss: f64) {
        let n = outline.len();
        let at = |i: usize, x: f64| [x, y0 + outline[i % n].0, z0 + outline[i % n].1];
        for i in 0..n {
            self.quad(at(i, x0), at(i + 1, x0), at(i + 1, x1), at(i, x1), color, gloss);
            self.tri([x0, y0, z0], at(i, x0), at(i + 1, x0), color, gloss);
            self.tri([x1, y0, z0], at(i + 1, x1), at(i, x1), color, gloss);
        }
    }

    /// A box between two corners.
    fn cuboid(&mut self, lo: V3, hi: V3, color: Rgb, gloss: f64) {
        let p =
            |i: usize| [[lo[0], hi[0]][i & 1], [lo[1], hi[1]][(i >> 1) & 1], [lo[2], hi[2]][(i >> 2) & 1]];
        for (a, b, c, d) in
            [(0, 1, 3, 2), (4, 5, 7, 6), (0, 1, 5, 4), (2, 3, 7, 6), (0, 2, 6, 4), (1, 3, 7, 5)]
        {
            self.quad(p(a), p(b), p(c), p(d), color, gloss);
        }
    }

    /// A rectangular section (half-width `hw` along Y, half-thickness `ht` across the path)
    /// swept along a path in the X-Z plane.
    fn sweep(&mut self, path: &[(f64, f64)], hw: f64, ht: f64, color: Rgb, gloss: f64) {
        let rings: Vec<[V3; 4]> = (0..path.len())
            .map(|i| {
                let (p, q) = (path[i.saturating_sub(1)], path[(i + 1).min(path.len() - 1)]);
                let (tx, tz) = (q.0 - p.0, q.1 - p.1);
                let l = (tx * tx + tz * tz).sqrt().max(1e-9);
                let (nx, nz) = (-tz / l * ht, tx / l * ht);
                let (x, z) = path[i];
                [[x + nx, -hw, z + nz], [x + nx, hw, z + nz], [x - nx, hw, z - nz], [x - nx, -hw, z - nz]]
            })
            .collect();
        for w in rings.windows(2) {
            for k in 0..4 {
                self.quad(w[0][k], w[0][(k + 1) % 4], w[1][(k + 1) % 4], w[1][k], color, gloss);
            }
        }
    }
}

/// Ear cups at x = ±CUP_X (outer face), centred on the ear.
const CUP_IN: f64 = 0.90;
const CUP_OUT: f64 = 1.20;

pub fn headset(edition: Edition) -> Vec<Tri> {
    let c = palette(edition);
    let mut b = Builder { tris: Vec::new() };
    let shell = squircle(0.42, 0.56, 48);
    let window = squircle(0.30, 0.42, 40);
    let cushion = squircle(0.37, 0.51, 40);
    for side in [1.0, -1.0] {
        let x = |v: f64| side * v;
        b.prism(&shell, 0.0, 0.0, x(CUP_IN), x(CUP_OUT), c.shell, 0.25);
        // The transparent cover, slightly raised on the outer face.
        b.prism(&window, 0.03, 0.04, x(CUP_OUT), x(CUP_OUT + 0.03), c.window, 0.6);
        b.prism(&cushion, 0.0, 0.0, x(0.74), x(CUP_IN), c.cushion, 0.0);
        // The arm from the cup to the headband.
        b.cuboid([x(1.03) - 0.04, -0.07, 0.48], [x(1.03) + 0.04, 0.07, 1.02], c.metal, 0.8);
    }
    // Right cup (-X, see the module notes): a red dot near the top of the outer face.
    b.prism(&circle(0.085, 16), -0.14, 0.30, -(CUP_OUT + 0.03), -(CUP_OUT + 0.05), ACCENT, 0.0);
    let arc: Vec<(f64, f64)> = (0..=48)
        .map(|i| {
            let t = std::f64::consts::PI * i as f64 / 48.0;
            (1.03 * t.cos(), 1.02 + 0.90 * t.sin())
        })
        .collect();
    b.sweep(&arc, 0.16, 0.055, c.band, 0.15);
    b.tris
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_is_symmetric_and_bounded() {
        let tris = headset(Edition::White);
        assert!(tris.len() > 500);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for t in &tris {
            for v in t.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        // Cups on both sides of the head, headband on top, nothing below the cups.
        assert!((lo[0] + hi[0]).abs() < 0.05, "x {lo:?} {hi:?}");
        assert!(hi[2] > 1.85 && lo[2] > -0.6, "z {lo:?} {hi:?}");
        // The accent is on the right cup only.
        assert!(tris.iter().filter(|t| t.color == ACCENT).all(|t| t.v.iter().all(|v| v[0] < 0.0)));
    }
}
