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
pub mod template;

use css::{Decl, Media, PageRule};
use krilla::page::PageSettings;
use scraper::Html;
use std::cell::RefCell;
use std::collections::HashMap;

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
    let root = t.root;
    compute(&mut t, root, width, &faces);
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

/// First, left (even) or right (odd) page: which `@page` rules apply. The first page is a right page.
#[derive(Clone, Copy, PartialEq)]
enum PageKind {
    First,
    Left,
    Right,
}

fn page_kind(index: usize) -> PageKind {
    match index {
        0 => PageKind::First,
        i if i % 2 == 1 => PageKind::Left,
        _ => PageKind::Right,
    }
}

/// The `@page` rules of a kind of page with a `page` name, least specific first: `@page`, `@page name`,
/// then `:left` / `:right`, then `:first` (each unnamed, then named).
fn rules_for<'a>(pages: &'a [PageRule], kind: PageKind, name: Option<&str>) -> Vec<&'a PageRule> {
    let level = |r: &PageRule| {
        let named = match (r.name.as_deref(), name) {
            (None, _) => 0,
            (Some(a), Some(b)) if a == b => 1,
            _ => return None,
        };
        let pseudo = match (r.pseudo.as_deref(), kind) {
            (None, _) => 0,
            (Some(":left"), PageKind::Left) | (Some(":right"), PageKind::Right | PageKind::First) => 2,
            (Some(":first"), PageKind::First) => 4,
            _ => return None,
        };
        Some(pseudo + named)
    };
    let mut rules: Vec<(u8, &PageRule)> = pages.iter().filter_map(|r| Some((level(r)?, r))).collect();
    rules.sort_by_key(|r| r.0);
    rules.into_iter().map(|r| r.1).collect()
}

/// Page size (landscape applied), content width, and the margins of each kind of page: the request's
/// options, else the document's `@page` rules, else A4 / 40pt. The column has one width: a page whose
/// side margins add up differently keeps its left margin and the common width.
fn page_setup(o: &Options, pages: &[PageRule], name: Option<&str>) -> ((f32, f32), f32, [[f32; 4]; 3]) {
    fn decls(rules: Vec<&PageRule>) -> Vec<&Decl> {
        rules.into_iter().flat_map(|p| &p.decls).collect()
    }
    let base_rules: Vec<&PageRule> = rules_for(pages, PageKind::Right, name).into_iter().filter(|p| p.pseudo.is_none()).collect();
    let base = page_props(o, &decls(base_rules), PageProps::default());
    // a named page's own size wins over the request's (that is the point of naming it)
    let named_size = name.is_some() && pages.iter().any(|p| p.name.as_deref() == name && p.decls.iter().any(|d| d.prop == "size"));
    let (w, h) = if named_size { base.size } else { o.size.or(base.size) }.unwrap_or_else(|| page_size("A4").unwrap());
    let landscape = if named_size { base.landscape } else { o.landscape.or(base.landscape) };
    let (pw, ph) = if landscape.unwrap_or(false) { (w.max(h), w.min(h)) } else { (w, h) };
    let base_m = o.margin.unwrap_or(base.margin.map(|m| m.unwrap_or(40.0)));
    let cw = (pw - base_m[1] - base_m[3]).max(1.0);
    let margins = [PageKind::First, PageKind::Left, PageKind::Right].map(|kind| {
        let mut m = match o.margin {
            Some(m) => m,
            None => {
                let p = page_props(o, &decls(rules_for(pages, kind, name)), PageProps::default());
                std::array::from_fn(|i| p.margin[i].unwrap_or(base_m[i]))
            }
        };
        m[1] = pw - m[3] - cw;
        m
    });
    ((pw, ph), cw, margins)
}

fn kind_index(kind: PageKind) -> usize {
    match kind {
        PageKind::First => 0,
        PageKind::Left => 1,
        PageKind::Right => 2,
    }
}

/// The `@page` margin boxes of a kind of page as HTML in their rectangle (x, y, w, h):
/// edges (left / center / right side by side), corners and sides. `counter(page)` / `counter(pages)`
/// become `{page}` / `{pages}`. The request's header / footer replace the top / bottom edges.
fn margin_bands(o: &Options, pages: &[PageRule], kind: PageKind, name: Option<&str>, (pw, ph): (f32, f32), m: [f32; 4]) -> Vec<(String, (f32, f32, f32, f32))> {
    let [mt, mr, mb, ml] = m;
    let mut boxes: Vec<(&str, &[Decl])> = Vec::new();
    for rule in rules_for(pages, kind, name) {
        for (name, decls) in &rule.boxes {
            boxes.retain(|b| b.0 != name);
            boxes.push((name, decls));
        }
    }
    let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    // a box's content and style, or None when it has no content
    let cell = |name: &str, align: &str, extra: &str| -> Option<String> {
        let decls = boxes.iter().find(|b| b.0 == name)?.1;
        let content = decls.iter().rev().find(|d| d.prop == "content").and_then(|d| {
            style::content(&d.value, |_| None, |counter, _, _| match counter {
                "page" => "{page}".into(),
                "pages" => "{pages}".into(),
                _ => String::new(),
            })
        })?;
        let css: String = decls.iter().filter(|d| d.prop != "content").map(|d| format!("{}: {};", d.prop, d.value.replace('"', "&quot;"))).collect();
        Some(format!("<div style=\"text-align: {align}; {extra} {css}\">{}</div>", escape(&content)))
    };
    let mut out = Vec::new();
    for (edge, request, y, h) in [("top", &o.header, 0.0, mt), ("bottom", &o.footer, ph - mb, mb)] {
        let html = request.clone().or_else(|| {
            let cells: Vec<Option<String>> = ["left", "center", "right"].iter().map(|slot| cell(&format!("{edge}-{slot}"), slot, "flex: 1;")).collect();
            cells.iter().any(Option::is_some).then(|| {
                let inner: String = cells.into_iter().map(|c| c.unwrap_or_else(|| "<div style=\"flex: 1\"></div>".into())).collect();
                format!("<div style=\"display: flex; align-items: center\">{inner}</div>")
            })
        });
        if let Some(html) = html {
            out.push((html, (ml, y, pw - ml - mr, h)));
        }
        for (side, x, w) in [("left", 0.0, ml), ("right", pw - mr, mr)] {
            if let Some(html) = cell(&format!("{edge}-{side}-corner"), "center", "") {
                out.push((html, (x, y, w, h)));
            }
        }
    }
    let ch = ph - mt - mb;
    for (side, x, w) in [("left", 0.0, ml), ("right", pw - mr, mr)] {
        for (i, part) in ["top", "middle", "bottom"].iter().enumerate() {
            if let Some(html) = cell(&format!("{side}-{part}"), "center", "") {
                out.push((html, (x, mt + ch * i as f32 / 3.0, w, ch / 3.0)));
            }
        }
    }
    out
}

/// Lay a tree (or one of its section roots) out, with the fonts for `::first-line`.
fn compute(tree: &mut layout::Tree, root: taffy::NodeId, width: f32, faces: &[fonts::DocFace]) {
    LCX.with(|lcx| fonts::with_doc_font_cx(faces, |fcx| tree.compute_with_fonts(root, width, fcx, &mut lcx.borrow_mut())));
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
    let ((_, ph), cw, margins) = page_setup(o, &sheet.pages, None);
    let ch = (ph - margins[2][0] - margins[2][2]).max(1.0);
    let media = Media { width: cw, height: ch };
    style::set_viewport(cw, ch);
    let sheet = if (cw - provisional.width).abs() < 0.5 && (ch - provisional.height).abs() < 0.5 { sheet } else { layout::stylesheet(&doc, &media, &o.images) };
    let faces = layout::load_font_faces(&sheet.font_faces, &o.images);

    let mut tree = LCX.with(|lcx| layout::build(&doc, &sheet, &faces, &o.images, &o.font, o.font_size, cw, &mut lcx.borrow_mut()));
    // sections of named pages (`page: wide`), each laid out at the width of its pages
    struct Section {
        root: taffy::NodeId,
        name: Option<std::rc::Rc<str>>,
        size: (f32, f32),
        margins: [[f32; 4]; 3],
    }
    let mut sections = Vec::new();
    for (root, name) in tree.sections() {
        let (size, scw, margins) = page_setup(o, &sheet.pages, name.as_deref());
        tree.set_width(root, scw);
        compute(&mut tree, root, scw, &faces);
        sections.push(Section { root, name, size, margins });
    }
    // the root's background goes to the whole page (the canvas), its borders stay on the root box
    let mut canvas = None;
    for sec in &sections {
        if let Some(d) = tree.deco.get_mut(&sec.root) {
            let c = layout::Deco { background: d.background.take(), layers: std::mem::take(&mut d.layers), color_clip: d.color_clip, ..Default::default() };
            if canvas.is_none() && (c.background.is_some() || !c.layers.is_empty()) {
                canvas = Some(c);
            }
        }
    }
    // pages of every section, numbered across sections (left / right / first by global index)
    let mut pages: Vec<(usize, render::Page)> = Vec::new();
    for (si, sec) in sections.iter().enumerate() {
        let offset = pages.len();
        let breaks = render::collect_breaks(&tree, sec.root);
        let height_of = |i: usize| {
            let m = sec.margins[kind_index(page_kind(offset + i))];
            (sec.size.1 - m[0] - m[2]).max(1.0)
        };
        pages.extend(render::paginate(height_of, &breaks).into_iter().map(|p| (si, p)));
    }
    // header / footer (the request's, else the @page margin boxes) per section and kind of page
    let mut bands: HashMap<(usize, usize), Vec<(String, (f32, f32, f32, f32))>> = HashMap::new();

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
    for (i, (si, pg)) in pages.iter().enumerate() {
        let sec = &sections[*si];
        let (start, end) = (pg.start, pg.end);
        let kind = page_kind(i);
        let k = kind_index(kind);
        let [mt, _, _, ml] = sec.margins[k];
        let (pw, ph) = sec.size;
        let page_bands = bands.entry((*si, k)).or_insert_with(|| margin_bands(o, &sheet.pages, kind, sec.name.as_deref(), (pw, ph), sec.margins[k]));
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
                render::draw(&mut s, &tree, sec.root, 0.0, 0.0, (start, end), &mut found);
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
            for (html, (bx, by, bw, bh)) in page_bands.iter() {
                let t = band(html, i + 1, n, o, *bw, &media, &faces);
                let th = t.taffy.layout(t.root).map(|l| l.size.height).unwrap_or(0.0);
                let y = by + (bh - th) / 2.0;
                let mut found = Vec::new();
                render::draw(&mut s, &t, t.root, *bx, y, (f32::NEG_INFINITY, f32::INFINITY), &mut found);
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
