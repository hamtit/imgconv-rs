// Large lossless inputs (bench/images/large): PNGs have no shrink-on-load, so
// they show the cost of decoding the whole image before resizing.
// Requires bench/images/src (fetch-images.sh) and the devDependency sharp.
import { mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';

const src = join(import.meta.dir, 'images/src');
const out = join(import.meta.dir, 'images/large');
mkdirSync(out, { recursive: true });
const photo = (id: string) => join(src, readdirSync(src).find((f) => f.startsWith(`photo-${id}-`))!);

// 6000x4000 RGB / opaque RGBA / RGBA with real alpha, from the 4000px photo.
const big = () => sharp(photo('10')).resize(6000, 4000, { kernel: 'lanczos3' });
await big().png({ compressionLevel: 1 }).toFile(join(out, 'large-rgb.png'));
await big().ensureAlpha().png({ compressionLevel: 1 }).toFile(join(out, 'large-rgba-opaque.png'));
const w = 6000, h = 4000;
const mask = Buffer.from(
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs><radialGradient id="g"><stop offset="0.8" stop-color="#fff"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs><ellipse cx="${w / 2}" cy="${h / 2}" rx="${w / 2}" ry="${h / 2}" fill="url(#g)"/></svg>`,
);
await big().ensureAlpha().composite([{ input: mask, blend: 'dest-in' }]).png({ compressionLevel: 1 }).toFile(join(out, 'large-rgba-alpha.png'));
// 4000px JPEGs for the JPEG path.
await sharp(photo('10')).jpeg({ quality: 90 }).toFile(join(out, 'large-4000.jpg'));
console.log(readdirSync(out));
