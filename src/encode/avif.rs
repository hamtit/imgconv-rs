//! AVIF encoding with avif-mini-encoder (pure Rust), asking of it what sharp 0.35 /
//! libvips 8.18 / libheif 1.23 ask of libaom: all-intra still picture,
//! constant quality, speed = 9 - effort, tune IQ for color (PSNR for alpha),
//! 4:2:0 below quality 90 (libvips' rule; sharp itself defaults to 4:4:4),
//! BT.601 full-range YCbCr with sRGB nclx, no intra block copy.

use avif_mini_encoder::{Chroma, ColorDesc, EncodeJob, Tune};

use crate::bail;
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

/// RGB(A) to full-range BT.601 YCbCr planes. 4:2:0 chroma is the average of
/// each 2x2 block.
fn rgb_to_ycbcr(img: &Image, subsampled: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
    let (w, h, c) = (img.width as usize, img.height as usize, img.channels as usize);
    let mut y = vec![0u8; w * h];
    let (cw, ch) = if subsampled { (w.div_ceil(2), h.div_ceil(2)) } else { (w, h) };
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    let px = |x: usize, yy: usize| {
        let o = (yy * w + x) * c;
        (f32::from(img.data[o]), f32::from(img.data[o + 1]), f32::from(img.data[o + 2]))
    };
    const KR: f32 = 0.299;
    const KB: f32 = 0.114;
    const KG: f32 = 1.0 - KR - KB;
    for yy in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, yy);
            y[yy * w + x] = (KR * r + KG * g + KB * b).round().clamp(0.0, 255.0) as u8;
        }
    }
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
            let (x0, y0) = if subsampled { (cx * 2, cy * 2) } else { (cx, cy) };
            let span = if subsampled { 2 } else { 1 };
            for yy in y0..(y0 + span).min(h) {
                for x in x0..(x0 + span).min(w) {
                    let p = px(x, yy);
                    r += p.0;
                    g += p.1;
                    b += p.2;
                    n += 1.0;
                }
            }
            let (r, g, b) = (r / n, g / n, b / n);
            let luma = KR * r + KG * g + KB * b;
            u[cy * cw + cx] = ((b - luma) / (2.0 * (1.0 - KB)) + 128.0).round().clamp(0.0, 255.0) as u8;
            v[cy * cw + cx] = ((r - luma) / (2.0 * (1.0 - KR)) + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    (y, u, v, cw)
}

pub fn encode(img: Image, s: Settings) -> Result<Vec<u8>> {
    // libvips' rule: 4:2:0 below quality 90, 4:4:4 from 90. (sharp overrides
    // it with chromaSubsampling '4:4:4' by default; 4:2:0 is kept here as it
    // encodes faster and smaller, see README.)
    let subsampled = s.quality < 90;
    let (width, height) = (img.width as usize, img.height as usize);
    let alpha_plane: Option<Vec<u8>> = img.has_alpha().then(|| img.data.chunks_exact(4).map(|p| p[3]).collect());
    let (y, u, v, _) = rgb_to_ycbcr(&img, subsampled);
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
    let map_err = |e: avif_mini_encoder::Error| Error(format!("AV1 encoding failed: {e}"));
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
        bail!("AV1 encoder produced no data");
    }
    drop((y, u, v));
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
