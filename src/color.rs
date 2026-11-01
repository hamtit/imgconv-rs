//! Embedded ICC profile handling: convert to sRGB like sharp does before
//! encoding (sharp strips the profile and outputs sRGB pixels).

use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};

use crate::image::Image;

/// True when the profile is (close enough to) sRGB, so no transform is needed.
fn is_srgb_like(profile: &ColorProfile) -> bool {
    let srgb = ColorProfile::new_srgb();
    let close = |a: moxcms::Xyzd, b: moxcms::Xyzd| {
        (a.x - b.x).abs() < 0.003 && (a.y - b.y).abs() < 0.003 && (a.z - b.z).abs() < 0.003
    };
    profile.color_space == DataColorSpace::Rgb
        && close(profile.red_colorant, srgb.red_colorant)
        && close(profile.green_colorant, srgb.green_colorant)
        && close(profile.blue_colorant, srgb.blue_colorant)
        && profile.red_trc.is_some()
        && format!("{:?}", profile.red_trc) == format!("{:?}", srgb.red_trc)
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
    let (Ok(cp), Ok(tc)) = (moxcms::CicpColorPrimaries::try_from(primaries), moxcms::TransferCharacteristics::try_from(transfer))
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
    let dst = ColorProfile::new_srgb();
    let Ok(transform) = profile.create_transform_8bit(layout, &dst, layout, TransformOptions::default())
    else {
        return;
    };
    let src = img.data.clone();
    let stride = img.stride();
    for (s, d) in src.chunks_exact(stride).zip(img.data.chunks_exact_mut(stride)) {
        if transform.transform(s, d).is_err() {
            img.data = src;
            return;
        }
    }
}

/// Gray profile on decoded gray data (expanded to R=G=B by the decoders):
/// map the gray channel through the profile into sRGB.
fn gray_to_srgb(img: &mut Image, profile: &ColorProfile) {
    let (src_layout, dst_layout) = if img.has_alpha() { (Layout::GrayAlpha, Layout::Rgba) } else { (Layout::Gray, Layout::Rgb) };
    let Ok(transform) = profile.create_transform_8bit(src_layout, &ColorProfile::new_srgb(), dst_layout, TransformOptions::default())
    else {
        return;
    };
    let c = img.channels as usize;
    let gray: Vec<u8> = img.data.chunks_exact(c).flat_map(|p| if c == 4 { vec![p[0], p[3]] } else { vec![p[0]] }).collect();
    let (src_stride, dst_stride) = (img.width as usize * (c - 2), img.stride());
    let mut out = vec![0u8; img.data.len()];
    for (s, d) in gray.chunks_exact(src_stride).zip(out.chunks_exact_mut(dst_stride)) {
        if transform.transform(s, d).is_err() {
            return;
        }
    }
    img.data = out;
}

/// Convert CMYK samples (as decoded by libjpeg) to RGB. With an embedded
/// CMYK profile the conversion goes through it; otherwise a naive formula.
/// `inverted` is true for Adobe-style JPEGs, which store inverted values.
pub fn cmyk_to_rgb(cmyk: &[u8], width: u32, height: u32, icc: Option<&[u8]>, inverted: bool) -> Image {
    let pixels = width as usize * height as usize;
    let mut normalized;
    let cmyk = if inverted {
        normalized = cmyk.to_vec();
        normalized.iter_mut().for_each(|v| *v = 255 - *v);
        &normalized[..]
    } else {
        cmyk
    };
    if let Some(profile) = icc.and_then(|b| ColorProfile::new_from_slice(b).ok())
        && profile.color_space == DataColorSpace::Cmyk
        && let Ok(t) = profile.create_transform_8bit(
            Layout::Rgba,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            TransformOptions::default(),
        )
    {
        let mut out = vec![0u8; pixels * 3];
        let row_in = width as usize * 4;
        let row_out = width as usize * 3;
        let ok = cmyk
            .chunks_exact(row_in)
            .zip(out.chunks_exact_mut(row_out))
            .all(|(s, d)| t.transform(s, d).is_ok());
        if ok {
            return Image::new(width, height, 3, out);
        }
    }
    let mut out = Vec::with_capacity(pixels * 3);
    for p in cmyk.chunks_exact(4) {
        let k = 255 - u32::from(p[3]);
        for &c in &p[..3] {
            out.push(((255 - u32::from(c)) * k / 255) as u8);
        }
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
