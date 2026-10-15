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

use krilla::page::PageSettings;
use std::cell::RefCell;

pub use images::ImagePolicy;

#[derive(Clone, Debug)]
pub struct Options {
    /// page size in pt (portrait)
    pub size: (f32, f32),
    pub landscape: bool,
    /// top, right, bottom, left in pt
    pub margin: [f32; 4],
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
            size: page_size("A4").unwrap(),
            landscape: false,
            margin: [40.0; 4],
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

/// A laid out header / footer for one page.
fn band(html: &str, page: usize, pages: usize, o: &Options, width: f32) -> layout::Tree {
    let html = html.replace("{page}", &page.to_string()).replace("{pages}", &pages.to_string());
    let mut t = LCX.with(|lcx| layout::build(&html, &o.images, &o.font, o.font_size, width, &mut lcx.borrow_mut()));
    t.compute(width);
    t
}

/// Render `html` to PDF bytes.
pub fn html_to_pdf(html: &str, o: &Options) -> Result<Vec<u8>, String> {
    let (pw, ph) = if o.landscape { (o.size.1, o.size.0) } else { o.size };
    let [mt, mr, mb, ml] = o.margin;
    let (cw, ch) = ((pw - ml - mr).max(1.0), (ph - mt - mb).max(1.0));

    let mut tree = LCX.with(|lcx| layout::build(html, &o.images, &o.font, o.font_size, cw, &mut lcx.borrow_mut()));
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
    for (i, &(start, end)) in pages.iter().enumerate() {
        let mut page = doc.start_page_with(PageSettings::from_wh(pw, ph).ok_or("invalid page size")?);
        // (x, y, w, h, href) in page coordinates
        let mut links: Vec<(f32, f32, f32, f32, String)> = Vec::new();
        {
            let mut s = page.surface();
            // content area: clipped vertically to the slice of the column this page shows
            if let Some(clip) = render_clip(pw, mt, end - start) {
                let mut found = Vec::new();
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&krilla::geom::Transform::from_translate(ml, mt - start));
                render::draw(&mut s, &tree, tree.root, 0.0, 0.0, (start, end), &mut found);
                s.pop();
                s.pop();
                links.extend(found.into_iter().filter_map(|l| {
                    let (x, y, w, h) = (l.rect.0 + ml, l.rect.1 + mt - start, l.rect.2, l.rect.3);
                    // only links visible in this page's content area
                    let href = tree.links.get(l.href as usize - 1)?.clone();
                    (y + h > mt && y < mt + (end - start)).then_some((x, y, w, h, href))
                }));
            }
            // header / footer, vertically centered in their margin
            for (html, top, height) in [(&o.header, 0.0, mt), (&o.footer, ph - mb, mb)] {
                let Some(html) = html else { continue };
                let t = band(html, i + 1, n, o, cw);
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
