#!/bin/sh
# Bakes the outer-face textures of the cups (src/textures/face-*.rgb): the right cup's
# orthographic outer face, cropped to the cup (78 x 92 mm = 426 x 502 px at x=383, y=588 in
# the 1200 x 1200 renders), box-filtered down to 64 x 76 texels and written as raw RGB.
# Needs ImageMagick and the reference renders in previews/refs/ (kept local, untracked).
set -eu
cd "$(dirname "$0")/.."
bake() {
    magick "previews/refs/$1.png" -background '#808080' -alpha remove -alpha off \
        -crop 426x502+383+588 +repage -filter Box -resize 64x76! -depth 8 "rgb:src/textures/face-$2.rgb"
}
bake white_kef_right_cup_outer_face_ortho_scaled white
bake black_kef_right_cup_outer_face_ortho black
