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

fn flip_h<const C: usize>(data: &mut [u8], w: usize) {
    let stride = w * C;
    for row in data.chunks_exact_mut(stride) {
        row.as_chunks_mut::<C>().0.reverse();
    }
}

fn rotate_180<const C: usize>(data: &mut [u8]) {
    data.as_chunks_mut::<C>().0.reverse();
}

fn flip_v(data: &mut [u8], stride: usize, h: usize) {
    for y in 0..h / 2 {
        let top = y * stride;
        let bot = (h - 1 - y) * stride;
        let (head, tail) = data.split_at_mut(bot);
        head[top..top + stride].swap_with_slice(&mut tail[..stride]);
    }
}

fn transpose_blocked<const C: usize>(src: &[u8], w: usize, h: usize, orientation: u8) -> Vec<u8> {
    const TILE: usize = 32;
    let nw = h;
    let mut out = vec![0u8; src.len()];
    macro_rules! run {
        ($dx:expr, $dy:expr) => {
            for by in (0..h).step_by(TILE) {
                let y_end = (by + TILE).min(h);
                for bx in (0..w).step_by(TILE) {
                    let x_end = (bx + TILE).min(w);
                    for y in by..y_end {
                        let row = &src[y * w * C..(y + 1) * w * C];
                        for x in bx..x_end {
                            let dx = $dx(x, y);
                            let dy = $dy(x, y);
                            let d = (dy * nw + dx) * C;
                            out[d..d + C].copy_from_slice(&row[x * C..x * C + C]);
                        }
                    }
                }
            }
        };
    }
    match orientation {
        5 => run!(|_x, y| y, |x, _y| x),
        6 => run!(|_x, y| h - 1 - y, |x, _y| x),
        7 => run!(|_x, y| h - 1 - y, |x, _y| w - 1 - x),
        8 => run!(|_x, y| y, |x, _y| w - 1 - x),
        _ => unreachable!(),
    }
    out
}

/// Apply an EXIF orientation so the pixels are upright.
pub fn apply(mut img: Image, orientation: u8) -> Image {
    if !(2..=8).contains(&orientation) {
        return img;
    }
    let (w, h, c) = (img.width as usize, img.height as usize, img.channels as usize);
    match orientation {
        2 => {
            if c == 4 {
                flip_h::<4>(&mut img.data, w);
            } else {
                flip_h::<3>(&mut img.data, w);
            }
            img
        }
        3 => {
            if c == 4 {
                rotate_180::<4>(&mut img.data);
            } else {
                rotate_180::<3>(&mut img.data);
            }
            img
        }
        4 => {
            flip_v(&mut img.data, w * c, h);
            img
        }
        5..=8 => {
            let out = if c == 4 {
                transpose_blocked::<4>(&img.data, w, h, orientation)
            } else {
                transpose_blocked::<3>(&img.data, w, h, orientation)
            };
            Image::new(img.height, img.width, img.channels, out)
        }
        _ => unreachable!(),
    }
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
