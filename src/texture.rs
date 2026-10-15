//! The outer face of a cup as a PS2-sized texture: the right cup's orthographic render,
//! cropped to the cup and box-filtered to 64 x 76 texels (`tools/bake-face.sh`), looked up
//! with no filtering so the texels stay square blocks.
//!
//! One picture serves both cups: the left cup shows it mirrored, which falls out of using
//! the same (y, z) lookup on both sides, with the red square turned white.

const W: usize = 64;
const H: usize = 76;
/// The cup's size in millimetres, which the texture covers edge to edge.
const CUP_W: f32 = 78.0;
const CUP_H: f32 = 92.0;

static WHITE: &[u8] = include_bytes!("textures/face-white.rgb");
static BLACK: &[u8] = include_bytes!("textures/face-black.rgb");

/// sRGB (0..1) of the face at `y` (towards the face, mm) and `z` (up, mm) from the cup's centre.
pub fn face(black: bool, left: bool, y: f32, z: f32) -> [f32; 3] {
    let u = ((y + CUP_W / 2.0) / CUP_W).clamp(0.0, 0.999);
    let v = ((CUP_H / 2.0 - z) / CUP_H).clamp(0.0, 0.999);
    let (x, row) = ((u * W as f32) as usize, (v * H as f32) as usize);
    let data = if black { BLACK } else { WHITE };
    let px = &data[(row * W + x) * 3..][..3];
    let c = [px[0], px[1], px[2]].map(|b| b as f32 / 255.0);
    // The left cup's square is white, the right one's red.
    if left && c[0] > c[1] + 0.2 && c[0] > c[2] + 0.2 {
        return if black { [0.91, 0.91, 0.91] } else { [0.95, 0.95, 0.95] };
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_have_the_size_of_the_texture() {
        assert_eq!((WHITE.len(), BLACK.len()), (W * H * 3, W * H * 3));
    }

    #[test]
    fn the_red_square_is_on_the_back_of_the_right_cup_only() {
        // Back is -Y, at mid height (z about 0 to -7 mm, y about -22).
        let red = |left| {
            (-28..-16).any(|y| {
                (-12..4).any(|z| {
                    let c = face(false, left, y as f32, z as f32);
                    c[0] > c[1] + 0.2
                })
            })
        };
        assert!(red(false) && !red(true));
    }
}
