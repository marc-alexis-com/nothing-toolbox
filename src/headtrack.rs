//! Head-tracking math (quaternion -> Euler angles, recentring) and nod/shake detection.

use std::collections::VecDeque;

pub type Quat = [f64; 4]; // x, y, z, w
pub type Mat3 = [[f64; 3]; 3];

pub fn mat_from_quat(q: Quat) -> Mat3 {
    let [x, y, z, w] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

pub fn matmul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// (yaw, pitch, roll) in degrees, ZYX convention: yaw about Z, pitch about Y, roll about X.
pub fn euler_deg(r: &Mat3) -> (f64, f64, f64) {
    let pitch = (-r[2][0]).clamp(-1.0, 1.0).asin().to_degrees();
    let yaw = r[1][0].atan2(r[0][0]).to_degrees();
    let roll = r[2][1].atan2(r[2][2]).to_degrees();
    (yaw, pitch, roll)
}

/// `a - b` folded into (-180, 180]: angle difference without the ±180 jump.
pub fn wrapdiff(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

pub fn norm(q: Quat) -> f64 {
    q.iter().map(|v| v * v).sum::<f64>().sqrt()
}

fn conj(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

fn mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Rotation of `q` relative to the reference pose `reference` (which becomes "straight").
pub fn recenter(reference: Quat, q: Quat) -> Quat {
    let r = mul(conj(reference), q);
    let n = norm(r);
    if n == 0.0 {
        r
    } else {
        r.map(|v| v / n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Nod,
    Shake,
}

/// Thresholds for one axis.
#[derive(Clone, Copy, Debug)]
pub struct AxisTuning {
    /// Minimum peak-to-peak amplitude in the window, degrees.
    pub amplitude: f64,
    /// Angular speed to exceed in BOTH directions, degrees per second.
    pub speed: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub nod: AxisTuning,
    pub shake: AxisTuning,
    /// Sliding window, seconds.
    pub window: f64,
    /// The head must be back within this many degrees of neutral when the gesture ends.
    pub return_tolerance: f64,
    /// Below this speed (deg/s) on both axes the head counts as still.
    pub still_speed: f64,
    /// Minimum time between two gestures, seconds.
    pub refractory: f64,
    /// Time the head must stay neutral and still before re-arming, seconds.
    pub calm_hold: f64,
    /// Neutral pose adaptation rate while still.
    pub base_alpha: f64,
    /// The gesture axis must swing at least this many times more than the other one.
    pub axis_ratio: f64,
    /// Exponential smoothing factor of the input angles.
    pub smoothing: f64,
    /// Time constant of the drift estimate, seconds. The headset's orientation can drift
    /// steadily (21 deg/s around Z was measured once); a slowly learnt per-axis rate is
    /// subtracted, so that a drifting head still looks still.
    pub drift_tau: f64,
    /// Rates further than this from the current drift estimate (deg/s) are motion, not
    /// drift, and are not learnt.
    pub drift_band: f64,
    /// Samples needed in the window before deciding anything.
    pub min_samples: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            // Measured on a B170: nods swing 20-80 deg at over 100 deg/s on the X axis,
            // shakes about as much on Z. Slow head tilts stay under 20 deg/s, so the speed
            // gate, not the amplitude, is what rejects them; a short nod, once smoothed,
            // can measure as little as 12 deg.
            nod: AxisTuning { amplitude: 10.0, speed: 45.0 },
            shake: AxisTuning { amplitude: 15.0, speed: 45.0 },
            window: 1.3,
            return_tolerance: 9.0,
            still_speed: 15.0,
            refractory: 0.5,
            calm_hold: 0.35,
            base_alpha: 0.08,
            axis_ratio: 1.8,
            smoothing: 0.5,
            drift_tau: 3.0,
            drift_band: 40.0,
            min_samples: 5,
        }
    }
}

/// Per-sample diagnostics, for tuning (`gestures --debug`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Diag {
    pub nod_swing: f64,
    pub shake_swing: f64,
    pub nod_speed: (f64, f64),
    pub shake_speed: (f64, f64),
    /// Current drift estimate (nod axis, shake axis), deg/s.
    pub drift: (f64, f64),
    pub armed: bool,
}

/// Nod/shake detector on head angles sampled at ~9 Hz.
///
/// Axes, measured on a B170: a nod is a rotation about the stream's X axis (the roll
/// angle of `euler_deg`), a shake a rotation about Z (yaw).
///
/// A gesture fires when, over the sliding window, its axis shows a peak-to-peak swing above
/// `amplitude`, a speed above `speed` in both directions (a real back-and-forth: the main
/// guard against false positives), a return near neutral, and dominance over the other axis.
/// After firing, the detector disarms until the head is neutral and still for `calm_hold`.
/// The neutral pose slowly follows the head while it is still, absorbing drift.
pub struct Detector {
    pub tuning: Tuning,
    samples: VecDeque<(f64, f64, f64)>, // (t, smoothed nod angle, smoothed shake angle)
    base: Option<(f64, f64)>,
    last: Option<(f64, f64)>,
    /// Previous raw input (t, nod, shake), drift estimate and drift-free angles.
    raw: Option<(f64, f64, f64)>,
    drift: (f64, f64),
    unwound: (f64, f64),
    last_fire: f64,
    armed: bool,
    calm_since: Option<f64>,
    pub diag: Diag,
}

impl Detector {
    pub fn new(tuning: Tuning) -> Detector {
        Detector {
            tuning,
            samples: VecDeque::new(),
            base: None,
            last: None,
            raw: None,
            drift: (0.0, 0.0),
            unwound: (0.0, 0.0),
            last_fire: f64::NEG_INFINITY,
            armed: true,
            calm_since: None,
            diag: Diag::default(),
        }
    }

    pub fn reset(&mut self) {
        *self = Detector::new(self.tuning);
    }

    /// Removes the learnt drift from the raw angles; returns drift-free angles.
    fn undrift(&mut self, t: f64, nod: f64, shake: f64) -> (f64, f64) {
        let tu = self.tuning;
        match self.raw {
            None => self.unwound = (nod, shake),
            Some((t0, n0, s0)) => {
                let dt = t - t0;
                let (dn, ds) = (wrapdiff(nod, n0), wrapdiff(shake, s0));
                if dt > 1e-4 {
                    let a = (dt / tu.drift_tau).min(1.0);
                    let (vn, vs) = (dn / dt, ds / dt);
                    if (vn - self.drift.0).abs() < tu.drift_band && (vs - self.drift.1).abs() < tu.drift_band
                    {
                        self.drift.0 += a * (vn - self.drift.0);
                        self.drift.1 += a * (vs - self.drift.1);
                    }
                }
                self.unwound.0 += dn - self.drift.0 * dt.max(0.0);
                self.unwound.1 += ds - self.drift.1 * dt.max(0.0);
            }
        }
        self.raw = Some((t, nod, shake));
        self.unwound
    }

    /// Feeds one sample: `nod` and `shake` are the head angles about the nod and shake
    /// axes, in degrees. Returns a gesture when one completes.
    pub fn push(&mut self, t: f64, nod: f64, shake: f64) -> Option<Gesture> {
        let tu = self.tuning;
        let (pitch, yaw) = self.undrift(t, nod, shake);
        let (p, y) = match self.last {
            None => (pitch, yaw),
            Some((lp, ly)) => {
                (lp + tu.smoothing * wrapdiff(pitch, lp), ly + tu.smoothing * wrapdiff(yaw, ly))
            }
        };
        self.last = Some((p, y));
        self.samples.push_back((t, p, y));
        while self.samples.front().is_some_and(|s| t - s.0 > tu.window) {
            self.samples.pop_front();
        }
        let (mut bp, mut by) = *self.base.get_or_insert((p, y));

        let (mut vp, mut vy) = (0.0, 0.0);
        if self.samples.len() >= 2 {
            let (t0, p0, y0) = self.samples[self.samples.len() - 2];
            let dt = t - t0;
            if dt > 1e-4 {
                vp = wrapdiff(p, p0) / dt;
                vy = wrapdiff(y, y0) / dt;
            }
        }
        let still = vp.abs() < tu.still_speed && vy.abs() < tu.still_speed;
        if still {
            bp += tu.base_alpha * wrapdiff(p, bp);
            by += tu.base_alpha * wrapdiff(y, by);
            self.base = Some((bp, by));
        }
        let calm = still
            && wrapdiff(p, bp).abs() < tu.return_tolerance
            && wrapdiff(y, by).abs() < tu.return_tolerance;
        self.calm_since = if calm { Some(self.calm_since.unwrap_or(t)) } else { None };
        self.diag.armed = self.armed;
        self.diag.drift = self.drift;

        if !self.armed {
            if self.calm_since.is_some_and(|c| t - c >= tu.calm_hold) && t - self.last_fire >= tu.refractory {
                self.armed = true;
                self.samples.clear(); // start again from neutral, not from the last gesture
            }
            return None;
        }
        if self.samples.len() < tu.min_samples {
            return None;
        }

        let dp: Vec<f64> = self.samples.iter().map(|s| wrapdiff(s.1, bp)).collect();
        let dy: Vec<f64> = self.samples.iter().map(|s| wrapdiff(s.2, by)).collect();
        let swing = |v: &[f64]| {
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        let (swing_p, swing_y) = (swing(&dp), swing(&dy));
        let speeds = |sel: fn(&(f64, f64, f64)) -> f64| -> (f64, f64) {
            let mut hi: f64 = 0.0;
            let mut lo: f64 = 0.0;
            for (a, b) in self.samples.iter().zip(self.samples.iter().skip(1)) {
                let dt = b.0 - a.0;
                if dt > 1e-4 {
                    let v = wrapdiff(sel(b), sel(a)) / dt;
                    hi = hi.max(v);
                    lo = lo.min(v);
                }
            }
            (hi, lo)
        };
        let (vp_hi, vp_lo) = speeds(|s| s.1);
        let (vy_hi, vy_lo) = speeds(|s| s.2);
        self.diag = Diag {
            nod_swing: swing_p,
            shake_swing: swing_y,
            nod_speed: (vp_lo, vp_hi),
            shake_speed: (vy_lo, vy_hi),
            drift: self.drift,
            armed: true,
        };

        let nod = swing_p >= tu.nod.amplitude
            && vp_hi > tu.nod.speed
            && vp_lo < -tu.nod.speed
            && dp.last().unwrap().abs() < tu.return_tolerance
            && swing_p >= tu.axis_ratio * swing_y;
        let shake = swing_y >= tu.shake.amplitude
            && vy_hi > tu.shake.speed
            && vy_lo < -tu.shake.speed
            && dy.last().unwrap().abs() < tu.return_tolerance
            && swing_y >= tu.axis_ratio * swing_p;
        let g = match (nod, shake) {
            (true, false) => Gesture::Nod,
            (false, true) => Gesture::Shake,
            _ => return None,
        };
        self.last_fire = t;
        self.armed = false;
        self.calm_since = None;
        self.samples.clear(); // do not count the swing back as a second gesture
        Some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// ZYX Euler (degrees) -> quaternion (x, y, z, w).
    fn quat_from_euler(yaw: f64, pitch: f64, roll: f64) -> Quat {
        let (cy, sy) = ((yaw.to_radians() / 2.0).cos(), (yaw.to_radians() / 2.0).sin());
        let (cp, sp) = ((pitch.to_radians() / 2.0).cos(), (pitch.to_radians() / 2.0).sin());
        let (cr, sr) = ((roll.to_radians() / 2.0).cos(), (roll.to_radians() / 2.0).sin());
        [
            sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy,
            cr * cp * sy - sr * sp * cy,
            cr * cp * cy + sr * sp * sy,
        ]
    }

    #[test]
    fn euler_round_trip() {
        for (y, p, r) in [(0.0, 0.0, 0.0), (20.0, -15.0, 10.0), (-40.0, 25.0, -30.0), (60.0, 10.0, 5.0)] {
            let (y2, p2, r2) = euler_deg(&mat_from_quat(quat_from_euler(y, p, r)));
            assert!(wrapdiff(y2, y).abs() < 1e-6 && (p2 - p).abs() < 1e-6 && wrapdiff(r2, r).abs() < 1e-6);
        }
    }

    #[test]
    fn recenter_on_itself_is_identity() {
        let q = quat_from_euler(30.0, -10.0, 5.0);
        let r = recenter(q, q);
        assert!((r[3] - 1.0).abs() < 1e-9 && r[..3].iter().all(|v| v.abs() < 1e-9));
    }

    /// Tiny deterministic PRNG + Box-Muller, for the noisy posture case.
    struct Lcg(u64);
    impl Lcg {
        fn uniform(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
        fn gauss(&mut self, sigma: f64) -> f64 {
            sigma * (-2.0 * self.uniform().ln()).sqrt() * (2.0 * PI * self.uniform()).cos()
        }
    }

    /// Replays (pitch(t), yaw(t)) at 9 Hz through quaternions, like the live path.
    /// Replays (nod angle(t), shake angle(t)) at 9 Hz through quaternions, like the live
    /// path: the nod is a rotation about X (roll), the shake about Z (yaw).
    fn replay(
        duration: f64,
        nod: impl Fn(f64) -> f64,
        shake: impl Fn(f64) -> f64,
        noise: f64,
    ) -> Vec<Gesture> {
        let mut det = Detector::new(Tuning::default());
        let mut rng = Lcg(1);
        let dt = 1.0 / 9.0;
        let mut out = Vec::new();
        for i in 0..(duration / dt).round() as usize {
            let t = i as f64 * dt;
            let (mut y, mut r) = (shake(t), nod(t));
            if noise > 0.0 {
                y += rng.gauss(noise);
                r += rng.gauss(noise);
            }
            let q = quat_from_euler(y, 0.0, r);
            let (y2, _, r2) = euler_deg(&mat_from_quat(q.map(|v| v / norm(q))));
            out.extend(det.push(t, r2, y2));
        }
        out
    }

    fn dip(a: f64, t0: f64, d: f64) -> impl Fn(f64) -> f64 {
        move |t| if (t0..=t0 + d).contains(&t) { -a * (PI * (t - t0) / d).sin() } else { 0.0 }
    }

    fn cycle(a: f64, t0: f64, d: f64) -> impl Fn(f64) -> f64 {
        move |t| if (t0..=t0 + d).contains(&t) { a * (2.0 * PI * (t - t0) / d).sin() } else { 0.0 }
    }

    fn zero(_: f64) -> f64 {
        0.0
    }

    #[test]
    fn clean_nod() {
        assert_eq!(replay(2.2, dip(24.0, 0.7, 0.78), zero, 0.0), vec![Gesture::Nod]);
    }

    #[test]
    fn clean_shake() {
        assert_eq!(replay(2.2, zero, cycle(24.0, 0.7, 0.7), 0.0), vec![Gesture::Shake]);
    }

    #[test]
    fn posture_noise_is_ignored() {
        let sway = |t: f64| 4.0 * (2.0 * PI * 0.3 * t).sin();
        assert!(replay(4.0, move |t| 0.6 * sway(t), sway, 2.0).is_empty());
    }

    #[test]
    fn slow_look_sideways_is_ignored() {
        let ramp = |t: f64| {
            if t < 0.7 {
                0.0
            } else if t < 3.7 {
                55.0 * (t - 0.7) / 3.0
            } else {
                55.0
            }
        };
        assert!(replay(5.0, zero, ramp, 0.0).is_empty());
    }

    #[test]
    fn slow_look_down_and_up_is_ignored() {
        let (a, t0, d) = (30.0, 0.7, 1.3);
        let look = move |t: f64| {
            if t < t0 || t > t0 + 2.0 * d + 1.0 {
                0.0
            } else if t < t0 + d {
                -a * (t - t0) / d
            } else if t < t0 + d + 1.0 {
                -a
            } else {
                -a * (1.0 - (t - t0 - d - 1.0) / d)
            }
        };
        assert!(replay(5.5, look, zero, 0.0).is_empty());
    }

    #[test]
    fn diagonal_is_ignored() {
        assert!(replay(2.2, dip(22.0, 0.7, 0.78), dip(22.0, 0.7, 0.78), 0.0).is_empty());
    }

    fn drift(rate: f64) -> impl Fn(f64) -> f64 {
        move |t| rate * t
    }

    #[test]
    fn steady_drift_alone_is_ignored() {
        // 21 deg/s around Z: measured on a B170 that had been streaming for a long time.
        assert!(replay(20.0, zero, drift(-21.0), 0.5).is_empty());
    }

    #[test]
    fn gestures_survive_drift() {
        let d = drift(-21.0);
        let s = cycle(24.0, 8.0, 0.7);
        assert_eq!(replay(12.0, zero, move |t| d(t) + s(t), 0.5), vec![Gesture::Shake]);
        let d = drift(-21.0);
        assert_eq!(replay(12.0, dip(24.0, 8.0, 0.78), d, 0.5), vec![Gesture::Nod]);
    }

    /// A real B170 recording (`nothing headtrack` output): still, 3 nods, still, 3 shakes,
    /// still, 3 slow head tilts, still. The headset had just been power-cycled.
    #[test]
    fn real_recording() {
        let mut det = Detector::new(Tuning::default());
        let mut seen = Vec::new();
        for line in include_str!("testdata/b170-gestures.txt").lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<f64> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let q = [f[4], f[5], f[6], f[7]];
            let (yaw, _, roll) = euler_deg(&mat_from_quat(q));
            if let Some(g) = det.push(f[0], roll, yaw) {
                seen.push((f[0], g));
            }
        }
        let count = |g: Gesture, from: f64, to: f64| {
            seen.iter().filter(|(t, x)| *x == g && (from..to).contains(t)).count()
        };
        assert_eq!(count(Gesture::Nod, 4.5, 12.0), 3, "{seen:?}");
        assert_eq!(count(Gesture::Shake, 17.0, 23.5), 3, "{seen:?}");
        // Nothing else before the end of the tilts.
        assert_eq!(seen.iter().filter(|(t, _)| *t < 34.5).count(), 6, "{seen:?}");
    }
}
