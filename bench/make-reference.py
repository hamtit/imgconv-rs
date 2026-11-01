"""Neutral quality reference for quality.ts: each still corpus image decoded by
Pillow, EXIF-transposed, converted to sRGB, and Lanczos-resized from full
resolution (premultiplied alpha) to sharp's fit-inside size. Written as PNG.

    python3 make-reference.py MAX OUTDIR
"""
import io
import os
import sys

from PIL import Image, ImageCms, ImageOps

MAX, OUT = int(sys.argv[1]), sys.argv[2]
CORPUS = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'images/corpus')
os.makedirs(OUT, exist_ok=True)
SRGB = ImageCms.createProfile('sRGB')


def fit_inside(w, h, m):
    if w <= m and h <= m:
        return w, h
    if w >= h:
        return m, max(1, round(h * m / w))
    return max(1, round(w * m / h)), m


for name in sorted(os.listdir(CORPUS)):
    if name.endswith('.json') or name.startswith('anim-'):
        continue
    if name.endswith('.svg'):
        continue
    im = Image.open(os.path.join(CORPUS, name))
    im.seek(0)
    icc = im.info.get('icc_profile')
    im = ImageOps.exif_transpose(im)
    has_alpha = im.mode in ('RGBA', 'LA', 'PA') or (im.mode == 'P' and 'transparency' in im.info)
    im = im.convert('RGBA' if has_alpha else 'RGB')
    if icc:
        src = ImageCms.ImageCmsProfile(io.BytesIO(icc))
        im = ImageCms.profileToProfile(im, src, SRGB, outputMode=im.mode)
    size = fit_inside(im.width, im.height, MAX)
    if size != im.size:
        if has_alpha:
            im = im.convert('RGBa').resize(size, Image.LANCZOS).convert('RGBA')
        else:
            im = im.resize(size, Image.LANCZOS)
    im.save(os.path.join(OUT, name + '.png'))
