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
  /** Pixel limit checked from the header before decoding (default 268402689, sharp's default). */
  maxPixels?: number;
  /** Convert animated GIFs to animations: animated JPEG XL for `jxl`, animated WebP otherwise (default true). */
  animated?: boolean;
  /** Encoder threads for this conversion (default 1). */
  threads?: number;
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
  data: Buffer;
  /** Output format; 'webp' for animated GIFs. */
  format: OutputFormat;
  width: number;
  height: number;
  /** More than 1 only for animations. */
  frames: number;
  input: ImageInfo;
}

/** Convert an encoded image (JPEG, PNG, GIF, WebP or SVG). Runs off the JS thread. */
export function convert(input: Buffer | Uint8Array, options: ConvertOptions, signal?: AbortSignal): Promise<ConvertResult>;
/** Convert raw 8-bit RGBA pixels (straight alpha), e.g. a decoded ICO entry. */
export function convertRgba(
  rgba: Buffer | Uint8Array,
  width: number,
  height: number,
  options: ConvertOptions,
  signal?: AbortSignal,
): Promise<ConvertResult>;
/** Read format, size and frame count from the header without decoding pixels. */
export function probe(input: Buffer | Uint8Array): ImageInfo;
export function versions(): { imgconvRs: string; libwebp: string; libaom: string; jxlMiniEncoder: string };
