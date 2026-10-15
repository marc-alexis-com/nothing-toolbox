//! A small software renderer in the spirit of a PS2 game, for `headtrack --viz` in kitty: the
//! low-poly headset (`lowpoly.rs`) lit per pixel, with an environment-mapped aluminium, a
//! see-through cover, a mirror floor with a projected shadow, rendered at a low resolution
//! and quantised to 15-bit colour with ordered dithering.
//!
//! Spaces: the model is in the head-tracking frame (`model.rs`), turned by the pose, then by
//! the orbit camera into view space: x right, y into the screen, z up.

use crate::headtrack::{mat_from_quat, matmul, Mat3, Quat};
use crate::kitty::Rgb8;
use crate::lowpoly::{self, Material, Pattern, Tri};
use crate::model::Edition;
use crate::texture;
use crate::viz;
use std::sync::OnceLock;

type V = [f32; 3];

/// Camera distance, as in the terminal view: a mild perspective.
const DISTANCE: f32 = 8.0;
/// Height of the picture's centre.
const LIFT: f32 = 0.35;
/// The scene spans about this many model units across the shorter side of the picture.
const SPAN: f32 = 5.0;
/// The floor, under the cups, and its radius.
const FLOOR_Z: f32 = -0.85;
const FLOOR_R: f32 = 3.2;
/// Key light, towards the light (world space: from the upper front left of the viewer).
const KEY: V = [-0.40, -0.65, 0.65];
const FILL: V = [0.75, 0.35, 0.10];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: V) -> V {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn mix(a: V, b: V, t: f32) -> V {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mat32(m: &Mat3) -> [[f32; 3]; 3] {
    m.map(|r| r.map(|v| v as f32))
}

fn apply(m: &[[f32; 3]; 3], p: V) -> V {
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2])
}

/// sRGB <-> linear, close enough for shading.
fn linear(c: V) -> V {
    c.map(|v| v.max(0.0).powf(2.2))
}

fn mesh(edition: Edition) -> &'static [Tri] {
    static WHITE: OnceLock<Vec<Tri>> = OnceLock::new();
    static BLACK: OnceLock<Vec<Tri>> = OnceLock::new();
    match edition {
        Edition::White => WHITE.get_or_init(|| lowpoly::headset(Edition::White)),
        Edition::Black => BLACK.get_or_init(|| lowpoly::headset(Edition::Black)),
    }
}

/// A vertex on screen: position, 1/w for perspective-correct interpolation, and attributes
/// (view-space normal, then a free position used by the floor and the procedural detail).
#[derive(Clone, Copy)]
struct Vert {
    x: f32,
    y: f32,
    inv_w: f32,
    a: [f32; 6],
}

/// Fills a triangle: `f(index, inv_w, attributes)` for each covered pixel centre.
fn raster(v: &[Vert; 3], w: usize, h: usize, mut f: impl FnMut(usize, f32, [f32; 6])) {
    let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
    if area.abs() < 1e-6 || !area.is_finite() {
        return;
    }
    let min = |k: fn(&Vert) -> f32| v.iter().map(k).fold(f32::MAX, f32::min);
    let max = |k: fn(&Vert) -> f32| v.iter().map(k).fold(f32::MIN, f32::max);
    let x0 = min(|p| p.x).floor().max(0.0) as usize;
    let x1 = (max(|p| p.x).ceil().max(0.0) as usize).min(w);
    let y0 = min(|p| p.y).floor().max(0.0) as usize;
    let y1 = (max(|p| p.y).ceil().max(0.0) as usize).min(h);
    let pre: [[f32; 6]; 3] = v.map(|p| p.a.map(|a| a * p.inv_w));
    for y in y0..y1 {
        let fy = y as f32 + 0.5;
        for x in x0..x1 {
            let fx = x as f32 + 0.5;
            let e = |a: &Vert, b: &Vert| (b.x - a.x) * (fy - a.y) - (fx - a.x) * (b.y - a.y);
            let b0 = e(&v[1], &v[2]) / area;
            let b1 = e(&v[2], &v[0]) / area;
            let b2 = e(&v[0], &v[1]) / area;
            if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                continue;
            }
            let iw = b0 * v[0].inv_w + b1 * v[1].inv_w + b2 * v[2].inv_w;
            let a = [0, 1, 2, 3, 4, 5].map(|k| (b0 * pre[0][k] + b1 * pre[1][k] + b2 * pre[2][k]) / iw);
            f(y * w + x, iw, a);
        }
    }
}

/// The camera: the orbit of the terminal view, the projection, and the lights in view space.
struct Camera {
    view: [[f32; 3]; 3],
    w: usize,
    h: usize,
    s: f32,
    key: V,
    fill: V,
    up: V,
}

impl Camera {
    fn new(w: usize, h: usize, az: f64, el: f64) -> Camera {
        let view = mat32(&viz::view_matrix(az, el));
        Camera {
            view,
            w,
            h,
            s: w.min(h) as f32 / SPAN,
            key: unit(apply(&view, KEY)),
            fill: unit(apply(&view, FILL)),
            up: apply(&view, [0.0, 0.0, 1.0]),
        }
    }

    /// View space -> screen.
    fn project(&self, p: V, a: [f32; 6]) -> Vert {
        let inv_w = DISTANCE / (DISTANCE + p[1]).max(0.5);
        Vert {
            x: self.w as f32 / 2.0 + p[0] * inv_w * self.s,
            y: self.h as f32 / 2.0 - (p[2] - LIFT) * inv_w * self.s,
            inv_w,
            a,
        }
    }
}

/// The studio around the headset, as seen in a reflection along `r` (view space, `up` is the
/// world's up): two soft boxes above, a bright horizon, a dark floor. This is what makes the
/// aluminium read as metal.
fn environment(r: V, cam: &Camera) -> V {
    let up = dot(r, cam.up);
    let sky = mix([0.30, 0.34, 0.42], [0.85, 0.88, 0.95], up.clamp(0.0, 1.0).powf(0.6));
    let ground = mix([0.10, 0.10, 0.12], [0.03, 0.03, 0.04], (-up).clamp(0.0, 1.0).sqrt());
    let mut c = if up >= 0.0 { sky } else { ground };
    // Horizon line, then a soft box towards the key light.
    c = add(c, scale([1.0, 0.97, 0.92], (1.0 - (up * 9.0).abs()).max(0.0) * 0.7));
    let k = dot(r, cam.key).max(0.0).powf(12.0);
    add(c, scale([1.2, 1.15, 1.05], k))
}

/// Lit colour (linear) and opacity of a surface point. `n` faces the camera, `p` is the
/// point in the model.
fn shade(m: &Material, n: V, p: V, cam: &Camera) -> (V, f32) {
    let eye = [0.0, -1.0, 0.0];
    let base = match m.pattern {
        Pattern::Plain => linear(m.rgb),
        Pattern::Face { black, left } => linear(texture::face(black, left, p[1] * 90.0, p[2] * 90.0)),
    };
    if m.glow {
        return (base, 1.0);
    }
    let hemi = 0.5 + 0.5 * dot(n, cam.up);
    let ambient = mix([0.10, 0.10, 0.12], [0.30, 0.32, 0.38], hemi);
    let diffuse = add(
        ambient,
        add(
            scale([1.0, 0.97, 0.92], dot(n, cam.key).max(0.0)),
            scale([0.30, 0.36, 0.48], dot(n, cam.fill).max(0.0)),
        ),
    );
    let half = unit(add(cam.key, eye));
    let spec = m.spec * dot(n, half).max(0.0).powf(m.shine);
    let facing = dot(n, eye).max(0.0);
    let rim = (1.0 - facing).powi(3) * 0.35;
    let mut c = [0, 1, 2].map(|k| base[k] * diffuse[k] + spec + rim * 0.5);
    if m.metal > 0.0 {
        // Reflect the eye ray about the normal.
        let d = [0.0, 1.0, 0.0];
        let r = sub(d, scale(n, 2.0 * dot(d, n)));
        let env = linear(environment(r, cam));
        c = mix(c, [0, 1, 2].map(|k| env[k] * base[k] * 1.3 + spec), m.metal);
    }
    let alpha = if m.alpha < 1.0 { m.alpha + (1.0 - m.alpha) * (1.0 - facing).powi(2) * 0.8 } else { 1.0 };
    (c, alpha)
}

/// Fixed backdrop: a vertical gradient, darker at the bottom, with a soft vignette.
fn backdrop(x: usize, y: usize, w: usize, h: usize) -> V {
    let t = y as f32 / h.max(1) as f32;
    let top = [0.165, 0.185, 0.235];
    let bottom = [0.040, 0.044, 0.056];
    let dx = x as f32 / w.max(1) as f32 - 0.5;
    let v = 1.0 - 0.35 * (dx * dx * 2.0);
    linear(mix(top, bottom, t.powf(0.8))).map(|c| c * v)
}

/// 4x4 Bayer thresholds for the ordered dither.
const BAYER: [[f32; 4]; 4] =
    [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];

/// Quantises to 5 bits per channel (the PS2's 16-bit frame buffer), dithered.
fn quantise(c: V, x: usize, y: usize) -> [u8; 3] {
    let t = (BAYER[y % 4][x % 4] + 0.5) / 16.0;
    c.map(|v| {
        let s = v.max(0.0).powf(1.0 / 2.2).min(1.0) * 31.0;
        let q = (s.floor() + if s.fract() > t { 1.0 } else { 0.0 }).min(31.0);
        (q * 255.0 / 31.0).round() as u8
    })
}

/// A model triangle in view space: corners, vertex normals, the corners in the model (for
/// the procedural detail), material.
type ViewTri<'a> = ([V; 3], [V; 3], [V; 3], &'a Material);

/// The mesh turned by `m` (pose then camera), optionally mirrored about the floor.
fn transformed<'a>(m: &[[f32; 3]; 3], mesh: &'a [Tri], cam: &Camera, mirror: bool) -> Vec<ViewTri<'a>> {
    // The floor is horizontal in the world: in view space, the plane through the floor's
    // centre with normal `up`.
    let floor = apply(&cam.view, [0.0, 0.0, FLOOR_Z]);
    let point = |p: V| if mirror { sub(p, scale(cam.up, 2.0 * dot(sub(p, floor), cam.up))) } else { p };
    let normal = |n: V| if mirror { sub(n, scale(cam.up, 2.0 * dot(n, cam.up))) } else { n };
    mesh.iter()
        .map(|t| {
            let model = t.v.map(|p| p.map(|c| c as f32));
            let v = model.map(|p| point(apply(m, p)));
            let n = t.n.map(|p| normal(apply(m, p.map(|c| c as f32))));
            (v, n, model, &t.mat)
        })
        .collect()
}

pub struct Frame {
    color: Vec<V>,
    depth: Vec<f32>,
}

/// Draws the opaque triangles of `tris` with a depth test into `fb`, then the see-through
/// ones from back to front, blended.
fn draw_model(tris: &[ViewTri], cam: &Camera, fb: &mut Frame) {
    let draw = |v: &[V; 3], n: &[V; 3], p: &[V; 3], mat: &Material, fb: &mut Frame| {
        // Two-sided: turn the normals towards the camera when the face looks away. Judged
        // from the normals, not the winding, which the mirror views reverse.
        let flip = dot(add(add(n[0], n[1]), n[2]), [0.0, -1.0, 0.0]) < 0.0;
        let verts = [0, 1, 2].map(|i| {
            let nn = if flip { scale(n[i], -1.0) } else { n[i] };
            cam.project(v[i], [nn[0], nn[1], nn[2], p[i][0], p[i][1], p[i][2]])
        });
        raster(&verts, cam.w, cam.h, |i, iw, a| {
            if iw <= fb.depth[i] {
                return;
            }
            let (c, alpha) = shade(mat, unit([a[0], a[1], a[2]]), [a[3], a[4], a[5]], cam);
            if alpha >= 1.0 {
                fb.depth[i] = iw;
                fb.color[i] = c;
            } else {
                fb.color[i] = mix(fb.color[i], c, alpha);
            }
        });
    };
    for (v, n, p, mat) in tris.iter().filter(|t| t.3.alpha >= 1.0) {
        draw(v, n, p, mat, fb);
    }
    let mut glass: Vec<_> = tris.iter().filter(|t| t.3.alpha < 1.0).collect();
    glass.sort_by(|a, b| {
        let d = |t: &&ViewTri| t.0.iter().map(|p| p[1]).sum::<f32>();
        d(b).total_cmp(&d(a))
    });
    for (v, n, p, mat) in glass {
        draw(v, n, p, mat, fb);
    }
}

/// Renders the headset in pose `q` (the head-tracking frame) for an orbit camera, `w` x `h`.
pub fn render(q: Quat, edition: Edition, w: usize, h: usize, az: f64, el: f64) -> Rgb8 {
    let cam = Camera::new(w, h, az, el);
    let model = matmul(&viz::view_matrix(az, el), &mat_from_quat(q));
    let m = mat32(&model);
    let mut fb =
        Frame { color: (0..w * h).map(|i| backdrop(i % w, i / w, w, h)).collect(), depth: vec![0.0; w * h] };

    // Mirror floor, while the camera is above it.
    let eye_z = -DISTANCE * cam.view[1][2];
    if eye_z > FLOOR_Z + 0.3 {
        let fade = ((eye_z - FLOOR_Z - 0.3) / 0.8).min(1.0);
        // The reflection: the model mirrored about the floor, in its own buffer.
        let mut refl = Frame { color: fb.color.clone(), depth: vec![0.0; w * h] };
        let tris = transformed(&m, mesh(edition), &cam, true);
        draw_model(&tris, &cam, &mut refl);

        // The floor: a disc of quads, faded into the backdrop at its rim.
        let (rings, segs) = (6, 40);
        let at = |r: f32, t: f32| {
            let p = [r * t.cos(), r * t.sin(), FLOOR_Z];
            let v = apply(&cam.view, p);
            cam.project(v, [0.0, 0.0, 0.0, p[0], p[1], 0.0])
        };
        let mut shadow = vec![false; w * h];
        let mut floor_px = vec![false; w * h];
        for i in 0..rings {
            for j in 0..segs {
                let (r0, r1) = (FLOOR_R * i as f32 / rings as f32, FLOOR_R * (i + 1) as f32 / rings as f32);
                let (t0, t1) = (
                    std::f32::consts::TAU * j as f32 / segs as f32,
                    std::f32::consts::TAU * (j + 1) as f32 / segs as f32,
                );
                for tri in [[at(r0, t0), at(r1, t0), at(r1, t1)], [at(r0, t0), at(r1, t1), at(r0, t1)]] {
                    raster(&tri, w, h, |i, iw, _| {
                        fb.depth[i] = iw;
                        floor_px[i] = true;
                    });
                }
            }
        }
        // The shadow: the model squashed onto the floor along the key light.
        let light = KEY;
        for t in mesh(edition) {
            if t.mat.alpha < 1.0 {
                continue;
            }
            let v = t.v.map(|p| {
                let p = apply(&mat32(&mat_from_quat(q)), p.map(|c| c as f32));
                let k = (p[2] - FLOOR_Z) / light[2];
                let g = [p[0] - light[0] * k * 0.5, p[1] - light[1] * k * 0.5, FLOOR_Z];
                cam.project(apply(&cam.view, g), [0.0; 6])
            });
            raster(&v, w, h, |i, _, _| shadow[i] = true);
        }
        // Shade the floor pixels: reflection, tint, grid, shadow, fade.
        let floor_rgb = linear([0.075, 0.080, 0.095]);
        let grid_rgb = linear([0.16, 0.20, 0.27]);
        let mut world = vec![[0.0f32; 2]; w * h];
        for i in 0..rings {
            for j in 0..segs {
                let (r0, r1) = (FLOOR_R * i as f32 / rings as f32, FLOOR_R * (i + 1) as f32 / rings as f32);
                let (t0, t1) = (
                    std::f32::consts::TAU * j as f32 / segs as f32,
                    std::f32::consts::TAU * (j + 1) as f32 / segs as f32,
                );
                for tri in [[at(r0, t0), at(r1, t0), at(r1, t1)], [at(r0, t0), at(r1, t1), at(r0, t1)]] {
                    raster(&tri, w, h, |i, _, a| world[i] = [a[3], a[4]]);
                }
            }
        }
        for i in 0..w * h {
            if !floor_px[i] {
                continue;
            }
            let [x, y] = world[i];
            let r = (x * x + y * y).sqrt() / FLOOR_R;
            let line = |v: f32| {
                let f = (v / 0.5).rem_euclid(1.0);
                f < 0.04 || f > 0.96
            };
            let mut c = mix(refl.color[i], floor_rgb, 0.55 + 0.35 * r);
            if line(x) || line(y) {
                c = mix(c, grid_rgb, 0.8 * (1.0 - r));
            }
            if shadow[i] {
                c = scale(c, 0.35 + 0.4 * r);
            }
            let edge = ((1.0 - r) / 0.35).clamp(0.0, 1.0) * fade;
            fb.color[i] = mix(fb.color[i], c, edge);
        }
    }

    let tris = transformed(&m, mesh(edition), &cam, false);
    draw_model(&tris, &cam, &mut fb);

    let mut px = Vec::with_capacity(w * h * 3);
    for (i, c) in fb.color.iter().enumerate() {
        px.extend(quantise(*c, i % w, i / w));
    }
    Rgb8 { w, h, px }
}

/// Scales a picture up by a whole factor, each pixel becoming a `k` x `k` block, so that the
/// terminal (which smooths what it scales) has almost nothing left to scale.
pub fn upscale(img: &Rgb8, k: usize) -> Rgb8 {
    if k <= 1 {
        return Rgb8 { w: img.w, h: img.h, px: img.px.clone() };
    }
    let (w, h) = (img.w * k, img.h * k);
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..img.h {
        let start = px.len();
        for x in 0..img.w {
            let p = &img.px[(y * img.w + x) * 3..][..3];
            for _ in 0..k {
                px.extend_from_slice(p);
            }
        }
        for _ in 1..k {
            px.extend_from_within(start..start + w * 3);
        }
    }
    Rgb8 { w, h, px }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zlib;

    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    /// A PNG file of `img`, to look at the renderer's output.
    pub fn png(img: &Rgb8) -> Vec<u8> {
        let mut raw = Vec::with_capacity((img.w * 3 + 1) * img.h);
        for row in img.px.chunks(img.w * 3) {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend(&body);
            out.extend(crc32(&body).to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend((img.w as u32).to_be_bytes());
        ihdr.extend((img.h as u32).to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        chunk(b"IDAT", &zlib::compress(&raw, img.w * 3 + 1));
        chunk(b"IEND", &[]);
        out
    }

    /// Poses from the real recording: (t, quaternion).
    fn recording() -> Vec<(f64, Quat)> {
        include_str!("testdata/b170-gestures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .map(|l| {
                let f: Vec<f64> = l.split_whitespace().map(|v| v.parse().unwrap()).collect();
                (f[0], [f[4], f[5], f[6], f[7]])
            })
            .collect()
    }

    fn red_columns(img: &Rgb8) -> Vec<usize> {
        img.px
            .chunks(3)
            .enumerate()
            .filter(|(_, c)| c[0] > 150 && c[1] < 90 && c[2] < 90)
            .map(|(i, _)| i % img.w)
            .collect()
    }

    #[test]
    fn the_right_cup_shows_from_behind() {
        // Default camera, head at rest: the red paddle on the back of the right cup is on the
        // right half of the picture, and nothing red on the left.
        for ed in [Edition::White, Edition::Black] {
            let img =
                render([0.0, 0.0, 0.0, 1.0], ed, 160, 120, viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION);
            let red = red_columns(&img);
            assert!(!red.is_empty() && red.iter().all(|&x| x > img.w / 2), "{red:?}");
        }
    }

    #[test]
    fn pose_and_camera_change_the_picture_and_it_stays_in_frame() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let a = render(id, Edition::White, 120, 90, 0.0, 0.0);
        assert_eq!(a.px.len(), 120 * 90 * 3);
        assert_ne!(a.px, render([0.259, 0.0, 0.0, 0.966], Edition::White, 120, 90, 0.0, 0.0).px);
        assert_ne!(a.px, render(id, Edition::White, 120, 90, 40.0, 30.0).px);
        let _ = render(id, Edition::Black, 3, 2, 0.0, 85.0);
    }

    #[test]
    fn upscale_repeats_pixels() {
        let img = Rgb8 { w: 2, h: 1, px: vec![1, 2, 3, 4, 5, 6] };
        let big = upscale(&img, 2);
        assert_eq!((big.w, big.h), (4, 2));
        assert_eq!(big.px, [1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6, 1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6]);
    }

    /// Writes previews to $NOTHING_PREVIEW_DIR: both editions, several cameras, and poses
    /// replayed from the recording. `cargo test --release previews -- --ignored`.
    #[test]
    #[ignore]
    fn previews() {
        let dir = std::env::var("NOTHING_PREVIEW_DIR").expect("NOTHING_PREVIEW_DIR");
        let id = [0.0, 0.0, 0.0, 1.0];
        let rec = recording();
        let at = |t: f64| rec.iter().min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs())).unwrap().1;
        let (w, h) = (std::env::var("W").map(|v| v.parse().unwrap()).unwrap_or(320), 240);
        for (name, ed) in [("white", Edition::White), ("black", Edition::Black)] {
            for (view, q, az, el) in [
                ("default-behind", id, viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION),
                ("behind-above", id, 0.0, 25.0),
                ("three-quarter", id, -35.0, 20.0),
                ("right", id, -90.0, 5.0),
                ("left", id, 90.0, 5.0),
                ("front", id, 180.0, 10.0),
                ("top", id, 0.0, 80.0),
                ("below", id, -30.0, -30.0),
                ("nod", at(5.6), viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION),
                ("shake", at(18.5), viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION),
                ("tilt", at(28.0), -35.0, 20.0),
            ] {
                let img = render(q, ed, w, h, az, el);
                std::fs::write(format!("{dir}/{name}-{view}.png"), png(&upscale(&img, 2))).unwrap();
            }
        }
    }
}
