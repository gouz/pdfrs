//! HTML/CSS -> PDF without a browser.
//!
//! html5ever parses, a small cascade computes styles, taffy lays out (block, flexbox, grid),
//! parley shapes and breaks text, krilla writes the PDF (subset fonts, selectable text).

pub mod css;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod render;
pub mod style;

use css::{Decl, Media};
use krilla::page::PageSettings;
use scraper::Html;
use std::cell::RefCell;

pub use images::ImagePolicy;

#[derive(Clone, Debug)]
pub struct Options {
    /// page size in pt (portrait). None: the document's `@page { size }`, else A4
    pub size: Option<(f32, f32)>,
    /// None: the document's `@page { size: landscape }`, else portrait
    pub landscape: Option<bool>,
    /// top, right, bottom, left in pt. None: the document's `@page { margin }`, else 40
    pub margin: Option<[f32; 4]>,
    /// default family: Helvetica, Times-Roman, Courier, a CSS generic, or a family of the fonts directory
    pub font: String,
    pub font_size: f32,
    /// HTML repeated in the top margin; {page} and {pages} are replaced
    pub header: Option<String>,
    /// HTML repeated in the bottom margin; {page} and {pages} are replaced
    pub footer: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub images: ImagePolicy,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            size: None,
            landscape: None,
            margin: None,
            font: "Helvetica".into(),
            font_size: 12.0,
            header: None,
            footer: None,
            title: None,
            author: None,
            subject: None,
            keywords: None,
            images: ImagePolicy::default(),
        }
    }
}

/// `A4`, `letter`... or `WIDTHxHEIGHT` in pt
pub fn page_size(name: &str) -> Option<(f32, f32)> {
    let n = name.trim().to_ascii_uppercase();
    Some(match n.as_str() {
        "A3" => (841.89, 1190.55),
        "A4" => (595.28, 841.89),
        "A5" => (419.53, 595.28),
        "A6" => (297.64, 419.53),
        "B5" => (498.9, 708.66),
        "LETTER" => (612.0, 792.0),
        "LEGAL" => (612.0, 1008.0),
        "TABLOID" => (792.0, 1224.0),
        _ => {
            let (w, h) = n.split_once('X')?;
            let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
            if w > 0.0 && h > 0.0 && w < 15_000.0 && h < 15_000.0 { (w, h) } else { return None }
        }
    })
}

thread_local! {
    static LCX: RefCell<parley::LayoutContext<layout::Brush>> = RefCell::new(parley::LayoutContext::new());
}

/// A laid out header / footer for one page. It can use the document's @font-face fonts.
fn band(html: &str, page: usize, pages: usize, o: &Options, width: f32, media: &Media, faces: &[fonts::DocFace]) -> layout::Tree {
    let html = html.replace("{page}", &page.to_string()).replace("{pages}", &pages.to_string());
    let doc = Html::parse_document(&html);
    let sheet = layout::stylesheet(&doc, media);
    let mut faces = faces.to_vec();
    faces.extend(layout::load_font_faces(&sheet.font_faces, &o.images));
    let mut t = LCX.with(|lcx| layout::build(&doc, &sheet, &faces, &o.images, &o.font, o.font_size, width, &mut lcx.borrow_mut()));
    t.compute(width);
    t
}

/// Page size (landscape applied) and margins: the request's options, else the document's `@page`, else A4 / 40pt.
fn page_setup(o: &Options, pages: &[Vec<Decl>]) -> ((f32, f32), [f32; 4]) {
    let (mut size, mut landscape, mut margin) = (None, None, [None; 4]);
    let px = |v: &str| style::len(v, o.font_size, o.font_size).and_then(style::Len::px);
    for d in pages.iter().flatten() {
        let v = d.value.to_ascii_lowercase();
        match d.prop.as_str() {
            "size" => {
                let dims: Vec<f32> = v.split_whitespace().filter_map(px).collect();
                match dims[..] {
                    [a] => size = Some((a, a)),
                    [a, b] => size = Some((a, b)),
                    _ => {}
                }
                for w in v.split_whitespace() {
                    match w {
                        "landscape" => landscape = Some(true),
                        "portrait" => landscape = Some(false),
                        w => {
                            if let Some(s) = page_size(w).filter(|_| !w.contains(|c: char| c.is_ascii_digit()) || w.len() <= 3) {
                                size = Some(s);
                            }
                        }
                    }
                }
            }
            "margin" => {
                let m: Vec<f32> = v.split_whitespace().filter_map(px).collect();
                let m = match m[..] {
                    [a] => [a; 4],
                    [a, b] => [a, b, a, b],
                    [a, b, c] => [a, b, c, b],
                    [a, b, c, d] => [a, b, c, d],
                    _ => continue,
                };
                margin = m.map(Some);
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                let i = ["margin-top", "margin-right", "margin-bottom", "margin-left"].iter().position(|p| *p == d.prop).unwrap();
                margin[i] = px(&v).or(margin[i]);
            }
            _ => {}
        }
    }
    let (w, h) = o.size.or(size).unwrap_or_else(|| page_size("A4").unwrap());
    let page = if o.landscape.or(landscape).unwrap_or(false) { (w.max(h), w.min(h)) } else { (w, h) };
    (page, o.margin.unwrap_or(margin.map(|m| m.unwrap_or(40.0))))
}

/// Forgets the document's @font-face fonts, however the render ends.
struct DocGuard;
impl Drop for DocGuard {
    fn drop(&mut self) {
        render::end_document();
    }
}

/// Render `html` to PDF bytes.
pub fn html_to_pdf(html: &str, o: &Options) -> Result<Vec<u8>, String> {
    let _guard = DocGuard;
    let doc = Html::parse_document(html);
    // @page first (it can sit in @media print), then the sheets for the real page area
    let provisional = Media { width: 515.28, height: 761.89 };
    let sheet = layout::stylesheet(&doc, &provisional);
    let ((pw, ph), [mt, mr, mb, ml]) = page_setup(o, &sheet.pages);
    let (cw, ch) = ((pw - ml - mr).max(1.0), (ph - mt - mb).max(1.0));
    let media = Media { width: cw, height: ch };
    let sheet = if (cw - provisional.width).abs() < 0.5 && (ch - provisional.height).abs() < 0.5 { sheet } else { layout::stylesheet(&doc, &media) };
    let faces = layout::load_font_faces(&sheet.font_faces, &o.images);

    let mut tree = LCX.with(|lcx| layout::build(&doc, &sheet, &faces, &o.images, &o.font, o.font_size, cw, &mut lcx.borrow_mut()));
    tree.compute(cw);
    let breaks = render::collect_breaks(&tree);
    let pages = render::paginate(ch, &breaks);

    let mut doc = krilla::Document::new();
    let mut meta = krilla::metadata::Metadata::new().creator("pdfrs".into()).producer("pdfrs".into());
    let title = o.title.clone().or_else(|| html_title(html));
    if let Some(t) = title {
        meta = meta.title(t);
    }
    if let Some(a) = &o.author {
        meta = meta.authors(vec![a.clone()]);
    }
    if let Some(s) = &o.subject {
        meta = meta.description(s.clone());
    }
    if let Some(k) = &o.keywords {
        meta = meta.keywords(k.split(',').map(|k| k.trim().to_string()).collect());
    }
    doc.set_metadata(meta);

    let n = pages.len();
    for (i, pg) in pages.iter().enumerate() {
        let (start, end) = (pg.start, pg.end);
        let mut page = doc.start_page_with(PageSettings::from_wh(pw, ph).ok_or("invalid page size")?);
        // (x, y, w, h, href) in page coordinates
        let mut links: Vec<(f32, f32, f32, f32, String)> = Vec::new();
        {
            let mut s = page.surface();
            // a table continued from the previous page: its <thead> again on top
            let hh = pg.header_height();
            if let (Some(h), Some(clip)) = (pg.header, render_clip(pw, mt, hh)) {
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&krilla::geom::Transform::from_translate(ml, mt - h.head.0));
                render::draw(&mut s, &tree, h.table, h.origin.0, h.origin.1, h.head, &mut Vec::new());
                s.pop();
                s.pop();
            }
            // content area: clipped vertically to the slice of the column this page shows
            let top = mt + hh;
            if let Some(clip) = render_clip(pw, top, end - start) {
                let mut found = Vec::new();
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&krilla::geom::Transform::from_translate(ml, top - start));
                render::draw(&mut s, &tree, tree.root, 0.0, 0.0, (start, end), &mut found);
                s.pop();
                s.pop();
                links.extend(found.into_iter().filter_map(|l| {
                    let (x, y, w, h) = (l.rect.0 + ml, l.rect.1 + top - start, l.rect.2, l.rect.3);
                    // only links visible in this page's content area
                    let href = tree.links.get(l.href as usize - 1)?.clone();
                    (y + h > top && y < top + (end - start)).then_some((x, y, w, h, href))
                }));
            }
            // header / footer, vertically centered in their margin
            for (html, top, height) in [(&o.header, 0.0, mt), (&o.footer, ph - mb, mb)] {
                let Some(html) = html else { continue };
                let t = band(html, i + 1, n, o, cw, &media, &faces);
                let th = t.taffy.layout(t.root).map(|l| l.size.height).unwrap_or(0.0);
                let y = top + (height - th) / 2.0;
                let mut found = Vec::new();
                render::draw(&mut s, &t, t.root, ml, y, (f32::NEG_INFINITY, f32::INFINITY), &mut found);
                links.extend(found.into_iter().filter_map(|l| Some((l.rect.0, l.rect.1, l.rect.2, l.rect.3, t.links.get(l.href as usize - 1)?.clone()))));
            }
            s.finish();
        }
        for (x, y, w, h, href) in links {
            if let Some(r) = krilla::geom::Rect::from_xywh(x, y, w.max(0.1), h.max(0.1)) {
                let target = krilla::annotation::Target::Action(krilla::action::Action::Link(krilla::action::LinkAction::new(href)));
                page.add_annotation(krilla::annotation::LinkAnnotation::new(r, target).into());
            }
        }
        page.finish();
    }
    doc.finish().map_err(|e| format!("{e:?}"))
}

fn render_clip(pw: f32, top: f32, h: f32) -> Option<krilla::geom::Path> {
    let mut pb = krilla::geom::PathBuilder::new();
    pb.push_rect(krilla::geom::Rect::from_xywh(0.0, top, pw, h.max(0.01))?);
    pb.finish()
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open = start + lower[start..].find('>')? + 1;
    let close = open + lower[open..].find("</title")?;
    let t = html[open..close].trim();
    (!t.is_empty()).then(|| t.to_string())
}
