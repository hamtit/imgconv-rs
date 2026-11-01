//! Debug helper: run decode + resize + orient and write the pixels as PNG.
//!   dump INPUT MAX OUTPUT.png
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let buf = std::fs::read(&a[0]).unwrap();
    let img = imgconv_rs::convert::prepare_for_debug(&buf, a[1].parse().unwrap()).unwrap();
    let f = std::fs::File::create(&a[2]).unwrap();
    let mut e = png::Encoder::new(f, img.width, img.height);
    e.set_color(if img.channels == 4 { png::ColorType::Rgba } else { png::ColorType::Rgb });
    e.write_header().unwrap().write_image_data(&img.data).unwrap();
}
