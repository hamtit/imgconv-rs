// Smoke test for the installed imgconv-rs-wasm package.
//   node wasm-smoke.mjs <package name or path>
import assert from 'node:assert/strict';
import { deflateSync } from 'node:zlib';

const pkg = process.argv[2] ?? 'imgconv-rs-wasm';
const { default: createImgconv } = await import(pkg);

function crc32(buf) {
  let c, crc = ~0;
  for (const b of buf) {
    c = (crc ^ b) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return ~crc >>> 0;
}

// An RGBA gradient as raw pixels and as a PNG.
function image(w, h) {
  const rgba = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      rgba.set([(x * 255) / w, (y * 255) / h, 128, 255], (y * w + x) * 4);
    }
  }
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) Buffer.from(rgba.subarray(y * w * 4, (y + 1) * w * 4)).copy(raw, y * (w * 4 + 1) + 1);
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  const png = Buffer.concat([
    Buffer.from('\x89PNG\r\n\x1a\n', 'latin1'),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
  return { rgba, png: new Uint8Array(png) };
}

const magic = {
  webp: (d) => Buffer.from(d.subarray(0, 4)).toString() === 'RIFF' && Buffer.from(d.subarray(8, 12)).toString() === 'WEBP',
  avif: (d) => Buffer.from(d.subarray(4, 12)).toString() === 'ftypavif',
  jxl: (d) => (d[0] === 0xff && d[1] === 0x0a) || Buffer.from(d.subarray(4, 8)).toString() === 'JXL ',
};

const imgconv = await createImgconv();
const { rgba, png } = image(96, 64);

const versions = imgconv.versions();
console.log(versions);
assert.equal(versions.libwebp, '1.6.0');
assert.equal(versions.jxlMiniEncoder, '0.2.0');

assert.deepEqual(imgconv.probe(png), { format: 'png', width: 96, height: 64, frames: 1, orientation: 1 });
assert.throws(() => imgconv.probe(new TextEncoder().encode('<html></html>')), /Unsupported image format/);

for (const format of ['webp', 'avif', 'jxl']) {
  const r = imgconv.convert(png, { format, quality: 65, maxDimension: 48 });
  assert.equal(r.format, format);
  assert.equal(r.width, 48);
  assert.equal(r.height, 32);
  assert.ok(magic[format](r.data), `${format} output has the wrong signature`);
  assert.equal(r.input.format, 'png');

  const raw = imgconv.convertRgba(rgba, 96, 64, { format, quality: 65 });
  assert.equal(raw.width, 96);
  assert.ok(magic[format](raw.data));
  console.log(format, r.data.length, 'bytes (resized),', raw.data.length, 'bytes (rgba)');
}

assert.throws(() => imgconv.convert(png, { format: 'bmp' }), /Unsupported output format/);
assert.throws(() => imgconv.convert(png, { format: 'webp', maxPixels: 10 }), /pixel limit/);
console.log('ok');
