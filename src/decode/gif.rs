//! GIF decoding with frame compositing onto the logical screen, as browsers
//! (and libvips' gifload) present it.

use std::io::Cursor;

use gif::{ColorOutput, DecodeOptions, DisposalMethod};

use crate::error::{Error, Result};
use crate::image::Image;

fn gif_err(e: gif::DecodingError) -> Error {
    Error::Decode(format!("GIF decoding failed: {e}"))
}

/// Streams composited RGBA frames of a GIF.
pub struct GifFrames<'a> {
    decoder: gif::Decoder<Cursor<&'a [u8]>>,
    canvas: Vec<u8>,
    previous: Option<Vec<u8>>,
    pending_dispose: Option<(DisposalMethod, u16, u16, u16, u16)>,
    pub width: u32,
    pub height: u32,
}

pub struct Frame<'f> {
    pub rgba: &'f [u8],
    /// Delay in milliseconds as stored (0 when unset).
    pub delay_ms: u32,
}

impl<'a> GifFrames<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self> {
        let mut opts = DecodeOptions::new();
        opts.set_color_output(ColorOutput::RGBA);
        let decoder = opts.read_info(Cursor::new(buf)).map_err(gif_err)?;
        let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
        if width == 0 || height == 0 {
            return Err(Error::InvalidImage("Invalid GIF dimensions".into()));
        }
        Ok(GifFrames {
            decoder,
            canvas: vec![0u8; width as usize * height as usize * 4],
            previous: None,
            pending_dispose: None,
            width,
            height,
        })
    }

    /// Next composited frame. Truncated data after at least one frame ends the
    /// animation instead of failing, as in browsers.
    pub fn next_frame(&mut self, first: bool) -> Result<Option<Frame<'_>>> {
        let (width, height) = (self.width, self.height);
        if let Some((dispose, left, top, w, h)) = self.pending_dispose.take() {
            match dispose {
                DisposalMethod::Background => {
                    let (x0, y0, x1, y1) = clip(width, height, left, top, w, h);
                    let stride = self.width as usize * 4;
                    for y in y0..y1 {
                        self.canvas[y * stride + x0 * 4..y * stride + x1 * 4].fill(0);
                    }
                }
                DisposalMethod::Previous => {
                    if let Some(prev) = self.previous.take() {
                        self.canvas = prev;
                    }
                }
                _ => {}
            }
        }
        let frame = match self.decoder.read_next_frame() {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(e) if !first => {
                let _ = e;
                return Ok(None);
            }
            Err(e) => return Err(gif_err(e)),
        };
        let (left, top, fw, fh) = (frame.left, frame.top, frame.width, frame.height);
        let dispose = frame.dispose;
        let delay_ms = u32::from(frame.delay) * 10;
        if dispose == DisposalMethod::Previous {
            self.previous = Some(self.canvas.clone());
        }
        let buffer = &frame.buffer[..];
        let (x0, y0, x1, y1) = clip(width, height, left, top, fw, fh);
        let stride = width as usize * 4;
        for y in y0..y1 {
            let src_row = (y - top as usize) * fw as usize * 4;
            for x in x0..x1 {
                let s = src_row + (x - left as usize) * 4;
                if buffer[s + 3] != 0 {
                    let d = y * stride + x * 4;
                    self.canvas[d..d + 4].copy_from_slice(&buffer[s..s + 4]);
                }
            }
        }
        self.pending_dispose = Some((dispose, left, top, fw, fh));
        Ok(Some(Frame { rgba: &self.canvas, delay_ms }))
    }
}

/// Clip a rectangle at (left, top) of size w x h to a `width` x `height`
/// canvas, as half-open pixel ranges.
fn clip(width: u32, height: u32, left: u16, top: u16, w: u16, h: u16) -> (usize, usize, usize, usize) {
    let x0 = (left as usize).min(width as usize);
    let y0 = (top as usize).min(height as usize);
    let x1 = (left as usize + w as usize).min(width as usize);
    let y1 = (top as usize + h as usize).min(height as usize);
    (x0, y0, x1, y1)
}

/// First frame of a GIF as a still image.
pub fn decode_first(buf: &[u8]) -> Result<Image> {
    let mut frames = GifFrames::new(buf)?;
    let (w, h) = (frames.width, frames.height);
    let frame = frames.next_frame(true)?.ok_or_else(|| Error::InvalidImage("GIF has no frames".into()))?;
    Ok(Image::new(w, h, 4, frame.rgba.to_vec()))
}
