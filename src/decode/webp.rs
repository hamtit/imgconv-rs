use std::ptr;

use libwebp_sys as w;

use super::Decoded;
use crate::bail;
use crate::error::Result;
use crate::image::Image;

fn features(buf: &[u8]) -> Result<w::WebPBitstreamFeatures> {
    unsafe {
        let mut f = std::mem::zeroed::<w::WebPBitstreamFeatures>();
        if w::WebPGetFeatures(buf.as_ptr(), buf.len(), &mut f) != w::VP8StatusCode::VP8_STATUS_OK {
            bail!("Invalid WebP header");
        }
        Ok(f)
    }
}

/// Canvas width, height and frame count.
pub fn probe(buf: &[u8]) -> Result<(u32, u32, u32)> {
    let f = features(buf)?;
    let mut frames = 1;
    if f.has_animation != 0 {
        frames = riff_chunks(buf).filter(|(id, _)| id == b"ANMF").count().max(1) as u32;
        if let Some((_, vp8x)) = riff_chunks(buf).find(|(id, _)| id == b"VP8X")
            && vp8x.len() >= 10
        {
            let cw = 1 + (u32::from(vp8x[4]) | u32::from(vp8x[5]) << 8 | u32::from(vp8x[6]) << 16);
            let ch = 1 + (u32::from(vp8x[7]) | u32::from(vp8x[8]) << 8 | u32::from(vp8x[9]) << 16);
            return Ok((cw, ch, frames));
        }
    }
    Ok((f.width as u32, f.height as u32, frames))
}

/// Top-level RIFF chunks: (fourcc, payload).
fn riff_chunks(buf: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut i = 12;
    std::iter::from_fn(move || {
        let head = buf.get(i..i + 8)?;
        let id = [head[0], head[1], head[2], head[3]];
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let start = i + 8;
        let end = start.checked_add(len)?.min(buf.len());
        i = start + len + (len & 1);
        Some((id, &buf[start..end]))
    })
}

/// Decode a still WebP (or the first frame of an animated one), optionally
/// shrinking by `shrink` with libwebp's built-in scaler.
pub fn decode(buf: &[u8], shrink: u32) -> Result<Decoded> {
    let f = features(buf)?;
    let icc = riff_chunks(buf).find(|(id, _)| id == b"ICCP").map(|(_, d)| d.to_vec());
    let image = if f.has_animation != 0 { first_frame(buf)? } else { decode_still(buf, &f, shrink)? };
    Ok(Decoded { image, icc, orientation: 1 })
}

fn decode_still(buf: &[u8], f: &w::WebPBitstreamFeatures, shrink: u32) -> Result<Image> {
    let channels: u8 = if f.has_alpha != 0 { 4 } else { 3 };
    let (mut width, mut height) = (f.width as u32, f.height as u32);
    unsafe {
        let mut config = std::mem::zeroed::<w::WebPDecoderConfig>();
        if !w::WebPInitDecoderConfig(&mut config) {
            bail!("libwebp version mismatch");
        }
        if shrink > 1 {
            width = width.div_ceil(shrink);
            height = height.div_ceil(shrink);
            config.options.use_scaling = 1;
            config.options.scaled_width = width as i32;
            config.options.scaled_height = height as i32;
        }
        let stride = width as usize * channels as usize;
        let mut out = vec![0u8; stride * height as usize];
        config.output.colorspace =
            if channels == 4 { w::WEBP_CSP_MODE::MODE_RGBA } else { w::WEBP_CSP_MODE::MODE_RGB };
        config.output.is_external_memory = 1;
        config.output.u.RGBA = w::WebPRGBABuffer { rgba: out.as_mut_ptr(), stride: stride as i32, size: out.len() };
        let status = w::WebPDecode(buf.as_ptr(), buf.len(), &mut config);
        w::WebPFreeDecBuffer(&mut config.output);
        if status != w::VP8StatusCode::VP8_STATUS_OK {
            bail!("WebP decoding failed ({status:?})");
        }
        Ok(Image::new(width, height, channels, out))
    }
}

fn first_frame(buf: &[u8]) -> Result<Image> {
    unsafe {
        let mut opts = std::mem::zeroed::<w::WebPAnimDecoderOptions>();
        if w::WebPAnimDecoderOptionsInit(&mut opts) == 0 {
            bail!("libwebp version mismatch");
        }
        opts.color_mode = w::WEBP_CSP_MODE::MODE_RGBA;
        opts.use_threads = 0;
        let data = w::WebPData { bytes: buf.as_ptr(), size: buf.len() };
        let dec = w::WebPAnimDecoderNew(&data, &opts);
        if dec.is_null() {
            bail!("Invalid animated WebP");
        }
        let mut info = std::mem::zeroed::<w::WebPAnimInfo>();
        let mut frame: *mut u8 = ptr::null_mut();
        let mut timestamp = 0;
        let ok = w::WebPAnimDecoderGetInfo(dec, &mut info) != 0
            && w::WebPAnimDecoderGetNext(dec, &mut frame, &mut timestamp) != 0;
        let result = if ok {
            let len = info.canvas_width as usize * info.canvas_height as usize * 4;
            let rgba = std::slice::from_raw_parts(frame, len).to_vec();
            Ok(Image::new(info.canvas_width, info.canvas_height, 4, rgba))
        } else {
            Err(crate::error::err("Animated WebP decoding failed"))
        };
        w::WebPAnimDecoderDelete(dec);
        result
    }
}
