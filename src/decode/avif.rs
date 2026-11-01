//! AVIF still images: container via avif-parse, AV1 via libaom's decoder.

use std::ptr;

use crate::aom::{self, sys};
use crate::bail;
use crate::error::{Result, err};
use crate::image::Image;

use super::Decoded;

fn parse(buf: &[u8]) -> Result<avif_parse::AvifData> {
    avif_parse::read_avif(&mut &buf[..]).map_err(|e| err(format!("Invalid AVIF: {e:?}")))
}

pub fn probe(buf: &[u8]) -> Result<(u32, u32)> {
    let data = parse(buf)?;
    let meta = data.primary_item_metadata().map_err(|e| err(format!("Invalid AV1 data: {e:?}")))?;
    Ok((meta.max_frame_width.get(), meta.max_frame_height.get()))
}

struct Decoder {
    ctx: sys::aom_codec_ctx_t,
}

impl Decoder {
    fn new() -> Result<Self> {
        unsafe {
            let mut ctx = std::mem::zeroed::<sys::aom_codec_ctx_t>();
            let cfg = sys::aom_codec_dec_cfg_t { threads: 1, w: 0, h: 0, allow_lowbitdepth: 1 };
            if sys::aom_codec_dec_init_ver(&mut ctx, sys::aom_codec_av1_dx(), &cfg, 0, sys::AOM_DECODER_ABI_VERSION as i32)
                != sys::aom_codec_err_t_AOM_CODEC_OK
            {
                bail!("AV1 decoder init failed");
            }
            Ok(Decoder { ctx })
        }
    }

    /// Decode one AV1 temporal unit and call `f` with the resulting frame.
    fn decode<T>(&mut self, obus: &[u8], f: impl FnOnce(&sys::aom_image_t) -> Result<T>) -> Result<T> {
        unsafe {
            if sys::aom_codec_decode(&mut self.ctx, obus.as_ptr(), obus.len(), ptr::null_mut())
                != sys::aom_codec_err_t_AOM_CODEC_OK
            {
                bail!("AV1 decoding failed: {}", aom::error_detail(&mut self.ctx));
            }
            let mut iter = ptr::null();
            let img = sys::aom_codec_get_frame(&mut self.ctx, &mut iter);
            if img.is_null() {
                bail!("AV1 decoder returned no frame");
            }
            f(&*img)
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            sys::aom_codec_destroy(&mut self.ctx);
        }
    }
}

/// Read sample (x, y) of plane `p` as an 8-bit value.
#[inline]
unsafe fn sample(img: &sys::aom_image_t, p: usize, x: usize, y: usize, high: bool, shift: u32) -> f32 {
    unsafe {
        let row = img.planes[p].offset(y as isize * img.stride[p] as isize);
        if high {
            let v = *(row as *const u16).add(x);
            f32::from(v) / (1 << shift) as f32
        } else {
            f32::from(*row.add(x))
        }
    }
}

fn to_rgb(img: &sys::aom_image_t, out: &mut [u8], channels: usize) -> Result<()> {
    let (w, h) = (img.d_w as usize, img.d_h as usize);
    let high = img.fmt & sys::AOM_IMG_FMT_HIGHBITDEPTH != 0;
    let shift = img.bit_depth.saturating_sub(8);
    let full = img.range == sys::aom_color_range_AOM_CR_FULL_RANGE;
    let (xs, ys) = (img.x_chroma_shift, img.y_chroma_shift);
    let mono = img.monochrome != 0;
    let identity = img.mc == 0;
    let (kr, kb) = aom::luma_coefficients(img.mc);
    let kg = 1.0 - kr - kb;
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x >> xs, y >> ys);
            unsafe {
                let mut yv = sample(img, 0, x, y, high, shift);
                let (mut u, mut v) = if mono {
                    (128.0, 128.0)
                } else {
                    (sample(img, 1, cx, cy, high, shift), sample(img, 2, cx, cy, high, shift))
                };
                let (r, g, b) = if identity {
                    (v, yv, u)
                } else {
                    if full {
                        u -= 128.0;
                        v -= 128.0;
                    } else {
                        yv = (yv - 16.0) * (255.0 / 219.0);
                        u = (u - 128.0) * (255.0 / 224.0);
                        v = (v - 128.0) * (255.0 / 224.0);
                    }
                    let r = yv + (2.0 - 2.0 * kr) * v;
                    let b = yv + (2.0 - 2.0 * kb) * u;
                    let g = (yv - kr * r - kb * b) / kg;
                    (r, g, b)
                };
                let o = (y * w + x) * channels;
                out[o] = r.round().clamp(0.0, 255.0) as u8;
                out[o + 1] = g.round().clamp(0.0, 255.0) as u8;
                out[o + 2] = b.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

pub fn decode(buf: &[u8]) -> Result<Decoded> {
    let data = parse(buf)?;
    // The pixel limit was checked against the primary item's sequence header;
    // an alpha item must match it before its decoder allocates anything.
    if data.alpha_item.is_some() {
        let primary = data.primary_item_metadata().map_err(|e| err(format!("Invalid AV1 data: {e:?}")))?;
        let alpha = data
            .alpha_item_metadata()
            .map_err(|e| err(format!("Invalid AV1 alpha data: {e:?}")))?
            .ok_or_else(|| err("Invalid AV1 alpha data"))?;
        if (alpha.max_frame_width, alpha.max_frame_height) != (primary.max_frame_width, primary.max_frame_height) {
            bail!("AVIF alpha plane size mismatch");
        }
    }
    let icc = icc_profile(buf);
    let mut dec = Decoder::new()?;
    let has_alpha = data.alpha_item.is_some();
    let channels = if has_alpha { 4 } else { 3 };
    let (w, h, mut pixels, cicp) = dec.decode(&data.primary_item, |img| {
        let (w, h) = (img.d_w, img.d_h);
        let mut out = vec![0u8; w as usize * h as usize * channels];
        to_rgb(img, &mut out, channels)?;
        Ok((w, h, out, (img.cp as u8, img.tc as u8)))
    })?;
    if let Some(alpha) = &data.alpha_item {
        let mut adec = Decoder::new()?;
        adec.decode(alpha, |img| {
            if img.d_w != w || img.d_h != h {
                bail!("AVIF alpha plane size mismatch");
            }
            let high = img.fmt & sys::AOM_IMG_FMT_HIGHBITDEPTH != 0;
            let shift = img.bit_depth.saturating_sub(8);
            let full = img.range == sys::aom_color_range_AOM_CR_FULL_RANGE;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let mut a = unsafe { sample(img, 0, x, y, high, shift) };
                    if !full {
                        a = (a - 16.0) * (255.0 / 219.0);
                    }
                    pixels[(y * w as usize + x) * 4 + 3] = a.round().clamp(0.0, 255.0) as u8;
                }
            }
            Ok(())
        })?;
        if data.premultiplied_alpha {
            for p in pixels.chunks_exact_mut(4) {
                let a = u32::from(p[3]);
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
        }
    }
    let mut image = Image::new(w, h, channels as u8, pixels);
    if icc.is_none() {
        // The AV1 sequence header's colour description (same as the nclx box).
        crate::color::cicp_to_srgb(&mut image, cicp.0, cicp.1);
    }
    Ok(Decoded { image, icc, orientation: 1 })
}

/// ISO-BMFF boxes in `buf`: (type, payload).
fn boxes(buf: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        let head = buf.get(i..i + 8)?;
        let mut size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = [head[4], head[5], head[6], head[7]];
        let mut start = i + 8;
        if size == 1 {
            let ext = buf.get(i + 8..i + 16)?;
            size = usize::try_from(u64::from_be_bytes(ext.try_into().ok()?)).ok()?;
            start = i + 16;
        } else if size == 0 {
            size = buf.len() - i;
        }
        let end = i.checked_add(size)?;
        if size < start - i || end > buf.len() {
            return None;
        }
        i = end;
        Some((kind, &buf[start..end]))
    })
}

/// The ICC profile from a `colr` property (meta/iprp/ipco), if any. AVIF
/// files written by common encoders carry at most one.
fn icc_profile(buf: &[u8]) -> Option<Vec<u8>> {
    let (_, meta) = boxes(buf).find(|(k, _)| k == b"meta")?;
    let (_, iprp) = boxes(meta.get(4..)?).find(|(k, _)| k == b"iprp")?;
    let (_, ipco) = boxes(iprp).find(|(k, _)| k == b"ipco")?;
    boxes(ipco)
        .filter(|(k, _)| k == b"colr")
        .find_map(|(_, c)| matches!(c.get(0..4), Some(b"prof") | Some(b"rICC")).then(|| c[4..].to_vec()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_icc_in_ipco() {
        fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            v.extend_from_slice(kind);
            v.extend_from_slice(body);
            v
        }
        let colr = bx(b"colr", &[b"prof".as_slice(), b"ICCDATA"].concat());
        let nclx = bx(b"colr", b"nclx\0\x01\0\x0d\0\x06\x80");
        let ipco = bx(b"ipco", &[nclx, colr].concat());
        let meta = bx(b"meta", &[vec![0, 0, 0, 0], bx(b"hdlr", b"x"), bx(b"iprp", &ipco)].concat());
        let file = [bx(b"ftyp", b"avif"), meta].concat();
        assert_eq!(super::icc_profile(&file).as_deref(), Some(b"ICCDATA".as_slice()));
        assert_eq!(super::icc_profile(&bx(b"ftyp", b"avif")), None);
    }
}
