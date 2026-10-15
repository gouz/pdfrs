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
        &"<span style='display:inline-block;padding:1px'>x".repeat(2000),
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

/// words of the PDF with their box: (text, x_min, y_min, x_max)
fn words(pdf: &[u8]) -> Vec<(String, f32, f32, f32)> {
    let dir = std::env::temp_dir().join(format!("pdfrs-bbox-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::write(&dir, pdf).unwrap();
    let out = std::process::Command::new("pdftotext").arg("-bbox").arg(&dir).arg("-").output().unwrap();
    let _ = std::fs::remove_file(&dir);
    let attr = |l: &str, a: &str| l.split(&format!("{a}=\"")).nth(1).and_then(|r| r.split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("<word"))
        .map(|l| {
            let text = l.split('>').nth(1).unwrap_or("").split('<').next().unwrap_or("").to_string();
            (text, attr(l, "xMin"), attr(l, "yMin"), attr(l, "xMax"))
        })
        .collect()
}

fn line_of(w: &[(String, f32, f32, f32)], word: &str) -> f32 {
    w.iter().find(|x| x.0 == word).unwrap_or_else(|| panic!("{word} not found in {w:?}")).2
}

#[test]
fn inline_blocks_and_images_stay_in_the_line() {
    if !has_pdftotext() {
        return;
    }
    let img = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let html = format!(
        r#"<p>avant <span style="display:inline-block;padding:2px 6px;background:#eee">badge</span> milieu <img src="{img}" width="10" height="10"> apres</p>
           <div style="display:flex"><span style="display:inline-block">item</span><img src="{img}" width="5"></div>"#
    );
    let w = words(&pdf(&html, &Options::default()));
    let y = line_of(&w, "avant");
    for word in ["badge", "milieu", "apres"] {
        assert!((line_of(&w, word) - y).abs() < 4.0, "{word} left the line: {w:?}");
    }
    let (badge, milieu) = (w.iter().find(|x| x.0 == "badge").unwrap(), w.iter().find(|x| x.0 == "milieu").unwrap());
    assert!(badge.3 + 6.0 <= milieu.1 + 0.5, "the badge's padding pushes the next word");
}

#[test]
fn wide_inline_block_shrinks_to_the_line() {
    if !has_pdftotext() {
        return;
    }
    let long = "mot ".repeat(200);
    let html = format!(r#"<p>a <span style="display:inline-block;padding:4px;border:1px solid">{long}</span> b</p>"#);
    let w = words(&pdf(&html, &Options::default()));
    let right = w.iter().map(|x| x.3).fold(0.0, f32::max);
    assert!(right <= 595.28 - 40.0 + 0.5, "text overflows the page: {right}");
    assert!(w.iter().filter(|x| x.0 == "mot").count() == 200);
}

#[test]
fn thead_repeats_on_every_page_of_the_table() {
    if !has_pdftotext() {
        return;
    }
    let rows: String = (0..120).map(|i| format!("<tr><td>r{i}</td><td>{i}</td></tr>")).collect();
    let html = format!("<p>avant</p><table><thead><tr><th>Entete</th><th>Qte</th></tr></thead><tbody>{rows}</tbody></table><p>apres</p>");
    let out = pdf(&html, &Options::default());
    let n = pages(&out);
    assert!(n >= 3);
    let text = text_of(&out);
    assert_eq!(text.matches("Entete").count(), n, "one header per page:\n{text}");
    // every row once, none hidden under the header
    let words: Vec<&str> = text.split_whitespace().collect();
    for i in 0..120 {
        let row = format!("r{i}");
        assert_eq!(words.iter().filter(|w| **w == row).count(), 1, "row {row}");
    }
}

#[test]
fn shadows_and_background_images_render() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let html = format!(
        r#"<div style="box-shadow: 0 4px 12px rgba(0,0,0,.3), 2px 2px 0 red, inset 0 0 4px blue; border-radius: 6px; height: 40px">a</div>
           <div style="background: url({png}) repeat; background-size: 4px; height: 3000px; border-radius: 10px">tall, tiled</div>
           <div style="background: url({png}) center / cover no-repeat; width: 50%; height: 60px"></div>
           <div style="background-image: url(https://x.test/a.png); height: 10px"></div>
           <div style="box-shadow: 0 0 400px 50px black; width: 5000px; height: 5000px"></div>"#
    );
    let t = std::time::Instant::now();
    let out = pdf(&html, &Options::default());
    assert!(pages(&out) >= 5);
    assert!(t.elapsed().as_secs_f32() < 10.0, "too slow: {:?}", t.elapsed());
}

#[test]
fn counters_markers_and_first_letter() {
    if !has_pdftotext() {
        return;
    }
    let html = r#"<style>
        body { counter-reset: ch }
        h2 { counter-increment: ch } h2::before { content: counter(ch, upper-roman) ". " }
        ol.n { list-style: none; counter-reset: item } ol.n li { counter-increment: item }
        ol.n li::before { content: counters(item, ".") ") " }
        ul li::marker { content: "-> " }
        p::first-letter { color: red; font-size: 2em }
        tr { counter-increment: row } td.num::before { content: counter(row) }
      </style>
      <h2>Alpha</h2><h2>Beta</h2>
      <ol class="n"><li>a<ol class="n"><li>b</li><li>c</li></ol></li><li>d</li></ol>
      <ol start="5"><li>five</li><li>six</li></ol><ul><li>dash</li></ul>
      <p>Lettrine intacte</p>
      <table><tr><td class="num"></td><td>x</td></tr><tr><td class="num"></td><td>y</td></tr></table>"#;
    let text = text_of(&pdf(html, &Options::default()));
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for want in ["I. Alpha", "II. Beta", "1) a", "1.1) b", "1.2) c", "2) d", "5. five", "6. six", "-> dash", "Lettrine intacte"] {
        assert!(flat.contains(want), "{want:?} not in {flat:?}");
    }
    // row counters (narrow cells: pdftotext glues the columns)
    assert!(flat.replace(' ', "").contains("1x2y"), "{flat:?}");
}

#[test]
fn page_margin_boxes_and_first_page() {
    if !has_pdftotext() {
        return;
    }
    let body = "<p>ligne</p>".repeat(150);
    let html = format!(
        r#"<style>
        @page {{ margin: 50px 40px; @top-right {{ content: "Entete" }} @bottom-center {{ content: "Page " counter(page) " / " counter(pages) }} }}
        @page :first {{ margin-top: 120px; @top-right {{ content: none }} }}
      </style>{body}"#
    );
    let out = pdf(&html, &Options::default());
    let n = pages(&out);
    assert!(n >= 2);
    let text = text_of(&out);
    let per_page: Vec<&str> = text.split('\x0c').filter(|p| !p.trim().is_empty()).collect();
    assert_eq!(per_page.len(), n);
    for (i, p) in per_page.iter().enumerate() {
        assert!(p.contains(&format!("Page {} / {n}", i + 1)), "page {}: {p}", i + 1);
        assert_eq!(p.contains("Entete"), i > 0, "header on page {}", i + 1);
    }
    // the request's footer wins over the margin boxes
    let out = pdf(&html, &Options { footer: Some("<p>pied</p>".into()), ..Options::default() });
    assert!(!text_of(&out).contains("Page 1 /"));
}

#[test]
fn tfoot_repeats_where_the_table_breaks() {
    if !has_pdftotext() {
        return;
    }
    let rows: String = (0..120).map(|i| format!("<tr><td>r{i}</td></tr>")).collect();
    let html = format!("<table><thead><tr><th>Entete</th></tr></thead><tfoot><tr><td>Pied</td></tr></tfoot><tbody>{rows}</tbody></table>");
    let out = pdf(&html, &Options::default());
    let n = pages(&out);
    assert!(n >= 3);
    let text = text_of(&out);
    assert_eq!(text.matches("Pied").count(), n, "one footer per page");
    assert_eq!(text.matches("Entete").count(), n, "one header per page");
}

#[test]
fn link_stylesheet_comes_from_the_image_directory_only() {
    if !has_pdftotext() {
        return;
    }
    let root = std::env::temp_dir().join(format!("pdfrs-link-{}", std::process::id()));
    std::fs::create_dir_all(root.join("site")).unwrap();
    std::fs::write(root.join("site/style.css"), "p::after { content: ' [styled]' }").unwrap();
    std::fs::write(root.join("secret.css"), "p::after { content: ' [secret]' }").unwrap();
    let images = pdfrs::ImagePolicy { base_dir: Some(root.join("site")), remote: None };
    let o = Options { images, ..Options::default() };
    let text = text_of(&pdf(r#"<link rel="stylesheet" href="style.css"><link rel="stylesheet" href="../secret.css"><p>x</p>"#, &o));
    assert!(text.contains("[styled]") && !text.contains("[secret]"), "{text}");
    // no image directory: nothing loads
    let text = text_of(&pdf(r#"<link rel="stylesheet" href="style.css"><p>x</p>"#, &Options::default()));
    assert!(!text.contains("[styled]"));
    std::fs::remove_dir_all(root).unwrap();
}

/// A minimal WOFF 1.0 encoder (zlib per table), to test decoding without a font tool.
fn to_woff1(sfnt: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let u16_at = |o: usize| u16::from_be_bytes([sfnt[o], sfnt[o + 1]]);
    let u32_at = |o: usize| u32::from_be_bytes([sfnt[o], sfnt[o + 1], sfnt[o + 2], sfnt[o + 3]]);
    let n = u16_at(4) as usize;
    let mut tables = Vec::new();
    for i in 0..n {
        let e = 12 + 16 * i;
        let (tag, checksum, offset, len) = (u32_at(e), u32_at(e + 4), u32_at(e + 8) as usize, u32_at(e + 12) as usize);
        let raw = &sfnt[offset..offset + len];
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(raw).unwrap();
        let comp = z.finish().unwrap();
        tables.push((tag, checksum, if comp.len() < len { comp } else { raw.to_vec() }, len));
    }
    let mut out = vec![0u8; 44 + 20 * n];
    let mut offset = out.len();
    let mut dir = Vec::new();
    for (tag, checksum, data, orig) in &tables {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        offset = out.len();
        out.extend_from_slice(data);
        dir.push((*tag, offset as u32, data.len() as u32, *orig as u32, *checksum));
    }
    let _ = offset;
    while out.len() % 4 != 0 {
        out.push(0);
    }
    let total_sfnt: u32 = 12 + 16 * n as u32 + tables.iter().map(|t| (t.3 as u32 + 3) & !3).sum::<u32>();
    let len = out.len() as u32;
    let mut h = Vec::new();
    h.extend_from_slice(b"wOFF");
    h.extend_from_slice(&u32_at(0).to_be_bytes());
    h.extend_from_slice(&len.to_be_bytes());
    h.extend_from_slice(&(n as u16).to_be_bytes());
    h.extend_from_slice(&0u16.to_be_bytes());
    h.extend_from_slice(&total_sfnt.to_be_bytes());
    h.extend_from_slice(&[0, 1, 0, 0]); // version 1.0
    h.extend_from_slice(&[0u8; 20]); // no metadata, no private data
    for (tag, off, comp, orig, checksum) in dir {
        for v in [tag, off, comp, orig, checksum] {
            h.extend_from_slice(&v.to_be_bytes());
        }
    }
    out[..h.len()].copy_from_slice(&h);
    out
}

#[test]
fn woff_font_face() {
    use base64::Engine;
    let dir = std::env::var("FONTS_DIR").unwrap_or("/usr/share/fonts".into());
    let Some(ttf) = ["liberation/LiberationSerif-Regular.ttf", "truetype/liberation/LiberationSerif-Regular.ttf"]
        .iter()
        .find_map(|f| std::fs::read(std::path::Path::new(&dir).join(f)).ok())
    else {
        return;
    };
    let woff = to_woff1(&ttf);
    assert_eq!(&woff[..4], b"wOFF");
    assert!(pdfrs::fonts::to_sfnt(woff.clone()).is_some(), "WOFF decodes");
    let b64 = base64::engine::general_purpose::STANDARD.encode(&woff);
    let html = format!("<style>@font-face {{ font-family: W; src: url(data:font/woff;base64,{b64}) format('woff') }} p {{ font-family: W }}</style><p>x</p>");
    assert!(String::from_utf8_lossy(&pdf(&html, &Options::default())).contains("LiberationSerif"));
}

#[test]
fn lot4_decorations_do_not_panic() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let html = format!(
        r#"<div style="background: url({png}) 0 0 / 5px repeat content-box, linear-gradient(red, blue) padding-box, url({png}) center / 50% auto no-repeat, #eee; background-clip: content-box, border-box; padding: 10px; border: 4px dotted; height: 50px"></div>
           <div style="box-shadow: inset 0 0 10px 3px rgba(0,0,0,.5), inset 3px 3px 0 red, 0 2px 4px #000; border-radius: 20px; height: 40px; border: 2px solid"></div>
           <div style="box-shadow: inset 0 0 0 1000px red; width: 10px; height: 10px"></div>
           <p>a <span style="vertical-align: -3px">b</span> <span style="vertical-align: 50%">c</span> <img src="{png}" width="5" style="vertical-align: 2em"></p>"#
    );
    pdf(&html, &Options::default());
}

/// A tiny HTTP server on 127.0.0.1: path -> (status, extra header, body). Returns its base URL.
fn serve(routes: Vec<(&'static str, u16, &'static str, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            let (status, header, body) = routes.iter().find(|r| r.0 == path).map(|r| (r.1, r.2, r.3.clone())).unwrap_or((404, "", Vec::new()));
            let mut s = stream;
            let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n{header}\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    base
}

#[test]
fn remote_resources_when_enabled() {
    if !has_pdftotext() {
        return;
    }
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==")
        .unwrap();
    let base = serve(vec![
        ("/a.png", 200, "", png.clone()),
        ("/moved.png", 302, "Location: /a.png\r\n", Vec::new()),
        ("/style.css", 200, "", b"@import url(more.css); p::after { content: ' [remote css]' }".to_vec()),
        ("/more.css", 200, "", b"p::before { content: '[imported] ' }".to_vec()),
    ]);
    let html = format!(r#"<link rel="stylesheet" href="{base}/style.css"><p>x</p><img src="{base}/a.png" width="20"><img src="{base}/moved.png" width="20"><img src="{base}/missing.png">"#);
    let allow = |allow_private| pdfrs::ImagePolicy {
        base_dir: None,
        remote: Some(std::sync::Arc::new(pdfrs::remote::RemoteConfig { allow_private, ..Default::default() })),
    };
    let out = pdf(&html, &Options { images: allow(true), ..Options::default() });
    let text = text_of(&out);
    assert!(text.contains("[imported] x [remote css]"), "{text}");
    assert!(String::from_utf8_lossy(&out).matches("/Subtype/Image").count() >= 1, "images embedded");
    // default guards: 127.0.0.1 is private, refused
    let text = text_of(&pdf(&html, &Options { images: allow(false), ..Options::default() }));
    assert!(!text.contains("[remote css]"));
    // remote resources off: refused too
    let text = text_of(&pdf(&html, &Options::default()));
    assert!(!text.contains("[remote css]"));
}

#[test]
fn gradient_text_and_fixed_backgrounds() {
    if !has_pdftotext() {
        return;
    }
    let lines = "<p>ligne</p>".repeat(120);
    let html = format!(
        r#"<style>
          h1 {{ background: linear-gradient(90deg, red, blue); -webkit-background-clip: text; -webkit-text-fill-color: transparent }}
          body {{ background: #fafafa linear-gradient(red, blue) fixed no-repeat; background-size: 100px 50px }}
        </style><h1>Titre <em>dégradé</em></h1>{lines}"#
    );
    let out = pdf(&html, &Options::default());
    assert!(pages(&out) >= 2);
    // the text stays real text, painted with a shading
    assert!(text_of(&out).contains("Titre dégradé"));
    assert!(String::from_utf8_lossy(&out).contains("/Shading"));
}

#[test]
fn left_right_pages_and_all_margin_boxes() {
    if !has_pdftotext() {
        return;
    }
    let body = "<p>ligne</p>".repeat(150);
    let html = format!(
        r#"<style>
        @page {{ margin: 50px 40px; @top-left-corner {{ content: "TL" }} @right-middle {{ content: "RM" }} @bottom-right-corner {{ content: counter(page) }} }}
        @page :left {{ margin-left: 60px; margin-right: 20px; @top-center {{ content: "Gauche" }} }}
        @page :right {{ margin-left: 20px; margin-right: 60px; @top-center {{ content: "Droite" }} }}
      </style>{body}"#
    );
    let out = pdf(&html, &Options::default());
    let n = pages(&out);
    assert!(n >= 3);
    let per_page: Vec<String> = text_of(&out).split('\x0c').map(String::from).collect();
    for i in 0..n {
        let p = &per_page[i];
        // page 1 is a right page, then left, right...
        assert!(p.contains(if i % 2 == 1 { "Gauche" } else { "Droite" }), "page {}: {p}", i + 1);
        assert!(p.contains("TL") && p.contains("RM"), "page {}: {p}", i + 1);
    }
    // mirrored margins: the column moves, its width does not
    let w = words(&out);
    let xs: Vec<f32> = w.iter().filter(|x| x.0 == "ligne").map(|x| x.1).collect();
    let (min, max) = xs.iter().fold((f32::MAX, f32::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    assert!((min - 20.0).abs() < 1.0 && (max - 60.0).abs() < 1.0, "x from {min} to {max}");
}
