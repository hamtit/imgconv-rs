#!/usr/bin/env bash
# Build the WebAssembly module (Emscripten must be on PATH, and the Rust
# target wasm32-unknown-emscripten installed).
# Usage: build-wasm.sh <out-dir>
#
# <out-dir> receives dist/imgconv.{js,wasm} (ES module, MODULARIZE) and
# licenses/. Plain wasm32, no SIMD and no threads: the C codecs are built
# with AOM_TARGET_CPU=generic; jxl-mini-encoder is pure Rust.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
out="$(realpath -m "${1:?out dir}")"
cd "$here"

./scripts/fetch-deps.sh
# One exception / setjmp model everywhere: rustc uses WebAssembly exceptions,
# so the C and C++ code (libaom's longjmp, mozjpeg, libwebp) is
# compiled for it too. The cc crate and cmake-rs pick these variables up.
export CFLAGS_wasm32_unknown_emscripten="-fwasm-exceptions"
export CXXFLAGS_wasm32_unknown_emscripten="-fwasm-exceptions"
rm -rf "$out"
mkdir -p "$out/dist"

# Rust wraps the whole crate (and the C libraries build.rs compiled) into one
# static library; em++ does the final link. libjpeg (mozjpeg-sys) reports
# errors by unwinding, so panic=unwind stays on; rustc compiles that with
# WebAssembly exceptions, hence -fwasm-exceptions at link time.
cargo rustc --release --locked --target wasm32-unknown-emscripten \
  --no-default-features --lib --crate-type staticlib
lib="target/wasm32-unknown-emscripten/release/libimgconv_rs.a"

funcs=imgconv_convert,imgconv_convert_rgba,imgconv_probe,imgconv_result_free,imgconv_last_error,imgconv_versions,malloc,free
em++ -O3 \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sEXPORT_NAME=createImgconv \
  -sENVIRONMENT=web,worker,node \
  -sFILESYSTEM=0 \
  -sALLOW_MEMORY_GROWTH=1 \
  -sINITIAL_MEMORY=64MB \
  -sMAXIMUM_MEMORY=4GB \
  -sSTACK_SIZE=5MB \
  -fwasm-exceptions \
  -sEXPORTED_FUNCTIONS="$(echo "$funcs" | tr ',' '\n' | sed 's/^/_/' | paste -sd, -)" \
  -sEXPORTED_RUNTIME_METHODS=HEAPU8,HEAPU32,UTF8ToString \
  "$lib" \
  -o "$out/dist/imgconv.js"

# Licenses: the same set as the native packages (crates + libaom),
# plus what Emscripten links in.
./scripts/collect-licenses.sh
cp -R licenses "$out/licenses"
em_root="$(em-config EMSCRIPTEN_ROOT)"
cp "$em_root/LICENSE" "$out/licenses/LICENSE.emscripten"
cp "$em_root/system/lib/libcxx/LICENSE.TXT" "$out/licenses/LICENSE.libcxx"
cp "$em_root/system/lib/libcxxabi/LICENSE.TXT" "$out/licenses/LICENSE.libcxxabi"
cp "$em_root/system/lib/compiler-rt/LICENSE.TXT" "$out/licenses/LICENSE.compiler-rt"
cp "$em_root/system/lib/libc/musl/COPYRIGHT" "$out/licenses/LICENSE.musl"

ls -l "$out/dist"
