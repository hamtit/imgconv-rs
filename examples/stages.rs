//! Per-stage timing on synthetic images: cargo run --release --no-default-features --example stages
use imgconv_rs::{color, convert, encode, image::Image, orient, resize, Options, OutputFormat};
use std::time::Instant;

fn synth(w: u32, h: u32, c: u8) -> Image {
    let mut s = 12345u32;
    let mut d = Vec::with_capacity((w * h * c as u32) as usize);
    for y in 0..h {
        for x in 0..w {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = (s >> 28) as u32;
            d.push(((x * 255 / w) + n) as u8);
            d.push(((y * 255 / h) + n) as u8);
            d.push((((x ^ y) & 255) + n) as u8);
            if c == 4 { d.push(if (x / 64 + y / 64) % 3 == 0 { 100 } else { 255 }); }
        }
    }
    Image::new(w, h, c, d)
}

fn t<R>(name: &str, n: u32, mut f: impl FnMut() -> R) {
    f();
    let t0 = Instant::now();
    for _ in 0..n { std::hint::black_box(f()); }
    println!("  {name:<28}{:>9.2} ms", t0.elapsed().as_secs_f64() * 1000.0 / n as f64);
}

fn main() {
    for (w, h, max) in [(4000u32, 2667u32, 320u32), (1920, 1280, 1920)] {
        for ch in [3u8, 4] {
            let big = synth(w, h, ch);
            println!("== {w}x{h} ch={ch} -> {}", if max == 1920 { "no resize".into() } else { max.to_string() });
            let (tw, th) = resize::fit_inside(w, h, max).unwrap_or((w, h));
            t("resize lanczos3", 5, || resize::resize(&big, tw, th).unwrap());
            let small = resize::resize(&big, tw, th).unwrap();
            for o in [3u8, 6] {
                t(&format!("orient {o}"), 10, || orient::apply(small.clone(), o));
            }
            t("clone (baseline)", 10, || small.clone());
            // Display P3-ish profile via cicp for a non-sRGB transform.
            let icc = moxcms::ColorProfile::new_display_p3().encode().unwrap();
            t("to_srgb (P3 icc)", 5, || { let mut i = small.clone(); color::to_srgb(&mut i, &icc); i });
            let gicc = moxcms::ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
            t("to_srgb (gray icc)", 5, || { let mut i = small.clone(); color::to_srgb(&mut i, &gicc); i });
            t("drop_opaque_alpha", 10, || { let mut i = small.clone(); i.drop_opaque_alpha(); i });
            let mut o = Options::new(OutputFormat::Avif, 65.0);
            for (fmt, eff) in [(OutputFormat::Webp, 4), (OutputFormat::Avif, 4), (OutputFormat::Avif, 8), (OutputFormat::Jxl, 7)] {
                o.format = fmt; o.effort = Some(eff);
                t(&format!("encode {} e{eff}", fmt.name()), 3, || convert::convert_rgba(&small.to_rgba(), small.width, small.height, &o).unwrap());
            }
        }
    }
    let _ = encode::avif::version;
}
