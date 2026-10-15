//! Render the benchmark documents in a loop: time per document, no HTTP.
//! cargo run --release --example bench [iterations]
use pdfrs::{html_to_pdf, ImagePolicy, Options};
use std::time::Instant;

fn main() {
    pdfrs::fonts::init(&[std::env::var("FONTS_DIR").unwrap_or("/usr/share/fonts".into()).into()]);
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(2000);
    let o = Options { images: ImagePolicy { base_dir: Some("demo".into()), remote: None }, ..Options::default() };
    let docs: Vec<(String, String)> = ["example", "flex", "images"].iter().map(|d| (d.to_string(), std::fs::read_to_string(format!("demo/{d}.html")).unwrap())).collect();
    for (name, html) in &docs {
        for _ in 0..50 {
            html_to_pdf(html, &o).unwrap();
        }
        let t = Instant::now();
        for _ in 0..n {
            html_to_pdf(html, &o).unwrap();
        }
        println!("{name}: {:.0} µs", t.elapsed().as_micros() as f64 / n as f64);
    }
}
