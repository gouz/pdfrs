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
        size: pdfrs::page_size("A5"),
        landscape: Some(true),
        margin: Some([60.0, 40.0, 40.0, 40.0]),
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
    let out = pdf(html, &Options { size: Some((400.0, 300.0)), ..Options::default() });
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

fn text_of(pdf: &[u8]) -> String {
    // the PDF's text, through pdftotext when available (the dev container and CI have poppler)
    let dir = std::env::temp_dir().join(format!("pdfrs-text-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::write(&dir, pdf).unwrap();
    let out = std::process::Command::new("pdftotext").arg(&dir).arg("-").output();
    let _ = std::fs::remove_file(&dir);
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn has_pdftotext() -> bool {
    std::process::Command::new("pdftotext").arg("-v").output().is_ok()
}

#[test]
fn page_rule_sets_size_and_margins_unless_the_request_does() {
    let html = "<style>@page { size: A5 landscape; margin: 10mm } @media print { p { color: red } }</style><p>x</p>";
    let out = String::from_utf8_lossy(&pdf(html, &Options::default())).into_owned();
    assert!(out.contains("MediaBox[0 0 595.28 419.53]"), "A5 landscape expected");
    let out = String::from_utf8_lossy(&pdf(html, &Options { size: pdfrs::page_size("A4"), landscape: Some(false), ..Options::default() })).into_owned();
    assert!(out.contains("MediaBox[0 0 595.28 841.89]"), "the request's A4 portrait wins");
}

#[test]
fn pseudo_elements_and_variables_render_text() {
    if !has_pdftotext() {
        return;
    }
    let html = r#"<style>
        :root { --accent: #c00; --label: "Total"; }
        .price::before { content: var(--label) " : "; color: var(--accent) }
        .price::after { content: " €" }
        a[href]::after { content: " (" attr(href) ")" }
      </style>
      <p class="price">42</p><p><a href="https://x.test">lien</a> <q>cité</q></p>"#;
    let text = text_of(&pdf(html, &Options::default()));
    assert!(text.contains("Total : 42 €"), "{text}");
    assert!(text.contains("lien (https://x.test)"), "{text}");
    assert!(text.contains("“cité”"), "{text}");
}

#[test]
fn calc_widths_use_the_containing_block() {
    if !has_pdftotext() {
        return;
    }
    // two boxes of calc(50% - 10px) + 20px gap fit on one line; without the fix-up they would be 0 wide
    let html = r#"<div style="display:flex;gap:20px;width:400px"><div style="width:calc(50% - 10px);flex-shrink:0">aaa bbb ccc ddd</div><div style="width:calc(50% - 10px);flex-shrink:0">eee</div></div>"#;
    let text = text_of(&pdf(html, &Options::default()));
    assert!(text.lines().next().unwrap_or("").contains("aaa bbb ccc ddd"), "{text}");
}

#[test]
fn font_face_is_scoped_to_its_document() {
    use base64::Engine;
    let dir = std::env::var("FONTS_DIR").unwrap_or("/usr/share/fonts".into());
    let Some(ttf) = ["liberation/LiberationSerif-Regular.ttf", "truetype/liberation/LiberationSerif-Regular.ttf"]
        .iter()
        .find_map(|f| std::fs::read(std::path::Path::new(&dir).join(f)).ok())
    else {
        return;
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&ttf);
    let html = format!("<style>@font-face {{ font-family: 'Maison'; src: url(data:font/ttf;base64,{b64}) format('truetype') }} p {{ font-family: Maison }}</style><p>x</p>");
    let out = pdf(&html, &Options::default());
    assert!(String::from_utf8_lossy(&out).contains("LiberationSerif"), "the @font-face font is embedded");
    // the next document on this thread does not see it
    assert!(pdfrs::fonts::resolve_stack("Maison").is_none());
    let out = pdf("<p style='font-family: Maison'>x</p>", &Options::default());
    assert!(!String::from_utf8_lossy(&out).contains("LiberationSerif"));
}

#[test]
fn lot1_features_do_not_panic() {
    let html = r#"<style>
      .g { background: linear-gradient(135deg, #f00, #00f 70%); height: 40px; border-radius: 8px }
      .r { background-image: radial-gradient(circle at 30% 40%, white, black); height: 40px }
      .rep { background: repeating-linear-gradient(90deg, #eee 0 10px, #ccc 10px 20px); height: 20px }
      .t { transform: rotate(-5deg) scale(1.1); transform-origin: left top; width: 100px; background: #def }
      .x::before { content: ""; display: block; height: 4px; background: red }
      @media (max-width: 100px) { body { color: red } }
      </style>
      <div class="g"></div><div class="r"></div><div class="rep"></div><div class="t">tourné</div><div class="x">x</div>
      <div dir="rtl"><p>שלום עולם and English</p><ul><li>عربي</li></ul></div>"#;
    pdf(html, &Options::default());
}
