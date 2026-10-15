#!/bin/sh
# Bakes the outer-face textures of the cups (src/textures/face-*.rgb): the right cup's
# orthographic outer face, cropped to the shell itself (78 x 90 mm = 411 x 474 px at x=393,
# y=616 in the 1200 x 1200 renders, measured on the shell's edges), box-filtered down to
# 64 x 74 texels and written as raw RGB.
# Needs ImageMagick and the reference renders in previews/refs/ (kept local, untracked).
set -eu
cd "$(dirname "$0")/.."
bake() {
    magick "previews/refs/$1.png" -background '#808080' -alpha remove -alpha off \
        -crop 411x474+393+616 +repage -filter Box -resize 64x74! -depth 8 "rgb:src/textures/face-$2.rgb"
}
bake white_kef_right_cup_outer_face_ortho_scaled white
bake black_kef_right_cup_outer_face_ortho black
