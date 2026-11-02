export type OutputFormat = 'webp' | 'avif' | 'jxl';
export type InputFormat = 'jpeg' | 'png' | 'gif' | 'webp' | 'avif' | 'svg';

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
  /** Convert animated GIFs to animated WebP regardless of `format` (default true). */
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
  /** More than 1 only for animated WebP. */
  frames: number;
  input: ImageInfo;
}

/** Convert an encoded image (JPEG, PNG, GIF, WebP, AVIF or SVG). Runs off the JS thread. */
export function convert(input: Buffer | Uint8Array, options: ConvertOptions, signal?: AbortSignal): Promise<ConvertResult>;
/** Convert raw 8-bit RGBA pixels (straight alpha), e.g. a decoded ICO entry. */
export function convertRgba(
  rgba: Buffer | Uint8Array,
  width: number,
  height: number,
  options: ConvertOptions,
  signal?: AbortSignal,
): Promise<ConvertResult>;
/**
 * Limit the estimated memory of conversions running at once, in megabytes
 * (0 = unlimited, the default; `IMGCONV_MEMORY_BUDGET_MB` sets the initial
 * value). A conversion over the limit waits on its worker thread; one that
 * alone exceeds it runs when nothing else is running.
 */
export function setMemoryBudget(megabytes: number): void;
/** Read format, size and frame count from the header without decoding pixels. */
export function probe(input: Buffer | Uint8Array): ImageInfo;
export function versions(): { imgconvRs: string; libwebp: string; libaom: string; libjxl: string };
