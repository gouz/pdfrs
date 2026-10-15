//! HTML/CSS -> PDF without a browser.
//!
//! html5ever parses, a small cascade computes styles, taffy lays out (block, flexbox, grid),
//! parley shapes and breaks text, krilla writes the PDF (subset fonts, selectable text).

pub mod css;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod remote;
pub mod render;
pub mod style;

use css::{Decl, Media, PageRule};
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
    let sheet = layout::stylesheet(&doc, media, &o.images);
    let mut faces = faces.to_vec();
    faces.extend(layout::load_font_faces(&sheet.font_faces, &o.images));
    let mut t = LCX.with(|lcx| layout::build(&doc, &sheet, &faces, &o.images, &o.font, o.font_size, width, &mut lcx.borrow_mut()));
    t.compute(width);
    t
}

/// What `@page` rules set: size, orientation, margins (each None when not set).
#[derive(Default, Clone, Copy)]
struct PageProps {
    size: Option<(f32, f32)>,
    landscape: Option<bool>,
    margin: [Option<f32>; 4],
}

fn page_props(o: &Options, decls: &[&Decl], mut p: PageProps) -> PageProps {
    let px = |v: &str| style::len(v, o.font_size, o.font_size).and_then(style::Len::px);
    for d in decls {
        let v = d.value.to_ascii_lowercase();
        match d.prop.as_str() {
            "size" => {
                let dims: Vec<f32> = v.split_whitespace().filter_map(px).collect();
                match dims[..] {
                    [a] => p.size = Some((a, a)),
                    [a, b] => p.size = Some((a, b)),
                    _ => {}
                }
                for w in v.split_whitespace() {
                    match w {
                        "landscape" => p.landscape = Some(true),
                        "portrait" => p.landscape = Some(false),
                        w => {
                            if let Some(s) = page_size(w).filter(|_| !w.contains(|c: char| c.is_ascii_digit()) || w.len() <= 3) {
                                p.size = Some(s);
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
                p.margin = m.map(Some);
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                let i = ["margin-top", "margin-right", "margin-bottom", "margin-left"].iter().position(|m| *m == d.prop).unwrap();
                p.margin[i] = px(&v).or(p.margin[i]);
            }
            _ => {}
        }
    }
    p
}

/// Page size (landscape applied), margins, and the first page's top / bottom margins: the request's
/// options, else the document's `@page` (and `@page :first`), else A4 / 40pt.
/// The first page keeps the others' size and side margins (the column has one width).
fn page_setup(o: &Options, pages: &[PageRule]) -> ((f32, f32), [f32; 4], (f32, f32)) {
    let decls = |first: bool| pages.iter().filter(|p| p.first == first).flat_map(|p| &p.decls).collect::<Vec<_>>();
    let base = page_props(o, &decls(false), PageProps::default());
    let first = page_props(o, &decls(true), base);
    let (w, h) = o.size.or(base.size).unwrap_or_else(|| page_size("A4").unwrap());
    let page = if o.landscape.or(base.landscape).unwrap_or(false) { (w.max(h), w.min(h)) } else { (w, h) };
    let margin = o.margin.unwrap_or(base.margin.map(|m| m.unwrap_or(40.0)));
    let first_margin = match o.margin {
        Some(m) => (m[0], m[2]),
        None => (first.margin[0].unwrap_or(margin[0]), first.margin[2].unwrap_or(margin[2])),
    };
    (page, margin, first_margin)
}

/// The `@page` margin boxes of an edge (`top` / `bottom`) as header / footer HTML: left, center
/// and right boxes side by side, `counter(page)` / `counter(pages)` as `{page}` / `{pages}`.
fn margin_band(pages: &[PageRule], first: bool, edge: &str) -> Option<String> {
    // later rules win, `:first` rules over the others on the first page
    let mut boxes: Vec<(&str, &[Decl])> = Vec::new();
    for rule in pages.iter().filter(|p| !p.first).chain(pages.iter().filter(|p| first && p.first)) {
        for (name, decls) in &rule.boxes {
            boxes.retain(|b| b.0 != name);
            boxes.push((name, decls));
        }
    }
    let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut cells = String::new();
    let mut any = false;
    for slot in ["left", "center", "right"] {
        let name = format!("{edge}-{slot}");
        let decls = boxes.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or(&[]);
        let content = decls.iter().rev().find(|d| d.prop == "content").and_then(|d| {
            style::content(&d.value, |_| None, |counter, _, _| match counter {
                "page" => "{page}".into(),
                "pages" => "{pages}".into(),
                _ => String::new(),
            })
        });
        let css: String = decls.iter().filter(|d| d.prop != "content").map(|d| format!("{}: {};", d.prop, d.value.replace('"', "&quot;"))).collect();
        any |= content.is_some();
        cells.push_str(&format!("<div style=\"flex: 1; text-align: {slot}; {css}\">{}</div>", escape(&content.unwrap_or_default())));
    }
    any.then(|| format!("<div style=\"display: flex; align-items: center\">{cells}</div>"))
}

/// Forgets the document's @font-face fonts, however the render ends.
struct DocGuard;
impl Drop for DocGuard {
    fn drop(&mut self) {
        render::end_document();
        remote::end_document();
    }
}

/// Render `html` to PDF bytes.
pub fn html_to_pdf(html: &str, o: &Options) -> Result<Vec<u8>, String> {
    let _guard = DocGuard;
    let doc = Html::parse_document(html);
    if let Some(remote) = &o.images.remote {
        remote::prefetch(layout::remote_images(&doc), remote);
    }
    // @page first (it can sit in @media print), then the sheets for the real page area
    let provisional = Media { width: 515.28, height: 761.89 };
    let sheet = layout::stylesheet(&doc, &provisional, &o.images);
    let ((pw, ph), [mt, mr, mb, ml], (first_mt, first_mb)) = page_setup(o, &sheet.pages);
    let (cw, ch) = ((pw - ml - mr).max(1.0), (ph - mt - mb).max(1.0));
    let media = Media { width: cw, height: ch };
    let sheet = if (cw - provisional.width).abs() < 0.5 && (ch - provisional.height).abs() < 0.5 { sheet } else { layout::stylesheet(&doc, &media, &o.images) };
    let faces = layout::load_font_faces(&sheet.font_faces, &o.images);

    let mut tree = LCX.with(|lcx| layout::build(&doc, &sheet, &faces, &o.images, &o.font, o.font_size, cw, &mut lcx.borrow_mut()));
    tree.compute(cw);
    // the root's background goes to the whole page (the canvas), its borders stay on the root box
    let canvas = tree.deco.get_mut(&tree.root).and_then(|d| {
        let c = layout::Deco { background: d.background.take(), layers: std::mem::take(&mut d.layers), color_clip: d.color_clip, ..Default::default() };
        (c.background.is_some() || !c.layers.is_empty()).then_some(c)
    });
    let breaks = render::collect_breaks(&tree);
    let pages = render::paginate((ph - first_mt - first_mb).max(1.0), ch, &breaks);
    // header / footer: the request's, else the @page margin boxes
    let bands = |first: bool| {
        (
            o.header.clone().or_else(|| margin_band(&sheet.pages, first, "top")),
            o.footer.clone().or_else(|| margin_band(&sheet.pages, first, "bottom")),
        )
    };
    let (first_bands, bands) = (bands(true), bands(false));

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
        let (mt, mb) = if i == 0 { (first_mt, first_mb) } else { (mt, mb) };
        let (header, footer) = if i == 0 { &first_bands } else { &bands };
        let mut page = doc.start_page_with(PageSettings::from_wh(pw, ph).ok_or("invalid page size")?);
        // (x, y, w, h, href) in page coordinates
        let mut links: Vec<(f32, f32, f32, f32, String)> = Vec::new();
        {
            let mut s = page.surface();
            if let Some(c) = &canvas {
                render::draw_canvas(&mut s, c, pw, ph);
            }
            // a table continued from the previous page: its <thead> again on top
            let hh = pg.header_height();
            if let (Some(h), Some(head), Some(clip)) = (pg.header, pg.header.and_then(|h| h.head), render_clip(pw, mt, hh)) {
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&krilla::geom::Transform::from_translate(ml, mt - head.0));
                render::draw(&mut s, &tree, h.table, h.origin.0, h.origin.1, head, &mut Vec::new());
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
            // a table broken by this page: its <tfoot> again right after the last row
            let fh = pg.footer_height();
            let foot_top = top + (end - start);
            if let (Some(f), Some(foot), Some(clip)) = (pg.footer, pg.footer.and_then(|f| f.foot), render_clip(pw, foot_top, fh)) {
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&krilla::geom::Transform::from_translate(ml, foot_top - foot.0));
                render::draw(&mut s, &tree, f.table, f.origin.0, f.origin.1, foot, &mut Vec::new());
                s.pop();
                s.pop();
            }
            // header / footer, vertically centered in their margin
            for (html, top, height) in [(header, 0.0, mt), (footer, ph - mb, mb)] {
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
