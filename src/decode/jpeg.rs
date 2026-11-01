use mozjpeg::{Decompress, Format, Marker};

use super::Decoded;
use crate::color;
use crate::error::{Result, err};
use crate::image::{Image, expand_gray};

/// Decode a JPEG, shrinking by `shrink` (1, 2, 4 or 8) in the DCT domain.
/// libjpeg errors unwind; the caller wraps conversions in catch_unwind.
pub fn decode(buf: &[u8], shrink: u32) -> Result<Decoded> {
    let markers = [Marker::APP(1), Marker::APP(2), Marker::APP(14)];
    let mut dec = Decompress::with_markers(&markers).from_mem(buf)?;
    if shrink > 1 {
        dec.scale((8 / shrink) as u8);
    }

    let mut orientation = 1;
    let mut icc_chunks: Vec<(u8, &[u8])> = Vec::new();
    let mut adobe = false;
    for m in dec.markers() {
        match m.marker {
            Marker::APP(1) if m.data.starts_with(b"Exif\0\0") => {
                orientation = crate::orient::exif_orientation(&m.data[6..]).unwrap_or(1);
            }
            Marker::APP(2) if m.data.len() > 14 && m.data.starts_with(b"ICC_PROFILE\0") => {
                icc_chunks.push((m.data[12], &m.data[14..]));
            }
            Marker::APP(14) if m.data.starts_with(b"Adobe") => adobe = true,
            _ => {}
        }
    }
    icc_chunks.sort_by_key(|c| c.0);
    let icc: Option<Vec<u8>> =
        (!icc_chunks.is_empty()).then(|| icc_chunks.iter().flat_map(|c| c.1.iter().copied()).collect());

    let image = match dec.image()? {
        Format::RGB(mut d) => {
            let (w, h) = (d.width() as u32, d.height() as u32);
            let data = d.read_scanlines::<u8>()?;
            d.finish()?;
            Image::new(w, h, 3, data)
        }
        Format::Gray(mut d) => {
            let (w, h) = (d.width() as u32, d.height() as u32);
            let data = d.read_scanlines::<u8>()?;
            d.finish()?;
            Image::new(w, h, 3, expand_gray(&data, false))
        }
        Format::CMYK(mut d) => {
            let (w, h) = (d.width() as u32, d.height() as u32);
            let data = d.read_scanlines::<u8>()?;
            d.finish()?;
            let img = color::cmyk_to_rgb(&data, w, h, icc.as_deref(), adobe);
            return Ok(Decoded { image: img, icc: None, orientation });
        }
    };
    if image.data.len() != image.width as usize * image.height as usize * 3 {
        return Err(err("Truncated JPEG"));
    }
    Ok(Decoded { image, icc, orientation })
}
