//! Embedded ICC profile handling: convert to sRGB like sharp does before
//! encoding (sharp strips the profile and outputs sRGB pixels).

use std::sync::{Arc, OnceLock};

use moxcms::{
    ColorProfile, DataColorSpace, Layout, ParametricCurve, ToneReprCurve, Transform8BitExecutor, TransformOptions,
};

use crate::image::Image;

fn srgb_profile() -> &'static ColorProfile {
    static SRGB: OnceLock<ColorProfile> = OnceLock::new();
    SRGB.get_or_init(ColorProfile::new_srgb)
}

fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn eval_trc(trc: &ToneReprCurve, x: f32) -> Option<f32> {
    match trc {
        ToneReprCurve::Lut(lut) => {
            if lut.is_empty() {
                Some(x)
            } else if lut.len() == 1 {
                let gamma = f32::from(lut[0]) / 256.0;
                Some(x.powf(gamma))
            } else {
                let pos = x * (lut.len() - 1) as f32;
                let i0 = (pos.floor() as usize).min(lut.len() - 1);
                let i1 = (i0 + 1).min(lut.len() - 1);
                let frac = pos - i0 as f32;
                Some((f32::from(lut[i0]) * (1.0 - frac) + f32::from(lut[i1]) * frac) / 65535.0)
            }
        }
        ToneReprCurve::Parametric(params) => Some(ParametricCurve::new(params)?.eval(x)),
    }
}

fn trc_is_srgb(trc: Option<&ToneReprCurve>) -> bool {
    let Some(trc) = trc else { return false };
    const SAMPLES: usize = 32;
    const MAX_ERR: f32 = 1.0 / 255.0;
    for i in 0..SAMPLES {
        let x = i as f32 / (SAMPLES - 1) as f32;
        let Some(y) = eval_trc(trc, x) else { return false };
        if (y - srgb_to_linear(x)).abs() > MAX_ERR {
            return false;
        }
    }
    true
}

/// True when the profile is (close enough to) sRGB, so no transform is needed.
fn is_srgb_like(profile: &ColorProfile) -> bool {
    let srgb = srgb_profile();
    let close = |a: moxcms::Xyzd, b: moxcms::Xyzd| {
        (a.x - b.x).abs() < 0.003 && (a.y - b.y).abs() < 0.003 && (a.z - b.z).abs() < 0.003
    };
    profile.color_space == DataColorSpace::Rgb
        && profile.lut_a_to_b_perceptual.is_none()
        && profile.lut_a_to_b_colorimetric.is_none()
        && profile.lut_a_to_b_saturation.is_none()
        && close(profile.red_colorant, srgb.red_colorant)
        && close(profile.green_colorant, srgb.green_colorant)
        && close(profile.blue_colorant, srgb.blue_colorant)
        && trc_is_srgb(profile.red_trc.as_ref())
        && trc_is_srgb(profile.green_trc.as_ref())
        && trc_is_srgb(profile.blue_trc.as_ref())
}

/// Transform RGB(A) pixels from the embedded profile to sRGB in place.
/// Invalid, unsupported or sRGB profiles leave the pixels unchanged.
pub fn to_srgb(img: &mut Image, icc: &[u8]) {
    let Ok(profile) = ColorProfile::new_from_slice(icc) else { return };
    apply_profile(img, &profile);
}

/// Convert from colour described by CICP (H.273) primaries and transfer
/// characteristics, as AVIF signals it without an ICC profile. Values that
/// browsers display as sRGB (BT.709 / unspecified primaries with an sRGB-like
/// transfer) are left unchanged.
pub fn cicp_to_srgb(img: &mut Image, primaries: u8, transfer: u8) {
    let srgb_primaries = matches!(primaries, 1 | 2);
    let srgb_transfer = matches!(transfer, 1 | 2 | 6 | 13 | 14 | 15);
    if srgb_primaries && srgb_transfer {
        return;
    }
    let (Ok(cp), Ok(tc)) =
        (moxcms::CicpColorPrimaries::try_from(primaries), moxcms::TransferCharacteristics::try_from(transfer))
    else {
        return;
    };
    let profile = ColorProfile::new_from_cicp(moxcms::CicpProfile {
        color_primaries: cp,
        transfer_characteristics: tc,
        matrix_coefficients: moxcms::MatrixCoefficients::Bt709,
        full_range: true,
    });
    apply_profile(img, &profile);
}

fn apply_profile(img: &mut Image, profile: &ColorProfile) {
    match profile.color_space {
        DataColorSpace::Rgb => {}
        DataColorSpace::Gray => return gray_to_srgb(img, profile),
        _ => return,
    }
    if is_srgb_like(profile) {
        return;
    }
    let layout = if img.has_alpha() { Layout::Rgba } else { Layout::Rgb };
    let Ok(transform) = profile.create_transform_8bit(layout, srgb_profile(), layout, TransformOptions::default())
    else {
        return;
    };
    transform_rows_in_place(img, |s, d| transform.transform(s, d).is_ok());
}

/// Transform each row through `f(src, dst)`, in place. A row that fails is
/// left as it was; if the first row fails nothing is changed.
fn transform_rows_in_place(img: &mut Image, mut f: impl FnMut(&[u8], &mut [u8]) -> bool) {
    let stride = img.stride();
    let mut scratch = vec![0u8; stride];
    for (i, row) in img.data.chunks_exact_mut(stride).enumerate() {
        scratch.copy_from_slice(row);
        if !f(&scratch, row) {
            row.copy_from_slice(&scratch);
            if i == 0 {
                return;
            }
        }
    }
}

/// Gray profile on decoded gray data (expanded to R=G=B by the decoders):
/// map the gray channel through the profile into sRGB.
fn gray_to_srgb(img: &mut Image, profile: &ColorProfile) {
    let (src_layout, dst_layout) =
        if img.has_alpha() { (Layout::GrayAlpha, Layout::Rgba) } else { (Layout::Gray, Layout::Rgb) };
    let Ok(transform) =
        profile.create_transform_8bit(src_layout, srgb_profile(), dst_layout, TransformOptions::default())
    else {
        return;
    };
    let c = img.channels as usize;
    let stride = img.stride();
    let mut gray = vec![0u8; img.width as usize * (c - 2)];
    let mut out = vec![0u8; stride];
    for (i, row) in img.data.chunks_exact_mut(stride).enumerate() {
        // Gray (and alpha) samples of this row, then the converted row back in place.
        for (g, p) in gray.chunks_exact_mut(c - 2).zip(row.chunks_exact(c)) {
            g[0] = p[0];
            if c == 4 {
                g[1] = p[3];
            }
        }
        if transform.transform(&gray, &mut out).is_ok() {
            row.copy_from_slice(&out);
        } else if i == 0 {
            return;
        }
    }
}

/// Reusable CMYK-to-sRGB converter for row-streaming CMYK JPEGs.
pub struct CmykConverter {
    transform: Option<Arc<Transform8BitExecutor>>,
    inverted: bool,
}

impl CmykConverter {
    pub fn new(icc: Option<&[u8]>, inverted: bool) -> Self {
        let transform = icc
            .and_then(|b| ColorProfile::new_from_slice(b).ok())
            .filter(|p| p.color_space == DataColorSpace::Cmyk)
            .and_then(|p| {
                p.create_transform_8bit(Layout::Rgba, srgb_profile(), Layout::Rgb, TransformOptions::default()).ok()
            });
        CmykConverter { transform, inverted }
    }

    /// Convert `cmyk` (4 bytes/pixel, inverted in place if Adobe-style) into `rgb` (3 bytes/pixel).
    pub fn convert_chunk(&self, cmyk: &mut [u8], rgb: &mut [u8]) {
        if self.inverted {
            cmyk.iter_mut().for_each(|v| *v = 255 - *v);
        }
        if let Some(ref t) = self.transform
            && t.transform(cmyk, rgb).is_ok()
        {
            return;
        }
        for (p, dst) in cmyk.as_chunks::<4>().0.iter().zip(rgb.as_chunks_mut::<3>().0) {
            let k = 255 - u32::from(p[3]);
            dst[0] = ((255 - u32::from(p[0])) * k / 255) as u8;
            dst[1] = ((255 - u32::from(p[1])) * k / 255) as u8;
            dst[2] = ((255 - u32::from(p[2])) * k / 255) as u8;
        }
    }
}

/// Convert CMYK samples (as decoded by libjpeg) to RGB. With an embedded
/// CMYK profile the conversion goes through it; otherwise a naive formula.
/// `inverted` is true for Adobe-style JPEGs, which store inverted values.
pub fn cmyk_to_rgb(mut cmyk: Vec<u8>, width: u32, height: u32, icc: Option<&[u8]>, inverted: bool) -> Image {
    let pixels = width as usize * height as usize;
    let conv = CmykConverter::new(icc, inverted);
    let mut out = vec![0u8; pixels * 3];
    let row_in = width as usize * 4;
    let row_out = width as usize * 3;
    for (s, d) in cmyk.chunks_exact_mut(row_in).zip(out.chunks_exact_mut(row_out)) {
        conv.convert_chunk(s, d);
    }
    Image::new(width, height, 3, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_linear_gray_to_srgb() {
        let icc = ColorProfile::new_gray_with_gamma(1.0).encode().unwrap();
        let mut img = Image::new(2, 1, 3, vec![128, 128, 128, 0, 0, 0]);
        to_srgb(&mut img, &icc);
        // Linear 0.5 is about 188 in sRGB.
        assert!((185..=191).contains(&img.data[0]), "{:?}", img.data);
        assert_eq!(img.data[0], img.data[1]);
        assert_eq!(&img.data[3..], &[0, 0, 0]);
        let mut rgba = Image::new(1, 1, 4, vec![128, 128, 128, 77]);
        to_srgb(&mut rgba, &icc);
        assert!((185..=191).contains(&rgba.data[0]));
        assert_eq!(rgba.data[3], 77);
    }

    #[test]
    fn converts_cicp_linear_transfer() {
        // BT.709 primaries, linear transfer (8): mid gray brightens.
        let mut img = Image::new(1, 1, 3, vec![128, 128, 128]);
        cicp_to_srgb(&mut img, 1, 8);
        assert!((185..=191).contains(&img.data[0]), "{:?}", img.data);
        // sRGB signalling is untouched.
        let mut same = Image::new(1, 1, 3, vec![128, 64, 32]);
        cicp_to_srgb(&mut same, 1, 13);
        assert_eq!(same.data, vec![128, 64, 32]);
    }
}
