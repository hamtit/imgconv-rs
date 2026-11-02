//! Lanczos3 downscaling (sharp's default kernel), alpha-aware.

use fast_image_resize::images::{Image as FirImage, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::error::{Result, err};
use crate::image::{Image, ImageView};

/// Target size for sharp's `fit: 'inside', withoutEnlargement: true`, or None
/// when the image already fits.
pub fn fit_inside(width: u32, height: u32, max: u32) -> Option<(u32, u32)> {
    if max == 0 || (width <= max && height <= max) {
        return None;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let m = f64::from(max);
    Some(if w >= h {
        (max, ((h * m / w).round() as u32).max(1))
    } else {
        (((w * m / h).round() as u32).max(1), max)
    })
}

pub fn resize(img: &ImageView, width: u32, height: u32) -> Result<Image> {
    if img.width == width && img.height == height {
        return Ok(Image::new(width, height, img.channels, img.data.to_vec()));
    }
    let pixel_type = if img.channels == 4 { PixelType::U8x4 } else { PixelType::U8x3 };
    let src = ImageRef::new(img.width, img.height, img.data, pixel_type).map_err(|e| err(format!("resize: {e}")))?;
    let mut dst = FirImage::new(width, height, pixel_type);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)).use_alpha(img.channels == 4);
    Resizer::new().resize(&src, &mut dst, &opts).map_err(|e| err(format!("resize: {e}")))?;
    Ok(Image::new(width, height, img.channels, dst.into_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_like_sharp() {
        assert_eq!(fit_inside(100, 50, 200), None);
        assert_eq!(fit_inside(4000, 3000, 320), Some((320, 240)));
        assert_eq!(fit_inside(3000, 4000, 320), Some((240, 320)));
        assert_eq!(fit_inside(1000, 3, 100), Some((100, 1)));
    }
}
