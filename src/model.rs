//! The head-tracking frame shared by the 3D views, and the headset's two colours.
//!
//! Frame: Z up, +X towards the wearer's right ear, +Y towards the face (right-handed); the
//! origin is the centre of the head, so that rotations turn the headset about it. Measured on
//! a B170: from rest, looking down turns the head about X by a negative angle (-15 to -20 deg),
//! which puts +Y in front.

pub type V3 = [f64; 3];

/// RGB, 0..1.
pub type Rgb = [f64; 3];

/// Colours of the two editions, from the headset's colour code (`C00C`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edition {
    White,
    Black,
}
