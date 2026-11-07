//! Node-API bindings (Node.js / Bun). Conversions run on the libuv thread
//! pool via AsyncTask, so they never block the JavaScript thread.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::convert::{self, DEFAULT_MAX_PIXELS, Options, OutputFormat};

/// Conversions currently running on the thread pool.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Return freed heap memory to the OS once no conversion is running. glibc
/// keeps freed memory in per-thread arenas, so without this the process
/// stays at its peak RSS after a burst of conversions.
fn trim_when_idle() {
    if ACTIVE.fetch_sub(1, Ordering::AcqRel) == 1 {
        crate::memory::trim();
    }
}
use crate::probe as probe_mod;

#[napi(object)]
pub struct ConvertOptions {
    /// "webp" | "avif" | "jxl"
    pub format: String,
    /// 0-100 (default 65). 100 means lossless for JPEG XL.
    pub quality: Option<f64>,
    /// Fit within this many pixels on each side, never enlarging.
    pub max_dimension: Option<u32>,
    /// Encoder effort (default: WebP 4, AVIF 4, JPEG XL 7).
    pub effort: Option<u32>,
    /// Pixel limit checked from the header before decoding (default 268402689).
    pub max_pixels: Option<f64>,
    /// Convert animated GIFs to animations: animated JPEG XL for `jxl`, animated WebP otherwise (default true).
    pub animated: Option<bool>,
    /// Encoder threads for this conversion (default 1).
    pub threads: Option<u32>,
}

#[napi(object)]
pub struct ImageInfo {
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub orientation: u32,
}

#[napi(object)]
pub struct ConvertResult {
    pub data: Buffer,
    /// Output format; "webp" for animated GIFs.
    pub format: String,
    pub width: u32,
    pub height: u32,
    /// More than 1 only for animations.
    pub frames: u32,
    pub input: ImageInfo,
}

fn to_options(o: &ConvertOptions) -> Result<Options> {
    let format = OutputFormat::parse(&o.format)
        .ok_or_else(|| Error::new(Status::InvalidArg, format!("Unsupported output format: {}", o.format)))?;
    let quality = o.quality.unwrap_or(65.0);
    if !(0.0..=100.0).contains(&quality) {
        return Err(Error::new(Status::InvalidArg, format!("Invalid quality: {quality}")));
    }
    let mut opts = Options::new(format, quality as f32);
    opts.max_dimension = o.max_dimension.filter(|&d| d > 0);
    opts.effort = o.effort;
    opts.max_pixels = o.max_pixels.map_or(DEFAULT_MAX_PIXELS, |p| p.max(0.0) as u64);
    opts.animated = o.animated.unwrap_or(true);
    opts.threads = o.threads.unwrap_or(1).max(1);
    Ok(opts)
}

fn info(i: &probe_mod::Info) -> ImageInfo {
    ImageInfo {
        format: i.format.name().into(),
        width: i.width,
        height: i.height,
        frames: i.frames,
        orientation: u32::from(i.orientation),
    }
}

enum Input {
    Encoded(Buffer),
    Rgba(Buffer, u32, u32),
}

pub struct ConvertTask {
    input: Input,
    /// Invalid options are reported by rejecting the promise, like any other failure.
    options: Result<Options>,
}

impl Task for ConvertTask {
    type Output = convert::Output;
    type JsValue = ConvertResult;

    fn compute(&mut self) -> Result<Self::Output> {
        let options = match &self.options {
            Ok(o) => o,
            Err(e) => return Err(Error::new(e.status, e.reason.clone())),
        };
        ACTIVE.fetch_add(1, Ordering::AcqRel);
        let r = match &self.input {
            Input::Encoded(b) => convert::convert(b, options),
            Input::Rgba(b, w, h) => convert::convert_rgba(b, *w, *h, options),
        };
        trim_when_idle();
        r.map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
    }

    fn resolve(&mut self, _env: Env, out: Self::Output) -> Result<Self::JsValue> {
        Ok(ConvertResult {
            input: info(&out.input),
            format: out.format.name().into(),
            width: out.width,
            height: out.height,
            frames: out.frames,
            data: out.data.into(),
        })
    }
}

/// Convert an encoded image (JPEG, PNG, GIF, WebP, AVIF or SVG).
#[napi(ts_return_type = "Promise<ConvertResult>")]
pub fn convert(input: Buffer, options: ConvertOptions, signal: Option<AbortSignal>) -> AsyncTask<ConvertTask> {
    let options = to_options(&options);
    AsyncTask::with_optional_signal(ConvertTask { input: Input::Encoded(input), options }, signal)
}

/// Convert raw 8-bit RGBA pixels (straight alpha), e.g. a decoded ICO entry.
#[napi(ts_return_type = "Promise<ConvertResult>")]
pub fn convert_rgba(
    rgba: Buffer,
    width: u32,
    height: u32,
    options: ConvertOptions,
    signal: Option<AbortSignal>,
) -> AsyncTask<ConvertTask> {
    let options = to_options(&options);
    AsyncTask::with_optional_signal(ConvertTask { input: Input::Rgba(rgba, width, height), options }, signal)
}

/// Read format, size and frame count from the header without decoding pixels.
#[napi]
pub fn probe(input: Buffer) -> Result<ImageInfo> {
    probe_mod::probe(&input).map(|i| info(&i)).map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
}

#[napi(object)]
pub struct Versions {
    pub imgconv_rs: String,
    pub libwebp: String,
    pub avif_mini_encoder: String,
    pub jxl_mini_encoder: String,
}

#[napi]
pub fn versions() -> Versions {
    let v = convert::versions();
    Versions {
        imgconv_rs: env!("CARGO_PKG_VERSION").into(),
        libwebp: v.libwebp,
        avif_mini_encoder: v.avif_mini_encoder,
        jxl_mini_encoder: v.jxl_mini_encoder,
    }
}
