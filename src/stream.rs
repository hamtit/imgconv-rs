//! Row-streaming Lanczos3 downscale.
//!
//! Decoders that cannot shrink while decoding (PNG, and JPEG when the scale
//! is under 2) used to expand the whole image before resizing.
//! `StreamResizer` takes the source one row at a time and keeps only a sliding
//! window of source rows: as soon as the window holds every row a band of
//! output rows needs, that band is resized (vertical pass first, as in a
//! one-shot resize) and the rows no later band needs are dropped. Peak memory
//! is the output image plus the window (tens of source rows).
//!
//! Each band is resized from the window with `fast_image_resize`'s
//! fractional crop box. The kernel reads pixels outside the crop box, so
//! every output row sees the same filter taps as a one-shot resize.

use fast_image_resize::images::{Image as FirImage, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::error::{Result, err};

/// Lanczos3 support.
const SUPPORT: f64 = 3.0;
/// Source rows a band of output rows spans (before the filter margin).
const BAND_SOURCE_ROWS: f64 = 64.0;

pub struct StreamResizer {
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    channels: usize,
    pixel_type: PixelType,
    scale: f64,
    radius: f64,
    /// Output rows per band.
    band: u32,
    resizer: Resizer,
    /// Source rows `win_start..win_end` (each `src_w * channels` bytes).
    win: Vec<u8>,
    win_start: usize,
    win_end: usize,
    out: Vec<u8>,
    next_out: u32,
}

impl StreamResizer {
    /// `channels` is 3 (RGB) or 4 (RGBA, straight alpha). The source is
    /// `src_w x src_h`, the result `dst_w x dst_h` (no upscaling).
    pub fn new(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32, channels: u8) -> Result<Self> {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 || dst_w > src_w || dst_h > src_h {
            return Err(err("stream resize: invalid sizes"));
        }
        let c = channels as usize;
        let scale = f64::from(src_h) / f64::from(dst_h);
        Ok(StreamResizer {
            src_w,
            src_h,
            dst_w,
            dst_h,
            channels: c,
            pixel_type: if c == 4 { PixelType::U8x4 } else { PixelType::U8x3 },
            scale,
            radius: SUPPORT * scale.max(1.0),
            band: ((BAND_SOURCE_ROWS / scale) as u32).clamp(1, 64),
            resizer: Resizer::new(),
            win: Vec::new(),
            win_start: 0,
            win_end: 0,
            out: vec![0u8; dst_w as usize * dst_h as usize * c],
            next_out: 0,
        })
    }

    /// Append one source row (`src_w * channels` bytes).
    pub fn push_row(&mut self, row: &[u8]) -> Result<()> {
        let rb = self.src_w as usize * self.channels;
        if row.len() != rb {
            return Err(err("stream resize: bad row length"));
        }
        if self.win_end >= self.src_h as usize {
            return Err(err("stream resize: too many rows"));
        }
        self.win.extend_from_slice(row);
        self.win_end += 1;
        self.emit()
    }

    /// Produce every band of output rows whose source window is complete.
    fn emit(&mut self) -> Result<()> {
        let rb = self.src_w as usize * self.channels;
        let orb = self.dst_w as usize * self.channels;
        while self.next_out < self.dst_h {
            let y0 = self.next_out;
            let y1 = (y0 + self.band).min(self.dst_h);
            let hi = (((f64::from(y1) - 0.5) * self.scale + self.radius).ceil() as usize).min(self.src_h as usize);
            if self.win_end < hi {
                break;
            }
            let lo = (((f64::from(y0) + 0.5) * self.scale - self.radius).floor().max(0.0)) as usize;
            let window = &self.win[(lo - self.win_start) * rb..(hi - self.win_start) * rb];
            let src = ImageRef::new(self.src_w, (hi - lo) as u32, window, self.pixel_type)
                .map_err(|e| err(format!("resize: {e}")))?;
            let mut dst = FirImage::from_slice_u8(
                self.dst_w,
                y1 - y0,
                &mut self.out[y0 as usize * orb..y1 as usize * orb],
                self.pixel_type,
            )
            .map_err(|e| err(format!("resize: {e}")))?;
            let top = f64::from(y0) * self.scale - lo as f64;
            // Rounding can push the box a hair past the window's last row.
            let height = (f64::from(y1 - y0) * self.scale).min((hi - lo) as f64 - top);
            let opts = ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                .use_alpha(self.channels == 4)
                .crop(0.0, top, f64::from(self.src_w), height);
            self.resizer.resize(&src, &mut dst, &opts).map_err(|e| err(format!("resize: {e}")))?;
            self.next_out = y1;
            // Rows above the next band's window are no longer needed.
            if y1 < self.dst_h {
                let next_lo = (((f64::from(y1) + 0.5) * self.scale - self.radius).floor().max(0.0)) as usize;
                if next_lo > self.win_start {
                    self.win.drain(..(next_lo - self.win_start) * rb);
                    self.win_start = next_lo;
                }
            }
        }
        Ok(())
    }

    /// Finish after all `src_h` rows were pushed; returns the RGB(A) pixels.
    pub fn finish(self) -> Result<Vec<u8>> {
        if self.next_out != self.dst_h || self.win_end != self.src_h as usize {
            return Err(err("stream resize: missing rows"));
        }
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Image;
    use crate::resize;

    fn pseudo(w: u32, h: u32, c: u8) -> Image {
        let mut s = 0x1234_5678u32;
        let data = (0..w as usize * h as usize * c as usize)
            .map(|i| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let x = (i / c as usize) % w as usize * 255 / w as usize;
                if c == 4 && i % 4 == 3 {
                    // Alpha: smooth and never near 0 (division by a tiny alpha magnifies rounding).
                    (96 + x / 2) as u8
                } else {
                    // Gradient plus noise.
                    (x as u32 / 2 + (s >> 25)) as u8
                }
            })
            .collect();
        Image::new(w, h, c, data)
    }

    fn check(sw: u32, sh: u32, dw: u32, dh: u32, c: u8) {
        let img = pseudo(sw, sh, c);
        let want = resize::resize(&img.view(), dw, dh).unwrap();
        let mut s = StreamResizer::new(sw, sh, dw, dh, c).unwrap();
        for row in img.data.chunks_exact(sw as usize * c as usize) {
            s.push_row(row).unwrap();
        }
        let got = s.finish().unwrap();
        assert_eq!(got.len(), want.data.len());
        let max = got.iter().zip(&want.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        let mean = got.iter().zip(&want.data).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / got.len() as f64;
        // Same passes in the same order as the one-shot resize.
        assert!(max <= 2 && mean < 0.1, "{sw}x{sh}->{dw}x{dh}x{c}: max diff {max}, mean {mean:.3}");
    }

    #[test]
    fn matches_one_shot_resize() {
        check(640, 480, 320, 240, 3);
        check(1000, 700, 100, 70, 3);
        check(997, 1013, 123, 150, 3);
        check(640, 480, 320, 240, 4);
        check(999, 1001, 77, 301, 4);
        check(300, 5000, 30, 500, 3);
        check(64, 64, 63, 63, 3);
    }
}
