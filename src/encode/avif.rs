//! AVIF encoding with libaom, reproducing what sharp 0.35 / libvips 8.18 /
//! libheif 1.23 ask of libaom: all-intra still picture, constant quality,
//! speed = 9 - effort, tune IQ for color (PSNR for alpha), 4:2:0 below
//! quality 90 (libvips' rule; sharp itself defaults to 4:4:4), BT.601 full-range YCbCr with sRGB nclx, auto tiles, no
//! intra block copy.

use std::ptr;

use crate::aom::{self, sys};
use crate::bail;
use crate::error::Result;
use crate::image::Image;

/// libheif's quality-to-quantizer table for AOM_TUNE_IQ.
const TUNE_IQ_QUANTIZER: [u8; 101] = [
    63, 63, 63, 62, 62, 62, 61, 61, 60, 60, 59, 59, 58, 58, 57, 57, 56, 56, 55, 55, 54, 54, 53, 53, 52, 52, 51,
    51, 50, 50, 49, 49, 48, 48, 47, 46, 46, 45, 45, 44, 43, 43, 42, 42, 41, 40, 40, 39, 39, 38, 37, 37, 36, 36,
    35, 34, 33, 33, 32, 31, 30, 30, 29, 28, 27, 27, 26, 25, 24, 24, 23, 22, 21, 21, 20, 19, 18, 18, 17, 16, 15,
    15, 14, 13, 12, 12, 11, 10, 9, 9, 8, 7, 6, 6, 5, 4, 3, 3, 2, 1, 0,
];

#[derive(Clone, Copy)]
pub struct Settings {
    pub quality: u32,
    /// sharp's effort 0-9 (libaom speed = 9 - effort).
    pub effort: u32,
    pub threads: u32,
}

enum Plane<'a> {
    Color { y: &'a [u8], u: &'a [u8], v: &'a [u8], chroma_w: usize, subsampled: bool },
    Alpha(&'a [u8]),
}

fn encode_av1(width: u32, height: u32, plane: Plane<'_>, s: Settings) -> Result<Vec<u8>> {
    let alpha = matches!(plane, Plane::Alpha(_));
    let subsampled = matches!(plane, Plane::Color { subsampled: true, .. });
    unsafe {
        let iface = sys::aom_codec_av1_cx();
        let mut cfg = std::mem::zeroed::<sys::aom_codec_enc_cfg_t>();
        if sys::aom_codec_enc_config_default(iface, &mut cfg, sys::AOM_USAGE_ALL_INTRA) != sys::aom_codec_err_t_AOM_CODEC_OK {
            bail!("aom_codec_enc_config_default failed");
        }
        cfg.g_w = width;
        cfg.g_h = height;
        cfg.g_limit = 1;
        cfg.g_lag_in_frames = 0;
        cfg.kf_mode = sys::aom_kf_mode_AOM_KF_DISABLED;
        cfg.kf_max_dist = 0;
        // Profile 0 covers 4:2:0 and monochrome; 4:4:4 needs profile 1.
        cfg.g_profile = if alpha || subsampled { 0 } else { 1 };
        cfg.g_bit_depth = sys::aom_bit_depth_AOM_BITS_8;
        cfg.g_input_bit_depth = 8;
        cfg.rc_end_usage = sys::aom_rc_mode_AOM_Q;
        cfg.rc_min_quantizer = 0;
        cfg.rc_max_quantizer = 63;
        cfg.g_error_resilient = 0;
        cfg.g_threads = s.threads.max(1);
        cfg.monochrome = u32::from(alpha);
        cfg.g_timebase = sys::aom_rational { num: 1, den: 30 };

        let mut ctx = std::mem::zeroed::<sys::aom_codec_ctx_t>();
        if sys::aom_codec_enc_init_ver(&mut ctx, iface, &cfg, 0, sys::AOM_ENCODER_ABI_VERSION as i32)
            != sys::aom_codec_err_t_AOM_CODEC_OK
        {
            bail!("AV1 encoder init failed: {}", aom::error_detail(&mut ctx));
        }
        struct Ctx(sys::aom_codec_ctx_t);
        impl Drop for Ctx {
            fn drop(&mut self) {
                unsafe {
                    sys::aom_codec_destroy(&mut self.0);
                }
            }
        }
        let mut ctx = Ctx(ctx);
        let c = &mut ctx.0 as *mut sys::aom_codec_ctx_t;

        let quality = s.quality.min(100) as usize;
        // Alpha is tuned for PSNR, which uses libheif's linear mapping.
        let (tune, cq_level) = if alpha {
            (sys::aom_tune_metric_AOM_TUNE_PSNR, ((100 - quality as i32) * 63 + 50) / 100)
        } else {
            (sys::aom_tune_metric_AOM_TUNE_IQ, i32::from(TUNE_IQ_QUANTIZER[quality]))
        };
        let speed = 9 - s.effort.min(9) as i32;
        macro_rules! control {
            ($id:ident, $v:expr) => {
                if sys::aom_codec_control(c, sys::$id as i32, $v) != sys::aom_codec_err_t_AOM_CODEC_OK {
                    bail!("aom_codec_control({}) failed: {}", stringify!($id), aom::error_detail(c));
                }
            };
        }
        control!(aome_enc_control_id_AOME_SET_CPUUSED, speed);
        control!(aome_enc_control_id_AOME_SET_CQ_LEVEL, cq_level);
        if cfg.g_threads > 1 {
            control!(aome_enc_control_id_AV1E_SET_ROW_MT, 1u32);
        }
        control!(aome_enc_control_id_AV1E_SET_AUTO_TILES, 1u32);
        control!(aome_enc_control_id_AV1E_SET_COLOR_RANGE, 1i32);
        control!(aome_enc_control_id_AV1E_SET_CHROMA_SAMPLE_POSITION, 0i32);
        if !alpha {
            control!(aome_enc_control_id_AV1E_SET_COLOR_PRIMARIES, 1i32);
            control!(aome_enc_control_id_AV1E_SET_MATRIX_COEFFICIENTS, 6i32);
            control!(aome_enc_control_id_AV1E_SET_TRANSFER_CHARACTERISTICS, 13i32);
        }
        control!(aome_enc_control_id_AOME_SET_TUNING, tune as i32);
        control!(aome_enc_control_id_AV1E_SET_SKIP_POSTPROC_FILTERING, 1u32);
        control!(aome_enc_control_id_AV1E_SET_ENABLE_INTRABC, 0i32);

        let mut img = std::mem::zeroed::<sys::aom_image_t>();
        let fmt = if subsampled || alpha { sys::aom_img_fmt_AOM_IMG_FMT_I420 } else { sys::aom_img_fmt_AOM_IMG_FMT_I444 };
        // Planes are supplied directly; aom_img_wrap with a dummy pointer only fills the descriptor.
        sys::aom_img_wrap(&mut img, fmt, width, height, 1, 1 as *mut u8);
        let gray_chroma;
        match plane {
            Plane::Color { y, u, v, chroma_w, .. } => {
                img.planes = [y.as_ptr() as *mut u8, u.as_ptr() as *mut u8, v.as_ptr() as *mut u8];
                img.stride = [width as i32, chroma_w as i32, chroma_w as i32];
            }
            Plane::Alpha(a) => {
                let cw = width.div_ceil(2) as usize;
                gray_chroma = vec![128u8; cw * height.div_ceil(2) as usize];
                img.planes = [a.as_ptr() as *mut u8, gray_chroma.as_ptr() as *mut u8, gray_chroma.as_ptr() as *mut u8];
                img.stride = [width as i32, cw as i32, cw as i32];
                img.monochrome = 1;
            }
        }
        img.range = sys::aom_color_range_AOM_CR_FULL_RANGE;
        if !alpha {
            img.cp = 1;
            img.tc = 13;
            img.mc = 6;
        }

        let mut out = Vec::new();
        let collect = |c: *mut sys::aom_codec_ctx_t, out: &mut Vec<u8>| {
            let mut iter = ptr::null();
            loop {
                let pkt = sys::aom_codec_get_cx_data(c, &mut iter);
                if pkt.is_null() {
                    break;
                }
                if (*pkt).kind == sys::aom_codec_cx_pkt_kind_AOM_CODEC_CX_FRAME_PKT {
                    let f = (*pkt).data.frame;
                    out.extend_from_slice(std::slice::from_raw_parts(f.buf as *const u8, f.sz));
                }
            }
        };
        if sys::aom_codec_encode(c, &img, 0, 1, 0) != sys::aom_codec_err_t_AOM_CODEC_OK {
            bail!("AV1 encoding failed: {}", aom::error_detail(c));
        }
        collect(c, &mut out);
        while sys::aom_codec_encode(c, ptr::null(), 0, 0, 0) == sys::aom_codec_err_t_AOM_CODEC_OK {
            let before = out.len();
            collect(c, &mut out);
            if out.len() == before {
                break;
            }
        }
        if out.is_empty() {
            bail!("AV1 encoder produced no data");
        }
        Ok(out)
    }
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

pub fn encode(img: &Image, s: Settings) -> Result<Vec<u8>> {
    // libvips' rule: 4:2:0 below quality 90, 4:4:4 from 90. (sharp overrides
    // it with chromaSubsampling '4:4:4' by default; 4:2:0 is kept here as it
    // encodes faster and smaller, see README.)
    let subsampled = s.quality < 90;
    let (y, u, v, chroma_w) = rgb_to_ycbcr(img, subsampled);
    let color = encode_av1(img.width, img.height, Plane::Color { y: &y, u: &u, v: &v, chroma_w, subsampled }, s)?;
    drop((y, u, v));
    let alpha = if img.has_alpha() {
        let a: Vec<u8> = img.data.chunks_exact(4).map(|p| p[3]).collect();
        Some(encode_av1(img.width, img.height, Plane::Alpha(&a), s)?)
    } else {
        None
    };
    let mut avif = avif_serialize::Aviffy::new();
    avif.set_full_color_range(true)
        .set_matrix_coefficients(avif_serialize::constants::MatrixCoefficients::Bt601)
        .set_transfer_characteristics(avif_serialize::constants::TransferCharacteristics::Srgb)
        .set_color_primaries(avif_serialize::constants::ColorPrimaries::Bt709)
        .set_chroma_subsampling((subsampled, subsampled))
        .set_seq_profile(if subsampled { 0 } else { 1 });
    Ok(avif.to_vec(&color, alpha.as_deref(), img.width, img.height, 8))
}

pub fn version() -> String {
    aom::version()
}
