#!/bin/sh
# Download the photo part of the benchmark corpus into bench/images/src
# (not committed). Generated inputs (WebP, GIF, alpha PNG, rotated JPEG) are
# made from these by make-corpus.ts.
set -eu
DIR=$(cd "$(dirname "$0")" && pwd)/images/src
mkdir -p "$DIR"
fetch() { [ -s "$DIR/$2" ] || curl -sSfL --retry 3 -o "$DIR/$2" "$1"; }
# Unsplash photos via picsum.photos (fixed ids and sizes).
for spec in 10:4000x2667 11:2500x1667 15:2500x1667 17:1920x1280 22:1920x1280 28:1600x1067 \
            29:1200x800 37:1200x800 42:1024x683 49:800x533 54:3000x2000 65:2048x1365; do
  id=${spec%%:*}; size=${spec#*:}; w=${size%x*}; h=${size#*x}
  fetch "https://picsum.photos/id/$id/$w/$h" "photo-$id-${w}x$h.jpg"
done
# Kodak Lossless True Color Image Suite (PNG, 768x512).
for n in 01 03 05 07 13 23; do
  fetch "https://r0k.us/graphics/kodak/kodak/kodim$n.png" "kodim$n.png"
done
ls -la "$DIR"
