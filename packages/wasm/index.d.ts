export type OutputFormat = 'webp' | 'avif' | 'jxl';
export type InputFormat = 'jpeg' | 'png' | 'gif' | 'webp' | 'svg';

export interface ConvertOptions {
  format: OutputFormat;
  /** 0-100 (default 65). 100 means lossless for JPEG XL. */
  quality?: number;
  /** Fit within this many pixels on each side, never enlarging. */
  maxDimension?: number;
  /** Encoder effort (default: WebP 4, AVIF 4, JPEG XL 7). */
  effort?: number;
  /** Pixel limit checked from the header before decoding (default 268402689). */
  maxPixels?: number;
  /** Convert animated GIFs to animations: animated JPEG XL for `jxl`, animated WebP otherwise (default true). */
  animated?: boolean;
}

export interface ImageInfo {
  format: InputFormat;
  /** Stored size before EXIF orientation. For GIF, the logical screen. */
  width: number;
  height: number;
  frames: number;
  /** EXIF orientation 1-8 (JPEG only; 1 otherwise). */
  orientation: number;
}

export interface ConvertResult {
  data: Uint8Array;
  /** Output format; 'webp' for animated GIFs. */
  format: OutputFormat;
  width: number;
  height: number;
  /** More than 1 only for animations. */
  frames: number;
  input: ImageInfo;
}

export interface Imgconv {
  convert(input: Uint8Array, options: ConvertOptions): ConvertResult;
  convertRgba(rgba: Uint8Array, width: number, height: number, options: ConvertOptions): ConvertResult;
  probe(input: Uint8Array): ImageInfo;
  versions(): { imgconvRs: string; libwebp: string; libaom: string; jxlMiniEncoder: string };
}

export interface ImgconvModuleOptions {
  /** Override where the .wasm file is loaded from. */
  locateFile?(path: string, prefix: string): string;
  /** Pass the module bytes instead of fetching them. */
  wasmBinary?: ArrayBuffer | Uint8Array;
  [key: string]: unknown;
}

export default function createImgconv(options?: ImgconvModuleOptions): Promise<Imgconv>;
