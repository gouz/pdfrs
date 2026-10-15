// End to end: HTML in, PDF bytes out (same cases as pdfjs's test.ts).
use pdfrs::{html_to_pdf, Options};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::var("FONTS_DIR").unwrap_or("/usr/share/fonts".into());
        assert!(pdfrs::fonts::init(&[dir.into()]) > 0, "no fonts found, set FONTS_DIR");
    });
}

fn pdf(html: &str, o: &Options) -> Vec<u8> {
    init();
    let pdf = html_to_pdf(html, o).expect("render");
    assert!(pdf.starts_with(b"%PDF-"));
    pdf
}

fn pages(pdf: &[u8]) -> usize {
    // `/Type /Page` objects, not the `/Type /Pages` tree root
    let s = String::from_utf8_lossy(pdf).replace("/Type /", "/Type/");
    s.match_indices("/Type/Page").filter(|(i, _)| !s[i + 10..].starts_with('s')).count()
}

#[test]
fn flex_header_footer_metadata() {
    let o = Options {
        size: pdfrs::page_size("A5").unwrap(),
        landscape: true,
        margin: [60.0, 40.0, 40.0, 40.0],
        footer: Some("<p>{page}/{pages}</p>".into()),
        title: Some("T".into()),
        ..Options::default()
    };
    let out = pdf(r#"<div style="display:flex;justify-content:space-between"><p>a</p><p>b</p></div>"#, &o);
    assert!(String::from_utf8_lossy(&out).contains("(T)") || String::from_utf8_lossy(&out).contains("<feff0054>"));
}

#[test]
fn what_crashed_or_hung_pdfjs_renders() {
    // font stacks, `normal` spacings, a min-height taller than the page, unknown fonts
    let html = r#"<style>p { font-family: "Segoe UI", sans-serif; letter-spacing: normal }</style>
      <div style="display:flex;gap:normal;min-height:2000px"><p style="font-family:Foo">a</p></div><p>gap: normal</p>"#;
    let out = pdf(html, &Options { size: (400.0, 300.0), ..Options::default() });
    assert!(pages(&out) >= 2);
}

#[test]
fn long_content_paginates() {
    let rows: String = (0..200).map(|i| format!("<tr><td>row {i}</td><td>{i}</td></tr>")).collect();
    let out = pdf(&format!("<table>{rows}</table><p style=\"page-break-before:always\">end</p>"), &Options::default());
    assert!(pages(&out) >= 4, "got {} pages", pages(&out));
}

#[test]
fn empty_document_has_one_page() {
    assert_eq!(pages(&pdf("", &Options::default())), 1);
}

#[test]
fn hostile_input_does_not_panic() {
    let o = Options::default();
    for html in [
        "<div style='width:-10px;height:1e30px;margin:-1e9px'>x</div>",
        "<table><tr><td colspan=999999 rowspan=0>x</td></tr></table>",
        "<ol start=-5><li>a<li value=abc>b</ol>",
        "<img src='data:image/png;base64,!!!'><img><svg><rect/></svg><svg width=0 height=0></svg>",
        "<p style='font-size:0;line-height:-3'>tiny</p><p style='font-size:10000px'>huge</p>",
        &"<div>".repeat(5000),
        &"<span>".repeat(5000),
        &"<ul><li>".repeat(2000),
        "<pre>\t\ttabs\n\n\nlines</pre><p>&nbsp;&amp;&lt;&#x1F600;</p>",
    ] {
        pdf(html, &o);
    }
}
