//! Format detection and header-only probing (no pixel decoding), used to
//! enforce the pixel limit before any large allocation happens.

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Gif,
    Webp,
    Svg,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Svg => "svg",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub format: Format,
    /// Stored dimensions, before EXIF orientation. For GIF this is the logical
    /// screen (canvas), which is what browsers and the animation use.
    pub width: u32,
    pub height: u32,
    /// Number of frames (1 for still images). Only GIF and WebP report more.
    pub frames: u32,
    /// EXIF orientation 1-8 (1 when absent). JPEG only.
    pub orientation: u8,
    /// Pixels of the largest single frame as stored (GIF frame descriptors
    /// can exceed the logical screen and are decoded at full size).
    pub max_frame_pixels: u64,
}

pub fn detect(buf: &[u8]) -> Option<Format> {
    if buf.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if buf.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if buf.starts_with(b"GIF87a") || buf.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
        return Some(Format::Webp);
    }
    if looks_like_svg(buf) {
        return Some(Format::Svg);
    }
    None
}

fn looks_like_svg(buf: &[u8]) -> bool {
    let head = &buf[..buf.len().min(4096)];
    let mut s = head;
    if s.starts_with(&[0xEF, 0xBB, 0xBF]) {
        s = &s[3..];
    }
    let trimmed = s.iter().position(|c| !c.is_ascii_whitespace()).map(|i| &s[i..]).unwrap_or(&[]);
    if !trimmed.starts_with(b"<") {
        return false;
    }
    head.windows(4).any(|w| w == b"<svg")
}

pub fn probe(buf: &[u8]) -> Result<Info> {
    let Some(format) = detect(buf) else { return Err(Error::UnsupportedFormat) };
    let mut max_frame_pixels = 0u64;
    let (width, height, frames, orientation) = match format {
        Format::Jpeg => {
            let (w, h, o) = probe_jpeg(buf)?;
            (w, h, 1, o)
        }
        Format::Png => {
            if buf.len() < 24 || &buf[12..16] != b"IHDR" {
                return Err(Error::InvalidImage("Invalid PNG header".into()));
            }
            (be32(&buf[16..]), be32(&buf[20..]), 1, 1)
        }
        Format::Gif => {
            let (w, h, n, largest) = probe_gif(buf)?;
            max_frame_pixels = largest;
            (w, h, n, 1)
        }
        Format::Webp => {
            let (w, h, n) = crate::decode::webp::probe(buf)?;
            (w, h, n, 1)
        }
        Format::Svg => {
            let (w, h) = crate::decode::svg::probe(buf)?;
            (w, h, 1, 1)
        }
    };
    if width == 0 || height == 0 {
        return Err(Error::InvalidImage("Invalid image dimensions".into()));
    }
    max_frame_pixels = max_frame_pixels.max(u64::from(width) * u64::from(height));
    Ok(Info { format, width, height, frames, orientation, max_frame_pixels })
}

fn be16(b: &[u8]) -> u32 {
    u32::from(b[0]) << 8 | u32::from(b[1])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn le16(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8
}

/// Width, height (from the first SOFn) and EXIF orientation of a JPEG.
pub fn probe_jpeg(buf: &[u8]) -> Result<(u32, u32, u8)> {
    let mut i = 2;
    let mut orientation = 1;
    while i + 4 <= buf.len() {
        if buf[i] != 0xFF {
            return Err(Error::InvalidImage("Invalid JPEG marker".into()));
        }
        let marker = buf[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = be16(&buf[i + 2..]) as usize;
        if len < 2 || i + 2 + len > buf.len() {
            break;
        }
        let seg = &buf[i + 4..i + 2 + len];
        match marker {
            0xE1 if seg.starts_with(b"Exif\0\0") => {
                orientation = crate::orient::exif_orientation(&seg[6..]).unwrap_or(1);
            }
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => {
                if seg.len() < 5 {
                    return Err(Error::InvalidImage("Invalid JPEG frame header".into()));
                }
                return Ok((be16(&seg[3..]), be16(&seg[1..]), orientation));
            }
            0xDA | 0xD9 => break,
            _ => {}
        }
        i += 2 + len;
    }
    Err(Error::InvalidImage("JPEG frame header not found".into()))
}

/// Logical screen size, frame count and largest frame descriptor area of a
/// GIF, by walking its block structure without decompressing image data.
pub fn probe_gif(buf: &[u8]) -> Result<(u32, u32, u32, u64)> {
    if buf.len() < 13 {
        return Err(Error::InvalidImage("Invalid GIF dimensions".into()));
    }
    let width = le16(&buf[6..]);
    let height = le16(&buf[8..]);
    let mut i = 13;
    if buf[10] & 0x80 != 0 {
        i += 3 << ((buf[10] & 7) + 1);
    }
    let mut frames = 0u32;
    let mut largest = 0u64;
    let skip_sub_blocks = |mut i: usize| -> Option<usize> {
        loop {
            let n = *buf.get(i)? as usize;
            i += 1;
            if n == 0 {
                return Some(i);
            }
            i += n;
        }
    };
    while let Some(&b) = buf.get(i) {
        match b {
            0x21 => match skip_sub_blocks(i + 2) {
                Some(next) => i = next,
                None => break,
            },
            0x2C => {
                let Some(desc) = buf.get(i + 1..i + 10) else { break };
                i += 10;
                if desc[8] & 0x80 != 0 {
                    i += 3 << ((desc[8] & 7) + 1);
                }
                // LZW minimum code size, then the data sub-blocks. A frame
                // counts once its descriptor is present, as in browsers.
                frames += 1;
                largest = largest.max(u64::from(le16(&desc[4..])) * u64::from(le16(&desc[6..])));
                match skip_sub_blocks(i + 1) {
                    Some(next) => i = next,
                    None => break,
                }
            }
            _ => break, // 0x3B trailer or garbage
        }
    }
    Ok((width, height, frames.max(1), largest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(detect(b"\xFF\xD8\xFF\xE0"), Some(Format::Jpeg));
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n...."), Some(Format::Png));
        assert_eq!(detect(b"GIF89a"), Some(Format::Gif));
        assert_eq!(detect(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::Webp));
        assert_eq!(detect(b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf"), None);
        assert_eq!(detect(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"), None);
        assert_eq!(detect(b"  <?xml version=\"1.0\"?><svg xmlns=\"\"/>"), Some(Format::Svg));
        assert_eq!(detect(b"<html>"), None);
    }
}
