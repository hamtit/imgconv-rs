//! C ABI for the WebAssembly build (wasm32-unknown-emscripten). Every
//! function takes and returns numbers only; `packages/wasm/index.js` wraps
//! them into the same `convert` / `convertRgba` / `probe` API as the addon.
//!
//! A conversion returns a handle to a `Output`: eleven `u32` fields that
//! JavaScript reads straight from `HEAPU32`, followed by the encoded data
//! (kept alive until `imgconv_result_free`).

use std::cell::RefCell;
use std::ffi::{CString, c_char};

use crate::convert::{self, DEFAULT_MAX_PIXELS, Options, OutputFormat};
use crate::probe::{self, Format};

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: &str) {
    let msg = CString::new(msg.replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = msg);
}

/// Input format names, indexed by the codes in the result.
pub const INPUT_FORMATS: [&str; 5] = ["jpeg", "png", "gif", "webp", "svg"];

fn input_code(f: Format) -> u32 {
    match f {
        Format::Jpeg => 0,
        Format::Png => 1,
        Format::Gif => 2,
        Format::Webp => 3,
        Format::Svg => 4,
    }
}

/// Field order is the layout contract with `packages/wasm/index.js`.
#[repr(C)]
pub struct WasmResult {
    data_ptr: u32,
    data_len: u32,
    /// 0 webp, 1 avif, 2 jxl
    format: u32,
    width: u32,
    height: u32,
    frames: u32,
    /// Index into `INPUT_FORMATS`
    in_format: u32,
    in_width: u32,
    in_height: u32,
    in_frames: u32,
    in_orientation: u32,
    data: Vec<u8>,
}

fn output_code(f: OutputFormat) -> u32 {
    match f {
        OutputFormat::Webp => 0,
        OutputFormat::Avif => 1,
        OutputFormat::Jxl => 2,
    }
}

fn options(
    format: u32,
    quality: f32,
    max_dimension: u32,
    effort: i32,
    max_pixels: f64,
    animated: u32,
) -> Option<Options> {
    let format = match format {
        0 => OutputFormat::Webp,
        1 => OutputFormat::Avif,
        2 => OutputFormat::Jxl,
        _ => {
            set_error("Unsupported output format");
            return None;
        }
    };
    if !(0.0..=100.0).contains(&quality) {
        set_error(&format!("Invalid quality: {quality}"));
        return None;
    }
    let mut o = Options::new(format, quality);
    o.max_dimension = (max_dimension > 0).then_some(max_dimension);
    // Negative means "default"; 0 is a real (fastest) setting.
    o.effort = u32::try_from(effort).ok();
    o.max_pixels = if max_pixels > 0.0 { max_pixels as u64 } else { DEFAULT_MAX_PIXELS };
    o.animated = animated != 0;
    // There are no threads in this build.
    o.threads = 1;
    Some(o)
}

fn finish(r: crate::Result<convert::Output>) -> *mut WasmResult {
    match r {
        Ok(out) => {
            let mut res = Box::new(WasmResult {
                data_ptr: 0,
                data_len: out.data.len() as u32,
                format: output_code(out.format),
                width: out.width,
                height: out.height,
                frames: out.frames,
                in_format: input_code(out.input.format),
                in_width: out.input.width,
                in_height: out.input.height,
                in_frames: out.input.frames,
                in_orientation: u32::from(out.input.orientation),
                data: out.data,
            });
            res.data_ptr = res.data.as_ptr() as usize as u32;
            Box::into_raw(res)
        }
        Err(e) => {
            set_error(&e.0);
            std::ptr::null_mut()
        }
    }
}

/// Convert an encoded image. Returns null on failure (see `imgconv_last_error`).
///
/// `format`: 0 webp, 1 avif, 2 jxl. `max_dimension` of 0 means no limit and a negative
/// `effort` the default; `max_pixels` of 0 means the default limit.
///
/// # Safety
/// `data` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imgconv_convert(
    data: *const u8,
    len: usize,
    format: u32,
    quality: f32,
    max_dimension: u32,
    effort: i32,
    max_pixels: f64,
    animated: u32,
) -> *mut WasmResult {
    let Some(opts) = options(format, quality, max_dimension, effort, max_pixels, animated) else {
        return std::ptr::null_mut();
    };
    let input = unsafe { std::slice::from_raw_parts(data, len) };
    finish(convert::convert(input, &opts))
}

/// Convert raw 8-bit RGBA pixels (straight alpha).
///
/// # Safety
/// `rgba` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imgconv_convert_rgba(
    rgba: *const u8,
    len: usize,
    width: u32,
    height: u32,
    format: u32,
    quality: f32,
    max_dimension: u32,
    effort: i32,
    max_pixels: f64,
    animated: u32,
) -> *mut WasmResult {
    let Some(opts) = options(format, quality, max_dimension, effort, max_pixels, animated) else {
        return std::ptr::null_mut();
    };
    let input = unsafe { std::slice::from_raw_parts(rgba, len) };
    finish(convert::convert_rgba(input, width, height, &opts))
}

/// Header-only probe. Writes `[format, width, height, frames, orientation]`
/// (five `u32`) to `out` and returns 0, or returns -1 on failure.
///
/// # Safety
/// `data` must point to `len` readable bytes and `out` to five writable `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imgconv_probe(data: *const u8, len: usize, out: *mut u32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(data, len) };
    match probe::probe(input) {
        Ok(i) => {
            let v = [input_code(i.format), i.width, i.height, i.frames, u32::from(i.orientation)];
            unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len()) };
            0
        }
        Err(e) => {
            set_error(&e.0);
            -1
        }
    }
}

/// Free a result returned by `imgconv_convert*`.
///
/// # Safety
/// `result` must come from `imgconv_convert*` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imgconv_result_free(result: *mut WasmResult) {
    if !result.is_null() {
        drop(unsafe { Box::from_raw(result) });
    }
}

/// NUL-terminated message of the last failure on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn imgconv_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// `"<imgconv-rs>\n<libwebp>\n<avif-mini-encoder>\n<jxl-mini-encoder>"`, NUL-terminated and static.
#[unsafe(no_mangle)]
pub extern "C" fn imgconv_versions() -> *const c_char {
    thread_local! {
        static V: CString = {
            let v = convert::versions();
            CString::new(format!("{}\n{}\n{}\n{}", env!("CARGO_PKG_VERSION"), v.libwebp, v.avif_mini_encoder, v.jxl_mini_encoder)).unwrap_or_default()
        };
    }
    V.with(|v| v.as_ptr())
}
