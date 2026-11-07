use std::io::Cursor;

use png::{ColorType, Transformations};

use super::Decoded;
use crate::error::{Error, Result};
use crate::image::{Image, expand_gray};
use crate::stream::StreamResizer;

fn png_err(e: png::DecodingError) -> Error {
    Error(format!("PNG decoding failed: {e}"))
}

/// Decode the default image of a PNG (the first frame of an APNG) to 8-bit.
/// With `target` (smaller than the image) a non-interlaced PNG is shrunk row
/// by row while decoding, so the full-size pixels never exist in memory.
pub fn decode(buf: &[u8], target: Option<(u32, u32)>) -> Result<Decoded> {
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(buf), png::Limits { bytes: usize::MAX });
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(png_err)?;
    let icc = reader.info().icc_profile.as_ref().map(|c| c.to_vec());
    let (w, h) = reader.info().size();
    if let Some((tw, th)) = target
        && tw <= w
        && th <= h
        && (tw, th) != (w, h)
        && !reader.info().interlaced
    {
        let image = decode_streamed(&mut reader, tw, th)?;
        return Ok(Decoded { image, icc, orientation: 1 });
    }
    let size = reader.output_buffer_size().ok_or_else(|| Error("PNG too large".into()))?;
    let mut data = vec![0u8; size];
    let frame = reader.next_frame(&mut data).map_err(png_err)?;
    data.truncate(frame.buffer_size());
    let (w, h) = (frame.width, frame.height);
    let image = match frame.color_type {
        ColorType::Rgb => Image::new(w, h, 3, data),
        ColorType::Rgba => Image::new(w, h, 4, data),
        ColorType::Grayscale => Image::new(w, h, 3, expand_gray(&data, false)),
        ColorType::GrayscaleAlpha => Image::new(w, h, 4, expand_gray(&data, true)),
        ColorType::Indexed => return Err(Error("PNG palette was not expanded".into())),
    };
    Ok(Decoded { image, icc, orientation: 1 })
}

fn decode_streamed(reader: &mut png::Reader<Cursor<&[u8]>>, tw: u32, th: u32) -> Result<Image> {
    let (w, h) = reader.info().size();
    let (color, _) = reader.output_color_type();
    let (channels, gray) = match color {
        ColorType::Rgb => (3u8, false),
        ColorType::Rgba => (4, false),
        ColorType::Grayscale => (3, true),
        ColorType::GrayscaleAlpha => (4, true),
        ColorType::Indexed => return Err(Error("PNG palette was not expanded".into())),
    };
    let mut stream = StreamResizer::new(w, h, tw, th, channels)?;
    let mut expanded = Vec::new();
    for _ in 0..h {
        let row =
            reader.next_row().map_err(png_err)?.ok_or_else(|| Error("PNG decoding failed: truncated image".into()))?;
        if gray {
            expanded.clear();
            if channels == 4 {
                row.data().chunks_exact(2).for_each(|p| expanded.extend_from_slice(&[p[0], p[0], p[0], p[1]]));
            } else {
                row.data().iter().for_each(|&g| expanded.extend_from_slice(&[g, g, g]));
            }
            stream.push_row(&expanded)?;
        } else {
            stream.push_row(row.data())?;
        }
    }
    // Read the rest of the stream (Adler-32, CRCs, IEND) so a damaged tail is
    // rejected exactly as it is when the image is decoded in one piece.
    reader.finish().map_err(png_err)?;
    Ok(Image::new(tw, th, channels, stream.finish()?))
}
