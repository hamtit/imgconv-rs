# imgconv-rs-wasm

[imgconv-rs](https://github.com/hamtit/imgconv-rs) compiled to WebAssembly
(Emscripten): decode (JPEG, PNG, GIF, WebP, SVG), resize, orient and
encode to WebP / AVIF / JPEG XL in one call. Same options and results as the
Node-API addon, but synchronous, single-threaded and without SIMD.

Not published to the npm registry; install from the GitHub release:

```sh
npm install https://github.com/hamtit/imgconv-rs/releases/download/v<version>/imgconv-rs-wasm-<version>.tgz
```

```js
import createImgconv from 'imgconv-rs-wasm';

const imgconv = await createImgconv();
const { data, width, height } = imgconv.convert(bytes, { format: 'webp', quality: 65, maxDimension: 320 });
imgconv.probe(bytes); // { format, width, height, frames, orientation }
imgconv.versions();   // { imgconvRs, libwebp, avifMiniEncoder, jxlMiniEncoder }
```

The module is an ES module for browsers, workers and Node.js. Conversions
block the calling thread, so run them in a Worker in browsers. `data` is a
`Uint8Array` copy. Memory grows on demand up to 4 GB.
