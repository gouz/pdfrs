//! DOM + computed styles -> taffy box tree (block, flex, grid; tables as grids) with parley text leaves.

use crate::css::{self, Decl, Media, Pseudo, Rule, Sheet};
use crate::fonts;
use crate::images::{self, ImagePolicy};
use crate::style::{self, Background, BgSize, BgSource, BorderStyle, BoxArea, Display, Gradient, Len, ObjectFit, Rgba, Shadow, Style, TextAlign, Transform, VAlign, WhiteSpace};
use ego_tree::NodeRef;
use parley::{FontContext, LayoutContext, StyleProperty, TextStyle};
use scraper::{ElementRef, Html, Node};
use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use taffy::prelude::*;

/// Default (user-agent) styles, close to browsers'.
const UA_CSS: &str = r#"
html, body, div, p, h1, h2, h3, h4, h5, h6, ul, ol, dl, dt, dd, section, article, aside, header, footer,
main, nav, form, blockquote, pre, figure, figcaption, address, hr, fieldset, legend, details, summary,
center, hgroup, menu, dir, noscript { display: block }
head, style, script, title, meta, link, template, base, datalist, param, source, track, area, map { display: none }
li { display: list-item }
table { display: table; border-spacing: 2px }
thead, tbody, tfoot { display: table-row-group }
tr { display: table-row }
td, th { display: table-cell; padding: 1px; vertical-align: middle }
caption { display: table-caption; text-align: center }
th { font-weight: bold; text-align: center }
p { margin: 1em 0 }
h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
h4 { margin: 1.33em 0; font-weight: bold }
h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: bold }
h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: bold }
ul, ol, menu, dir { margin: 1em 0; padding-left: 40px }
ol { list-style-type: decimal }
ul ul, ol ul { list-style-type: circle }
ul ul ul, ol ul ul, ul ol ul { list-style-type: square }
li ul, li ol { margin: 0 }
dl { margin: 1em 0 }
dd { margin-left: 40px }
blockquote, figure { margin: 1em 40px }
pre { white-space: pre; margin: 1em 0; font-family: monospace }
code, kbd, samp, tt { font-family: monospace }
b, strong { font-weight: bold }
i, em, cite, var, dfn, address { font-style: italic }
u, ins { text-decoration: underline }
s, strike, del { text-decoration: line-through }
small, sub, sup { font-size: 0.83em }
sub { vertical-align: sub }
sup { vertical-align: super }
big { font-size: 1.17em }
mark { background-color: yellow }
a { color: #0000ee; text-decoration: underline }
hr { border-top: 1px solid #888; margin: 0.5em 0 }
center { text-align: center }
img { object-fit: contain }
[dir=rtl i] { direction: rtl }
[dir=ltr i] { direction: ltr }
q::before { content: open-quote }
q::after { content: close-quote }
"#;

static UA: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let mut sheet = Sheet::default();
    css::parse_stylesheet(UA_CSS, &Media { width: 0.0, height: 0.0 }, &mut sheet);
    sheet.rules
});

/// Text paint and the inline box it belongs to (link index, background).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brush {
    pub color: Rgba,
    pub background: Option<Rgba>,
    /// 1-based index in Tree::links, 0 = not a link
    pub link: u32,
    /// 1-based index in Tree::text_fills: the element whose `background-clip: text` paints this text
    pub text_fill: u32,
}

pub struct TextLeaf {
    pub layout: parley::Layout<Brush>,
    pub text: String,
    pub align: parley::Alignment,
    /// `::first-line` of the block: the pieces and container to rebuild the text from, the declarations
    first_line: Option<FirstLine>,
}

struct FirstLine {
    pieces: Vec<Piece>,
    container: Rc<Style>,
    decls: Rc<Vec<Decl>>,
    rem: f32,
}

#[derive(Clone)]
pub enum ImageKind {
    Raster(krilla::image::Image),
    Svg(Arc<usvg::Tree>),
}

pub struct ImageLeaf {
    pub kind: ImageKind,
    pub natural: (f32, f32),
    pub fit: ObjectFit,
    pub radius: [f32; 4],
}

pub enum Leaf {
    Text(TextLeaf),
    Image(ImageLeaf),
}

/// What a background layer paints.
pub enum LayerPaint {
    Gradient(Rc<Gradient>),
    /// decoded image, natural size
    Image(ImageKind, (f32, f32)),
}

/// A background layer, ready to draw.
pub struct BgLayer {
    pub paint: LayerPaint,
    pub size: BgSize,
    pub position: (Len, Len),
    pub repeat: (bool, bool),
    pub origin: BoxArea,
    pub clip: BoxArea,
    pub fixed: bool,
}

impl std::fmt::Debug for BgLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BgLayer({:?}, {:?}, {:?})", self.size, self.origin, self.clip)
    }
}

/// What is drawn for an element box besides its children.
#[derive(Clone, Debug, Default)]
pub struct Deco {
    pub background: Option<Rgba>,
    pub border_width: [f32; 4],
    pub border_style: [Option<BorderStyle>; 4],
    pub border_color: [Rgba; 4],
    pub radius: [f32; 4],
    pub opacity: f32,
    pub clip: bool,
    pub break_before: bool,
    pub break_after: bool,
    pub avoid_break_inside: bool,
    /// background image layers, first on top
    pub layers: Vec<Rc<BgLayer>>,
    /// the background color's clip: the last layer's background-clip
    pub color_clip: BoxArea,
    pub shadows: Option<Rc<Vec<Shadow>>>,
    /// `transform`, `transform-origin`, font size (for em units)
    pub transform: Option<(Rc<str>, Option<Rc<str>>, f32)>,
}

impl Deco {
    /// anything to draw or to take into account when paginating
    fn needed(&self) -> bool {
        self.background.is_some()
            || !self.layers.is_empty()
            || self.shadows.is_some()
            || self.border_width.iter().any(|w| *w > 0.0)
            || self.opacity < 1.0
            || self.clip
            || self.transform.is_some()
            || self.break_before
            || self.break_after
            || self.avoid_break_inside
    }
}

/// A style length given as `calc(px + pct%)`, fixed up once the containing block is known.
#[derive(Clone, Copy, Debug)]
enum CalcField {
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    Margin(usize),
    Padding(usize),
    Inset(usize),
    FlexBasis,
}

pub struct Tree {
    pub taffy: TaffyTree<Leaf>,
    pub root: NodeId,
    pub deco: HashMap<NodeId, Deco>,
    pub links: Vec<String>,
    calc: Vec<(NodeId, Vec<(CalcField, f32, f32)>)>,
    /// (inline box root, text leaf holding it), inner boxes first
    inline: Vec<(NodeId, NodeId)>,
    /// table -> its <thead> cells, repeated at the top of the pages the table continues on
    pub headers: HashMap<NodeId, Vec<NodeId>>,
    /// table -> its <tfoot> cells, repeated at the bottom of the pages the table breaks on
    pub footers: HashMap<NodeId, Vec<NodeId>>,
    /// table cells: not split across pages when they fit on one
    pub cells: std::collections::HashSet<NodeId>,
    /// elements with `background-clip: text` (Brush::text_fill - 1)
    pub text_fills: Vec<NodeId>,
    /// top-level boxes with a `page` name
    top_pages: HashMap<NodeId, Rc<str>>,
}

#[derive(Clone)]
enum Piece {
    Text(String, Rc<Style>, Brush),
    Break(Rc<Style>, Brush),
    /// an inline-block or an inline image: a box laid out on its own, placed in the line
    Box(NodeId, Rc<Style>),
}

#[derive(Default)]
struct Ctx {
    children: Vec<NodeId>,
    group: Vec<Piece>,
    /// the element whose box this is: its direct children of a flex / grid box are blockified
    owner: Option<ego_tree::NodeId>,
}

struct Builder<'a> {
    sheet: &'a Sheet,
    rem: f32,
    policy: &'a ImagePolicy,
    taffy: TaffyTree<Leaf>,
    deco: HashMap<NodeId, Deco>,
    links: Vec<String>,
    fcx: &'a mut FontContext,
    lcx: &'a mut LayoutContext<Brush>,
    /// CSS counters: name -> nested instances (innermost last)
    counters: HashMap<String, Vec<i32>>,
    /// nesting of boxes and of walk() calls: bounded, so hostile HTML cannot overflow the stack
    box_depth: usize,
    walk_depth: usize,
    calc: Vec<(NodeId, Vec<(CalcField, f32, f32)>)>,
    inline: Vec<(NodeId, NodeId)>,
    headers: HashMap<NodeId, Vec<NodeId>>,
    footers: HashMap<NodeId, Vec<NodeId>>,
    cells: std::collections::HashSet<NodeId>,
    /// `::first-letter` style waiting for the first letter of the current block
    first_letter: Option<Rc<Style>>,
    /// `::first-line` declarations waiting for the first text of the current block
    first_line: Option<Rc<Vec<Decl>>>,
    /// `background-clip: text` elements (their node, once built), and the one painting the current text
    text_fills: Vec<Option<NodeId>>,
    text_fill: u32,
    top_pages: HashMap<NodeId, Rc<str>>,
    /// decoded background images by url (the same image is often repeated on many boxes)
    bg_images: HashMap<Rc<str>, Option<(ImageKind, (f32, f32))>>,
}

/// Deeper boxes are flattened into their ancestor; deeper inline content is dropped.
const MAX_BOX_DEPTH: usize = 100;
const MAX_WALK_DEPTH: usize = 400;

/// The <style> sheets of a document, for a given medium.
/// `<link rel="stylesheet" href>` are read from the image directory only (never from the network).
pub fn stylesheet(doc: &Html, media: &Media, policy: &ImagePolicy) -> Sheet {
    let mut sheet = Sheet::default();
    let sel = scraper::Selector::parse("style, link").unwrap();
    for s in doc.select(&sel) {
        if !s.value().attr("media").is_none_or(|m| css::media_matches(m, media)) {
            continue;
        }
        if s.value().name() == "style" {
            css::parse_stylesheet(&with_imports(&s.text().collect::<String>(), None, policy, 0), media, &mut sheet);
        } else if s.value().attr("rel").is_some_and(|r| r.split_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))) {
            let href = s.value().attr("href").unwrap_or("");
            if let Some(css) = load_css(href, policy) {
                css::parse_stylesheet(&with_imports(&css, Some(href), policy, 0), media, &mut sheet);
            }
        }
    }
    sheet
}

/// A stylesheet file: from the image directory, or http(s) when remote resources are enabled.
fn load_css(href: &str, policy: &ImagePolicy) -> Option<String> {
    let data = match images::resolve(href, policy) {
        Some(images::Source::File(f)) => std::fs::read(&f).ok(),
        Some(images::Source::Data(d)) => Some(d),
        None => None,
    };
    if data.is_none() {
        eprintln!("pdfrs: stylesheet blocked or not found: {}", href.chars().take(80).collect::<String>());
    }
    // url() in the sheet are relative to the sheet, not to the document
    data.map(|d| absolute_urls(&String::from_utf8_lossy(&d), href))
}

/// `rel` relative to `base` (a URL or a path in the image directory)
fn join_url(base: &str, rel: &str) -> String {
    let lower = rel.to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("data:") || lower.starts_with('#') {
        return rel.to_string();
    }
    if let Some(path) = rel.strip_prefix('/') {
        return match base.split_once("://") {
            Some((scheme, rest)) => format!("{scheme}://{}/{path}", rest.split('/').next().unwrap_or("")),
            None => path.to_string(),
        };
    }
    let dir = base.split(['?', '#']).next().unwrap_or("");
    // the directory of the base: up to its last '/' (keeping `scheme://host`)
    let cut = dir.rfind('/').filter(|i| !dir[..*i].ends_with('/') && !dir[..*i].ends_with(':'));
    match cut {
        Some(i) => format!("{}/{rel}", &dir[..i]),
        None if dir.contains("://") => format!("{dir}/{rel}"),
        None => rel.to_string(),
    }
}

/// Rewrite the relative `url(...)` of a stylesheet against the stylesheet's own location.
fn absolute_urls(css: &str, base: &str) -> String {
    if !base.contains('/') {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        out.push_str(&rest[..i + 4]);
        rest = &rest[i + 4..];
        let close = rest.find(')').unwrap_or(rest.len());
        let inner = rest[..close].trim();
        let q = inner.chars().next().filter(|c| *c == '"' || *c == '\'');
        let target = q.map_or(inner, |q| inner.trim_matches(q));
        out.push_str(&format!("\"{}\"", join_url(base, target)));
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

/// Inline the `@import url(x) [media];` statements of a stylesheet (recursively, 4 levels at most).
fn with_imports(css: &str, base: Option<&str>, policy: &ImagePolicy, depth: usize) -> String {
    if !css.contains("@import") || depth >= 4 {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("@import") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find(';').map(|e| at + e) else {
            rest = "";
            break;
        };
        let stmt = rest[at + "@import".len()..end].trim();
        rest = &rest[end + 1..];
        // `url("x") media`, `"x" media`
        let (target, media) = if let Some(u) = stmt.strip_prefix("url(") {
            let close = u.find(')').unwrap_or(u.len());
            (&u[..close], u.get(close + 1..).unwrap_or(""))
        } else {
            let q = stmt.chars().next().unwrap_or('"');
            let inner = &stmt[1.min(stmt.len())..];
            let close = inner.find(q).unwrap_or(inner.len());
            (&inner[..close], inner.get(close + 1..).unwrap_or(""))
        };
        let target = target.trim().trim_matches(|c| c == '"' || c == '\'');
        let target = base.map_or(target.to_string(), |b| join_url(b, target));
        if let Some(imported) = load_css(&target, policy) {
            let imported = with_imports(&imported, Some(&target), policy, depth + 1);
            if media.trim().is_empty() {
                out.push_str(&imported);
            } else {
                out.push_str(&format!("@media {} {{ {imported} }}", media.trim()));
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_relative_to_their_stylesheet() {
        assert_eq!(join_url("https://cdn.test/css/a.css", "b.css"), "https://cdn.test/css/b.css");
        assert_eq!(join_url("https://cdn.test/css/a.css", "../img/x.png"), "https://cdn.test/css/../img/x.png");
        assert_eq!(join_url("https://cdn.test/css/a.css", "/root.css"), "https://cdn.test/root.css");
        assert_eq!(join_url("https://cdn.test", "b.css"), "https://cdn.test/b.css");
        assert_eq!(join_url("css/site.css", "../img/x.png"), "css/../img/x.png");
        assert_eq!(join_url("site.css", "img.png"), "img.png");
        assert_eq!(join_url("css/site.css", "data:image/png;base64,AA"), "data:image/png;base64,AA");
        assert_eq!(absolute_urls("a { background: url('i.png') } @font-face { src: url(f.woff2) }", "s/x.css"), "a { background: url(\"s/i.png\") } @font-face { src: url(\"s/f.woff2\") }");
    }
}

/// The http(s) `<img>` sources of a document, to download in parallel before laying it out.
pub fn remote_images(doc: &Html) -> Vec<String> {
    let sel = scraper::Selector::parse("img[src]").unwrap();
    doc.select(&sel).filter_map(|i| i.value().attr("src")).filter(|s| s.starts_with("http://") || s.starts_with("https://")).map(String::from).collect()
}

/// Load the @font-face fonts a sheet declares: `data:` URLs, or files of the image directory.
/// TrueType, OpenType, WOFF and WOFF2.
pub fn load_font_faces(faces: &[Vec<Decl>], policy: &ImagePolicy) -> Vec<fonts::DocFace> {
    let mut out = Vec::new();
    for decls in faces {
        let get = |p: &str| decls.iter().rev().find(|d| d.prop == p).map(|d| d.value.as_str());
        let Some(family) = get("font-family").map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|f| !f.is_empty()) else {
            continue;
        };
        let weight = get("font-weight").and_then(|w| match w.split_whitespace().next()? {
            "normal" => Some(400.0),
            "bold" => Some(700.0),
            n => n.parse().ok(),
        });
        let italic = get("font-style").is_some_and(|s| s.contains("italic") || s.contains("oblique"));
        let srcs = get("src").unwrap_or("");
        let data = css::split_top_level(srcs, ',').find_map(|src| {
            let src = src.trim();
            let url = src.find("url(").map(|i| &src[i + 4..])?;
            let url = url[..url.find(')')?].trim().trim_matches(|c| c == '"' || c == '\'');
            let data = if url.to_ascii_lowercase().starts_with("data:") {
                let (meta, body) = url.split_once(',')?;
                if !meta.to_ascii_lowercase().ends_with(";base64") {
                    return None;
                }
                use base64::Engine;
                let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
                base64::engine::general_purpose::STANDARD.decode(clean).ok()?
            } else {
                match images::resolve(url, policy)? {
                    images::Source::File(f) => std::fs::read(f).ok()?,
                    images::Source::Data(d) => d,
                }
            };
            let sfnt = fonts::to_sfnt(data);
            if sfnt.is_none() {
                eprintln!("pdfrs: @font-face {family:?}: not a TrueType / OpenType / WOFF / WOFF2 font");
            }
            sfnt
        });
        if let Some(data) = data {
            out.push(fonts::DocFace { family, data: parley::fontique::Blob::new(Arc::new(data)), weight, italic });
        }
    }
    out
}

/// Build the box tree of a document, the root (<body>) being a block of `width` pt.
#[allow(clippy::too_many_arguments)]
pub fn build(doc: &Html, sheet: &Sheet, faces: &[fonts::DocFace], policy: &ImagePolicy, font: &str, font_size: f32, width: f32, lcx: &mut LayoutContext<Brush>) -> Tree {
    fonts::with_doc_font_cx(faces, |fcx| {
        let mut b = Builder {
            sheet,
            rem: font_size,
            policy,
            taffy: TaffyTree::new(),
            deco: HashMap::new(),
            links: Vec::new(),
            fcx,
            lcx,
            counters: HashMap::new(),
            box_depth: 0,
            walk_depth: 0,
            calc: Vec::new(),
            inline: Vec::new(),
            headers: HashMap::new(),
            footers: HashMap::new(),
            cells: Default::default(),
            first_letter: None,
            first_line: None,
            text_fills: Vec::new(),
            text_fill: 0,
            top_pages: HashMap::new(),
            bg_images: HashMap::new(),
        };
        b.taffy.disable_rounding();
        let base = Rc::new(Style::root(font, font_size));
        let html_el = doc.root_element();
        let html_style = Rc::new(b.style_of(html_el, &base));
        let body_sel = scraper::Selector::parse("body").unwrap();
        let body = doc.select(&body_sel).next().unwrap_or(html_el);
        let mut body_style = b.style_of(body, &html_style);
        // the page margins replace body's margin; the root fills the content width
        body_style.margin = [Len::Px(0.0); 4];
        body_style.display = Display::Block;
        body_style.size[0] = Len::Px(width);
        body_style.background = body_style.background.or(html_style.background);
        body_style.bg = body_style.bg.clone().or(html_style.bg.clone());
        b.apply_counters(body, &body_style, &mut Vec::new());
        let root = b.build_box(body, Rc::new(body_style)).unwrap_or_else(|| b.taffy.new_leaf(taffy::Style::default()).unwrap());
        Tree { taffy: b.taffy, root, deco: b.deco, links: b.links, calc: b.calc, inline: b.inline, headers: b.headers, footers: b.footers, cells: b.cells, text_fills: b.text_fills.into_iter().map(|n| n.unwrap_or(root)).collect(), top_pages: b.top_pages }
    })
}

fn attr<'a>(el: &ElementRef<'a>, name: &str) -> Option<&'a str> {
    el.value().attr(name)
}

fn apply_transform(t: &str, tr: Transform) -> String {
    match tr {
        Transform::None => t.to_string(),
        Transform::Upper => t.to_uppercase(),
        Transform::Lower => t.to_lowercase(),
        Transform::Capitalize => {
            let mut out = String::with_capacity(t.len());
            let mut start = true;
            for c in t.chars() {
                if start && c.is_alphanumeric() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    start = c.is_whitespace() || c == '-';
                    out.push(c);
                }
            }
            out
        }
    }
}

/// Default marker text of a list item (`disc` -> "•", `decimal` -> "3.", `"→ "` -> "→ ").
fn default_marker(kind: &str, n: i32) -> Option<String> {
    if let Some(q) = kind.chars().next().filter(|c| *c == '"' || *c == '\'') {
        return Some(kind.trim_matches(q).to_string());
    }
    match kind {
        "none" => None,
        "disc" | "circle" | "square" => Some(style::format_counter(n, kind)),
        _ => Some(format!("{}.", style::format_counter(n, kind))),
    }
}

/// parley text style of a run
fn text_style(s: &Style, brush: Brush) -> TextStyle<'static, 'static, Brush> {
    let mut family = s.font_family.to_string();
    if let Some(fb) = &fonts::registry().fallback {
        family.push_str(&format!(", \"{fb}\""));
    }
    let (collapse, wrap) = match s.white_space {
        WhiteSpace::Normal => (parley::WhiteSpaceCollapse::Collapse, parley::TextWrapMode::Wrap),
        WhiteSpace::NoWrap => (parley::WhiteSpaceCollapse::Collapse, parley::TextWrapMode::NoWrap),
        WhiteSpace::Pre => (parley::WhiteSpaceCollapse::Preserve, parley::TextWrapMode::NoWrap),
        WhiteSpace::PreWrap => (parley::WhiteSpaceCollapse::Preserve, parley::TextWrapMode::Wrap),
        WhiteSpace::PreLine => (parley::WhiteSpaceCollapse::PreserveBreaks, parley::TextWrapMode::Wrap),
    };
    TextStyle {
        font_family: parley::FontFamily::Source(Cow::Owned(family)),
        font_size: s.font_size,
        font_weight: parley::FontWeight::new(s.font_weight),
        font_style: if s.italic { parley::FontStyle::Italic } else { parley::FontStyle::Normal },
        brush,
        has_underline: s.underline,
        has_strikethrough: s.strike,
        line_height: match s.line_height {
            style::LineHeight::Normal => parley::LineHeight::MetricsRelative(1.0),
            style::LineHeight::Factor(f) => parley::LineHeight::FontSizeRelative(f),
            style::LineHeight::Px(px) => parley::LineHeight::Absolute(px),
        },
        letter_spacing: s.letter_spacing,
        word_spacing: s.word_spacing,
        white_space_collapse: collapse,
        text_wrap_mode: wrap,
        overflow_wrap: parley::OverflowWrap::BreakWord,
        // only shifts apply to text runs (top / middle / bottom are for boxes and table cells)
        vertical_align: if matches!(s.vertical_align, VAlign::Sub | VAlign::Super | VAlign::Length(_)) { valign(s.vertical_align) } else { parley::VerticalAlign::BASELINE },
        ..TextStyle::default()
    }
}

/// A parley layout for a group of inline pieces in `container` (inline boxes sized 0 until laid out).
fn text_layout(fcx: &mut FontContext, lcx: &mut LayoutContext<Brush>, pieces: &[Piece], container: &Style) -> (parley::Layout<Brush>, String) {
    let root = text_style(container, brush_of(container));
    let mut b = lcx.tree_builder(fcx, 1.0, true, &root);
    if container.rtl {
        b.set_base_direction(parley::BaseDirection::Rtl);
    }
    for p in pieces {
        match p {
            Piece::Text(t, s, brush) => {
                b.push_style_span(text_style(s, brush.clone()));
                b.push_text(t);
                b.pop_style_span();
            }
            Piece::Break(s, brush) => {
                b.push_style_span(text_style(s, brush.clone()));
                b.push_style_modification_span(&[StyleProperty::WhiteSpaceCollapse(parley::WhiteSpaceCollapse::Preserve)]);
                b.push_text("\n");
                b.pop_style_span();
                b.pop_style_span();
            }
            Piece::Box(node, s) => {
                // sized before layout (Tree::size_inline_boxes)
                b.push_inline_box(parley::InlineBox {
                    id: u64::from(*node),
                    kind: parley::InlineBoxKind::InFlow,
                    index: 0,
                    width: 0.0,
                    height: 0.0,
                    baseline: None,
                    vertical_align: valign(s.vertical_align),
                });
            }
        }
    }
    b.build()
}

/// Rebuild a leaf's text with its first `n` visible characters in its ::first-line style (and a line
/// break after them if `brk`), keeping the inline boxes' sizes; broken at width `w`.
fn restyle_first_line(t: &mut TextLeaf, n: usize, brk: bool, w: f32, fcx: &mut FontContext, lcx: &mut LayoutContext<Brush>) {
    let Some(fl) = &t.first_line else { return };
    let pieces = first_line_pieces(&fl.pieces, n, brk, &fl.decls, fl.rem);
    let (mut layout, text) = text_layout(fcx, lcx, &pieces, &fl.container);
    let sizes: Vec<_> = t.layout.inline_boxes().map(|b| (b.id, b.width, b.height, b.baseline)).collect();
    for ib in layout.inline_boxes_mut() {
        if let Some(sz) = sizes.iter().find(|s| s.0 == ib.id) {
            (ib.width, ib.height, ib.baseline) = (sz.1, sz.2, sz.3);
        }
    }
    layout.break_all_lines(Some(w + 0.01));
    (t.layout, t.text) = (layout, text);
}

/// The pieces with their first `n` non-whitespace characters in the ::first-line style.
fn first_line_pieces(pieces: &[Piece], n: usize, brk: bool, decls: &[Decl], rem: f32) -> Vec<Piece> {
    let refs: Vec<&Decl> = decls.iter().collect();
    let restyle = |s: &Rc<Style>, b: &Brush| {
        let fl = Style::compute(s, &refs, rem);
        let brush = Brush { color: fl.color, background: fl.background.or(b.background), ..b.clone() };
        (Rc::new(fl), brush)
    };
    let mut left = n;
    let mut out = Vec::with_capacity(pieces.len() + 1);
    for p in pieces {
        match p {
            Piece::Text(t, s, b) if left > 0 => {
                // split after `left` visible characters (and the whitespace right after them)
                let mut cut = t.len();
                let mut count = 0;
                for (i, c) in t.char_indices() {
                    if count == left && !c.is_whitespace() {
                        cut = i;
                        break;
                    }
                    if !c.is_whitespace() {
                        count += 1;
                    }
                }
                left -= count.min(left);
                let (fs, fb) = restyle(s, b);
                let styled = &t[..cut];
                out.push(Piece::Text(apply_transform(styled, fs.transform), fs.clone(), fb.clone()));
                if brk && left == 0 {
                    out.push(Piece::Break(fs, fb));
                }
                if cut < t.len() {
                    out.push(Piece::Text(t[cut..].to_string(), s.clone(), b.clone()));
                }
            }
            Piece::Break(s, b) if left > 0 => {
                let (fs, fb) = restyle(s, b);
                out.push(Piece::Break(fs, fb));
            }
            p => out.push(p.clone()),
        }
    }
    out
}

fn valign(v: VAlign) -> parley::VerticalAlign {
    match v {
        VAlign::Baseline => parley::VerticalAlign::BASELINE,
        VAlign::Top => parley::VerticalAlign::TOP,
        VAlign::Middle => parley::VerticalAlign::MIDDLE,
        VAlign::Bottom => parley::VerticalAlign::BOTTOM,
        VAlign::Sub => parley::VerticalAlign::SUB,
        VAlign::Super => parley::VerticalAlign::SUPER,
        VAlign::TextTop => parley::VerticalAlign::TEXT_TOP,
        VAlign::TextBottom => parley::VerticalAlign::TEXT_BOTTOM,
        VAlign::Length(px) => parley::VerticalAlign::length(px),
    }
}

fn brush_of(s: &Style) -> Brush {
    Brush { color: s.color, background: None, link: 0, text_fill: 0 }
}

fn to_taffy(s: &Style) -> taffy::Style {
    let rect = |v: [Len; 4]| taffy::Rect { top: v[0].lpa(), right: v[1].lpa(), bottom: v[2].lpa(), left: v[3].lpa() };
    let bw = |i: usize| if s.border_style[i] == BorderStyle::None { 0.0 } else { s.border_width[i] };
    let mut t = taffy::Style {
        display: match s.display {
            Display::Flex => taffy::Display::Flex,
            Display::Grid | Display::Table => taffy::Display::Grid,
            Display::None => taffy::Display::None,
            _ => taffy::Display::Block,
        },
        position: if s.position_absolute { taffy::Position::Absolute } else { taffy::Position::Relative },
        inset: rect(s.inset),
        size: taffy::Size { width: s.size[0].dim(), height: s.size[1].dim() },
        min_size: taffy::Size { width: s.min_size[0].lpa(), height: s.min_size[1].lpa() },
        max_size: taffy::Size { width: s.max_size[0].lpa(), height: s.max_size[1].lpa() },
        margin: rect(s.margin),
        padding: taffy::Rect { top: s.padding[0].lp(), right: s.padding[1].lp(), bottom: s.padding[2].lp(), left: s.padding[3].lp() },
        border: taffy::Rect {
            top: LengthPercentage::length(bw(0)),
            right: LengthPercentage::length(bw(1)),
            bottom: LengthPercentage::length(bw(2)),
            left: LengthPercentage::length(bw(3)),
        },
        box_sizing: if s.border_box { taffy::BoxSizing::BorderBox } else { taffy::BoxSizing::ContentBox },
        float: s.float,
        clear: s.clear,
        flex_direction: s.flex_direction,
        flex_wrap: s.flex_wrap,
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        flex_basis: s.flex_basis.dim(),
        justify_content: s.justify_content.unwrap_or(taffy::AlignContent::NORMAL),
        align_content: s.align_content.unwrap_or(taffy::AlignContent::NORMAL),
        align_items: s.align_items.unwrap_or(taffy::AlignItems::NORMAL),
        align_self: s.align_self,
        justify_items: s.justify_items.unwrap_or(taffy::AlignItems::NORMAL),
        justify_self: s.justify_self,
        gap: taffy::Size { width: s.gap[1].lp(), height: s.gap[0].lp() },
        aspect_ratio: s.aspect_ratio,
        direction: if s.rtl { taffy::Direction::Rtl } else { taffy::Direction::Ltr },
        ..Default::default()
    };
    if let Some(cols) = s.grid_template_columns.as_deref().and_then(|v| v.parse::<taffy::GridTemplateTracks<String, taffy::GridTemplateComponent<String>>>().ok()) {
        t.grid_template_columns = cols.tracks;
        t.grid_template_column_names = cols.line_names;
    }
    if let Some(rows) = s.grid_template_rows.as_deref().and_then(|v| v.parse::<taffy::GridTemplateTracks<String, taffy::GridTemplateComponent<String>>>().ok()) {
        t.grid_template_rows = rows.tracks;
        t.grid_template_row_names = rows.line_names;
    }
    if let Some(flow) = s.grid_auto_flow.as_deref().and_then(|v| v.parse().ok()) {
        t.grid_auto_flow = flow;
    }
    if let Some(l) = s.grid_column.as_deref().and_then(grid_line) {
        t.grid_column = l;
    }
    if let Some(l) = s.grid_row.as_deref().and_then(grid_line) {
        t.grid_row = l;
    }
    t
}

/// `1 / 3`, `span 2`, `2`
fn grid_line(v: &str) -> Option<taffy::Line<taffy::GridPlacement<String>>> {
    let mut parts = v.split('/');
    let start: taffy::GridPlacement<String> = parts.next()?.trim().parse().ok()?;
    let end = match parts.next() {
        Some(e) => e.trim().parse().ok()?,
        None => taffy::GridPlacement::Auto,
    };
    Some(taffy::Line { start, end })
}

fn deco_of(s: &Style) -> Deco {
    Deco {
        background: s.background,
        border_width: std::array::from_fn(|i| if s.border_style[i] == BorderStyle::None { 0.0 } else { s.border_width[i] }),
        border_style: s.border_style.map(|b| (b != BorderStyle::None).then_some(b)),
        border_color: s.border_color.map(|c| c.unwrap_or(s.color)),
        radius: s.radius,
        opacity: s.opacity,
        clip: s.overflow_hidden,
        break_before: s.break_before,
        break_after: s.break_after,
        avoid_break_inside: s.break_inside_avoid,
        transform: s.transform_fn.clone().map(|t| (t, s.transform_origin.clone(), s.font_size)),
        layers: Vec::new(),
        color_clip: s.bg.as_ref().map_or(BoxArea::Border, |b| Background::nth(&b.clips, b.images.len().max(1) - 1, BoxArea::Border)),
        shadows: s.shadows.clone(),
    }
}

/// Decode image bytes: SVG, or PNG / JPEG / GIF / WebP. With the natural size in pt (1 px = 1 pt).
fn decode(data: Vec<u8>) -> Option<(ImageKind, (f32, f32))> {
    if images::is_svg(&data) {
        let tree = svg_tree(&String::from_utf8_lossy(&data))?;
        let size = (tree.size().width(), tree.size().height());
        Some((ImageKind::Svg(Arc::new(tree)), size))
    } else {
        let image = raster(data)?;
        let (w, h) = image.size();
        Some((ImageKind::Raster(image), (w as f32, h as f32)))
    }
}

fn calc_fields(s: &Style) -> Vec<(CalcField, f32, f32)> {
    let mut out = Vec::new();
    let mut add = |f, l: Len| {
        if let Len::Calc(px, pct) = l {
            out.push((f, px, pct));
        }
    };
    add(CalcField::Width, s.size[0]);
    add(CalcField::Height, s.size[1]);
    add(CalcField::MinWidth, s.min_size[0]);
    add(CalcField::MinHeight, s.min_size[1]);
    add(CalcField::MaxWidth, s.max_size[0]);
    add(CalcField::MaxHeight, s.max_size[1]);
    add(CalcField::FlexBasis, s.flex_basis);
    for i in 0..4 {
        add(CalcField::Margin(i), s.margin[i]);
        add(CalcField::Padding(i), s.padding[i]);
        add(CalcField::Inset(i), s.inset[i]);
    }
    out
}

impl Builder<'_> {
    fn style_of(&self, el: ElementRef, parent: &Style) -> Style {
        let inline = attr(&el, "style").map(css::parse_declarations).unwrap_or_default();
        let decls = css::matched(el, None, &UA, &self.sheet.rules, &inline);
        let mut s = Style::compute(parent, &decls, self.rem);
        // presentational attributes
        if let Some(align) = attr(&el, "align") {
            if matches!(el.value().name(), "td" | "th" | "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                s.text_align = match align.to_ascii_lowercase().as_str() {
                    "center" => TextAlign::Center,
                    "right" => TextAlign::Right,
                    "justify" => TextAlign::Justify,
                    _ => TextAlign::Left,
                };
            }
        }
        if let Some(bg) = attr(&el, "bgcolor").and_then(style::color) {
            s.background = s.background.or(Some(bg));
        }
        s
    }

    /// Decoration and calc() fix-ups of a new element node.
    fn finish_node(&mut self, node: NodeId, s: &Style) {
        let mut d = deco_of(s);
        if let Some(bg) = &s.bg {
            for (i, src) in bg.images.iter().enumerate() {
                let paint = match src {
                    BgSource::Gradient(g) => LayerPaint::Gradient(g.clone()),
                    BgSource::Url(url) => {
                        let policy = self.policy;
                        let decoded = self
                            .bg_images
                            .entry(url.clone())
                            .or_insert_with(|| {
                                let img = images::load(url, policy).and_then(decode);
                                if img.is_none() {
                                    eprintln!("pdfrs: background image blocked or not found: {}", url.chars().take(80).collect::<String>());
                                }
                                img
                            })
                            .clone();
                        let Some((kind, natural)) = decoded else { continue };
                        LayerPaint::Image(kind, natural)
                    }
                };
                d.layers.push(Rc::new(BgLayer {
                    paint,
                    size: Background::nth(&bg.sizes, i, BgSize::Auto),
                    position: Background::nth(&bg.positions, i, (Len::Pct(0.0), Len::Pct(0.0))),
                    repeat: Background::nth(&bg.repeats, i, (true, true)),
                    origin: Background::nth(&bg.origins, i, BoxArea::Padding),
                    clip: Background::nth(&bg.clips, i, BoxArea::Border),
                    fixed: Background::nth(&bg.fixed, i, false),
                }));
            }
        }
        if d.needed() {
            self.deco.insert(node, d);
        }
        let calc = calc_fields(s);
        if !calc.is_empty() {
            self.calc.push((node, calc));
        }
    }

    /// `::first-letter` / `::first-line` of a block: armed for the text that follows (in this box or
    /// its first child boxes). Returns which ones this block set.
    #[inline(never)]
    fn first_pseudos(&mut self, el: ElementRef, s: &Style) -> (bool, bool) {
        if !self.sheet.has_pseudo {
            return (false, false);
        }
        let mut owns = (false, false);
        let decls = css::matched(el, Some(Pseudo::FirstLine), &UA, &self.sheet.rules, &[]);
        if !decls.is_empty() {
            self.first_line = Some(Rc::new(decls.into_iter().cloned().collect()));
            owns.1 = true;
        }
        let decls = css::matched(el, Some(Pseudo::FirstLetter), &UA, &self.sheet.rules, &[]);
        if !decls.is_empty() {
            let mut fl = Style::compute(s, &decls, self.rem);
            fl.display = Display::Inline;
            self.first_letter = Some(Rc::new(fl));
            owns.0 = true;
        }
        owns
    }

    /// `::before` / `::after` content of `el`: inline text in the current group, or its own box.
    #[inline(never)]
    fn pseudo(&mut self, el: ElementRef, s: &Rc<Style>, which: Pseudo, brush: &Brush, ctx: &mut Ctx, container: &Rc<Style>) {
        if !self.sheet.has_pseudo && el.value().name() != "q" {
            return;
        }
        let decls = css::matched(el, Some(which), &UA, &self.sheet.rules, &[]);
        if decls.is_empty() {
            return;
        }
        let ps = Style::compute(s, &decls, self.rem);
        let Some(text) = ps.content.as_deref().and_then(|c| style::content(c, |a| attr(&el, a).map(String::from), |n, sep, st| self.counter_text(n, sep, st))) else { return };
        let text = apply_transform(&text, ps.transform);
        let ps = Rc::new(ps);
        match ps.display {
            Display::None => {}
            Display::Inline | Display::Contents => {
                let b = Brush { color: ps.color, background: ps.background.or(brush.background), ..brush.clone() };
                ctx.group.push(Piece::Text(text, ps.clone(), b));
            }
            _ => {
                let inline = ps.inline_block && !ps.position_absolute && ps.float == taffy::Float::None;
                if !inline {
                    self.flush(ctx, container);
                }
                let mut inner = Ctx::default();
                if !text.is_empty() {
                    inner.group.push(Piece::Text(text, ps.clone(), brush_of(&ps)));
                    self.flush(&mut inner, &ps);
                }
                if let Ok(node) = self.taffy.new_with_children(to_taffy(&ps), &inner.children) {
                    self.finish_node(node, &ps);
                    if inline {
                        ctx.group.push(Piece::Box(node, ps));
                    } else {
                        ctx.children.push(node);
                    }
                }
            }
        }
    }

    /// An element box (block / flex / grid / list item / table / cell) and its content.

    fn build_box(&mut self, el: ElementRef, s: Rc<Style>) -> Option<NodeId> {
        if s.display == Display::Table {
            return self.build_table(el, s);
        }
        let mut ctx = Ctx { owner: Some(el.id()), ..Ctx::default() };
        if s.display == Display::ListItem {
            let n = self.counters.get("list-item").and_then(|c| c.last()).copied().unwrap_or(1);
            // ::marker: its style, and its content instead of the list-style-type one
            let decls = if self.sheet.has_pseudo { css::matched(el, Some(Pseudo::Marker), &UA, &self.sheet.rules, &[]) } else { Vec::new() };
            let mut ms = if decls.is_empty() { (*s).clone() } else { Style::compute(&s, &decls, self.rem) };
            let text = match ms.content.as_deref() {
                Some(c) => style::content(c, |a| attr(&el, a).map(String::from), |n, sep, st| self.counter_text(n, sep, st)),
                None => default_marker(&s.list_style, n),
            };
            if let Some(m) = text {
                ms.white_space = WhiteSpace::NoWrap;
                ms.underline = false;
                ms.strike = false;
                let ms = Rc::new(ms);
                let leaf = self.text_leaf(vec![Piece::Text(m, ms.clone(), brush_of(&ms))], &ms);
                if let Some(leaf) = leaf {
                    // outside marker: right edge against the item's left edge
                    let _ = self.taffy.set_style(
                        leaf,
                        taffy::Style {
                            position: taffy::Position::Absolute,
                            inset: if s.rtl {
                                taffy::Rect { top: LengthPercentageAuto::length(0.0), left: LengthPercentageAuto::percent(1.0), bottom: auto(), right: auto() }
                            } else {
                                taffy::Rect { top: LengthPercentageAuto::length(0.0), right: LengthPercentageAuto::percent(1.0), bottom: auto(), left: auto() }
                            },
                            margin: if s.rtl {
                                taffy::Rect { left: LengthPercentageAuto::length(s.font_size * 0.5), ..taffy::Rect::zero() }
                            } else {
                                taffy::Rect { right: LengthPercentageAuto::length(s.font_size * 0.5), ..taffy::Rect::zero() }
                            },
                            ..Default::default()
                        },
                    );
                    ctx.children.push(leaf);
                }
            }
        }
        // background-clip: text: this element's background paints the text inside it
        let outer_fill = self.text_fill;
        let fill_slot = s.bg.as_ref().filter(|b| b.clips.contains(&BoxArea::Text)).map(|_| {
            self.text_fills.push(None);
            self.text_fills.len() - 1
        });
        if let Some(i) = fill_slot {
            self.text_fill = i as u32 + 1;
        }
        let brush = Brush { text_fill: self.text_fill, ..brush_of(&s) };
        let (owns_first_letter, owns_first_line) = self.first_pseudos(el, &s);
        self.pseudo(el, &s, Pseudo::Before, &brush, &mut ctx, &s);
        self.walk(*el, &s, brush.clone(), &mut ctx, &s);
        self.pseudo(el, &s, Pseudo::After, &brush, &mut ctx, &s);
        self.flush(&mut ctx, &s);
        if owns_first_letter {
            self.first_letter = None;
        }
        if owns_first_line {
            self.first_line = None;
        }
        self.text_fill = outer_fill;
        let mut ts = to_taffy(&s);
        if s.display == Display::TableCell {
            // a cell centers (vertical-align) its content in the row height
            ts.display = taffy::Display::Flex;
            ts.flex_direction = FlexDirection::Column;
            ts.justify_content = match s.vertical_align {
                VAlign::Middle => JustifyContent::CENTER,
                VAlign::Bottom => JustifyContent::END,
                _ => JustifyContent::START,
            };
        }
        let node = self.taffy.new_with_children(ts, &ctx.children).ok()?;
        self.finish_node(node, &s);
        if let Some(i) = fill_slot {
            self.text_fills[i] = Some(node);
        }
        Some(node)
    }

    /// Children of `parent` in an inline context: text and inline elements join `ctx.group`,
    /// block-level children close the group and become boxes.
    fn walk(&mut self, parent: NodeRef<Node>, s: &Rc<Style>, brush: Brush, ctx: &mut Ctx, container: &Rc<Style>) {
        if self.walk_depth >= MAX_WALK_DEPTH {
            return;
        }
        self.walk_depth += 1;
        self.walk_children(parent, s, brush, ctx, container);
        self.walk_depth -= 1;
    }

    fn walk_children(&mut self, parent: NodeRef<Node>, s: &Rc<Style>, brush: Brush, ctx: &mut Ctx, container: &Rc<Style>) {
        // counters reset by these children live until the end of the parent (siblings see them)
        let mut scope = Vec::new();
        for child in parent.children() {
            match child.value() {
                Node::Text(t) => self.text(t, s, &brush, ctx),
                Node::Element(_) => {
                    let Some(el) = ElementRef::wrap(child) else { continue };
                    let mut cs = self.style_of(el, s);
                    if cs.display == Display::None {
                        continue;
                    }
                    // sub / sup shift their inline descendants too
                    if matches!(s.vertical_align, VAlign::Sub | VAlign::Super | VAlign::Length(_)) && cs.vertical_align == VAlign::Baseline && cs.display == Display::Inline {
                        cs.vertical_align = s.vertical_align;
                    }
                    self.apply_counters(el, &cs, &mut scope);
                    let cs = Rc::new(cs);
                    // children of a flex / grid box are its items, whatever their display (blockified)
                    let item = Some(parent.id()) == ctx.owner && matches!(container.display, Display::Flex | Display::Grid);
                    let in_flow = !cs.position_absolute && cs.float == taffy::Float::None;
                    match el.value().name() {
                        "br" => {
                            ctx.group.push(Piece::Break(cs, brush.clone()));
                            continue;
                        }
                        "img" | "svg" if item || !in_flow || cs.display != Display::Inline => {
                            self.flush(ctx, container);
                            if let Some(n) = self.replaced(el, &cs) {
                                ctx.children.push(n);
                            }
                            continue;
                        }
                        "img" | "svg" => {
                            if let Some(n) = self.replaced(el, &cs) {
                                ctx.group.push(Piece::Box(n, cs));
                            }
                            continue;
                        }
                        _ => {}
                    }
                    match cs.display {
                        Display::Inline | Display::Contents => {
                            let mut b = Brush { color: cs.color, ..brush.clone() };
                            if cs.display == Display::Inline {
                                b.background = cs.background.or(brush.background);
                            }
                            if el.value().name() == "a" {
                                if let Some(href) = attr(&el, "href").filter(|h| {
                                    let h = h.to_ascii_lowercase();
                                    h.starts_with("http://") || h.starts_with("https://") || h.starts_with("mailto:") || h.starts_with("tel:")
                                }) {
                                    self.links.push(href.to_string());
                                    b.link = self.links.len() as u32;
                                }
                            }
                            self.pseudo(el, &cs, Pseudo::Before, &b, ctx, container);
                            self.walk(*el, &cs, b.clone(), ctx, container);
                            self.pseudo(el, &cs, Pseudo::After, &b, ctx, container);
                        }
                        _ if self.box_depth >= MAX_BOX_DEPTH => self.walk(*el, &cs, brush.clone(), ctx, container),
                        _ if cs.inline_block && !item && in_flow => {
                            self.box_depth += 1;
                            if let Some(n) = self.build_box(el, cs.clone()) {
                                ctx.group.push(Piece::Box(n, cs));
                            }
                            self.box_depth -= 1;
                        }
                        _ => {
                            self.flush(ctx, container);
                            let page = cs.page.clone().filter(|_| self.box_depth == 0);
                            self.box_depth += 1;
                            if let Some(n) = self.build_box(el, cs) {
                                ctx.children.push(n);
                                if let Some(page) = page {
                                    self.top_pages.insert(n, page);
                                }
                            }
                            self.box_depth -= 1;
                        }
                    }
                }
                _ => {}
            }
        }
        self.end_counter_scope(scope);
    }

    /// A text node into the group; the first letter split off when a ::first-letter waits for it.
    #[inline(never)]
    fn text(&mut self, t: &str, s: &Rc<Style>, brush: &Brush, ctx: &mut Ctx) {
        let text = apply_transform(t, s.transform);
        let Some(fl) = self.first_letter.take_if(|_| !text.trim().is_empty()) else {
            ctx.group.push(Piece::Text(text, s.clone(), brush.clone()));
            return;
        };
        // the first letter, with the punctuation around it
        let start = text.len() - text.trim_start().len();
        let mut end = start;
        let mut seen_letter = false;
        for (i, c) in text[start..].char_indices() {
            if c.is_alphanumeric() {
                if seen_letter {
                    break;
                }
                seen_letter = true;
            } else if !(c.is_ascii_punctuation() || "«»“”‘’„".contains(c)) || seen_letter && !"'’.,;:!?»”".contains(c) {
                break;
            }
            end = start + i + c.len_utf8();
        }
        if start > 0 {
            ctx.group.push(Piece::Text(text[..start].to_string(), s.clone(), brush.clone()));
        }
        let b = Brush { color: fl.color, background: fl.background.or(brush.background), ..brush.clone() };
        ctx.group.push(Piece::Text(text[start..end].to_string(), fl, b));
        ctx.group.push(Piece::Text(text[end..].to_string(), s.clone(), brush.clone()));
    }

    /// CSS counters of an element, in order: counter-reset, counter-set, counter-increment
    /// (lists reset `list-item`, list items increment it). New instances go into `scope`.
    #[inline(never)]
    fn apply_counters(&mut self, el: ElementRef, s: &Style, scope: &mut Vec<String>) {
        let mut resets = s.counter_reset.as_deref().cloned().unwrap_or_default();
        let tag = el.value().name();
        if matches!(tag, "ol" | "ul" | "menu" | "dir") && !resets.iter().any(|r| r.0 == "list-item") {
            let start: i32 = attr(&el, "start").and_then(|v| v.trim().parse().ok()).unwrap_or(1);
            resets.push(("list-item".into(), start.saturating_sub(1)));
        }
        for (name, v) in resets {
            self.counters.entry(name.clone()).or_default().push(v);
            scope.push(name);
        }
        let mut sets = s.counter_set.as_deref().cloned().unwrap_or_default();
        let mut incs = s.counter_increment.as_deref().cloned().unwrap_or_default();
        if s.display == Display::ListItem && !incs.iter().chain(&sets).any(|c| c.0 == "list-item") {
            match attr(&el, "value").and_then(|v| v.trim().parse::<i32>().ok()) {
                Some(v) => sets.push(("list-item".into(), v)),
                None => incs.push(("list-item".into(), 1)),
            }
        }
        for (name, v, add) in sets.into_iter().map(|(n, v)| (n, v, false)).chain(incs.into_iter().map(|(n, v)| (n, v, true))) {
            let stack = self.counters.entry(name.clone()).or_default();
            if stack.is_empty() {
                stack.push(0);
                scope.push(name);
            }
            let top = stack.last_mut().unwrap();
            *top = if add { top.saturating_add(v) } else { v };
        }
    }

    fn end_counter_scope(&mut self, scope: Vec<String>) {
        for name in scope {
            if let Some(stack) = self.counters.get_mut(&name) {
                stack.pop();
            }
        }
    }

    /// `counter(name, style)` / `counters(name, sep, style)`
    fn counter_text(&self, name: &str, sep: Option<&str>, style: &str) -> String {
        let stack = self.counters.get(name).map(Vec::as_slice).unwrap_or(&[]);
        match sep {
            Some(sep) => {
                let all: Vec<String> = if stack.is_empty() { vec![style::format_counter(0, style)] } else { stack.iter().map(|n| style::format_counter(*n, style)).collect() };
                all.join(sep)
            }
            None => style::format_counter(stack.last().copied().unwrap_or(0), style),
        }
    }

    fn flush(&mut self, ctx: &mut Ctx, container: &Rc<Style>) {
        let group = std::mem::take(&mut ctx.group);
        if let Some(n) = self.text_leaf(group, container) {
            ctx.children.push(n);
        }
    }

    /// One anonymous block of inline content; None when it is only collapsible whitespace.
    fn text_leaf(&mut self, group: Vec<Piece>, container: &Rc<Style>) -> Option<NodeId> {
        let visible = group.iter().any(|p| match p {
            Piece::Text(t, s, _) => matches!(s.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine) && !t.is_empty() || !t.trim().is_empty(),
            Piece::Break(..) | Piece::Box(..) => true,
        });
        if !visible {
            return None;
        }
        let (layout, text) = text_layout(self.fcx, self.lcx, &group, container);
        let align = match container.text_align {
            TextAlign::Start => parley::Alignment::Start,
            TextAlign::Left => parley::Alignment::Left,
            TextAlign::Right => parley::Alignment::Right,
            TextAlign::Center => parley::Alignment::Center,
            TextAlign::Justify => parley::Alignment::Justify,
        };
        let first_line = self.first_line.take().map(|decls| FirstLine { pieces: group.clone(), container: container.clone(), decls, rem: self.rem });
        let leaf = self.taffy.new_leaf_with_context(taffy::Style::default(), Leaf::Text(TextLeaf { layout, text, align, first_line })).ok()?;
        for p in &group {
            if let Piece::Box(node, _) = p {
                self.inline.push((*node, leaf));
            }
        }
        Some(leaf)
    }

    /// <img> and inline <svg>: replaced boxes with a natural size.
    fn replaced(&mut self, el: ElementRef, s: &Rc<Style>) -> Option<NodeId> {
        let (kind, natural) = if el.value().name() == "svg" {
            let tree = svg_tree(&el.html())?;
            let (vw, vh) = (tree.size().width(), tree.size().height());
            (ImageKind::Svg(Arc::new(tree)), (vw, vh))
        } else {
            let src = attr(&el, "src").unwrap_or("");
            let Some(data) = images::load(src, self.policy) else {
                eprintln!("pdfrs: image blocked or not found: {}", src.chars().take(80).collect::<String>());
                return None;
            };
            decode(data)?
        };
        let mut s = (**s).clone();
        // width / height attributes, CSS wins (same as browsers)
        for (i, a) in ["width", "height"].iter().enumerate() {
            if s.size[i] == Len::Auto {
                if let Some(l) = attr(&el, a).and_then(|v| style::len(v, s.font_size, self.rem)) {
                    s.size[i] = l;
                }
            }
        }
        if s.max_size[0] == Len::Auto {
            s.max_size[0] = Len::Pct(100.0);
        }
        let mut ts = to_taffy(&s);
        ts.display = taffy::Display::Block;
        ts.item_is_replaced = true;
        if natural.0 > 0.0 && natural.1 > 0.0 {
            ts.aspect_ratio = Some(natural.0 / natural.1);
        }
        let fit = if el.value().name() == "svg" { ObjectFit::Contain } else { s.object_fit };
        let node = self.taffy.new_leaf_with_context(ts, Leaf::Image(ImageLeaf { kind, natural, fit, radius: s.radius })).ok()?;
        self.finish_node(node, &s);
        Some(node)
    }

    /// Tables become grids: one auto track per column, cells placed by row / colspan / rowspan.
    fn build_table(&mut self, el: ElementRef, s: Rc<Style>) -> Option<NodeId> {
        let mut rows: Vec<(ElementRef, Rc<Style>)> = Vec::new();
        let mut captions = Vec::new();
        let (header_rows, footer_rows) = self.table_rows(el, &s, &mut rows, &mut captions);
        let mut header_cells = Vec::new();
        let mut footer_cells = Vec::new();
        let mut scope = Vec::new();
        let mut last_row = None;

        let mut children = Vec::new();
        let mut taken: Vec<Vec<bool>> = Vec::new();
        let mut ncols = 1;
        let first_row = if captions.is_empty() { 0 } else { 1 };
        let mut cells = Vec::new();
        for (r, (row, rs)) in rows.iter().enumerate() {
            let mut c = 0;
            for cell in row.children().filter_map(ElementRef::wrap) {
                let mut cs = self.style_of(cell, rs);
                if cs.display != Display::TableCell {
                    continue;
                }
                cs.background = cs.background.or(rs.background);
                let span = |a| attr(&cell, a).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
                let (colspan, rowspan) = (span("colspan"), span("rowspan"));
                while taken.get(r).and_then(|t| t.get(c)).copied().unwrap_or(false) {
                    c += 1;
                }
                for rr in r..r + rowspan {
                    if taken.len() <= rr {
                        taken.resize(rr + 1, Vec::new());
                    }
                    if taken[rr].len() < c + colspan {
                        taken[rr].resize(c + colspan, false);
                    }
                    for cc in c..c + colspan {
                        taken[rr][cc] = true;
                    }
                }
                ncols = ncols.max(c + colspan);
                cells.push((cell, cs, r, c, rowspan, colspan));
                c += colspan;
            }
        }
        let nrows = rows.len();
        for (cell, mut cs, r, c, rowspan, colspan) in cells {
            // counters of rows (once per row) and cells, in document order
            if last_row != Some(r) {
                last_row = Some(r);
                let (row, rs) = (rows[r].0, rows[r].1.clone());
                self.apply_counters(row, &rs, &mut scope);
            }
            self.apply_counters(cell, &cs, &mut scope);
            if s.border_collapse {
                // collapse: neighbours overlap their shared border
                if c > 0 {
                    cs.margin[3] = Len::Px(-cs.border_width[3]);
                }
                if r > 0 {
                    cs.margin[0] = Len::Px(-cs.border_width[0]);
                }
            }
            if let Some(n) = self.build_box(cell, Rc::new(cs)) {
                let mut st = self.taffy.style(n).ok()?.clone();
                st.grid_row = taffy::Line { start: line((first_row + r + 1) as i16), end: span(rowspan as u16) };
                st.grid_column = taffy::Line { start: line((c + 1) as i16), end: span(colspan as u16) };
                let _ = self.taffy.set_style(n, st);
                children.push(n);
                self.cells.insert(n);
                if r < header_rows {
                    header_cells.push(n);
                }
                if r >= nrows - footer_rows {
                    footer_cells.push(n);
                }
            }
        }
        for cap in captions {
            let mut st = self.taffy.style(cap).ok()?.clone();
            st.grid_row = taffy::Line { start: line(1), end: span(1) };
            st.grid_column = taffy::Line { start: line(1), end: line(-1) };
            let _ = self.taffy.set_style(cap, st);
            children.push(cap);
        }
        let mut ts = to_taffy(&s);
        ts.display = taffy::Display::Grid;
        ts.grid_template_columns = (0..ncols).map(|_| taffy::GridTemplateComponent::AUTO).collect();
        let spacing = if s.border_collapse { 0.0 } else { s.border_spacing };
        ts.gap = taffy::Size { width: LengthPercentage::length(spacing), height: LengthPercentage::length(spacing) };
        // shrink to content like a browser table, unless a width is given
        if s.size[0] == Len::Auto {
            ts.justify_content = JustifyContent::START;
        }
        let node = self.taffy.new_with_children(ts, &children).ok()?;
        self.finish_node(node, &s);
        self.end_counter_scope(scope);
        if !header_cells.is_empty() {
            self.headers.insert(node, header_cells);
        }
        if !footer_cells.is_empty() {
            self.footers.insert(node, footer_cells);
        }
        Some(node)
    }

    /// Rows of a table (thead first, tfoot last); returns the number of <thead> and <tfoot> rows.
    fn table_rows<'d>(&mut self, el: ElementRef<'d>, s: &Rc<Style>, rows: &mut Vec<(ElementRef<'d>, Rc<Style>)>, captions: &mut Vec<NodeId>) -> (usize, usize) {
        // thead first, tfoot last, like a browser
        let mut groups: [Vec<(ElementRef<'d>, Rc<Style>)>; 3] = Default::default();
        for child in el.children().filter_map(ElementRef::wrap) {
            let cs = Rc::new(self.style_of(child, s));
            match cs.display {
                Display::TableRow => groups[1].push((child, cs)),
                Display::TableRowGroup => {
                    let g = match child.value().name() {
                        "thead" => 0,
                        "tfoot" => 2,
                        _ => 1,
                    };
                    for tr in child.children().filter_map(ElementRef::wrap) {
                        let mut ts = self.style_of(tr, &cs);
                        if ts.display == Display::TableRow {
                            ts.background = ts.background.or(cs.background);
                            groups[g].push((tr, Rc::new(ts)));
                        }
                    }
                }
                Display::TableCaption => {
                    let mut caption = (*cs).clone();
                    caption.display = Display::Block;
                    if let Some(n) = self.build_box(child, Rc::new(caption)) {
                        captions.push(n);
                    }
                }
                _ => {}
            }
        }
        let (header_rows, footer_rows) = (groups[0].len(), groups[2].len());
        rows.extend(groups.into_iter().flatten());
        (header_rows, footer_rows)
    }
}

fn raster(data: Vec<u8>) -> Option<krilla::image::Image> {
    let kind = imagesize::image_type(&data).ok()?;
    let data: krilla::Data = data.into();
    match kind {
        imagesize::ImageType::Png => krilla::image::Image::from_png(data, true).ok(),
        imagesize::ImageType::Jpeg => krilla::image::Image::from_jpeg(data, true).ok(),
        imagesize::ImageType::Gif => krilla::image::Image::from_gif(data, true).ok(),
        imagesize::ImageType::Webp => krilla::image::Image::from_webp(data, true).ok(),
        _ => None,
    }
}

fn svg_tree(markup: &str) -> Option<usvg::Tree> {
    let opts = usvg::Options { fontdb: fonts::registry().svg_fonts.clone(), ..Default::default() };
    usvg::Tree::from_str(markup, &opts).ok()
}

fn side_mut<T>(r: &mut taffy::Rect<T>, i: usize) -> &mut T {
    match i {
        0 => &mut r.top,
        1 => &mut r.right,
        2 => &mut r.bottom,
        _ => &mut r.left,
    }
}

/// Measure a leaf: text wraps at the available width, images keep their aspect ratio.
pub fn measure(inputs: taffy::LayoutInput, leaf: Option<&mut Leaf>, style: &taffy::Style) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, avail| match leaf {
        Some(Leaf::Text(t)) => {
            let max = known.width.or(match avail.width {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MinContent => Some(0.0),
                AvailableSpace::MaxContent => None,
            });
            t.layout.break_all_lines(max);
            Size { width: known.width.unwrap_or_else(|| t.layout.width()), height: known.height.unwrap_or_else(|| t.layout.height()) }
        }
        Some(Leaf::Image(img)) => {
            let (nw, nh) = img.natural;
            let ratio = if nw > 0.0 && nh > 0.0 { nw / nh } else { 1.0 };
            match (known.width, known.height) {
                (Some(w), Some(h)) => Size { width: w, height: h },
                (Some(w), None) => Size { width: w, height: w / ratio },
                (None, Some(h)) => Size { width: h * ratio, height: h },
                (None, None) => {
                    let w = match avail.width {
                        AvailableSpace::Definite(a) => nw.min(a),
                        AvailableSpace::MinContent => 0.0,
                        AvailableSpace::MaxContent => nw,
                    };
                    Size { width: w, height: w / ratio }
                }
            }
        }
        None => Size::ZERO,
    })
}

impl Tree {
    /// Resolve calc() lengths against their parent's content box; true if a style changed.
    fn fix_calc(&mut self) -> bool {
        let mut changed = false;
        for (node, fields) in &self.calc {
            let Some(parent) = self.taffy.parent(*node) else { continue };
            let Ok(pl) = self.taffy.layout(parent) else { continue };
            let (bw, bh) = (pl.content_box_width(), pl.content_box_height());
            let Ok(mut st) = self.taffy.style(*node).cloned() else { continue };
            let before = st.clone();
            for &(f, px, pct) in fields {
                let w = px + bw * pct / 100.0;
                let h = px + bh * pct / 100.0;
                match f {
                    CalcField::Width => st.size.width = Dimension::length(w),
                    CalcField::Height => st.size.height = Dimension::length(h),
                    CalcField::MinWidth => st.min_size.width = LengthPercentageAuto::length(w),
                    CalcField::MinHeight => st.min_size.height = LengthPercentageAuto::length(h),
                    CalcField::MaxWidth => st.max_size.width = LengthPercentageAuto::length(w),
                    CalcField::MaxHeight => st.max_size.height = LengthPercentageAuto::length(h),
                    CalcField::FlexBasis => st.flex_basis = Dimension::length(w),
                    // margins and paddings are relative to the width, on every side
                    CalcField::Margin(i) => *side_mut(&mut st.margin, i) = LengthPercentageAuto::length(w),
                    CalcField::Padding(i) => *side_mut(&mut st.padding, i) = LengthPercentage::length(w),
                    CalcField::Inset(i) => *side_mut(&mut st.inset, i) = LengthPercentageAuto::length(if i % 2 == 0 { h } else { w }),
                }
            }
            if st != before {
                let _ = self.taffy.set_style(*node, st);
                changed = true;
            }
        }
        changed
    }

    /// Lay the tree out and set every text leaf's final line breaks and alignment.
    /// Lay out, then style the `::first-line` of the blocks that have one (needs fonts: that text
    /// is shaped again) and lay out once more if it changed.
    pub fn compute_with_fonts(&mut self, root: NodeId, width: f32, fcx: &mut FontContext, lcx: &mut LayoutContext<Brush>) {
        self.compute_root(root, width);
        let leaves: Vec<NodeId> = self.taffy_leaves(root).into_iter().filter(|n| matches!(self.taffy.get_node_context(*n), Some(Leaf::Text(t)) if t.first_line.is_some())).collect();
        if leaves.is_empty() {
            return;
        }
        let mut changed = false;
        for leaf in leaves {
            let w = self.taffy.layout(leaf).map(|l| l.content_box_width()).unwrap_or(0.0);
            let Some(Leaf::Text(t)) = self.taffy.get_node_context_mut(leaf) else { continue };
            // the number of visible characters in the ::first-line style: the most that still fit on the
            // first line once styled. Too many styled: shrink; room left for more: try styling them too.
            let first_count = |t: &TextLeaf| t.layout.lines().next().map_or(0, |l| t.text[l.text_range()].chars().filter(|c| !c.is_whitespace()).count());
            let mut styled = first_count(t);
            let mut too_many = usize::MAX;
            let mut forced_break = false;
            for _ in 0..10 {
                restyle_first_line(t, styled, false, w, fcx, lcx);
                let n = first_count(t);
                if n < styled {
                    too_many = too_many.min(styled);
                    styled = n;
                } else if n > styled && n < too_many {
                    styled = n;
                } else {
                    // fits; unstyled text left on the first line would not fit styled: end the line there
                    forced_break = n > styled;
                    break;
                }
            }
            if forced_break {
                restyle_first_line(t, styled, true, w, fcx, lcx);
            }
            changed = true;
            let _ = self.taffy.mark_dirty(leaf);
        }
        if changed {
            self.compute_root(root, width);
        }
    }

    /// Split the root into sections of consecutive top-level boxes with the same `page` name, each
    /// its own root (a copy of the body box) to lay out at its page's width. One section without a
    /// name: the root as is.
    pub fn sections(&mut self) -> Vec<(NodeId, Option<Rc<str>>)> {
        let children = self.taffy.children(self.root).unwrap_or_default();
        let mut groups: Vec<(Option<Rc<str>>, Vec<NodeId>)> = Vec::new();
        for c in children {
            let name = self.top_pages.get(&c).cloned();
            match groups.last_mut() {
                Some(g) if g.0 == name => g.1.push(c),
                _ => groups.push((name, vec![c])),
            }
        }
        if groups.len() <= 1 && groups.first().is_none_or(|g| g.0.is_none()) {
            return vec![(self.root, groups.pop().and_then(|g| g.0))];
        }
        let style = self.taffy.style(self.root).cloned().unwrap_or_default();
        let deco = self.deco.get(&self.root).cloned();
        let mut out = Vec::new();
        for (i, (name, nodes)) in groups.into_iter().enumerate() {
            let root = if i == 0 {
                let _ = self.taffy.set_children(self.root, &nodes);
                self.root
            } else {
                let r = self.taffy.new_with_children(style.clone(), &nodes).unwrap_or(self.root);
                if let Some(d) = &deco {
                    self.deco.insert(r, d.clone());
                }
                r
            };
            out.push((root, name));
        }
        out
    }

    /// Lay a section root out at `width`.
    pub fn set_width(&mut self, root: NodeId, width: f32) {
        if let Ok(mut st) = self.taffy.style(root).cloned() {
            st.size.width = Dimension::length(width);
            let _ = self.taffy.set_style(root, st);
        }
    }

    fn taffy_leaves(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        stack.extend(self.inline.iter().map(|i| i.0));
        while let Some(n) = stack.pop() {
            let children = self.taffy.children(n).unwrap_or_default();
            if children.is_empty() {
                out.push(n);
            }
            stack.extend(children);
        }
        out
    }

    pub fn compute(&mut self, width: f32) {
        self.compute_root(self.root, width);
    }

    pub fn compute_root(&mut self, root: NodeId, width: f32) {
        let space = Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent };
        self.size_inline_boxes();
        let _ = self.taffy.compute_layout_with_measure(root, space, |inputs, _, leaf, style| measure(inputs, leaf, style));
        // calc(px + pct%) resolved against the containing block just laid out, inline boxes wider
        // than their line shrunk to it (shrink-to-fit): then lay out again
        for _ in 0..4 {
            let calc = self.fix_calc();
            let fit = self.fit_inline_boxes();
            if !calc && !fit {
                break;
            }
            let _ = self.taffy.compute_layout_with_measure(root, space, |inputs, _, leaf, style| measure(inputs, leaf, style));
        }
        self.finish_text(root);
    }

    /// Final line breaks and alignment of the text leaves of a subtree, at their laid out width.
    fn finish_text(&mut self, start: NodeId) {
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            stack.extend(self.taffy.children(n).unwrap_or_default());
            let w = self.taffy.layout(n).map(|l| l.content_box_width()).unwrap_or(0.0);
            if let Some(Leaf::Text(t)) = self.taffy.get_node_context_mut(n) {
                t.layout.break_all_lines(Some(w + 0.01));
                t.layout.align(t.align, parley::AlignmentOptions::default());
            }
        }
    }

    /// Lay an inline box out on its own (max-content width, or `width`), then give its
    /// margin-box size and baseline to the parley box that stands for it in its line.
    fn lay_out_inline_box(&mut self, bx: NodeId, leaf: NodeId, width: Option<f32>) -> f32 {
        let space = Size { width: width.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite), height: AvailableSpace::MaxContent };
        let _ = self.taffy.compute_layout_with_measure(bx, space, |inputs, _, leaf, style| measure(inputs, leaf, style));
        self.finish_text(bx);
        let Ok(l) = self.taffy.layout(bx).cloned() else { return 0.0 };
        let (w, h) = (l.size.width + l.margin.left + l.margin.right, l.size.height + l.margin.top + l.margin.bottom);
        // an inline-block sits on the baseline of its last line of text; an image (or no text) on its bottom edge
        let baseline = match self.taffy.get_node_context(bx) {
            Some(Leaf::Image(_)) => None,
            _ => self.last_baseline(bx, -l.location.y).map(|b| b + l.margin.top),
        };
        if let Some(Leaf::Text(t)) = self.taffy.get_node_context_mut(leaf) {
            for ib in t.layout.inline_boxes_mut() {
                if ib.id == u64::from(bx) {
                    (ib.width, ib.height, ib.baseline) = (w, h, baseline);
                }
            }
        }
        let _ = self.taffy.mark_dirty(leaf);
        w
    }

    fn size_inline_boxes(&mut self) {
        for (bx, leaf) in self.inline.clone() {
            self.lay_out_inline_box(bx, leaf, None);
        }
    }

    /// Inline boxes wider than their line, laid out again at the line's width; true if any shrank.
    fn fit_inline_boxes(&mut self) -> bool {
        let mut changed = false;
        for (bx, leaf) in self.inline.clone() {
            let avail = self.taffy.layout(leaf).map(|l| l.content_box_width()).unwrap_or(0.0);
            let Ok(l) = self.taffy.layout(bx).cloned() else { continue };
            let current = l.size.width + l.margin.left + l.margin.right;
            if current > avail + 0.5 {
                let margins = l.margin.left + l.margin.right;
                if self.lay_out_inline_box(bx, leaf, Some((avail - margins).max(0.0))) < current - 0.5 {
                    changed = true;
                }
            }
        }
        changed
    }

    /// Baseline of the last line of text in a subtree, from its top (border box).
    fn last_baseline(&self, n: NodeId, oy: f32) -> Option<f32> {
        let l = self.taffy.layout(n).ok()?;
        let y = oy + l.location.y;
        if let Some(Leaf::Text(t)) = self.taffy.get_node_context(n) {
            return t.layout.lines().last().map(|line| y + l.padding.top + l.border.top + line.metrics().baseline);
        }
        let children = self.taffy.children(n).ok()?;
        children.iter().rev().find_map(|c| self.last_baseline(*c, y))
    }
}
