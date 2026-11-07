//! The conversion pipeline: probe → (pixel limit) → decode with
//! shrink-on-load → Lanczos3 fit-inside resize → EXIF orientation → sRGB →
//! encode. Animated GIFs become animated JPEG XL (for `jxl`) or animated WebP, frame by frame.

use crate::decode::{self, Decoded, gif::GifFrames, shrink_on_load};
use crate::encode::{avif, jxl, webp};
use crate::error::Result;
use crate::image::{Image, ImageView};
use crate::probe::{self, Format, Info};
use crate::{bail, color, orient, resize};

/// sharp's default `limitInputPixels` (0x3FFF * 0x3FFF).
pub const DEFAULT_MAX_PIXELS: u64 = 268_402_689;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Webp,
    Avif,
    Jxl,
}

impl OutputFormat {
    pub fn name(self) -> &'static str {
        match self {
            OutputFormat::Webp => "webp",
            OutputFormat::Avif => "avif",
            OutputFormat::Jxl => "jxl",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "webp" => Some(OutputFormat::Webp),
            "avif" => Some(OutputFormat::Avif),
            "jxl" => Some(OutputFormat::Jxl),
            _ => None,
        }
    }

    /// Encoder effort used when none is given: sharp's defaults (WebP 4,
    /// AVIF 4) and jxl-mini-encoder's effort 7 for JPEG XL.
    pub fn default_effort(self) -> u32 {
        match self {
            OutputFormat::Webp | OutputFormat::Avif => 4,
            OutputFormat::Jxl => jxl::DEFAULT_EFFORT,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub format: OutputFormat,
    /// 0-100, the same scale for all formats (100 = lossless JPEG XL).
    pub quality: f32,
    /// Fit within this many pixels on each side, never enlarging.
    pub max_dimension: Option<u32>,
    pub effort: Option<u32>,
    pub max_pixels: u64,
    /// Convert animated GIFs to animations: JPEG XL for `jxl`, otherwise animated WebP.
    pub animated: bool,
    /// Encoder threads for one conversion (AVIF / JPEG XL).
    pub threads: u32,
}

impl Options {
    pub fn new(format: OutputFormat, quality: f32) -> Self {
        Options { format, quality, max_dimension: None, effort: None, max_pixels: DEFAULT_MAX_PIXELS, animated: true, threads: 1 }
    }
}

#[derive(Clone, Debug)]
pub struct Output {
    pub data: Vec<u8>,
    pub format: OutputFormat,
    pub width: u32,
    pub height: u32,
    /// Frames in the output (more than 1 only for animated WebP).
    pub frames: u32,
    pub input: Info,
}

fn check_pixels(width: u32, height: u32, frames: u32, max: u64) -> Result<()> {
    let pixels = u64::from(width) * u64::from(height) * u64::from(frames.max(1));
    if pixels > max {
        bail!("Image exceeds pixel limit: {pixels} pixels");
    }
    Ok(())
}

/// Decode, resize, orient and color-convert a still image.
fn prepare_still(buf: &[u8], info: &Info, max_dimension: Option<u32>) -> Result<Image> {
    let (ow, oh) = orient::oriented_size(info.width, info.height, info.orientation);
    let target = max_dimension.and_then(|m| resize::fit_inside(ow, oh, m));
    // Target in stored (pre-orientation) axes.
    let stored_target = target.map(|(w, h)| if info.orientation >= 5 { (h, w) } else { (w, h) });
    let shrink = |t: Option<(u32, u32)>| t.map_or(1, |t| shrink_on_load(info.width, info.height, t));

    let Decoded { mut image, icc, orientation } = match info.format {
        Format::Jpeg => decode::jpeg::decode(buf, shrink(stored_target), stored_target)?,
        Format::Webp => decode::webp::decode(buf, shrink(stored_target))?,
        Format::Png => decode::png::decode(buf, stored_target)?,
        Format::Gif => Decoded::plain(decode::gif::decode_first(buf)?),
        // Rendered straight at the target size.
        Format::Svg => Decoded::plain(decode::svg::decode(buf, stored_target)?),
    };
    // Before the resize, so opaque images are resized as RGB.
    image.drop_opaque_alpha();
    let mut image = match stored_target {
        Some((w, h)) if (image.width, image.height) != (w, h) => resize::resize(&image.view(), w, h)?,
        _ => image,
    };
    image = orient::apply(image, orientation);
    if let Some(icc) = icc {
        color::to_srgb(&mut image, &icc);
    }
    // Shrinking can make a nearly opaque image fully opaque.
    image.drop_opaque_alpha();
    Ok(image)
}

/// Encode a still. Takes the image by value so AVIF can free the RGB pixels
/// before its AV1 encode.
fn encode_still(image: Image, opts: &Options) -> Result<Vec<u8>> {
    let effort = opts.effort.unwrap_or(opts.format.default_effort());
    match opts.format {
        OutputFormat::Webp => webp::encode(&image, opts.quality, effort as i32),
        OutputFormat::Avif => avif::encode(
            image,
            avif::Settings { quality: opts.quality.round().clamp(0.0, 100.0) as u32, effort, threads: opts.threads },
        ),
        OutputFormat::Jxl => jxl::encode(&image, opts.quality.clamp(0.0, 100.0), effort, opts.threads),
    }
}

fn convert_animated_gif_jxl(buf: &[u8], info: &Info, opts: &Options) -> Result<Output> {
    let mut frames = GifFrames::new(buf)?;
    let target = opts.max_dimension.and_then(|m| resize::fit_inside(frames.width, frames.height, m));
    let (w, h) = target.unwrap_or((frames.width, frames.height));
    let (cw, ch) = (frames.width, frames.height);
    let (mut canvases, mut delays): (Vec<Vec<u8>>, Vec<u32>) = (Vec::new(), Vec::new());
    // Opaque canvases (nearly every GIF) are kept as RGB: 25% less memory than RGBA.
    let mut opaque = true;
    let mut first = true;
    while let Some(frame) = frames.next_frame(first)? {
        first = false;
        let canvas = ImageView { width: cw, height: ch, channels: 4, data: frame.rgba };
        let resized;
        let rgba: &[u8] = match target {
            Some((tw, th)) => {
                resized = resize::resize(&canvas, tw, th)?.data;
                &resized
            }
            None => frame.rgba,
        };
        if opaque && !rgba.chunks_exact(4).all(|p| p[3] == 255) {
            opaque = false;
            for c in &mut canvases {
                *c = c.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            }
        }
        canvases.push(if opaque {
            rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
        } else {
            rgba.to_vec()
        });
        delays.push(frame.delay_ms / 10);
    }
    let effort = opts.effort.unwrap_or(OutputFormat::Jxl.default_effort());
    let data = jxl::encode_animation(&canvases, opaque, &delays, w, h, opts.quality, effort, opts.threads)?;
    Ok(Output { data, format: OutputFormat::Jxl, width: w, height: h, frames: delays.len() as u32, input: *info })
}

fn convert_animated_gif(buf: &[u8], info: &Info, opts: &Options) -> Result<Output> {
    let mut frames = GifFrames::new(buf)?;
    let target = opts.max_dimension.and_then(|m| resize::fit_inside(frames.width, frames.height, m));
    let (w, h) = target.unwrap_or((frames.width, frames.height));
    let mut enc = webp::AnimEncoder::new(w, h, opts.quality, opts.effort.unwrap_or(4) as i32)?;
    let (cw, ch) = (frames.width, frames.height);
    let mut first = true;
    while let Some(frame) = frames.next_frame(first)? {
        first = false;
        let canvas = ImageView { width: cw, height: ch, channels: 4, data: frame.rgba };
        let delay = frame.delay_ms;
        match target {
            Some((tw, th)) => enc.add(resize::resize(&canvas, tw, th)?.view(), delay)?,
            None => enc.add(canvas, delay)?,
        }
    }
    let data = enc.finish()?;
    // libwebp merges identical consecutive frames, so count what was written.
    let (_, _, count) = decode::webp::probe(&data)?;
    Ok(Output { data, format: OutputFormat::Webp, width: w, height: h, frames: count, input: *info })
}

fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    // libjpeg reports fatal errors by unwinding.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "decoder panicked".into());
            Err(crate::error::Error(msg))
        }
    }
}


/// Convert an encoded image.
pub fn convert(buf: &[u8], opts: &Options) -> Result<Output> {
    guard(|| {
        let info = probe::probe(buf)?;
        check_pixels(info.width, info.height, 1, opts.max_pixels)?;
        if info.max_frame_pixels > opts.max_pixels {
            bail!("Image exceeds pixel limit: {} pixels", info.max_frame_pixels);
        }
        if info.format == Format::Gif && info.frames > 1 && opts.animated {
            check_pixels(info.width, info.height, info.frames, opts.max_pixels)?;
            if opts.format == OutputFormat::Jxl {
                return convert_animated_gif_jxl(buf, &info, opts);
            }
            return convert_animated_gif(buf, &info, opts);
        }
        let image = prepare_still(buf, &info, opts.max_dimension)?;
        let (width, height) = (image.width, image.height);
        let data = encode_still(image, opts)?;
        Ok(Output { data, format: opts.format, width, height, frames: 1, input: info })
    })
}

/// Convert raw 8-bit RGBA pixels (e.g. a decoded ICO entry).
pub fn convert_rgba(rgba: &[u8], width: u32, height: u32, opts: &Options) -> Result<Output> {
    if width == 0 || height == 0 || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        bail!("RGBA buffer length {} does not match {width}x{height}", rgba.len());
    }
    check_pixels(width, height, 1, opts.max_pixels)?;
    guard(|| {
        let view = ImageView { width, height, channels: 4, data: rgba };
        let target = opts.max_dimension.and_then(|m| resize::fit_inside(width, height, m));
        // Opaque input is taken as RGB (the alpha is dropped while copying),
        // so it is resized without alpha. Otherwise the borrowed pixels are
        // resized directly, and copied only when no resize is needed.
        let image = if view.is_opaque() {
            let mut rgb = view.to_rgb();
            if let Some((w, h)) = target {
                rgb = resize::resize(&rgb.view(), w, h)?;
            }
            rgb
        } else {
            match target {
                Some((w, h)) => resize::resize(&view, w, h)?,
                None => Image::new(width, height, 4, rgba.to_vec()),
            }
        };
        let mut image = image;
        image.drop_opaque_alpha();
        let (out_w, out_h) = (image.width, image.height);
        let data = encode_still(image, opts)?;
        let input = Info { format: Format::Png, width, height, frames: 1, orientation: 1, max_frame_pixels: u64::from(width) * u64::from(height) };
        Ok(Output { data, format: opts.format, width: out_w, height: out_h, frames: 1, input })
    })
}

pub struct Versions {
    pub libwebp: String,
    pub avif_mini_encoder: String,
    pub jxl_mini_encoder: String,
}

pub fn versions() -> Versions {
    Versions { libwebp: webp::version(), avif_mini_encoder: avif::version(), jxl_mini_encoder: jxl::version() }
}

/// Decode, resize, orient and color-convert without encoding (for tests and tools).
#[doc(hidden)]
pub fn prepare_for_debug(buf: &[u8], max_dimension: u32) -> Result<Image> {
    let info = probe::probe(buf)?;
    prepare_still(buf, &info, Some(max_dimension).filter(|&m| m > 0))
}
