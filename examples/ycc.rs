use std::time::Instant;
use imgconv_rs::image::Image;
// verbatim copy of encode::avif::rgb_to_ycbcr (private)
fn rgb_to_ycbcr(img: &Image, subsampled: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
    let (w, h, c) = (img.width as usize, img.height as usize, img.channels as usize);
    let mut y = vec![0u8; w * h];
    let (cw, ch) = if subsampled { (w.div_ceil(2), h.div_ceil(2)) } else { (w, h) };
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    let px = |x: usize, yy: usize| { let o = (yy * w + x) * c; (f32::from(img.data[o]), f32::from(img.data[o + 1]), f32::from(img.data[o + 2])) };
    const KR: f32 = 0.299; const KB: f32 = 0.114; const KG: f32 = 1.0 - KR - KB;
    for yy in 0..h { for x in 0..w { let (r, g, b) = px(x, yy); y[yy * w + x] = (KR * r + KG * g + KB * b).round().clamp(0.0, 255.0) as u8; } }
    for cy in 0..ch { for cx in 0..cw {
        let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
        let (x0, y0) = if subsampled { (cx * 2, cy * 2) } else { (cx, cy) };
        let span = if subsampled { 2 } else { 1 };
        for yy in y0..(y0 + span).min(h) { for x in x0..(x0 + span).min(w) { let p = px(x, yy); r += p.0; g += p.1; b += p.2; n += 1.0; } }
        let (r, g, b) = (r / n, g / n, b / n);
        let luma = KR * r + KG * g + KB * b;
        u[cy * cw + cx] = ((b - luma) / (2.0 * (1.0 - KB)) + 128.0).round().clamp(0.0, 255.0) as u8;
        v[cy * cw + cx] = ((r - luma) / (2.0 * (1.0 - KR)) + 128.0).round().clamp(0.0, 255.0) as u8;
    } }
    (y, u, v, cw)
}
fn main() {
    for (w, h) in [(320usize, 213usize), (1920, 1280)] {
        let img = Image::new(w as u32, h as u32, 3, (0..w*h*3).map(|i| (i*7) as u8).collect());
        for sub in [true, false] {
            rgb_to_ycbcr(&img, sub);
            let t = Instant::now();
            for _ in 0..20 { std::hint::black_box(rgb_to_ycbcr(&img, sub)); }
            println!("{w}x{h} sub={sub}: {:.2} ms", t.elapsed().as_secs_f64()*1000.0/20.0);
        }
    }
}
