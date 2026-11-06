//! Runs the napi-rs setup (nothing else: every codec is Rust or a -sys crate).

use std::env;

fn main() {
    if env::var_os("CARGO_FEATURE_NAPI").is_some() {
        napi_build::setup();
    }
}
