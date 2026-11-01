#![allow(non_upper_case_globals)]
//! Benchmark helper: SSIMULACRA2 score of candidates against a reference.
//!   ssim2 REFERENCE.png CANDIDATE.(png|jxl) [CANDIDATE...]
//! Prints one line per candidate: "<score> <path>", or "NaN <path> <reason>".
//! Alpha is composited over mid-gray before scoring.

use std::ptr;

use imgconv_rs::jxl::sys::*;
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
    let bytes = std::fs::read(path).unwrap();
    unsafe {
        let dec = JxlDecoderCreate(ptr::null());
        assert_eq!(
            JxlDecoderSubscribeEvents(dec, (JxlDecoderStatus_JXL_DEC_BASIC_INFO | JxlDecoderStatus_JXL_DEC_FULL_IMAGE) as i32),
            JxlDecoderStatus_JXL_DEC_SUCCESS
        );
        JxlDecoderSetInput(dec, bytes.as_ptr(), bytes.len());
        JxlDecoderCloseInput(dec);
        let format = JxlPixelFormat { num_channels: 4, data_type: JxlDataType_JXL_TYPE_UINT8, endianness: JxlEndianness_JXL_NATIVE_ENDIAN, align: 0 };
        let mut info = std::mem::MaybeUninit::<JxlBasicInfo>::zeroed().assume_init();
        let mut out = Vec::new();
        loop {
            match JxlDecoderProcessInput(dec) {
                JxlDecoderStatus_JXL_DEC_BASIC_INFO => {
                    JxlDecoderGetBasicInfo(dec, &mut info);
                }
                JxlDecoderStatus_JXL_DEC_NEED_IMAGE_OUT_BUFFER => {
                    let mut size = 0;
                    JxlDecoderImageOutBufferSize(dec, &format, &mut size);
                    out.resize(size, 0);
                    JxlDecoderSetImageOutBuffer(dec, &format, out.as_mut_ptr().cast(), size);
                }
                JxlDecoderStatus_JXL_DEC_FULL_IMAGE => {}
                JxlDecoderStatus_JXL_DEC_SUCCESS => break,
                s => panic!("JXL decode failed: status {s}"),
            }
        }
        JxlDecoderDestroy(dec);
        Rgba { w: info.xsize, h: info.ysize, data: out }
    }
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
