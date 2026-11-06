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

/// Encode composited RGBA canvases as an animated JPEG XL (loops forever).
/// `delays_cs` are GIF delays in 1/100 s, one per frame; 0 becomes 10 as in browsers.
pub fn encode_animation(
    frames: &[Vec<u8>],
    delays_cs: &[u32],
    width: u32,
    height: u32,
    quality: f32,
    effort: u32,
    threads: u32,
) -> Result<Vec<u8>> {
    let opaque = frames.iter().all(|f| f.chunks_exact(4).all(|p| p[3] == 255));
    let rgb: Vec<Vec<u8>>;
    let sources: Vec<&[u8]> = if opaque {
        rgb = frames.iter().map(|f| f.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()).collect();
        rgb.iter().map(|v| v.as_slice()).collect()
    } else {
        frames.iter().map(|v| v.as_slice()).collect()
    };
    let mut list = Vec::with_capacity(frames.len());
    for (data, &cs) in sources.iter().zip(delays_cs) {
        let image = if opaque {
            jxl_mini_encoder::ImageRef::rgb8(width, height, data)
        } else {
            jxl_mini_encoder::ImageRef::rgba8(width, height, data)
        }
        .map_err(|e| Error(format!("jxl: {e}")))?;
        list.push(jxl_mini_encoder::AnimationFrame { image, duration: if cs == 0 { 10 } else { cs } });
    }
    jxl_mini_encoder::Encoder::new()
        .quality(quality.clamp(0.0, 100.0))
        .effort(effort)
        .threads(threads.max(1) as usize)
        .encode_animation(&list, &jxl_mini_encoder::AnimationOptions::default())
        .map_err(|e| Error(format!("jxl: {e}")))
}

pub fn version() -> String {
    jxl_mini_encoder::VERSION.to_string()
}
