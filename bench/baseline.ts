// Baseline for the benchmarks: the usual way to do this conversion with sharp
// (libvips) in a Node/Bun server. One function per step, no tuning beyond a
// small libvips cache. JPEG XL is not built into sharp, so it needs an
// external encoder: set BASELINE_JXL_MODULE to a module exporting
// `encode(rgba: Uint8Array, width: number, height: number, quality: number): Uint8Array | Promise<Uint8Array>`.
// Without it, the JPEG XL baseline is unavailable.
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import sharp from 'sharp';

export const QUALITY = 65;
/** Whether the JPEG XL baseline can run (see the header comment). */
export const JXL_AVAILABLE = Boolean(process.env.BASELINE_JXL_MODULE);
export const MAX_INPUT_PIXELS = 268_402_689; // sharp's default limit (0x3FFF x 0x3FFF)
const TIMEOUT_SECONDS = 30;

sharp.cache({ memory: 50, items: 20 });

async function save(outputPath: string, data: Buffer | Uint8Array): Promise<void> {
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, data);
}

const resizeOpts = (max: number) => ({ width: max, height: max, fit: 'inside', withoutEnlargement: true }) as const;

export async function validateInputPixels(buf: Buffer): Promise<void> {
  const input = sharp(buf, { limitInputPixels: MAX_INPUT_PIXELS });
  try {
    const { width, height } = await input.metadata();
    if (!width || !height || width * height > MAX_INPUT_PIXELS) throw new Error('Image exceeds pixel limit');
  } finally {
    input.destroy();
  }
}

export function validateGifCanvasPixels(buf: Buffer): void {
  if (buf.length < 10 || buf[0] !== 0x47 || buf[1] !== 0x49 || buf[2] !== 0x46) throw new Error('Invalid GIF');
  if (buf.readUInt16LE(6) * buf.readUInt16LE(8) > MAX_INPUT_PIXELS) throw new Error('Image exceeds pixel limit');
}

export async function countGifFrames(buf: Buffer): Promise<number> {
  const input = sharp(buf, { limitInputPixels: MAX_INPUT_PIXELS });
  try {
    return (await input.metadata()).pages ?? 1;
  } finally {
    input.destroy();
  }
}

/** Animated GIF to animated WebP, frame by frame. */
export async function encodeAnimatedGif(buf: Buffer, outputPath: string, max?: number): Promise<void> {
  const pipeline = sharp(buf, { animated: true, limitInputPixels: MAX_INPUT_PIXELS });
  try {
    if (max) pipeline.resize(resizeOpts(max));
    const out = await pipeline.webp({ quality: QUALITY, loop: 0 }).timeout({ seconds: TIMEOUT_SECONDS }).toBuffer();
    await save(outputPath, out);
  } finally {
    pipeline.destroy();
  }
}

export async function encodeWithSharp(buf: Buffer, outputPath: string, format: 'avif' | 'webp', max?: number): Promise<void> {
  const pipeline = sharp(buf);
  try {
    if (max) pipeline.resize(resizeOpts(max));
    const encoded = format === 'webp' ? pipeline.webp({ quality: QUALITY }) : pipeline.avif({ quality: QUALITY });
    await save(outputPath, await encoded.toBuffer());
  } finally {
    pipeline.destroy();
  }
}

export async function encodeJxl(buf: Buffer, outputPath: string, max?: number): Promise<void> {
  const modulePath = process.env.BASELINE_JXL_MODULE;
  if (!modulePath) throw new Error('JPEG XL baseline needs BASELINE_JXL_MODULE (see bench/baseline.ts)');
  const { encode } = await import(modulePath);
  await validateInputPixels(buf);
  const pipeline = sharp(buf, { autoOrient: true });
  try {
    if (max) pipeline.resize(resizeOpts(max));
    const { data, info } = await pipeline
      .toColorspace('srgb')
      .ensureAlpha()
      .raw({ depth: 'uchar' })
      .timeout({ seconds: TIMEOUT_SECONDS })
      .toBuffer({ resolveWithObject: true });
    await save(outputPath, await encode(data, info.width, info.height, QUALITY));
  } finally {
    pipeline.destroy();
  }
}
