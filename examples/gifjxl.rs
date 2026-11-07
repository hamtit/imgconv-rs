//! Bench helper: GIF -> JPEG XL through the same path as `convert`.
//!   gifjxl INPUT.gif QUALITY EFFORT THREADS [OUT.jxl]
use imgconv_rs::convert::{Options, OutputFormat, convert};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let buf = std::fs::read(&a[0]).unwrap();
    let mut o = Options::new(OutputFormat::Jxl, a[1].parse().unwrap());
    o.effort = Some(a[2].parse().unwrap());
    o.threads = a[3].parse().unwrap();
    let t = std::time::Instant::now();
    let out = convert(&buf, &o).unwrap();
    println!("{} frames {} B {:.2} s", out.frames, out.data.len(), t.elapsed().as_secs_f64());
    if let Some(p) = a.get(4) {
        std::fs::write(p, &out.data).unwrap();
    }
}
