//! Image conversion for web delivery. See README.md.

pub mod aom;
pub mod color;
pub mod convert;
pub mod decode;
pub mod encode;
pub mod error;
pub mod image;
pub mod jxl;
pub mod memory;
pub mod orient;
pub mod probe;
pub mod resize;
pub mod stream;

#[cfg(feature = "napi")]
mod node;

#[cfg(target_os = "emscripten")]
pub mod wasm;

pub use convert::{Options, Output, OutputFormat, convert, convert_rgba};
pub use error::{Error, Result};
pub use probe::{Format, Info, probe};
