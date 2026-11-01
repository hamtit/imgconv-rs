# imgconv-rs

画像変換だけを行う、Rust 製の Node-API アドオン。Bun と Node.js から使える。入力をデコードして縮小・向きの補正・色変換をし、WebP / AVIF / JPEG XL にエンコードするまでを 1 回のネイティブ呼び出しで行う。

sharp (libvips) は汎用の画像処理ライブラリだが、Web 配信用の画像変換で使うのは「デコード → 縮小 → WebP / AVIF / JPEG XL へのエンコード」だけであることが多い。その部分だけに絞って、変換時間とメモリ使用量がどう変わるかを確かめるために作った。測定結果は [ベンチマーク](#ベンチマーク) を参照。

## できること

| | 対応 |
|---|---|
| 入力 | JPEG (CMYK を含む)、PNG (16bit・パレット・APNG の先頭フレーム)、GIF、WebP (アニメーションは先頭フレーム)、AVIF、SVG |
| 出力 | WebP、AVIF、JPEG XL (`quality: 100` で可逆)。複数フレームの GIF は出力形式に関わらずアニメーション WebP |
| 縮小 | sharp の `fit: 'inside', withoutEnlargement: true` と同じ寸法。Lanczos3、アルファは乗算済みで処理。JPEG は sharp と同じ規則で DCT 縮小デコード (shrink-on-load)、WebP は libwebp のスケーラーで 2 の累乗まで縮小してからデコード、SVG は目標サイズで直接ラスタライズ |
| 色 | 埋め込み ICC プロファイルを sRGB に変換してから出力 (sharp と同じ)。完全に不透明なアルファチャンネルは捨てる |
| 向き | EXIF Orientation を適用して正立させる |
| 安全性 | デコード前にヘッダーから画素数を調べて上限 (既定は sharp と同じ 268,402,689) を超えたら拒否。アニメーション GIF は全フレームの合計で判定 |

使っていない機能 (切り抜き、合成、フィルター、メタデータの保持、TIFF / HEIC 入力など) は持たない。

### 中身

| 処理 | ライブラリ | 版 | sharp 0.35.5 との関係 |
|---|---|---|---|
| WebP エンコード・デコード | libwebp (`libwebp-sys`) | 1.6.0 | sharp と同じ版。libvips と同じ設定 (preset default、method 4、alpha_quality 100) にしており、同じ画素からは**同一バイト列**を出力する |
| AVIF エンコード・デコード | libaom (ソースからビルド) | 3.15.1 | sharp と同じ版。libheif 1.23 が libaom に渡す設定 (all-intra、speed = 9 - effort、tune IQ、quality 90 未満は 4:2:0 (libvips の規則。sharp は既定で 4:4:4 に上書きしているが、ここでは合わせない)、BT.601 full range、auto tiles など) を再現 |
| JPEG XL エンコード | libjxl (ソースからビルド) | 0.12.0 | libjxl-native と同じ版・同じ設定 (effort 7、buffering 2、quality → distance) |
| JPEG デコード | mozjpeg (`mozjpeg-sys` 2.2) | | sharp と同じく mozjpeg (libjpeg-turbo 系の SIMD デコーダー) |
| PNG / GIF デコード | `png` / `gif` クレート | | Rust 実装 |
| 縮小 | `fast_image_resize` | 6 | SIMD (AVX2 / NEON) の Lanczos3 |
| 色変換 | `moxcms` | 0.9 | Rust 実装の ICC 変換 |
| SVG | `resvg` | 0.49 | テキストは描画しない (フォントを持たない) |

## 使い方

```ts
import { convert, convertRgba, probe } from 'imgconv-rs';

const result = await convert(buffer, { format: 'avif', quality: 65, maxDimension: 320 });
// result.data: Buffer, result.format: 'avif' (アニメーション GIF なら 'webp'),
// result.width / result.height / result.frames, result.input: 入力のヘッダー情報

probe(buffer); // { format: 'jpeg', width, height, frames, orientation } (画素はデコードしない)
await convertRgba(rgba, width, height, { format: 'webp' }); // ICO から取り出した RGBA など
```

変換はスレッドプール (libuv / Bun の Node-API ワーカー) で動き、JavaScript のスレッドを止めない。オプションは `index.d.ts` を参照。

- `threads` (既定 1): 1 枚の変換に使うエンコーダーのスレッド数 (AVIF / JPEG XL)。並行に何枚も変換するサーバーでは 1 が最も速い ([ベンチマーク](#ベンチマーク))
- 変換が 1 つも動いていない状態になると `malloc_trim(0)` を呼び、解放済みのヒープを OS に返す (glibc のみ)

## ビルド

Linux (glibc / musl、x64 / arm64) 向け。必要なもの: Rust、CMake、C/C++ コンパイラー、clang (bindgen 用)、nasm (x64)、curl。

```bash
./scripts/build.sh
```

`scripts/fetch-deps.sh` が libaom と libjxl のソースを `vendor/` に取得し (SHA-256 を検証)、`build.rs` が CMake で静的ライブラリとしてビルドして組み込む。できた `imgconv-rs.linux-<arch>-<libc>.node` を `index.js` が読み込む。

```bash
bun install
bun test test/
cargo test --release --no-default-features
```

## リリース

リリースは自動で、バージョン番号の置き場所は `Cargo.toml` の `version` だけ (`package.json` には書かない。`Cargo.lock` の `imgconv-rs` の版は `cargo update -p imgconv-rs` で揃える。CI が食い違いを検出する)。

1. PR で `Cargo.toml` (と `Cargo.lock`) の `version` を上げる。
2. `main` にマージする。CI が通ったあと、タグ `v<version>` がまだなければ、タグと GitHub Release を作る。

`main` への push でもタグがすでにあれば、ビルドとテストだけを行う。`v<version>` のタグを手で push してもリリースされる (`Cargo.toml` と合わない名前のタグは拒否)。PR と手動実行では、ビルド・テスト・パッケージ作成までを行い、パッケージはワークフローのアーティファクトに残る。

Release には 4 つの npm tarball が付く。npm レジストリには公開していないので、Release の URL で依存する。

| ファイル | 内容 |
|---|---|
| `imgconv-rs-linux-x64-<version>.tgz` | Linux x64 (glibc) のアドオンだけ |
| `imgconv-rs-linux-arm64-<version>.tgz` | Linux arm64 (glibc) のアドオンだけ |
| `imgconv-rs-<version>.tgz` | x64 と arm64 を 1 つにまとめたもの |
| `imgconv-rs-wasm-<version>.tgz` | WebAssembly 版 ([packages/wasm](packages/wasm/README.md)) |

x64 / arm64 の単体パッケージは `os` / `cpu` を持つので、`optionalDependencies` に両方並べればパッケージマネージャーが合う方だけ入れる。どれも Debian bookworm (`oven/bun:*-debian` と同じ glibc) でビルドし、依存ライブラリのライセンス文書 (`licenses/`) を含む。

```sh
npm install https://github.com/hamtit/imgconv-rs/releases/download/v0.1.0/imgconv-rs-0.1.0.tgz
```

WebAssembly 版は Emscripten で `wasm32-unknown-emscripten` 向けにビルドする (`scripts/build-wasm.sh`)。SIMD とスレッドは使わず、同期 API で、`threads` オプションはない。ブラウザーでは Worker で動かす。

## ベンチマーク

sharp 0.35.5 を使った一般的な変換 (WebP / AVIF は sharp、JPEG XL は sharp で RGBA に展開して別スレッドの libjxl でエンコード) とこのライブラリを、同じ入力・同じ品質 (65) で比べた。比較側の実装は `bench/baseline.ts` (JPEG XL は sharp 単体では出せないので、外部のエンコーダーを `BASELINE_JXL_MODULE` で渡す。未指定なら JPEG XL のシナリオは飛ばす)。

- 環境: Intel Xeon 2.8 GHz 4 コア、Bun 1.4.4、Linux (glibc)
- 入力: 29 枚 (Unsplash の JPEG 12 枚 800〜4000px、Kodak の PNG 6 枚、WebP 4 枚、アルファ付き PNG 2 枚、EXIF 回転 JPEG、静止 GIF、アニメーション GIF 3 枚)。`bench/fetch-images.sh` と `bench/make-corpus.ts` で再現できる
- 方法: 10 並列で 29 枚を変換し、ファイル保存まで含めて測る。1 回ごとに新しいプロセスで 3 回測った中央値 (`bench/run-all.ts`)。メモリは RSS のピーク (VmHWM) と、変換後 1 秒待って GC した後の RSS

### 速度とメモリ (10 並列で 29 枚)

| 形式 | 最大辺 | 時間 sharp → このライブラリ | CPU 時間 | ピーク RSS | 変換後の RSS |
|---|---|---|---|---|---|
| WebP | 320 | 992 → 799 ms (**-19%**) | 3.1 → 2.5 s (-19%) | 273 → 138 MB (**-49%**) | 272 → 126 MB |
| WebP | 1920 | 2.71 → 2.00 s (**-26%**) | 9.8 → 7.6 s (-22%) | 492 → 237 MB (**-52%**) | 488 → 195 MB |
| AVIF | 320 | 5.19 → 3.30 s (**-36%**) | 19.4 → 12.6 s (-35%) | 274 → 148 MB (**-46%**) | 273 → 131 MB |
| AVIF | 1920 | 53.2 → 37.0 s (**-30%**) | 195 → 133 s (-32%) | 1277 → 458 MB (**-64%**) | 1112 → 138 MB |
| JPEG XL | 320 | 1.28 → 0.85 s (**-33%**) | 4.4 → 3.2 s (-27%) | 332 → 153 MB (**-54%**) | 326 → 123 MB |
| JPEG XL | 1920 | 9.05 → 7.73 s (**-15%**) | 33.0 → 27.2 s (-18%) | 1326 → 677 MB (**-49%**) | 1200 → 84 MB |

1 枚ずつ (並列なし) 変換した場合 (最大辺 1920、29 枚の合計):

| 形式 | sharp | このライブラリ 1 スレッド | このライブラリ 4 スレッド |
|---|---|---|---|
| AVIF | 194 s / 457 MB | 136 s / 178 MB | 43 s / 175 MB |
| JPEG XL | 33.2 s / 506 MB | 31.1 s / 227 MB | 13.7 s / 321 MB |

### 画質 (同じ品質設定での出力サイズと SSIMULACRA2)

基準画像は両者のどちらとも違う実装 (Pillow で原寸から Lanczos 縮小、EXIF 回転・sRGB 変換済み) で作り (`bench/make-reference.py`)、静止画 26 枚の平均を取った (`bench/quality.ts`)。SSIMULACRA2 は高いほど良い。

| 形式 | 最大辺 | 出力サイズ合計 sharp / このライブラリ | SSIMULACRA2 sharp / このライブラリ |
|---|---|---|---|
| WebP | 320 | 279 / 278 KB | 64.8 / **67.4** |
| WebP | 1920 | 4.56 / 4.59 MB | 66.1 / 66.5 |
| AVIF | 320 | 317 / 300 KB | 73.4 / **76.1** |
| AVIF | 1920 | 4.94 / 4.87 MB | **79.6** / 78.6 |
| JPEG XL | 320 | 231 / 231 KB | 62.0 / **64.7** |
| JPEG XL | 1920 | 3.34 / 3.34 MB | 66.3 / 66.5 |

### 結果の読み方

- **速くなった理由**: エンコーダーは同じ (libwebp・libaom・libjxl の同じ版・同じ設定) なので、差は周辺から来ている。sharp は変換 1 枚ごとに libaom を CPU 数のスレッドで動かすため、10 並列だとスレッドが CPU 数を大きく超えて奪い合う。このライブラリは 1 枚 1 スレッドで、並列数は Node-API のスレッドプール (CPU 数) で決まる。JPEG XL は比較側では sharp で RGBA に展開してから Worker に転送するが、それがなくなった (Worker の起動や JavaScript ヒープも不要)
- **メモリが減った理由**: 並列で同時に動くエンコードの数が CPU 数に抑えられることと、libvips の常駐分・キャッシュがないこと。変換後の RSS が大きく下がるのは、変換が 1 つも動いていないときに `malloc_trim(0)` で解放済みのメモリを OS に返しているため (sharp の場合は glibc の arena にピークのまま残る)
- **WebP**: 同じ画素からは sharp と同一のバイト列になる。画質の差は縮小処理の差。320px での差は、sharp (libvips) の縮小がこの基準よりわずかにぼけるため
- **AVIF**: sharp は `chromaSubsampling` の既定を 4:4:4 にしている (libvips 自体の既定は quality 90 未満で 4:2:0)。このライブラリは libvips の規則どおり 4:2:0 にしているので、縮小しない大きい画像では色差の分だけ sharp より約 1 点低く、ファイルは 1.5% 小さい。サムネイルでは縮小の差のほうが大きく、このライブラリが上回る
- **JPEG XL**: 同じ libjxl・同じ設定。違いは縮小と、完全に不透明な画像でアルファチャンネルを付けないこと
- **アニメーション GIF**: 両者とも libwebp でアニメーション WebP にしており、出力サイズはほぼ同じ (上の表の速度・メモリには含まれる)

## ライセンス

MIT ([LICENSE](LICENSE))。静的に組み込む libwebp・libaom・libjxl・mozjpeg などのライセンスは、リリースの tarball の `licenses/` に含まれる。
