//! JPEG XL encoding with jxl-mini-encoder (pure Rust): effort 7, distance from
//! the 0-100 quality (lossless at 100), sRGB. The output is a bare codestream.

use crate::error::{Error, Result};
use crate::image::Image;

pub const DEFAULT_EFFORT: u32 = 7;

pub fn encode(img: &Image, quality: f32, effort: u32, threads: u32) -> Result<Vec<u8>> {
    let pixels = if img.has_alpha() {
        jxl_mini_encoder::ImageRef::rgba8(img.width, img.height, &img.data)
    } else {
        jxl_mini_encoder::ImageRef::rgb8(img.width, img.height, &img.data)
    }
    .map_err(|e| Error(format!("jxl: {e}")))?;
    jxl_mini_encoder::Encoder::new()
        .quality(quality.clamp(0.0, 100.0))
        .effort(effort)
        .threads(threads.max(1) as usize)
        .encode(&pixels)
        .map_err(|e| Error(format!("jxl: {e}")))
}

pub fn version() -> String {
    jxl_mini_encoder::VERSION.to_string()
}
