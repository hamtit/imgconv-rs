// Build bench/images/corpus from bench/images/src (see fetch-images.sh).
// Requires the devDependency sharp: bun install && bun make-corpus.ts
import { mkdirSync, readdirSync, copyFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';

const src = join(import.meta.dir, 'images/src');
const out = join(import.meta.dir, 'images/corpus');
mkdirSync(out, { recursive: true });

const files = readdirSync(src).sort();
for (const f of files) copyFileSync(join(src, f), join(out, f));

const photo = (id: string) => join(src, files.find((f) => f.startsWith(`photo-${id}-`))!);

// WebP inputs, as many feeds now serve.
for (const id of ['17', '22', '29', '65']) {
  const name = `webp-${id}.webp`;
  if (!existsSync(join(out, name))) await sharp(photo(id)).webp({ quality: 82 }).toFile(join(out, name));
}

// PNG with an alpha channel (soft-edged circle cut out of a photo).
for (const id of ['37', '42']) {
  const { width, height } = await sharp(photo(id)).metadata();
  const mask = Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">` +
      `<defs><radialGradient id="g"><stop offset="0.8" stop-color="#fff"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs>` +
      `<ellipse cx="${width / 2}" cy="${height / 2}" rx="${width / 2}" ry="${height / 2}" fill="url(#g)"/></svg>`,
  );
  await sharp(photo(id)).ensureAlpha().composite([{ input: mask, blend: 'dest-in' }]).png().toFile(join(out, `alpha-${id}.png`));
}

// JPEG with EXIF orientation 6 (stored sideways).
await sharp(photo('28')).rotate(-90).withMetadata({ orientation: 6 }).jpeg({ quality: 90 }).toFile(join(out, 'rotated-28.jpg'));

// Animated GIFs: a pan across a photo.
async function animatedGif(id: string, width: number, height: number, frames: number) {
  // A source exactly `height` tall and 1.5x the frame width (cropped from
  // the photo as needed), so every frame is covered and the pan moves.
  const base = sharp(photo(id)).resize({ width: Math.ceil(width * 1.5), height, fit: 'cover' });
  const { data, info } = await base.ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  if (info.width < width || info.height !== height) throw new Error(`pan source ${info.width}x${info.height} too small`);
  const span = info.width - width;
  const pages: Buffer[] = [];
  for (let i = 0; i < frames; i++) {
    const x = Math.floor((span * i) / Math.max(1, frames - 1));
    const frame = Buffer.alloc(width * height * 4);
    for (let y = 0; y < height; y++) {
      data.copy(frame, y * width * 4, (y * info.width + x) * 4, (y * info.width + x + width) * 4);
    }
    pages.push(frame);
  }
  await sharp(Buffer.concat(pages), { raw: { width, height: height * frames, channels: 4, pageHeight: height } })
    .gif({ delay: Array(frames).fill(80), loop: 0 })
    .toFile(join(out, `anim-${id}-${width}x${height}x${frames}.gif`));
}
await animatedGif('15', 480, 270, 24);
await animatedGif('54', 320, 320, 40);
await animatedGif('11', 800, 450, 12);
// A single-frame GIF goes through the still path.
await sharp(photo('49')).gif().toFile(join(out, 'still-49.gif'));

const list = readdirSync(out).sort();
writeFileSync(join(out, 'index.json'), JSON.stringify(list, null, 2));
console.log(`${list.length} files in ${out}`);
