//! Benchmark helper: SSIMULACRA2 score of candidates against a reference.
//!   ssim2 REFERENCE.png CANDIDATE.(png|jxl) [CANDIDATE...]
//! Prints one line per candidate: "<score> <path>", or "NaN <path> <reason>".
//! Alpha is composited over mid-gray before scoring.

use ssimulacra2::{ColorPrimaries, Rgb, TransferCharacteristic, compute_frame_ssimulacra2};

struct Rgba {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

fn read_png(path: &str) -> Rgba {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let data = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        t => panic!("unexpected PNG color type {t:?}"),
    };
    Rgba { w: info.width, h: info.height, data }
}

fn read_jxl(path: &str) -> Rgba {
    let image = jxl_oxide::JxlImage::builder().open(path).unwrap();
    let render = image.render_frame(0).unwrap();
    let fb = render.image_all_channels();
    let ch = fb.channels();
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let data = fb
        .buf()
        .chunks(ch)
        .flat_map(|p| match ch {
            1 => [to8(p[0]), to8(p[0]), to8(p[0]), 255],
            2 => [to8(p[0]), to8(p[0]), to8(p[0]), to8(p[1])],
            3 => [to8(p[0]), to8(p[1]), to8(p[2]), 255],
            _ => [to8(p[0]), to8(p[1]), to8(p[2]), to8(p[3])],
        })
        .collect();
    Rgba { w: fb.width() as u32, h: fb.height() as u32, data }
}

fn to_rgb(img: &Rgba) -> Rgb {
    let px = img
        .data
        .chunks(4)
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            let c = |v: u8| (f32::from(v) / 255.0) * a + 0.5 * (1.0 - a);
            [c(p[0]), c(p[1]), c(p[2])]
        })
        .collect();
    Rgb::new(px, img.w as usize, img.h as usize, TransferCharacteristic::SRGB, ColorPrimaries::BT709).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reference = read_png(&args[0]);
    for cand in &args[1..] {
        let img = if cand.ends_with(".jxl") { read_jxl(cand) } else { read_png(cand) };
        if (img.w, img.h) != (reference.w, reference.h) {
            println!("NaN {cand} size {}x{} vs {}x{}", img.w, img.h, reference.w, reference.h);
            continue;
        }
        match compute_frame_ssimulacra2(to_rgb(&reference), to_rgb(&img)) {
            Ok(s) => println!("{s:.3} {cand}"),
            Err(e) => println!("NaN {cand} {e:?}"),
        }
    }
}
