// imgconv-rs for WebAssembly: the same convert / convertRgba / probe /
// versions API as the Node-API addon, on top of the Emscripten module.
//
// Conversions run synchronously on the calling thread (there are no threads
// in this build); use a Worker in browsers to keep the UI responsive.
import createModule from './dist/imgconv.js';

const OUTPUT = ['webp', 'avif', 'jxl'];
const INPUT = ['jpeg', 'png', 'gif', 'webp', 'svg'];

// WasmResult (src/wasm.rs): eleven u32 fields.
const R = { dataPtr: 0, dataLen: 1, format: 2, width: 3, height: 4, frames: 5, inFormat: 6, inWidth: 7, inHeight: 8, inFrames: 9, inOrientation: 10 };

function toBytes(data, what) {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  throw new TypeError(`${what} must be a Uint8Array (or Buffer)`);
}

/** Instantiate the module. Options are passed to the Emscripten loader (`wasmBinary`, `locateFile`). */
export default async function createImgconv(moduleOptions) {
  const m = await createModule(moduleOptions);

  const lastError = () => m.UTF8ToString(m._imgconv_last_error());

  // Run `fn(ptr)` with a copy of `bytes` in the wasm heap.
  function withBytes(bytes, fn) {
    // Pointers above 2 GiB come back as negative i32s; index with the unsigned value.
    const ptr = m._malloc(Math.max(bytes.length, 1)) >>> 0;
    if (!ptr) throw new Error('imgconv-rs: out of memory');
    try {
      m.HEAPU8.set(bytes, ptr);
      return fn(ptr);
    } finally {
      m._free(ptr);
    }
  }

  function format(options) {
    const i = OUTPUT.indexOf(options?.format);
    if (i < 0) throw new Error(`Unsupported output format: ${options?.format}`);
    return i;
  }

  function result(handle) {
    if (!handle) throw new Error(lastError());
    try {
      const h = handle >>> 2;
      const f = m.HEAPU32;
      const ptr = f[h + R.dataPtr];
      return {
        // A copy: the wasm memory may move or be reused.
        data: m.HEAPU8.slice(ptr, ptr + f[h + R.dataLen]),
        format: OUTPUT[f[h + R.format]],
        width: f[h + R.width],
        height: f[h + R.height],
        frames: f[h + R.frames],
        input: {
          format: INPUT[f[h + R.inFormat]],
          width: f[h + R.inWidth],
          height: f[h + R.inHeight],
          frames: f[h + R.inFrames],
          orientation: f[h + R.inOrientation],
        },
      };
    } finally {
      m._imgconv_result_free(handle);
    }
  }

  const common = (o) => [
    o.quality ?? 65,
    o.maxDimension ?? 0,
    o.effort ?? -1,
    o.maxPixels ?? 0,
    o.animated === false ? 0 : 1,
  ];

  return {
    /** Convert an encoded image (JPEG, PNG, GIF, WebP, AVIF or SVG). */
    convert(input, options) {
      const bytes = toBytes(input, 'input');
      const fmt = format(options);
      return result(withBytes(bytes, (p) => m._imgconv_convert(p, bytes.length, fmt, ...common(options))));
    },
    /** Convert raw 8-bit RGBA pixels (straight alpha). */
    convertRgba(rgba, width, height, options) {
      const bytes = toBytes(rgba, 'rgba');
      const fmt = format(options);
      return result(
        withBytes(bytes, (p) => m._imgconv_convert_rgba(p, bytes.length, width, height, fmt, ...common(options))),
      );
    },
    /** Read format, size and frame count from the header without decoding pixels. */
    probe(input) {
      const bytes = toBytes(input, 'input');
      const out = m._malloc(5 * 4) >>> 0;
      try {
        const rc = withBytes(bytes, (p) => m._imgconv_probe(p, bytes.length, out));
        if (rc !== 0) throw new Error(lastError());
        const v = m.HEAPU32.subarray(out >>> 2, (out >>> 2) + 5);
        return { format: INPUT[v[0]], width: v[1], height: v[2], frames: v[3], orientation: v[4] };
      } finally {
        m._free(out);
      }
    },
    versions() {
      const [imgconvRs, libwebp, libaom, jxlMiniEncoder] = m.UTF8ToString(m._imgconv_versions()).split('\n');
      return { imgconvRs, libwebp, libaom, jxlMiniEncoder };
    },
  };
}
