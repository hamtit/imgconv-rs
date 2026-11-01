//! Still-image decoders. Each returns sRGB-ready 8-bit pixels plus the
//! metadata the pipeline needs (embedded ICC profile, EXIF orientation).

pub mod avif;
pub mod gif;
pub mod jpeg;
pub mod png;
pub mod svg;
pub mod webp;

use crate::image::Image;

pub struct Decoded {
    pub image: Image,
    /// Embedded ICC profile for RGB data, applied by the pipeline.
    pub icc: Option<Vec<u8>>,
    /// EXIF orientation (1-8) still to be applied.
    pub orientation: u8,
}

impl Decoded {
    pub fn plain(image: Image) -> Self {
        Decoded { image, icc: None, orientation: 1 }
    }
}

/// Power-of-two shrink (1, 2, 4 or 8) a decoder may apply while decoding,
/// using sharp's JPEG shrink-on-load rule (`fastShrinkOnLoad`): the largest
/// factor not above the required shrink, halved when the shrink's integer
/// part equals it (libjpeg rounding would otherwise leave the decoded image
/// slightly smaller than needed and blur the result).
pub fn shrink_on_load(width: u32, height: u32, target: (u32, u32)) -> u32 {
    let shrink = (f64::from(width) / f64::from(target.0)).min(f64::from(height) / f64::from(target.1));
    let mut factor = match shrink {
        s if s >= 8.0 => 8,
        s if s >= 4.0 => 4,
        s if s >= 2.0 => 2,
        _ => 1,
    };
    if factor > 1 && shrink as u32 == factor {
        factor /= 2;
    }
    factor
}

#[cfg(test)]
mod tests {
    use super::shrink_on_load;

    #[test]
    fn matches_sharp_rule() {
        assert_eq!(shrink_on_load(4000, 2667, (1920, 1280)), 1); // 2.08
        assert_eq!(shrink_on_load(1600, 1067, (320, 213)), 4); // 5.0
        assert_eq!(shrink_on_load(4000, 2667, (320, 213)), 8); // 12.5
        assert_eq!(shrink_on_load(1280, 960, (320, 240)), 2); // 4.0
        assert_eq!(shrink_on_load(2560, 1920, (320, 240)), 4); // 8.0
        assert_eq!(shrink_on_load(1000, 1000, (900, 900)), 1);
    }
}
