use std::io::Cursor;

use png::{ColorType, Transformations};

use super::Decoded;
use crate::error::{Error, Result};
use crate::image::{Image, expand_gray};

fn png_err(e: png::DecodingError) -> Error {
    Error(format!("PNG decoding failed: {e}"))
}

/// Decode the default image of a PNG (the first frame of an APNG) to 8-bit.
pub fn decode(buf: &[u8]) -> Result<Decoded> {
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(buf), png::Limits { bytes: usize::MAX });
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(png_err)?;
    let icc = reader.info().icc_profile.as_ref().map(|c| c.to_vec());
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
