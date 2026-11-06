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

/// Quality from which flat, sparsely changing animations are coded lossless:
/// there the palette coding of the changes is exact and smaller than lossy.
const AUTO_LOSSLESS_QUALITY: f32 = 70.0;

/// Encode composited RGBA canvases as an animated JPEG XL (loops forever).
/// `delays_cs` are GIF delays in 1/100 s, one per frame; <= 1 becomes 10 as in browsers and the WebP path.
/// Takes the canvases by value so opaque ones can be reduced to RGB in place
/// (no second copy of every frame).
///
/// Flat-colour animations with few changing pixels (screen recordings, simple
/// games) are coded lossless when `quality` is at least 70, since that is both
/// exact and smaller than lossy for them.
pub fn encode_animation(
    mut frames: Vec<Vec<u8>>,
    delays_cs: &[u32],
    width: u32,
    height: u32,
    quality: f32,
    effort: u32,
    threads: u32,
) -> Result<Vec<u8>> {
    let opaque = frames.iter().all(|f| f.chunks_exact(4).all(|p| p[3] == 255));
    if opaque {
        for f in &mut frames {
            let n = f.len() / 4;
            for i in 0..n {
                f.copy_within(i * 4..i * 4 + 3, i * 3);
            }
            f.truncate(n * 3);
            f.shrink_to_fit();
        }
    }
    let mut list = Vec::with_capacity(frames.len());
    for (data, &cs) in frames.iter().zip(delays_cs) {
        let image = if opaque {
            jxl_mini_encoder::ImageRef::rgb8(width, height, data)
        } else {
            jxl_mini_encoder::ImageRef::rgba8(width, height, data)
        }
        .map_err(|e| Error(format!("jxl: {e}")))?;
        list.push(jxl_mini_encoder::AnimationFrame { image, duration: if cs <= 1 { 10 } else { cs } });
    }
    let mut enc = jxl_mini_encoder::Encoder::new()
        .quality(quality.clamp(0.0, 100.0))
        .effort(effort)
        .threads(threads.max(1) as usize);
    if quality >= AUTO_LOSSLESS_QUALITY && jxl_mini_encoder::animation_prefers_lossless(&list) {
        enc = enc.lossless();
    }
    enc.encode_animation(&list, &jxl_mini_encoder::AnimationOptions::default())
        .map_err(|e| Error(format!("jxl: {e}")))
}

pub fn version() -> String {
    jxl_mini_encoder::VERSION.to_string()
}
