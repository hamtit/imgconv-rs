use std::fmt;

/// Conversion error.
///
/// The variant says which stage failed, so callers can react to it (for
/// example, treat [`Error::UnsupportedFormat`] and [`Error::PixelLimit`] as the
/// caller's problem and [`Error::Internal`] as ours). The `Display` text is
/// meant for logs, not for end users.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input is not in a supported image format.
    UnsupportedFormat,
    /// The input is malformed, truncated or has unusable dimensions (found by our own checks).
    InvalidImage(String),
    /// The image has more pixels than `Options::max_pixels` allows.
    PixelLimit { pixels: u64 },
    /// A raw RGBA buffer does not hold `width * height * 4` bytes.
    BufferSize { actual: usize, width: u32, height: u32 },
    /// A decoder library rejected the input.
    Decode(String),
    /// Resizing failed.
    Resize(String),
    /// An encoder failed.
    Encode(String),
    /// A bug or a broken environment: a panic inside a codec, or a library version mismatch.
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedFormat => f.write_str("Unsupported image format"),
            Error::PixelLimit { pixels } => write!(f, "Image exceeds pixel limit: {pixels} pixels"),
            Error::BufferSize { actual, width, height } => {
                write!(f, "RGBA buffer length {actual} does not match {width}x{height}")
            }
            Error::InvalidImage(msg)
            | Error::Decode(msg)
            | Error::Resize(msg)
            | Error::Encode(msg)
            | Error::Internal(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Decode(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, OutputFormat, convert, convert_rgba, probe};

    /// The JS tests and callers match on these texts, so they must not drift.
    #[test]
    fn display_texts_are_stable() {
        assert_eq!(Error::UnsupportedFormat.to_string(), "Unsupported image format");
        assert_eq!(Error::PixelLimit { pixels: 2048 }.to_string(), "Image exceeds pixel limit: 2048 pixels");
        assert_eq!(
            Error::BufferSize { actual: 3, width: 64, height: 32 }.to_string(),
            "RGBA buffer length 3 does not match 64x32"
        );
        assert_eq!(Error::InvalidImage("Invalid PNG header".into()).to_string(), "Invalid PNG header");
    }

    #[test]
    fn stages_report_their_variant() {
        assert_eq!(probe(b"<html></html>").unwrap_err(), Error::UnsupportedFormat);
        assert!(matches!(probe(b"\x89PNG\r\n\x1a\n").unwrap_err(), Error::InvalidImage(_)));

        let opts = Options::new(OutputFormat::Webp, 80.0);
        let rgba = vec![0u8; 64 * 32 * 4];
        assert_eq!(
            convert_rgba(&rgba[1..], 64, 32, &opts).unwrap_err(),
            Error::BufferSize { actual: rgba.len() - 1, width: 64, height: 32 }
        );
        let small = Options { max_pixels: 1000, ..opts };
        assert_eq!(convert_rgba(&rgba, 64, 32, &small).unwrap_err(), Error::PixelLimit { pixels: 2048 });
        assert_eq!(convert(b"not an image", &small).unwrap_err(), Error::UnsupportedFormat);
    }
}
