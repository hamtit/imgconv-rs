//! SVG rasterization with resvg. Text is not rendered (no font database).
//! `<image>` references are never resolved: neither files (an untrusted SVG
//! must not read the server's disk) nor data URLs (an embedded raster would
//! be decoded at full size, bypassing the pixel limit checked on the SVG).

use resvg::tiny_skia;
use resvg::usvg;

use crate::error::{Error, Result};
use crate::image::Image;

fn parse(buf: &[u8]) -> Result<usvg::Tree> {
    let options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(buf, &options).map_err(|e| Error::InvalidImage(format!("Invalid SVG: {e}")))
}

/// Intrinsic size in pixels (rounded up, at least 1).
pub fn probe(buf: &[u8]) -> Result<(u32, u32)> {
    let size = parse(buf)?.size();
    Ok((size.width().ceil().max(1.0) as u32, size.height().ceil().max(1.0) as u32))
}

/// Render at `size` (the intrinsic size when None). Vector input is rendered
/// directly at the target size instead of being resized afterwards.
pub fn decode(buf: &[u8], size: Option<(u32, u32)>) -> Result<Image> {
    let tree = parse(buf)?;
    let intrinsic = tree.size();
    let (w, h) = size.unwrap_or((intrinsic.width().ceil().max(1.0) as u32, intrinsic.height().ceil().max(1.0) as u32));
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| Error::InvalidImage("Invalid SVG dimensions".into()))?;
    let transform = tiny_skia::Transform::from_scale(w as f32 / intrinsic.width(), h as f32 / intrinsic.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut data = pixmap.take();
    // tiny-skia stores premultiplied RGBA.
    for p in data.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(Image::new(w, h, 4, data))
}
