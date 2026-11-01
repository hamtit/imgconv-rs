//! EXIF orientation: tag parsing and the matching pixel transform.

use crate::image::Image;

/// Read the Orientation tag (0x0112) from a TIFF-structured EXIF payload
/// (the bytes after "Exif\0\0").
pub fn exif_orientation(tiff: &[u8]) -> Option<u8> {
    let le = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = tiff.get(o..o + 2)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = tiff.get(o..o + 4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    for i in 0..count.min(512) {
        let entry = ifd + 2 + i * 12;
        if u16_at(entry)? == 0x0112 {
            let v = u16_at(entry + 8)?;
            return (1..=8).contains(&v).then_some(v as u8);
        }
    }
    None
}

/// Dimensions after applying `orientation`.
pub fn oriented_size(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if orientation >= 5 { (height, width) } else { (width, height) }
}

/// Apply an EXIF orientation so the pixels are upright.
pub fn apply(img: Image, orientation: u8) -> Image {
    if !(2..=8).contains(&orientation) {
        return img;
    }
    let (w, h, c) = (img.width as usize, img.height as usize, img.channels as usize);
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; img.data.len()];
    for y in 0..h {
        let row = &img.data[y * w * c..(y + 1) * w * c];
        for x in 0..w {
            // Destination of source pixel (x, y).
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => unreachable!(),
            };
            let d = (dy * nw + dx) * c;
            out[d..d + c].copy_from_slice(&row[x * c..x * c + c]);
        }
    }
    Image::new(nw as u32, nh as u32, img.channels, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> Image {
        // 3x2 gray-ish RGB with distinct pixels 0..6
        let data = (0u8..6).flat_map(|v| [v, v, v]).collect();
        Image::new(3, 2, 3, data)
    }

    fn px(i: &Image) -> Vec<u8> {
        i.data.chunks(3).map(|p| p[0]).collect()
    }

    #[test]
    fn rotates() {
        // 0 1 2
        // 3 4 5
        assert_eq!(px(&apply(img(), 2)), vec![2, 1, 0, 5, 4, 3]);
        assert_eq!(px(&apply(img(), 3)), vec![5, 4, 3, 2, 1, 0]);
        assert_eq!(px(&apply(img(), 4)), vec![3, 4, 5, 0, 1, 2]);
        let r6 = apply(img(), 6); // 90° clockwise
        assert_eq!((r6.width, r6.height), (2, 3));
        assert_eq!(px(&r6), vec![3, 0, 4, 1, 5, 2]);
        let r8 = apply(img(), 8); // 90° counter-clockwise
        assert_eq!(px(&r8), vec![2, 5, 1, 4, 0, 3]);
        assert_eq!(px(&apply(img(), 5)), vec![0, 3, 1, 4, 2, 5]);
        assert_eq!(px(&apply(img(), 7)), vec![5, 2, 4, 1, 3, 0]);
    }

    #[test]
    fn parses_exif() {
        let mut t = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        t.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0]);
        assert_eq!(exif_orientation(&t), Some(6));
        assert_eq!(exif_orientation(b"XX"), None);
    }
}
