use mozjpeg::{Decompress, Format, Marker};

use super::Decoded;
use crate::color;
use crate::error::{Error, Result};
use crate::image::{Image, expand_gray};
use crate::stream::StreamResizer;

/// Scanlines read per call when streaming.
const CHUNK_ROWS: usize = 16;

/// Read an RGB or gray JPEG scanline chunk by scanline chunk, shrinking to
/// `target` as rows arrive. `gray` expands each chunk to RGB.
fn read_streamed<R: std::io::BufRead>(
    mut d: mozjpeg::decompress::DecompressStarted<R>,
    target: (u32, u32),
    gray: bool,
) -> Result<Image> {
    let (w, h) = (d.width(), d.height());
    let comps = if gray { 1 } else { 3 };
    let mut stream = StreamResizer::new(w as u32, h as u32, target.0, target.1, 3)?;
    let mut chunk = vec![0u8; w * comps * CHUNK_ROWS];
    let mut done = 0;
    while done < h {
        let n = CHUNK_ROWS.min(h - done);
        let rows = d.read_scanlines_into(&mut chunk[..w * comps * n])?;
        if gray {
            for row in rows.chunks_exact(w) {
                stream.push_row(&expand_gray(row, false))?;
            }
        } else {
            for row in rows.chunks_exact(w * 3) {
                stream.push_row(row)?;
            }
        }
        done += n;
    }
    d.finish()?;
    Ok(Image::new(target.0, target.1, 3, stream.finish()?))
}

/// True when the (already DCT-shrunk) `w x h` image should be shrunk further.
fn needs_stream(target: Option<(u32, u32)>, w: usize, h: usize) -> Option<(u32, u32)> {
    target.filter(|&(tw, th)| (tw as usize) <= w && (th as usize) <= h && (tw as usize, th as usize) != (w, h))
}

/// Decode a JPEG, shrinking by `shrink` (1, 2, 4 or 8) in the DCT domain and
/// then, when `target` is smaller than the result, row by row while decoding.
/// libjpeg errors unwind; the caller wraps conversions in catch_unwind.
pub fn decode(buf: &[u8], shrink: u32, target: Option<(u32, u32)>) -> Result<Decoded> {
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
            if let Some(t) = needs_stream(target, d.width(), d.height()) {
                let image = read_streamed(d, t, false)?;
                return Ok(Decoded { image, icc, orientation });
            }
            let (w, h) = (d.width() as u32, d.height() as u32);
            let data = d.read_scanlines::<u8>()?;
            d.finish()?;
            Image::new(w, h, 3, data)
        }
        Format::Gray(mut d) => {
            if let Some(t) = needs_stream(target, d.width(), d.height()) {
                let image = read_streamed(d, t, true)?;
                return Ok(Decoded { image, icc, orientation });
            }
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
        return Err(Error::InvalidImage("Truncated JPEG".into()));
    }
    Ok(Decoded { image, icc, orientation })
}
