//! AVIF encoding with avif-mini-encoder (pure Rust), asking of it what sharp 0.35 /
//! libvips 8.18 / libheif 1.23 ask of libaom: all-intra still picture,
//! constant quality, speed = 9 - effort, tune IQ for color (PSNR for alpha),
//! 4:2:0 below quality 90 (libvips' rule; sharp itself defaults to 4:4:4),
//! BT.601 full-range YCbCr with sRGB nclx, no intra block copy.

use avif_mini_encoder::{Chroma, ColorDesc, EncodeJob, Tune};

use crate::error::{Error, Result};
use crate::image::Image;

/// libheif's quality-to-quantizer table for AOM_TUNE_IQ.
const TUNE_IQ_QUANTIZER: [u8; 101] = [
    63, 63, 63, 62, 62, 62, 61, 61, 60, 60, 59, 59, 58, 58, 57, 57, 56, 56, 55, 55, 54, 54, 53, 53, 52, 52, 51, 51, 50,
    50, 49, 49, 48, 48, 47, 46, 46, 45, 45, 44, 43, 43, 42, 42, 41, 40, 40, 39, 39, 38, 37, 37, 36, 36, 35, 34, 33, 33,
    32, 31, 30, 30, 29, 28, 27, 27, 26, 25, 24, 24, 23, 22, 21, 21, 20, 19, 18, 18, 17, 16, 15, 15, 14, 13, 12, 12, 11,
    10, 9, 9, 8, 7, 6, 6, 5, 4, 3, 3, 2, 1, 0,
];

#[derive(Clone, Copy)]
pub struct Settings {
    pub quality: u32,
    /// sharp's effort 0-9 (libaom speed = 9 - effort).
    pub effort: u32,
    pub threads: u32,
}

/// libaom's quantizer_to_qindex: the 0-63 "quantizer" scale of cq_level to AV1 qindex.
const QUANTIZER_TO_QINDEX: [u8; 64] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 64, 68, 72, 76, 80, 84, 88, 92, 96, 100, 104, 108,
    112, 116, 120, 124, 128, 132, 136, 140, 144, 148, 152, 156, 160, 164, 168, 172, 176, 180, 184, 188, 192, 196, 200,
    204, 208, 212, 216, 220, 224, 228, 232, 236, 240, 244, 249, 255,
];

fn qindex(quantizer: i32) -> u8 {
    QUANTIZER_TO_QINDEX[quantizer.clamp(0, 63) as usize]
}

#[inline(always)]
fn rgb_to_y(r: i32, g: i32, b: i32) -> u8 {
    ((19595 * r + 38470 * g + 7471 * b + 32768) >> 16) as u8
}

#[inline(always)]
fn rgb_to_cb(r: i32, g: i32, b: i32) -> u8 {
    (((-11058 * r - 21710 * g + 32768 * b + 8_421_376) >> 16).clamp(0, 255)) as u8
}

#[inline(always)]
fn rgb_to_cr(r: i32, g: i32, b: i32) -> u8 {
    (((32768 * r - 27439 * g - 5329 * b + 8_421_376) >> 16).clamp(0, 255)) as u8
}

#[inline(always)]
fn rgb4_to_cb(rs: i32, gs: i32, bs: i32) -> u8 {
    (((-11058 * rs - 21710 * gs + 32768 * bs + 33_685_504) >> 18).clamp(0, 255)) as u8
}

#[inline(always)]
fn rgb4_to_cr(rs: i32, gs: i32, bs: i32) -> u8 {
    (((32768 * rs - 27439 * gs - 5329 * bs + 33_685_504) >> 18).clamp(0, 255)) as u8
}

/// 1-pass fixed-point RGB(A) to full-range BT.601 YCbCr (+ optional alpha) planes.
/// 4:2:0 chroma is the average of each 2x2 block, processed 2 rows at a time.
fn rgb_to_ycbcr(img: &Image, subsampled: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let (w, h, c) = (img.width as usize, img.height as usize, img.channels as usize);
    let mut y = vec![0u8; w * h];
    let mut alpha = img.has_alpha().then(|| vec![0u8; w * h]);

    if !subsampled {
        let mut u = vec![0u8; w * h];
        let mut v = vec![0u8; w * h];
        if let Some(ref mut a) = alpha {
            for (i, p) in img.data.as_chunks::<4>().0.iter().enumerate() {
                let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                y[i] = rgb_to_y(r, g, b);
                u[i] = rgb_to_cb(r, g, b);
                v[i] = rgb_to_cr(r, g, b);
                a[i] = p[3];
            }
        } else {
            for (i, p) in img.data.as_chunks::<3>().0.iter().enumerate() {
                let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                y[i] = rgb_to_y(r, g, b);
                u[i] = rgb_to_cb(r, g, b);
                v[i] = rgb_to_cr(r, g, b);
            }
        }
        return (y, u, v, alpha);
    }

    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    let stride = w * c;
    let pairs_y = h / 2;
    let pairs_x = w / 2;

    for cy in 0..pairs_y {
        let y0 = cy * 2;
        let y1 = y0 + 1;
        let row0 = &img.data[y0 * stride..(y0 + 1) * stride];
        let row1 = &img.data[y1 * stride..(y1 + 1) * stride];
        let (y_row0, y_row1) = y[y0 * w..(y1 + 1) * w].split_at_mut(w);
        let u_row = &mut u[cy * cw..(cy + 1) * cw];
        let v_row = &mut v[cy * cw..(cy + 1) * cw];
        let mut a_rows = alpha.as_mut().map(|a| a[y0 * w..(y1 + 1) * w].split_at_mut(w));

        for cx in 0..pairs_x {
            let x0 = cx * 2;
            let x1 = x0 + 1;
            let o0 = x0 * c;
            let o1 = x1 * c;
            let (r00, g00, b00) = (i32::from(row0[o0]), i32::from(row0[o0 + 1]), i32::from(row0[o0 + 2]));
            let (r01, g01, b01) = (i32::from(row0[o1]), i32::from(row0[o1 + 1]), i32::from(row0[o1 + 2]));
            let (r10, g10, b10) = (i32::from(row1[o0]), i32::from(row1[o0 + 1]), i32::from(row1[o0 + 2]));
            let (r11, g11, b11) = (i32::from(row1[o1]), i32::from(row1[o1 + 1]), i32::from(row1[o1 + 2]));
            y_row0[x0] = rgb_to_y(r00, g00, b00);
            y_row0[x1] = rgb_to_y(r01, g01, b01);
            y_row1[x0] = rgb_to_y(r10, g10, b10);
            y_row1[x1] = rgb_to_y(r11, g11, b11);
            if let Some((a0, a1)) = a_rows.as_mut() {
                a0[x0] = row0[o0 + 3];
                a0[x1] = row0[o1 + 3];
                a1[x0] = row1[o0 + 3];
                a1[x1] = row1[o1 + 3];
            }
            let rs = r00 + r01 + r10 + r11;
            let gs = g00 + g01 + g10 + g11;
            let bs = b00 + b01 + b10 + b11;
            u_row[cx] = rgb4_to_cb(rs, gs, bs);
            v_row[cx] = rgb4_to_cr(rs, gs, bs);
        }
        if w & 1 != 0 {
            let x0 = pairs_x * 2;
            let o0 = x0 * c;
            let (r00, g00, b00) = (i32::from(row0[o0]), i32::from(row0[o0 + 1]), i32::from(row0[o0 + 2]));
            let (r10, g10, b10) = (i32::from(row1[o0]), i32::from(row1[o0 + 1]), i32::from(row1[o0 + 2]));
            y_row0[x0] = rgb_to_y(r00, g00, b00);
            y_row1[x0] = rgb_to_y(r10, g10, b10);
            if let Some((a0, a1)) = a_rows.as_mut() {
                a0[x0] = row0[o0 + 3];
                a1[x0] = row1[o0 + 3];
            }
            let (rs, gs, bs) = ((r00 + r10) * 2, (g00 + g10) * 2, (b00 + b10) * 2);
            u_row[pairs_x] = rgb4_to_cb(rs, gs, bs);
            v_row[pairs_x] = rgb4_to_cr(rs, gs, bs);
        }
    }

    if h & 1 != 0 {
        let y0 = pairs_y * 2;
        let row0 = &img.data[y0 * stride..(y0 + 1) * stride];
        let y_row0 = &mut y[y0 * w..(y0 + 1) * w];
        let u_row = &mut u[pairs_y * cw..(pairs_y + 1) * cw];
        let v_row = &mut v[pairs_y * cw..(pairs_y + 1) * cw];
        let mut a_row0 = alpha.as_mut().map(|a| &mut a[y0 * w..(y0 + 1) * w]);
        for cx in 0..pairs_x {
            let x0 = cx * 2;
            let x1 = x0 + 1;
            let o0 = x0 * c;
            let o1 = x1 * c;
            let (r00, g00, b00) = (i32::from(row0[o0]), i32::from(row0[o0 + 1]), i32::from(row0[o0 + 2]));
            let (r01, g01, b01) = (i32::from(row0[o1]), i32::from(row0[o1 + 1]), i32::from(row0[o1 + 2]));
            y_row0[x0] = rgb_to_y(r00, g00, b00);
            y_row0[x1] = rgb_to_y(r01, g01, b01);
            if let Some(a0) = a_row0.as_mut() {
                a0[x0] = row0[o0 + 3];
                a0[x1] = row0[o1 + 3];
            }
            let (rs, gs, bs) = ((r00 + r01) * 2, (g00 + g01) * 2, (b00 + b01) * 2);
            u_row[cx] = rgb4_to_cb(rs, gs, bs);
            v_row[cx] = rgb4_to_cr(rs, gs, bs);
        }
        if w & 1 != 0 {
            let x0 = pairs_x * 2;
            let o0 = x0 * c;
            let (r00, g00, b00) = (i32::from(row0[o0]), i32::from(row0[o0 + 1]), i32::from(row0[o0 + 2]));
            y_row0[x0] = rgb_to_y(r00, g00, b00);
            if let Some(a0) = a_row0.as_mut() {
                a0[x0] = row0[o0 + 3];
            }
            u_row[pairs_x] = rgb_to_cb(r00, g00, b00);
            v_row[pairs_x] = rgb_to_cr(r00, g00, b00);
        }
    }

    (y, u, v, alpha)
}

pub fn encode(img: Image, s: Settings) -> Result<Vec<u8>> {
    // libvips' rule: 4:2:0 below quality 90, 4:4:4 from 90. (sharp overrides
    // it with chromaSubsampling '4:4:4' by default; 4:2:0 is kept here as it
    // encodes faster and smaller, see README.)
    let subsampled = s.quality < 90;
    let (width, height) = (img.width as usize, img.height as usize);
    let (y, u, v, alpha_plane) = rgb_to_ycbcr(&img, subsampled);
    drop(img);

    let quality = s.quality.min(100) as usize;
    let speed = 9u32.saturating_sub(s.effort).min(9) as u8;
    let color_planes = [y.as_slice(), u.as_slice(), v.as_slice()];
    let color_job = EncodeJob {
        planes: &color_planes,
        width,
        height,
        chroma: if subsampled { Chroma::Yuv420 } else { Chroma::Yuv444 },
        color: Some(ColorDesc { primaries: 1, transfer: 13, matrix: 6 }),
        full_range: true,
        qindex: qindex(i32::from(TUNE_IQ_QUANTIZER[quality])),
        tune: Tune::Iq,
        speed,
    };
    let threads = s.threads.max(1) as usize;
    let map_err = |e: avif_mini_encoder::Error| Error::Encode(format!("AV1 encoding failed: {e}"));
    let (color, alpha) = match &alpha_plane {
        Some(a) => {
            let alpha_planes = [a.as_slice()];
            let alpha_job = EncodeJob {
                planes: &alpha_planes,
                chroma: Chroma::Mono,
                color: None,
                // libheif: tune PSNR with the quantizer taken linearly from quality
                qindex: qindex(((100 - quality as i32) * 63 + 50) / 100),
                tune: Tune::Psnr,
                speed: (speed + 2).min(9),
                ..color_job
            };
            let (c, a) = avif_mini_encoder::encode_av1_pair(color_job, alpha_job, threads);
            (c.map_err(map_err)?.0, Some(a.map_err(map_err)?.0))
        }
        None => {
            let c = avif_mini_encoder::encode_av1_threads(
                color_job.planes,
                width,
                height,
                color_job.chroma,
                color_job.color,
                true,
                color_job.qindex,
                Tune::Iq,
                speed,
                threads,
            );
            (c.map_err(map_err)?.0, None)
        }
    };
    if color.is_empty() {
        return Err(Error::Encode("AV1 encoder produced no data".into()));
    }
    drop((y, u, v, alpha_plane));
    let mut avif = avif_serialize::Aviffy::new();
    avif.set_full_color_range(true)
        .set_matrix_coefficients(avif_serialize::constants::MatrixCoefficients::Bt601)
        .set_transfer_characteristics(avif_serialize::constants::TransferCharacteristics::Srgb)
        .set_color_primaries(avif_serialize::constants::ColorPrimaries::Bt709)
        .set_chroma_subsampling((subsampled, subsampled))
        .set_seq_profile(if subsampled { 0 } else { 1 });
    Ok(avif.to_vec(&color, alpha.as_deref(), width as u32, height as u32, 8))
}

pub fn version() -> String {
    avif_mini_encoder::VERSION.to_string()
}
