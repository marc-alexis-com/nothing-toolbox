//! A low-poly Nothing Headphone (1) for the kitty view (`ps2.rs`), built from measurements of
//! product photos and orthographic renders (approximate, to a few millimetres), in the
//! head-tracking frame of `model.rs`: Z up, +X the wearer's right, +Y the face.
//!
//! Worn, seen from outside the head along X, each cup is a stack: a leather cushion (18 mm),
//! an aluminium shell (12.5 mm, a rounded rectangle 78 x 92 mm), and a clear cover standing
//! 9.5 mm proud (an oval 57 x 69 mm, a little below the centre). Under the cover, a moulded
//! plate with a raised "cassette" frame and two round grilles. A flat Z-shaped strip of
//! black metal hangs each cup by the top of its cover from a slider at the end of the
//! headband, which is nearly an arc with a pad under its top.
//!
//! The right cup carries all the controls: a round button at the top front of its outer
//! face, a roller and a paddle on its back edge, the ports on its bottom edge. Its plate has
//! a red square where the left one has a white square. One liberty for the viewer: the
//! paddle is drawn flat white, so that the right cup also shows from behind.
//!
//! The geometry comes from these numbers. The outer face of a cup (shell, plate, grilles, the
//! squares, the button) carries a texture baked from a render of the product, pixelised to
//! 64 x 76 texels (`texture.rs`, `tools/bake-face.sh`) and projected along the cup's axis.

use crate::model::{Edition, V3};

/// Model units per millimetre (the head is about 1.7 units wide).
const MM: f64 = 1.0 / 90.0;
/// Sign of X on the wearer's right (see `model.rs`): flip it to swap the sides.
pub const RIGHT: f64 = 1.0;

/// Procedural detail drawn on a surface, in model coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pattern {
    Plain,
    /// The pixelised outer face of a cup (`texture.rs`), looked up by the point's Y and Z.
    Face {
        black: bool,
        left: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// sRGB, 0..1.
    pub rgb: [f32; 3],
    /// Highlight strength and sharpness.
    pub spec: f32,
    pub shine: f32,
    /// How much of the colour is the reflected studio (metal).
    pub metal: f32,
    /// Below 1 for the clear covers.
    pub alpha: f32,
    /// Drawn at full colour whatever the light (the red marks).
    pub glow: bool,
    pub pattern: Pattern,
}

pub struct Tri {
    pub v: [V3; 3],
    /// Vertex normals: shared across a curved surface, the face's own across a crease.
    pub n: [V3; 3],
    pub mat: Material,
}

fn hex(c: u32) -> [f32; 3] {
    [(c >> 16) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
}

const fn plain(rgb: [f32; 3], spec: f32, shine: f32, metal: f32) -> Material {
    Material { rgb, spec, shine, metal, alpha: 1.0, glow: false, pattern: Pattern::Plain }
}

/// The parts' finishes, per edition.
pub struct Palette {
    pub shell: Material,
    pub cushion: Material,
    pub lining: Material,
    pub band: Material,
    pub slider: Material,
    pub pad: Material,
    pub strap: Material,
    /// The outer face's cap, and what stands on it: all of them take the face texture.
    pub face: Material,
    pub plate: Material,
    pub frame: Material,
    pub grille: Material,
    pub cover: Material,
    pub paddle: Material,
    pub control: Material,
    pub port: Material,
}

pub fn palette(e: Edition) -> Palette {
    // The paddle is drawn flat white, so that it shows whatever the light.
    let white = |c: u32| Material { glow: true, ..plain(hex(c), 0.0, 1.0, 0.0) };
    let face = |black: bool, m: Material| Material { pattern: Pattern::Face { black, left: false }, ..m };
    match e {
        Edition::White => Palette {
            shell: plain(hex(0xc9ccd0), 0.10, 6.0, 0.0),
            cushion: plain(hex(0x101112), 0.22, 9.0, 0.0),
            lining: plain(hex(0x060607), 0.0, 1.0, 0.0),
            band: plain(hex(0xdcdedf), 0.25, 12.0, 0.0),
            slider: plain(hex(0xd2d4d6), 0.30, 14.0, 0.0),
            pad: plain(hex(0x262728), 0.12, 8.0, 0.0),
            strap: plain(hex(0x26282a), 0.6, 30.0, 0.55),
            face: face(false, plain(hex(0xc9ccd0), 0.10, 6.0, 0.0)),
            plate: face(false, plain(hex(0xd3d5d7), 0.15, 10.0, 0.0)),
            frame: face(false, plain(hex(0xdddfe0), 0.2, 12.0, 0.0)),
            grille: face(false, plain(hex(0x8e9296), 0.05, 4.0, 0.0)),
            cover: Material { alpha: 0.16, ..plain(hex(0xeaf2f6), 1.2, 60.0, 0.3) },
            paddle: white(0xf6f6f6),
            control: plain(hex(0xd7dadd), 0.5, 18.0, 0.3),
            port: plain(hex(0x141516), 0.0, 1.0, 0.0),
        },
        Edition::Black => Palette {
            shell: plain(hex(0x34373c), 0.10, 6.0, 0.0),
            cushion: plain(hex(0x0c0d0e), 0.22, 9.0, 0.0),
            lining: plain(hex(0x050506), 0.0, 1.0, 0.0),
            band: plain(hex(0x3c3e41), 0.25, 12.0, 0.0),
            slider: plain(hex(0x393b3e), 0.30, 14.0, 0.0),
            pad: plain(hex(0x1e1f20), 0.12, 8.0, 0.0),
            strap: plain(hex(0x1c1d1f), 0.6, 30.0, 0.55),
            face: face(true, plain(hex(0x34373c), 0.10, 6.0, 0.0)),
            plate: face(true, plain(hex(0x2c2f33), 0.15, 10.0, 0.0)),
            frame: face(true, plain(hex(0x34373b), 0.2, 12.0, 0.0)),
            grille: face(true, plain(hex(0x1f2022), 0.05, 4.0, 0.0)),
            cover: Material { alpha: 0.30, ..plain(hex(0x4b5058), 1.2, 60.0, 0.3) },
            paddle: white(0xe4e4e4),
            control: plain(hex(0x3d4045), 0.5, 18.0, 0.3),
            port: plain(hex(0x0e0f10), 0.0, 1.0, 0.0),
        },
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 {
        a
    } else {
        mul(a, 1.0 / l)
    }
}

/// A rounded rectangle `w` x `h` (corner radius `r`) as a closed outline, `n` segments per
/// corner, counter-clockwise from the right side.
fn rrect(w: f64, h: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let (cx, cy) = (w / 2.0 - r, h / 2.0 - r);
    let mut out = Vec::with_capacity(4 * (n + 1));
    for (k, (sx, sy)) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].iter().enumerate() {
        for i in 0..=n {
            let t = std::f64::consts::FRAC_PI_2 * (k as f64 + i as f64 / n.max(1) as f64);
            out.push((sx * cx + r * t.cos(), sy * cy + r * t.sin()));
        }
    }
    out
}

/// A superellipse `w` x `h` (a squarish oval, as the cushion's outline), `n` points,
/// counter-clockwise from the right side.
fn oval(w: f64, h: f64, n: usize) -> Vec<(f64, f64)> {
    const POWER: f64 = 2.6;
    let pw = |v: f64| v.signum() * v.abs().powf(2.0 / POWER);
    (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            (w / 2.0 * pw(t.cos()), h / 2.0 * pw(t.sin()))
        })
        .collect()
}

/// A local frame: `at(a, b, c) = o + a u + b v + c w`, in millimetres.
#[derive(Clone, Copy)]
struct Frame {
    o: V3,
    u: V3,
    v: V3,
    w: V3,
}

impl Frame {
    fn at(&self, a: f64, b: f64, c: f64) -> V3 {
        add(self.o, add(mul(self.u, a), add(mul(self.v, b), mul(self.w, c))))
    }
}

struct Builder {
    tris: Vec<Tri>,
}

impl Builder {
    fn tri(&mut self, v: [V3; 3], n: [V3; 3], mat: Material) {
        self.tris.push(Tri { v: v.map(|p| mul(p, MM)), n, mat });
    }

    /// A surface through rows of points (rows along the length, points around): quads between
    /// neighbours, `closed` joining the last point of a row to the first. Normals point away
    /// from each row's centre; `smooth` shares them between faces, else each face is flat.
    fn grid(&mut self, rows: &[Vec<V3>], closed: bool, smooth: bool, mat: Material) {
        let n = rows[0].len();
        let quads = if closed { n } else { n - 1 };
        let centre = |r: &Vec<V3>| mul(r.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / r.len() as f64);
        let face = |i: usize, j: usize| {
            let (a, b, c, d) = (rows[i][j], rows[i][(j + 1) % n], rows[i + 1][(j + 1) % n], rows[i + 1][j]);
            let mid = mul(add(add(a, b), add(c, d)), 0.25);
            let axis = mul(add(centre(&rows[i]), centre(&rows[i + 1])), 0.5);
            let mut f = cross(sub(c, a), sub(d, b));
            // A collapsed quad (the tip of a taper) faces away from the axis.
            f = unit(if dot(f, f) < 1e-12 { sub(mid, axis) } else { f });
            if dot(f, sub(mid, axis)) < 0.0 {
                f = mul(f, -1.0);
            }
            f
        };
        let faces: Vec<Vec<V3>> =
            (0..rows.len() - 1).map(|i| (0..quads).map(|j| face(i, j)).collect()).collect();
        let normal = |i: usize, j: usize, fi: usize, fj: usize| {
            if !smooth {
                return faces[fi][fj];
            }
            let mut s = [0.0; 3];
            for di in [i.wrapping_sub(1), i] {
                for dj in [(j + quads - 1) % quads.max(1), j % quads.max(1)] {
                    if di < faces.len() && (closed || (dj + 1 == j || dj == j) && dj < quads) {
                        s = add(s, faces[di][dj]);
                    }
                }
            }
            if dot(s, s) < 1e-12 {
                faces[fi][fj]
            } else {
                unit(s)
            }
        };
        for i in 0..rows.len() - 1 {
            for j in 0..quads {
                let k = (j + 1) % n;
                let v = [rows[i][j], rows[i][k], rows[i + 1][k], rows[i + 1][j]];
                let nn =
                    [normal(i, j, i, j), normal(i, k, i, j), normal(i + 1, k, i, j), normal(i + 1, j, i, j)];
                self.tri([v[0], v[1], v[2]], [nn[0], nn[1], nn[2]], mat);
                self.tri([v[0], v[2], v[3]], [nn[0], nn[2], nn[3]], mat);
            }
        }
    }

    /// A flat polygon (convex outline) in plane `c` of frame `f`, facing `w * facing`.
    fn cap(&mut self, f: &Frame, outline: &[(f64, f64)], c: f64, facing: f64, mat: Material) {
        let n = mul(f.w, facing);
        let mid = outline.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
        let mid = f.at(mid.0 / outline.len() as f64, mid.1 / outline.len() as f64, c);
        for i in 0..outline.len() {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            self.tri([mid, f.at(a.0, a.1, c), f.at(b.0, b.1, c)], [n; 3], mat);
        }
    }

    /// Rings of an outline along `w`: (c, inset) pairs, each ring the outline shrunk by inset
    /// (per side). Optional flat caps at both ends.
    #[allow(clippy::too_many_arguments)]
    fn loft(
        &mut self,
        f: &Frame,
        w: f64,
        h: f64,
        r: f64,
        n: usize,
        rings: &[(f64, f64)],
        caps: (bool, bool),
        smooth: bool,
        mat: Material,
    ) {
        let outline = |e: f64| rrect(w - 2.0 * e, h - 2.0 * e, (r - e).max(0.5), n);
        self.loft_with(f, &outline, rings, caps, smooth, mat);
    }

    /// `loft` for any outline: `outline(inset)` is the ring shrunk by `inset` per side.
    fn loft_with(
        &mut self,
        f: &Frame,
        outline: &dyn Fn(f64) -> Vec<(f64, f64)>,
        rings: &[(f64, f64)],
        caps: (bool, bool),
        smooth: bool,
        mat: Material,
    ) {
        let rows: Vec<Vec<V3>> =
            rings.iter().map(|&(c, e)| outline(e).iter().map(|p| f.at(p.0, p.1, c)).collect()).collect();
        self.grid(&rows, true, smooth, mat);
        let (first, last) = (rings[0], rings[rings.len() - 1]);
        let dir = (last.0 - first.0).signum();
        if caps.0 {
            self.cap(f, &outline(first.1), first.0, -dir, mat);
        }
        if caps.1 {
            self.cap(f, &outline(last.1), last.0, dir, mat);
        }
    }

    /// A rounded-rectangle section (`w` across Y, `t` across the path) swept along a path in
    /// the X-Z plane at y = `y0`, with end caps.
    #[allow(clippy::too_many_arguments)]
    fn sweep(&mut self, path: &[(f64, f64)], y0: f64, w: f64, t: f64, r: f64, smooth: bool, mat: Material) {
        let section = rrect(t, w, r, if r > 0.0 { 2 } else { 0 });
        let frames: Vec<Frame> = (0..path.len())
            .map(|i| {
                let (p, q) = (path[i.saturating_sub(1)], path[(i + 1).min(path.len() - 1)]);
                let tan = unit([q.0 - p.0, 0.0, q.1 - p.1]);
                let side = [-tan[2], 0.0, tan[0]];
                Frame { o: [path[i].0, y0, path[i].1], u: side, v: [0.0, 1.0, 0.0], w: tan }
            })
            .collect();
        let rows: Vec<Vec<V3>> =
            frames.iter().map(|f| section.iter().map(|s| f.at(s.0, s.1, 0.0)).collect()).collect();
        self.grid(&rows, true, smooth, mat);
        let (a, b) = (frames[0], frames[frames.len() - 1]);
        self.cap(&a, &section, 0.0, -1.0, mat);
        self.cap(&b, &section, 0.0, 1.0, mat);
    }
}

/// The cushion: a squarish oval about the shell's size, a wall 18.5 mm wide round an opening
/// of 39 x 55 mm (measured on a photo of the inside).
const CUSH_W: f64 = 76.0;
const CUSH_H: f64 = 92.0;
const CUSH_WALL: f64 = 18.5;

/// The cup outline (aluminium shell), in millimetres.
const CUP_W: f64 = 78.0;
const CUP_H: f64 = 92.0;
const CUP_R: f64 = 22.0;
/// Distances from the centre along X: cushion tip, shell, cover. The stack is 40 mm (cushion
/// 18, shell 12.5, cover 9.5, from the side render) and the whole headset about 190 mm across
/// (the spec sheet gives 174 mm with the cups swivelled in).
const CUSHION_IN: f64 = 54.0;
const SHELL_IN: f64 = CUSHION_IN + 18.0;
const SHELL_OUT: f64 = SHELL_IN + 12.5;
const COVER_OUT: f64 = SHELL_OUT + 9.5;
/// The cover and plate, centred a little below the cup's centre.
const COVER_W: f64 = 57.0;
const COVER_H: f64 = 69.0;
const COVER_Z: f64 = -4.0;
const PLATE_OUT: f64 = SHELL_OUT + 5.5;
/// The headband: an arc through the sliders' centre line, `BAND_R` out from the middle and
/// `BAND_R * BAND_K` high, centred above the cups.
const BAND_R: f64 = SHELL_OUT;
const BAND_Z: f64 = 50.0;
/// Top of the headband: 97 mm above the cups, 189 mm overall as in the spec sheet.
const BAND_TOP: f64 = 143.0;
const BAND_K: f64 = (BAND_TOP - BAND_Z) / BAND_R;
/// Middle of the shell's thickness, where the controls sit on its edges.
const EDGE_X: f64 = (SHELL_IN + SHELL_OUT) / 2.0;

fn cup(b: &mut Builder, c: &Palette, side: f64, right: bool) {
    // Local frame: w outwards along X, u along Y (towards the face), v up.
    let f = Frame { o: [0.0; 3], u: [0.0, side, 0.0], v: [0.0, 0.0, 1.0], w: [side, 0.0, 0.0] };
    let n = 4;
    // The textured parts show the left cup's markings on the left one.
    let faced = |m: Material| match m.pattern {
        Pattern::Face { black, .. } => Material { pattern: Pattern::Face { black, left: !right }, ..m },
        Pattern::Plain => m,
    };
    // Cushion: a fat ring of leather, its outline a squarish oval, rolling over at the tip
    // into a deep opening that ends in a dark mesh (the profile is the tube's section).
    let tip = CUSHION_IN;
    let outline = |e: f64| oval(CUSH_W - 2.0 * e, CUSH_H - 2.0 * e, 24);
    // The section: the outer wall, then a half circle of radius `r` over the tip (so that
    // the cushion is round against the ear), then down the inner wall into the opening.
    let r = CUSH_WALL / 2.0;
    let mut rings = vec![(SHELL_IN, 1.0), (tip + r + 5.0, 0.0), (tip + r + 2.0, 0.2)];
    for i in 0..=8 {
        let t = std::f64::consts::PI * i as f64 / 8.0;
        rings.push((tip + r * (1.0 - t.sin()), r * (1.0 - t.cos())));
    }
    rings.extend([(tip + r + 2.0, CUSH_WALL), (tip + r + 4.0, CUSH_WALL)]);
    b.loft_with(&f, &outline, &rings, (false, false), true, c.cushion);
    b.cap(&f, &outline(CUSH_WALL), tip + r + 4.5, -1.0, c.lining);
    // Shell, with a bevel round the outer face.
    b.loft(
        &f,
        CUP_W,
        CUP_H,
        CUP_R,
        n,
        &[(SHELL_IN, 0.6), (SHELL_IN + 1.5, 0.0), (SHELL_OUT - 2.0, 0.0), (SHELL_OUT, 2.0)],
        (true, false),
        true,
        c.shell,
    );
    b.cap(&f, &rrect(CUP_W - 4.0, CUP_H - 4.0, CUP_R - 2.0, n), SHELL_OUT, 1.0, faced(c.face));

    // Under the cover: the plate, the raised frame, the two grilles.
    let cf = Frame { o: [0.0, 0.0, COVER_Z], ..f };
    b.loft(
        &cf,
        COVER_W - 2.0,
        COVER_H - 2.0,
        24.0,
        n,
        &[(SHELL_OUT, 0.0), (PLATE_OUT, 0.0)],
        (false, true),
        false,
        faced(c.plate),
    );
    let ff = Frame { o: [0.0, 0.0, -7.0], ..f };
    b.loft(
        &ff,
        24.0,
        46.0,
        6.0,
        1,
        &[(PLATE_OUT - 0.5, 0.0), (PLATE_OUT + 1.5, 0.0)],
        (false, true),
        false,
        faced(c.frame),
    );
    for z in [-7.0 + 8.5, -7.0 - 8.5] {
        let gf = Frame { o: [0.0, 0.0, z], ..f };
        b.loft(
            &gf,
            13.4,
            13.4,
            6.7,
            3,
            &[(PLATE_OUT + 1.0, 0.0), (PLATE_OUT + 1.7, 0.0)],
            (false, true),
            false,
            faced(c.grille),
        );
    }
    // The clear cover: straight sides, a rounded outer edge, a flat top.
    b.loft(
        &cf,
        COVER_W,
        COVER_H,
        25.0,
        n + 1,
        &[(SHELL_OUT, 0.0), (COVER_OUT - 2.5, 0.0), (COVER_OUT - 0.7, 0.8), (COVER_OUT, 2.5)],
        (false, true),
        true,
        c.cover,
    );

    if right {
        // Button: top front of the outer face.
        let bf = Frame { o: [0.0, 27.0, 29.5], u: [0.0, 1.0, 0.0], ..f };
        b.loft(
            &bf,
            6.0,
            6.0,
            3.0,
            2,
            &[(SHELL_OUT - 1.0, 0.0), (SHELL_OUT + 1.2, 0.0), (SHELL_OUT + 1.6, 0.8)],
            (false, true),
            true,
            c.control,
        );
        // Roller and paddle on the back edge (-Y), facing backwards.
        let back = |z: f64, d: f64| Frame {
            o: [side * d, -CUP_W / 2.0, z],
            u: [side, 0.0, 0.0],
            v: [0.0, 0.0, 1.0],
            w: [0.0, -1.0, 0.0],
        };
        b.loft(
            &back(18.0, EDGE_X),
            8.0,
            13.0,
            4.0,
            2,
            &[(-1.0, 0.0), (1.4, 0.0), (1.8, 0.7)],
            (false, true),
            true,
            c.control,
        );
        b.loft(
            &back(-4.0, EDGE_X),
            3.0,
            13.5,
            1.2,
            1,
            &[(-1.0, 0.0), (1.2, 0.0)],
            (false, true),
            false,
            c.paddle,
        );
        // Ports on the bottom edge: USB-C towards the front, then the jack.
        let bottom = |y: f64| Frame {
            o: [side * EDGE_X, y, -CUP_H / 2.0],
            u: [side, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            w: [0.0, 0.0, -1.0],
        };
        b.loft(&bottom(10.0), 3.2, 9.0, 1.5, 1, &[(-0.5, 0.0), (0.15, 0.0)], (false, true), false, c.port);
        b.loft(&bottom(24.0), 3.6, 3.6, 1.8, 2, &[(-0.5, 0.0), (0.15, 0.0)], (false, true), false, c.port);
    }

    // The strap: from the slider's pivot down, out over the cup, down into the cover's top.
    let top = COVER_Z + COVER_H / 2.0;
    let d0 = BAND_R;
    let d1 = (SHELL_OUT + COVER_OUT) / 2.0 + 1.0;
    let path: Vec<(f64, f64)> = [
        (d0, BAND_Z - 2.0),
        (d0, BAND_Z - 5.0),
        (d0 + 4.0, BAND_Z - 9.0),
        (d1 - 4.0, BAND_Z - 9.0),
        (d1, BAND_Z - 13.0),
        (d1, top - 3.0),
    ]
    .iter()
    .map(|&(d, z)| (side * d, z))
    .collect();
    b.sweep(&path, 0.0, 7.5, 2.4, 0.0, false, c.strap);
    // The pivot: a short cone under the slider.
    let pf =
        Frame { o: [side * d0, 0.0, BAND_Z], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], w: [0.0, 0.0, -1.0] };
    b.loft(&pf, 9.0, 9.0, 4.5, 1, &[(0.0, 0.0), (3.0, 1.5), (4.5, 2.5)], (false, true), true, c.slider);
}

pub fn headset(edition: Edition) -> Vec<Tri> {
    let c = palette(edition);
    let mut b = Builder { tris: Vec::new() };
    cup(&mut b, &c, RIGHT, true);
    cup(&mut b, &c, -RIGHT, false);

    // The headband: a flat band on a half circle, sliders at both ends, a pad under the top.
    let arc = |t0: f64, t1: f64, r: f64, n: usize| -> Vec<(f64, f64)> {
        (0..=n)
            .map(|i| {
                let t = t0 + (t1 - t0) * i as f64 / n as f64;
                (r * t.cos(), BAND_Z + r * BAND_K * t.sin())
            })
            .collect()
    };
    let pi = std::f64::consts::PI;
    let slider = 0.42;
    b.sweep(&arc(slider - 0.08, pi - slider + 0.08, BAND_R, 16), 0.0, 21.0, 7.5, 3.0, true, c.band);
    for (t0, t1) in [(0.0, slider), (pi - slider, pi)] {
        b.sweep(&arc(t0, t1, BAND_R, 3), 0.0, 25.0, 11.0, 2.5, true, c.slider);
    }
    // The pad: thickest in the middle, tapering to both ends.
    let (span, segs) = (0.5, 10);
    let rows: Vec<Vec<V3>> = (0..=segs)
        .map(|i| {
            let t = pi / 2.0 + span * (2.0 * i as f64 / segs as f64 - 1.0);
            let k = 1.0 - (2.0 * i as f64 / segs as f64 - 1.0).powi(4);
            let thick = 1.0 + 8.0 * k.sqrt();
            let (ct, st) = (t.cos(), t.sin());
            let r0 = BAND_R - 3.75;
            rrect(thick, 17.0, (thick / 2.0).min(4.0), 2)
                .iter()
                .map(|&(a, y)| {
                    let r = r0 - thick / 2.0 + a;
                    [r * ct, y, BAND_Z + r * BAND_K * st]
                })
                .collect()
        })
        .collect();
    b.grid(&rows, true, true, c.pad);
    b.tris
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(tris: &[Tri]) -> (V3, V3) {
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for t in tris {
            for v in t.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        (lo, hi)
    }

    #[test]
    fn proportions_and_budget() {
        for e in [Edition::White, Edition::Black] {
            let tris = headset(e);
            // PS2 territory: a few thousand triangles.
            assert!((800..3600).contains(&tris.len()), "{}", tris.len());
            let (lo, hi) = bounds(&tris);
            // Ear to ear: 188 mm across the covers; the cups 78 mm deep along Y.
            assert!(
                (hi[0] - COVER_OUT * MM).abs() < 1e-6 && (lo[0] + COVER_OUT * MM).abs() < 1e-6,
                "{lo:?} {hi:?}"
            );
            assert!((hi[1] - lo[1] - CUP_W * MM).abs() < 0.05, "{lo:?} {hi:?}");
            assert!((2.0 * COVER_OUT - 188.0).abs() < 1.0);
            // Cups centred on the ears, headband top about 97 mm above the cups.
            assert!((lo[2] + CUP_H / 2.0 * MM).abs() < 0.02, "{lo:?}");
            let above = (hi[2] - CUP_H / 2.0 * MM) / MM;
            assert!((90.0..130.0).contains(&above), "{above}");
        }
    }

    #[test]
    fn mirror_symmetric_but_for_the_controls() {
        // Every left-cup point has a right-cup twin, except the parts only the right one has.
        let tris = headset(Edition::White);
        let key = |v: V3| {
            (
                (v[0] / MM * 4.0).round() as i64,
                (v[1] / MM * 4.0).round() as i64,
                (v[2] / MM * 4.0).round() as i64,
            )
        };
        let right: std::collections::HashSet<_> =
            tris.iter().flat_map(|t| t.v).filter(|v| v[0] > 0.0).map(key).collect();
        let left: Vec<_> = tris.iter().flat_map(|t| t.v).filter(|v| v[0] < 0.0).map(key).collect();
        let missing = left.iter().filter(|k| !right.contains(&(-k.0, k.1, k.2))).count();
        assert_eq!(missing, 0);
    }

    #[test]
    fn the_controls_tell_the_sides_apart() {
        let c = palette(Edition::Black);
        let tris = headset(Edition::Black);
        let side = |m: Material| {
            tris.iter().filter(move |t| t.mat == m).flat_map(|t| t.v).map(|v| v[0].signum() * RIGHT)
        };
        assert!(side(c.paddle).all(|s| s > 0.0));
        let face = |left: bool| Material { pattern: Pattern::Face { black: true, left }, ..c.face };
        assert!(side(face(false)).all(|s| s > 0.0) && side(face(true)).all(|s| s < 0.0));
        assert!(side(c.control).all(|s| s > 0.0));
        // Normals have unit length.
        assert!(tris.iter().flat_map(|t| t.n).all(|n| (dot(n, n) - 1.0).abs() < 1e-6));
    }
}
