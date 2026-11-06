# imgconv-rs

画像変換だけを行う、Rust 製の Node-API アドオン。Bun と Node.js から使える。入力をデコードして縮小・向きの補正・色変換をし、WebP / AVIF / JPEG XL にエンコードするまでを 1 回のネイティブ呼び出しで行う。

sharp (libvips) は汎用の画像処理ライブラリだが、Web 配信用の画像変換で使うのは「デコード → 縮小 → WebP / AVIF / JPEG XL へのエンコード」だけであることが多い。その部分だけに絞って、変換時間とメモリ使用量がどう変わるかを確かめるために作った。測定結果は [ベンチマーク](#ベンチマーク) を参照。

## できること

| | 対応 |
|---|---|
| 入力 | JPEG (CMYK を含む)、PNG (16bit・パレット・APNG の先頭フレーム)、GIF、WebP (アニメーションは先頭フレーム)、SVG |
| 出力 | WebP、AVIF、JPEG XL (`quality: 100` で可逆)。複数フレームの GIF はアニメーションになる (JPEG XL ならアニメーション JPEG XL、それ以外はアニメーション WebP) |
| 縮小 | sharp の `fit: 'inside', withoutEnlargement: true` と同じ寸法。Lanczos3、アルファは乗算済みで処理。JPEG は sharp と同じ規則で DCT 縮小デコード (shrink-on-load)、WebP は libwebp のスケーラーで 2 の累乗まで縮小してからデコード、SVG は目標サイズで直接ラスタライズ。PNG と、DCT 縮小でも目標に届かない JPEG は、行を読みながら縮小する (展開した元の大きさの画素列はメモリに置かない。プログレッシブ JPEG は libjpeg が係数を全体ぶん持つので、その分は減らない。結果は一括の縮小と同じ) |
| 色 | 埋め込み ICC プロファイルを sRGB に変換してから出力 (sharp と同じ)。完全に不透明なアルファチャンネルは捨てる |
| 向き | EXIF Orientation を適用して正立させる |
| 安全性 | デコード前にヘッダーから画素数を調べて上限 (既定は sharp と同じ 268,402,689) を超えたら拒否。アニメーション GIF は全フレームの合計で判定 |

使っていない機能 (切り抜き、合成、フィルター、メタデータの保持、TIFF / HEIC 入力など) は持たない。

### 中身

| 処理 | ライブラリ | 版 | sharp 0.35.5 との関係 |
|---|---|---|---|
| WebP エンコード・デコード | libwebp (`libwebp-sys`) | 1.6.0 | sharp と同じ版。libvips と同じ設定 (preset default、method 4、alpha_quality 100) にしており、同じ画素からは**同一バイト列**を出力する |
| AVIF エンコード | [avif-mini-encoder](https://github.com/hamtit/avif-mini-encoder) (純 Rust、git タグで固定) | 0.1.1 | libaom 3.15.1 の代わり。libheif 1.23 が libaom に渡す設定 (all-intra、speed = 9 - effort、tune IQ、quality 90 未満は 4:2:0 (libvips の規則。sharp は既定で 4:4:4 に上書きしているが、ここでは合わせない)、BT.601 full range など) を参考にしている。出力は libaom と一致しない。1 タイルのみなので、幅 4096・面積 4096×2304 を超える画像はエラーになる |
| JPEG XL エンコード | [jxl-mini-encoder](https://github.com/hamtit/jxl-mini-encoder) (純 Rust、git タグで固定) | 0.3.0 | libjxl 0.12.0 の代わり。effort 7、quality → distance の変換は libjxl と同じ。libjxl との違いは [libjxl を使っていないことによる差](#libjxl-を使っていないことによる差) |
| JPEG デコード | mozjpeg (`mozjpeg-sys` 2.2) | | sharp と同じく mozjpeg (libjpeg-turbo 系の SIMD デコーダー) |
| PNG / GIF デコード | `png` / `gif` クレート | | Rust 実装 |
| 縮小 | `fast_image_resize` | 6 | SIMD (AVX2 / NEON) の Lanczos3 |
| 色変換 | `moxcms` | 0.9 | Rust 実装の ICC 変換 |
| SVG | `resvg` | 0.49 | テキストは描画しない (フォントを持たない) |

## 使い方

```ts
import { convert, convertRgba, probe } from 'imgconv-rs';

const result = await convert(buffer, { format: 'avif', quality: 65, maxDimension: 320 });
// result.data: Buffer, result.format: 'avif' (アニメーション GIF は 'jxl' 指定なら 'jxl'、それ以外は 'webp'),
// result.width / result.height / result.frames, result.input: 入力のヘッダー情報

probe(buffer); // { format: 'jpeg', width, height, frames, orientation } (画素はデコードしない)
await convertRgba(rgba, width, height, { format: 'webp' }); // ICO から取り出した RGBA など
```

変換はスレッドプール (libuv / Bun の Node-API ワーカー) で動き、JavaScript のスレッドを止めない。オプションは `index.d.ts` を参照。

- `threads` (既定 1): 1 枚の変換に使うエンコーダーのスレッド数 (AVIF / JPEG XL)。並行に何枚も変換するサーバーでは 1 が最も速い ([ベンチマーク](#ベンチマーク))
- 変換が 1 つも動いていない状態になると `malloc_trim(0)` を呼び、解放済みのヒープを OS に返す (glibc のみ)

## ビルド

Linux (glibc / musl、x64 / arm64) 向け。必要なもの: Rust、C コンパイラー、nasm (x64。無いと mozjpeg の SIMD デコードが無効になる)。

```bash
./scripts/build.sh
```

AVIF と JPEG XL のエンコーダーは純 Rust (avif-mini-encoder、jxl-mini-encoder) で、libaom も CMake も要らない。できた `imgconv-rs.linux-<arch>-<libc>.node` を `index.js` が読み込む。

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
| AVIF | 320 | 4.34 → 1.75 s (**-60%**) | 16.0 → 6.6 s (-59%) | 295 → 128 MB (**-57%**) | 295 → 119 MB |
| AVIF | 1920 | 44.4 → 24.5 s (**-45%**) | 162 → 91 s (-44%) | 1198 → 225 MB (**-81%**) | 1017 → 144 MB |
| JPEG XL | 320 | 1.28 → 0.85 s (**-33%**) | 4.4 → 3.2 s (-27%) | 332 → 153 MB (**-54%**) | 326 → 123 MB |
| JPEG XL | 1920 | 9.05 → 7.73 s (**-15%**) | 33.0 → 27.2 s (-18%) | 1326 → 677 MB (**-49%**) | 1200 → 84 MB |

1 枚ずつ (並列なし) 変換した場合 (最大辺 1920、29 枚の合計):

| 形式 | sharp | このライブラリ 1 スレッド | このライブラリ 4 スレッド |
|---|---|---|---|
| AVIF | 164 s / 472 MB | 91 s / 100 MB | 86 s / 121 MB |
| JPEG XL | 33.2 s / 506 MB | 31.1 s / 227 MB | 13.7 s / 321 MB |

### 画質 (同じ品質設定での出力サイズと SSIMULACRA2)

基準画像は両者のどちらとも違う実装 (Pillow で原寸から Lanczos 縮小、EXIF 回転・sRGB 変換済み) で作り (`bench/make-reference.py`)、静止画 26 枚の平均を取った (`bench/quality.ts`)。SSIMULACRA2 は高いほど良い。

| 形式 | 最大辺 | 出力サイズ合計 sharp / このライブラリ | SSIMULACRA2 sharp / このライブラリ |
|---|---|---|---|
| WebP | 320 | 279 / 278 KB | 64.8 / **67.4** |
| WebP | 1920 | 4.56 / 4.59 MB | 66.1 / 66.5 |
| AVIF | 320 | 317 / 291 KB | 73.4 / **75.5** |
| AVIF | 1920 | 4.94 / 4.96 MB | **79.6** / 78.2 |
| JPEG XL | 320 | 231 / 231 KB | 62.0 / **64.7** |
| JPEG XL | 1920 | 3.34 / 3.34 MB | 66.3 / 66.5 |

### 結果の読み方

- **速くなった理由**: WebP は sharp と同じ libwebp なので、差は周辺から来ている。AVIF は sharp の libaom 3.15.1 と、このライブラリの avif-mini-encoder という別のエンコーダーの比較で、差にはエンコーダー自体の速さも含まれる。sharp は変換 1 枚ごとに libaom を CPU 数のスレッドで動かすため、10 並列だとスレッドが CPU 数を大きく超えて奪い合う。このライブラリは 1 枚 1 スレッドで、並列数は Node-API のスレッドプール (CPU 数) で決まる。JPEG XL は比較側では sharp で RGBA に展開してから Worker に転送するが、それがなくなった (Worker の起動や JavaScript ヒープも不要)
- **メモリが減った理由**: 並列で同時に動くエンコードの数が CPU 数に抑えられることと、libvips の常駐分・キャッシュがないこと。変換後の RSS が大きく下がるのは、変換が 1 つも動いていないときに `malloc_trim(0)` で解放済みのメモリを OS に返しているため (sharp の場合は glibc の arena にピークのまま残る)
- **WebP**: 同じ画素からは sharp と同一のバイト列になる。画質の差は縮小処理の差。320px での差は、sharp (libvips) の縮小がこの基準よりわずかにぼけるため
- **AVIF**: エンコーダーが違うので出力は libaom と一致しない。サムネイル (320) では sharp よりファイルが 8% 小さく SSIMULACRA2 も 2.2 点高いが、縮小しない大きい画像 (1920) では 1.3 点低く、ファイルは 0.3% 大きい。sharp は `chromaSubsampling` の既定を 4:4:4 にしている (libvips 自体の既定は quality 90 未満で 4:2:0) が、このライブラリは libvips の規則どおり 4:2:0 にしており、その分の色差も 1920 での差に含まれる。1 枚ずつ変換するときの `threads` は CDEF の強度探索などにしか効かず、1 → 4 スレッドで 91 → 86 s と効果が小さい (libaom 版は 136 → 43 s だった)
- **JPEG XL**: この表の JPEG XL の数値は、libjxl 0.12.0 を使っていた版 (v0.2.0) で測ったもの。現在は jxl-mini-encoder に替えており、再測定していない (差は次の節)。当時の違いは縮小と、完全に不透明な画像でアルファチャンネルを付けないこと
- **アニメーション GIF**: 両者とも libwebp でアニメーション WebP にしており、出力サイズはほぼ同じ (上の表の速度・メモリには含まれる)

### libjxl を使っていないことによる差

JPEG XL のエンコーダーを libjxl 0.12.0 から jxl-mini-encoder 0.1.0 に替えた。libjxl のアルゴリズムと定数を元にした純 Rust 実装だが、出力は libjxl と同じにならない。替える前に、同じ画素・effort 7・1 スレッドで両者を比べた (Kodak 2 枚と写真 3 枚 1200×800 / 1920×1280 / 4000×2667、同じマシン、時間は 3 回の最速値、メモリは入力読み込み後の増分の最大 RSS)。

| | libjxl 0.12.0 | jxl-mini-encoder 0.1.0 |
|---|---|---|
| 時間 (非可逆 q75 / q90) | 基準 | ほぼ同じ (±15%、ばらつきの範囲)。4 スレッドでも同じ |
| 時間 (可逆) | 基準 | 7〜41% 速い |
| メモリ (非可逆) | 21〜287 MB | 3〜188 MB (768×512 と 1920×1280 で約 1/4〜1/6、4000×2667 で約 2/3) |
| メモリ (可逆) | 24〜153 MB | 11〜40 MB |
| サイズ (可逆) | 基準 | 2.4〜4.7% 大きい。画素は完全に一致 |
| 画質 (非可逆、同じ quality) | 基準 | SSIMULACRA2 が 0.3〜2.0 点低く、ファイルも小さい |
| サイズ (非可逆、同じ SSIMULACRA2) | 基準 | 低〜中画質 (65〜75) で 0〜4% 大きく、高画質 (80〜85) で ±1% 以内 (Kodak 2 枚) |

- 同じ `quality` を指定すると、libjxl より少し画質が低くサイズも小さい。画質を揃えたいときは `quality` を数ポイント上げる
- 未実装: AFV、ノイズ合成、スプライン、butteraugli による反復量子化。入力は 8 bit sRGB の RGB / RGBA だけ (このライブラリが渡す画素はもともとそうなっている)
- 出力は ICC を持たない裸のコードストリーム (`FF 0A`)。jxl-oxide と libjxl に加えて jxl-rs (`jxl_cli`) でデコードできることを確認した
- 未測定: RGBA の画質とサイズ、effort 7 以外、4000×2667 の可逆でのメモリと時間 (jxl-mini-encoder 側)、wasm ビルドでの動作 (CI で確認する)

## ライセンス

MIT ([LICENSE](LICENSE))。静的に組み込む libwebp・mozjpeg などのライセンスは、リリースの tarball の `licenses/` に含まれる。
