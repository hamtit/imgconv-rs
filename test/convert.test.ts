// Behaviour tests for the Node-API addon. sharp (dev dependency) builds the
// fixtures and decodes WebP/AVIF outputs.
import { describe, expect, test } from 'bun:test';
import sharp from 'sharp';
import { readFileSync } from 'node:fs';
import { convert, convertRgba, probe, versions } from '..';

// Cargo.toml is the one place the version is written.
const cargoVersion = readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)![1];

async function photo(width: number, height: number) {
  // A smooth gradient with a sharp edge: compresses like a photo, easy to check.
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
    <defs><linearGradient id="g"><stop offset="0" stop-color="#d33"/><stop offset="1" stop-color="#36c"/></linearGradient></defs>
    <rect width="100%" height="100%" fill="url(#g)"/><rect x="0" y="0" width="${width / 4}" height="${height / 4}" fill="#ff0"/></svg>`;
  return sharp(Buffer.from(svg)).jpeg({ quality: 90 }).toBuffer();
}

async function pixel(buf: Buffer, x: number, y: number) {
  const { data, info } = await sharp(buf).raw().toBuffer({ resolveWithObject: true });
  const o = (y * info.width + x) * info.channels;
  return [...data.subarray(o, o + info.channels)];
}

describe('versions', () => {
  test('reports the bundled codec versions', () => {
    expect(versions()).toEqual({ imgconvRs: cargoVersion, libwebp: '1.6.0', avifMiniEncoder: '0.1.1', jxlMiniEncoder: '0.4.1' });
  });
});

describe('probe', () => {
  test('reads size and format from headers', async () => {
    expect(probe(await photo(640, 480))).toEqual({ format: 'jpeg', width: 640, height: 480, frames: 1, orientation: 1 });
    const png = await sharp({ create: { width: 3, height: 2, channels: 4, background: '#0000' } }).png().toBuffer();
    expect(probe(png)).toMatchObject({ format: 'png', width: 3, height: 2 });
  });

  test('rejects unknown data', () => {
    expect(() => probe(Buffer.from('<html></html>'))).toThrow('Unsupported image format');
  });
});

describe('convert', () => {
  for (const format of ['webp', 'avif'] as const) {
    test(`${format}: resizes to fit inside maxDimension`, async () => {
      const r = await convert(await photo(1200, 800), { format, quality: 65, maxDimension: 320 });
      expect(r).toMatchObject({ format, width: 320, height: 213, frames: 1 });
      const meta = await sharp(r.data).metadata();
      expect([meta.width, meta.height]).toEqual([320, 213]);
      // Top-left stays yellow, so the image is neither flipped nor shifted.
      const [red, green, blue] = await pixel(r.data, 5, 5);
      expect(red).toBeGreaterThan(200);
      expect(green).toBeGreaterThan(200);
      expect(blue).toBeLessThan(80);
    });
  }

  test('jxl: produces a JPEG XL codestream', async () => {
    const r = await convert(await photo(1200, 800), { format: 'jxl', quality: 65, maxDimension: 320 });
    expect(r).toMatchObject({ format: 'jxl', width: 320, height: 213 });
    expect([...r.data.subarray(0, 2)]).toEqual([0xff, 0x0a]);
  });

  test('never enlarges', async () => {
    const r = await convert(await photo(200, 100), { format: 'webp', maxDimension: 320 });
    expect([r.width, r.height]).toEqual([200, 100]);
  });

  test('applies EXIF orientation', async () => {
    // Stored 400x200 (landscape), orientation 6 = display rotated 90° clockwise.
    const stored = await sharp(await photo(400, 200)).withMetadata({ orientation: 6 }).jpeg().toBuffer();
    expect(probe(stored).orientation).toBe(6);
    const r = await convert(stored, { format: 'webp', maxDimension: 100 });
    expect([r.width, r.height]).toEqual([50, 100]);
    // The yellow top-left corner of the stored image ends up top-right.
    const [red, green, blue] = await pixel(r.data, 45, 3);
    expect(red > 200 && green > 200 && blue < 80).toBe(true);
  });

  test('converts an embedded ICC profile to sRGB', async () => {
    const p3 = await sharp({ create: { width: 16, height: 16, channels: 3, background: '#ff0000' } })
      .withIccProfile('p3').jpeg({ quality: 95 }).toBuffer();
    const r = await convert(p3, { format: 'webp', quality: 90 });
    const [red, green, blue] = await pixel(r.data, 8, 8);
    expect(red).toBeGreaterThan(245);
    expect(green).toBeLessThan(12);
    expect(blue).toBeLessThan(12);
  });

  test('keeps transparency and drops an opaque alpha channel', async () => {
    const half = await sharp({ create: { width: 64, height: 64, channels: 4, background: { r: 0, g: 128, b: 0, alpha: 0.5 } } }).png().toBuffer();
    for (const format of ['webp', 'avif'] as const) {
      const r = await convert(half, { format, quality: 90 });
      const meta = await sharp(r.data).metadata();
      expect(meta.hasAlpha).toBe(true);
      const alpha = (await pixel(r.data, 32, 32))[3];
      expect(Math.abs(alpha - 128)).toBeLessThan(6);
    }
    const opaque = await sharp({ create: { width: 64, height: 64, channels: 4, background: '#08f' } }).png().toBuffer();
    expect((await sharp((await convert(opaque, { format: 'avif' })).data).metadata()).hasAlpha).toBe(false);
  });

  test('decodes PNG, WebP, GIF and SVG input', async () => {
    const base = sharp(await photo(300, 200));
    const inputs = {
      png: await base.clone().png().toBuffer(),
      webp: await base.clone().webp().toBuffer(),
      gif: await base.clone().gif().toBuffer(),
      svg: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200"><rect width="75" height="50" fill="#ff0"/><rect x="75" width="225" height="200" fill="#36c"/></svg>'),
    };
    for (const [name, buf] of Object.entries(inputs)) {
      const r = await convert(buf, { format: 'webp', maxDimension: 150 });
      expect(r.input.format).toBe(name);
      expect([r.width, r.height]).toEqual([150, 100]);
      const [red, green, blue] = await pixel(r.data, 4, 4);
      expect(red > 190 && green > 190 && blue < 90).toBe(true);
    }
  });

  test('converts an animated GIF to an animated WebP, frame by frame', async () => {
    const frames = 5;
    const raw = Buffer.alloc(200 * 100 * 4 * frames);
    for (let f = 0; f < frames; f++) {
      for (let i = 0; i < 200 * 100; i++) raw.set([f * 50, 0, 255 - f * 50, 255], (f * 200 * 100 + i) * 4);
    }
    const gif = await sharp(raw, { raw: { width: 200, height: 100 * frames, channels: 4, pageHeight: 100 } })
      .gif({ delay: Array(frames).fill(70), loop: 0 }).toBuffer();
    expect(probe(gif)).toMatchObject({ format: 'gif', frames });
    {
      const r = await convert(gif, { format: 'avif', maxDimension: 100 });
      expect(r).toMatchObject({ format: 'webp', frames, width: 100, height: 50 });
      const meta = await sharp(r.data, { animated: true }).metadata();
      expect(meta.pages).toBe(frames);
      expect(meta.delay).toEqual(Array(frames).fill(70));
      expect(meta.loop).toBe(0);
    }
    // JPEG XL gets an animated JPEG XL with every frame.
    const jxl = await convert(gif, { format: 'jxl', maxDimension: 100 });
    expect(jxl).toMatchObject({ format: 'jxl', frames, width: 100, height: 50 });
    expect(jxl.data[0] === 0xff && jxl.data[1] === 0x0a).toBe(true);
    // A single-frame GIF goes to the requested still format.
    const still = await sharp(raw.subarray(0, 200 * 100 * 4), { raw: { width: 200, height: 100, channels: 4 } }).gif().toBuffer();
    expect((await convert(still, { format: 'avif' })).format).toBe('avif');
  });

  test('never resolves SVG <image> references (files or data URLs)', async () => {
    const png = await sharp({ create: { width: 64, height: 64, channels: 3, background: '#f00' } }).png().toBuffer();
    for (const href of [`data:image/png;base64,${png.toString('base64')}`, '/etc/hostname', 'file:///etc/hostname']) {
      const svg = Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><image href="${href}" width="64" height="64"/></svg>`);
      const r = await convert(svg, { format: 'webp', quality: 90 });
      expect((await pixel(r.data, 32, 32))[3]).toBe(0);
    }
  });

  test('enforces the pixel limit before decoding', async () => {
    const big = await photo(400, 300);
    await expect(convert(big, { format: 'webp', maxPixels: 1000 })).rejects.toThrow('Image exceeds pixel limit: 120000 pixels');
    // A forged 65535x65535 PNG header is rejected without allocating.
    const png = await sharp({ create: { width: 1, height: 1, channels: 3, background: '#000' } }).png().toBuffer();
    png.writeUInt32BE(65535, 16);
    png.writeUInt32BE(65535, 20);
    await expect(convert(png, { format: 'webp' })).rejects.toThrow('pixel limit');
    // A 1x1 GIF screen with a 3000x3000 frame descriptor is judged by the frame.
    const gif = Buffer.from([
      ...Buffer.from('GIF89a'), 1, 0, 1, 0, 0, 0, 0,
      0x2c, 0, 0, 0, 0, 0xb8, 0x0b, 0xb8, 0x0b, 0, 2, 0, 0x3b,
    ]);
    expect(probe(gif)).toMatchObject({ width: 1, height: 1, frames: 1 });
    await expect(convert(gif, { format: 'webp', maxPixels: 1000 })).rejects.toThrow('Image exceeds pixel limit: 9000000 pixels');
  });

  test('reports corrupt input as an error', async () => {
    const jpeg = await photo(400, 300);
    await expect(convert(jpeg.subarray(0, 200), { format: 'webp' })).rejects.toThrow();
    await expect(convert(Buffer.from('GIF89a'), { format: 'webp' })).rejects.toThrow();
    await expect(convert(jpeg, { format: 'png' as never })).rejects.toThrow('Unsupported output format');
  });

  test('runs conversions concurrently off the JS thread', async () => {
    const input = await photo(1600, 1000);
    const results = await Promise.all(Array.from({ length: 8 }, () => convert(input, { format: 'webp', maxDimension: 400 })));
    expect(new Set(results.map((r) => r.data.length)).size).toBe(1);
  });
});

describe('convertRgba', () => {
  test('encodes raw RGBA pixels', async () => {
    const rgba = Buffer.alloc(64 * 32 * 4, 0);
    for (let i = 0; i < 64 * 32; i++) rgba.set([255, 0, 0, 255], i * 4);
    const r = await convertRgba(rgba, 64, 32, { format: 'webp', maxDimension: 32 });
    expect([r.width, r.height]).toEqual([32, 16]);
    expect((await pixel(r.data, 10, 10)).slice(0, 3).map((v) => Math.round(v / 10))).toEqual([25, 0, 0]);
    await expect(convertRgba(rgba.subarray(1), 64, 32, { format: 'webp' })).rejects.toThrow('does not match');
  });
});

describe('row-streaming shrink', () => {
  // PNG (and JPEG at a scale under 2) are shrunk while decoding; the result
  // must match shrinking the fully decoded image.
  test('PNG, gray and RGBA inputs shrink like a full decode', async () => {
    const rgb = await sharp(await photo(900, 600)).png().toBuffer();
    const rgba = await sharp(await photo(900, 600)).ensureAlpha(0.5).png().toBuffer();
    const gray = await sharp(await photo(900, 600)).grayscale().png().toBuffer();
    for (const input of [rgb, rgba, gray]) {
      const out = await convert(input, { format: 'webp', quality: 100, maxDimension: 300 });
      expect([out.width, out.height]).toEqual([300, 200]);
      const reference = await sharp(input).resize(300, 200, { kernel: 'lanczos3' }).raw().toBuffer({ resolveWithObject: true });
      const got = await sharp(out.data).raw().toBuffer({ resolveWithObject: true });
      expect(got.info.channels).toBe(reference.info.channels);
      let sum = 0;
      for (let i = 0; i < got.data.length; i++) sum += Math.abs(got.data[i] - reference.data[i]);
      expect(sum / got.data.length).toBeLessThan(3);
    }
  });

  test('JPEG scaled by less than 2 is shrunk while decoding', async () => {
    const input = await photo(1000, 700);
    const out = await convert(input, { format: 'webp', quality: 100, maxDimension: 640 });
    expect([out.width, out.height]).toEqual([640, 448]);
    // The yellow square in the top-left corner survives the shrink.
    const [r, g, b] = await pixel(out.data, 10, 10);
    expect(r).toBeGreaterThan(240);
    expect(g).toBeGreaterThan(240);
    expect(b).toBeLessThan(15);
  });
});

describe('alpha after shrinking', () => {
  test('an image that becomes fully opaque when shrunk loses its alpha channel', async () => {
    const rgba = Buffer.alloc(100 * 100 * 4, 255);
    rgba[3] = 254; // one nearly opaque pixel
    const out = await convertRgba(rgba, 100, 100, { format: 'webp', maxDimension: 20 });
    expect((await sharp(out.data).metadata()).hasAlpha).toBe(false);
  });
});

describe('damaged PNG tail', () => {
  test('is rejected the same way whether or not the image is shrunk', async () => {
    const png = await sharp(await photo(600, 400)).png().toBuffer();
    const damaged = png.subarray(0, png.length - 20);
    await expect(convert(damaged, { format: 'webp' })).rejects.toThrow();
    await expect(convert(damaged, { format: 'webp', maxDimension: 100 })).rejects.toThrow();
  });
});
