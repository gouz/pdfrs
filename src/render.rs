//! Pagination and PDF drawing (krilla).
//!
//! The document is laid out once as one tall column of the content width, then cut into pages.
//! Cuts never split a text line, an image or a `break-inside: avoid` box shorter than a page
//! (they are moved to the next page); boxes that span a cut are drawn on both pages, clipped.

use crate::layout::{BgLayer, Brush, Deco, ImageKind, ImageLeaf, LayerPaint, Leaf, TextLeaf, Tree};
use crate::style::{self, BgSize, BorderStyle, BoxArea, Gradient, GradientKind, Len, ObjectFit, Rgba, Shadow};
use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Point, Rect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, LinearGradient, Paint, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla_svg::{SurfaceExt, SvgSettings};
use std::cell::RefCell;
use std::collections::HashMap;
use taffy::{NodeId, TraversePartialTree};

/// Vertical ranges that must not be cut, and forced page breaks.
#[derive(Default)]
pub struct Breaks {
    atoms: Vec<(f32, f32)>,
    forced: Vec<f32>,
    /// bottom of the last visible thing (text, image, background, border)
    bottom: f32,
    tables: Vec<TableHead>,
}

/// A table with a <thead> and / or a <tfoot>, repeated on the pages it spans.
#[derive(Clone, Copy, Debug)]
pub struct TableHead {
    pub table: NodeId,
    /// absolute origin of the table's parent (what draw() takes)
    pub origin: (f32, f32),
    /// header rows / footer rows, absolute y ranges
    pub head: Option<(f32, f32)>,
    pub foot: Option<(f32, f32)>,
    pub top: f32,
    pub bottom: f32,
}

/// A page: the slice [start, end) of the column, between a repeated table header and footer if any.
#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub start: f32,
    pub end: f32,
    pub header: Option<TableHead>,
    pub footer: Option<TableHead>,
}

impl Page {
    pub fn header_height(&self) -> f32 {
        self.header.and_then(|h| h.head).map_or(0.0, |h| h.1 - h.0)
    }
    pub fn footer_height(&self) -> f32 {
        self.footer.and_then(|f| f.foot).map_or(0.0, |f| f.1 - f.0)
    }
}

pub fn collect_breaks(tree: &Tree) -> Breaks {
    let mut b = Breaks::default();
    walk_breaks(tree, tree.root, 0.0, 0.0, &mut b);
    b.bottom = b.atoms.iter().map(|a| a.1).fold(b.bottom, f32::max);
    b.atoms.sort_by(|a, b| a.0.total_cmp(&b.0));
    b.forced.sort_by(f32::total_cmp);
    b
}

fn walk_breaks(tree: &Tree, n: NodeId, ox: f32, oy: f32, b: &mut Breaks) {
    let Ok(l) = tree.taffy.layout(n) else { return };
    let (x, y) = (ox + l.location.x, oy + l.location.y);
    let range = |cells: Option<&Vec<NodeId>>| {
        let rows = cells?.iter().filter_map(|c| tree.taffy.layout(*c).ok()).map(|c| (y + c.location.y, y + c.location.y + c.size.height));
        Some(rows.fold((f32::INFINITY, f32::NEG_INFINITY), |a, r| (a.0.min(r.0), a.1.max(r.1)))).filter(|r| r.0.is_finite())
    };
    let (head, foot) = (range(tree.headers.get(&n)), range(tree.footers.get(&n)));
    if head.is_some() || foot.is_some() {
        b.tables.push(TableHead { table: n, origin: (ox, oy), head, foot, top: y, bottom: y + l.size.height });
    }
    if let Some(d) = tree.deco.get(&n) {
        if d.break_before {
            b.forced.push(y);
        }
        if d.break_after {
            b.forced.push(y + l.size.height);
        }
        if d.avoid_break_inside {
            b.atoms.push((y, y + l.size.height));
        }
        if d.background.is_some() || !d.layers.is_empty() || d.border_width.iter().any(|w| *w > 0.0) {
            b.bottom = b.bottom.max(y + l.size.height);
        }
    }
    match tree.taffy.get_node_context(n) {
        Some(Leaf::Text(t)) => {
            let top = y + l.padding.top + l.border.top;
            for line in t.layout.lines() {
                let m = line.metrics();
                b.atoms.push((top + m.block_min_coord, top + m.block_max_coord));
            }
        }
        Some(Leaf::Image(_)) => b.atoms.push((y, y + l.size.height)),
        None => {}
    }
    if tree.cells.contains(&n) {
        b.atoms.push((y, y + l.size.height));
    }
    for c in tree.taffy.child_ids(n) {
        walk_breaks(tree, c, x, y, b);
    }
}

/// Pages of `page_height` (`first_height` for the first one), cut so no unbreakable range is split. Always at least one page.
/// A page that starts inside the body of a table with a <thead> repeats the header on top
/// (and has that much less room).
pub fn paginate(first_height: f32, page_height: f32, breaks: &Breaks) -> Vec<Page> {
    const EPS: f32 = 0.01;
    let mut pages = Vec::new();
    let mut start = 0.0f32;
    loop {
        let page_height = if pages.is_empty() { first_height } else { page_height };
        // innermost table whose body continues here: its header on top
        let header = breaks
            .tables
            .iter()
            .filter(|t| t.head.is_some_and(|h| start > 0.0 && h.1 <= start + EPS && start < t.bottom - EPS && h.1 - h.0 <= page_height / 2.0))
            .max_by(|a, b| a.top.total_cmp(&b.top))
            .copied();
        let page = page_height - header.and_then(|h| h.head).map_or(0.0, |h| h.1 - h.0);
        let mut cut = cut_at(start, page, breaks);
        // innermost table broken by the cut before its footer: room for the footer at the bottom
        let footer = breaks
            .tables
            .iter()
            .filter(|t| t.foot.is_some_and(|f| t.top < cut - EPS && cut < f.0 - EPS && f.1 - f.0 <= page / 2.0))
            .max_by(|a, b| a.top.total_cmp(&b.top))
            .copied();
        if let Some(f) = footer.and_then(|f| f.foot) {
            cut = cut_at(start, page - (f.1 - f.0), breaks);
        }
        pages.push(Page { start, end: cut, header, footer });
        start = cut;
        // stop when nothing visible remains below the cut (forced breaks at the very end included)
        if breaks.bottom <= start + EPS || pages.len() >= 10_000 {
            break;
        }
    }
    pages
}

/// Where a page starting at `start` with `page` of room ends: at a forced break, else as low as
/// possible without splitting an unbreakable range shorter than a page.
fn cut_at(start: f32, page: f32, breaks: &Breaks) -> f32 {
    const EPS: f32 = 0.01;
    let limit = start + page;
    let mut cut = breaks.forced.iter().copied().find(|&f| f > start + EPS && f < limit).unwrap_or(limit);
    // move the cut above any unbreakable range it crosses, until it crosses none
    loop {
        let crossing = breaks
            .atoms
            .iter()
            .filter(|(t, b)| *t < cut - EPS && *b > cut + EPS && b - t <= page && *t > start + EPS)
            .map(|(t, _)| *t)
            .fold(f32::INFINITY, f32::min);
        if crossing.is_finite() && crossing < cut {
            cut = crossing;
        } else {
            break;
        }
    }
    if cut <= start + EPS { limit } else { cut }
}

thread_local! {
    /// krilla fonts by (blob id, face index): parsed once per thread
    static FONTS: RefCell<HashMap<(u64, u32), Option<Font>>> = RefCell::new(HashMap::new());
    /// the same for the @font-face fonts of the document being rendered, dropped after it
    static DOC_FONTS: RefCell<HashMap<(u64, u32), Option<Font>>> = RefCell::new(HashMap::new());
}

fn krilla_font(f: &parley::FontData) -> Option<Font> {
    let cache = if crate::fonts::registry().is_shared(f.data.id()) { &FONTS } else { &DOC_FONTS };
    cache.with(|c| {
        c.borrow_mut()
            .entry((f.data.id(), f.index))
            .or_insert_with(|| {
                let blob = f.data.clone();
                Font::new(krilla::Data::from(std::sync::Arc::new(blob) as std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>), f.index)
            })
            .clone()
    })
}

/// Forget the fonts of the document just rendered (its @font-face).
pub fn end_document() {
    DOC_FONTS.with(|c| c.borrow_mut().clear());
}

/// Stop offsets along a gradient line of `length`: missing positions spread evenly, never decreasing.
fn stop_offsets(stops: &[(Rgba, Option<Len>)], length: f32) -> Vec<(Rgba, f32)> {
    let n = stops.len();
    let mut pos: Vec<Option<f32>> = stops.iter().map(|(_, p)| p.map(|p| style::resolve(p, length) / length.max(0.001))).collect();
    if pos[0].is_none() {
        pos[0] = Some(0.0);
    }
    if pos[n - 1].is_none() {
        pos[n - 1] = Some(1.0);
    }
    let mut i = 0;
    while i < n {
        if pos[i].is_none() {
            let (a, prev) = (i - 1, pos[i - 1].unwrap());
            let b = (i..n).find(|&j| pos[j].is_some()).unwrap();
            let next = pos[b].unwrap();
            for (k, slot) in pos.iter_mut().enumerate().take(b).skip(i) {
                *slot = Some(prev + (next - prev) * (k - a) as f32 / (b - a) as f32);
            }
            i = b;
        }
        i += 1;
    }
    let mut last = f32::NEG_INFINITY;
    stops.iter().zip(pos).map(|((c, _), p)| {
        last = p.unwrap().max(last);
        (*c, last)
    }).collect()
}

fn krilla_stops(stops: &[(Rgba, f32)]) -> Vec<Stop> {
    stops
        .iter()
        .map(|(c, o)| Stop {
            offset: NormalizedF32::new(o.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE),
            color: rgb::Color::new(c.0, c.1, c.2).into(),
            opacity: NormalizedF32::new(c.3 as f32 / 255.0).unwrap_or(NormalizedF32::ONE),
        })
        .collect()
}

/// A repeating gradient repeats the span of its stops: the line is shortened to it, offsets rescaled.
fn repeat_span(offsets: &mut [(Rgba, f32)], repeating: bool) -> (f32, f32, SpreadMethod) {
    let (first, last) = (offsets[0].1, offsets[offsets.len() - 1].1);
    if !repeating || last - first <= 0.0001 {
        return (0.0, 1.0, SpreadMethod::Pad);
    }
    for o in offsets.iter_mut() {
        o.1 = (o.1 - first) / (last - first);
    }
    (first, last, SpreadMethod::Repeat)
}

fn gradient_paint(g: &Gradient, x: f32, y: f32, w: f32, h: f32) -> Option<Paint> {
    match g.kind {
        GradientKind::Linear(_) | GradientKind::LinearCorner(..) => {
            let a = match g.kind {
                GradientKind::Linear(deg) => deg.to_radians(),
                // the 50% line goes through the two other corners
                GradientKind::LinearCorner(sx, sy) => (sx as f32 * h).atan2(-(sy as f32) * w),
                _ => unreachable!(),
            };
            let (dx, dy) = (a.sin(), -a.cos());
            let len = (w * a.sin()).abs() + (h * a.cos()).abs();
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let mut offsets = stop_offsets(&g.stops, len);
            let (from, to, spread) = repeat_span(&mut offsets, g.repeating);
            let start = (cx - dx * len / 2.0, cy - dy * len / 2.0);
            Some(
                LinearGradient {
                    x1: start.0 + dx * len * from,
                    y1: start.1 + dy * len * from,
                    x2: start.0 + dx * len * to,
                    y2: start.1 + dy * len * to,
                    transform: Transform::identity(),
                    spread_method: spread,
                    stops: krilla_stops(&offsets),
                    anti_alias: false,
                }
                .into(),
            )
        }
        GradientKind::Radial { circle, at } => {
            let (cx, cy) = (x + style::resolve(at.0, w), y + style::resolve(at.1, h));
            // farthest-corner
            let (fx, fy) = ((cx - x).max(x + w - cx), (cy - y).max(y + h - cy));
            let (rx, ry) = if circle {
                let r = fx.hypot(fy);
                (r, r)
            } else {
                (fx * std::f32::consts::SQRT_2, fy * std::f32::consts::SQRT_2)
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let mut offsets = stop_offsets(&g.stops, rx);
            let (from, to, spread) = repeat_span(&mut offsets, g.repeating);
            let k = ry / rx;
            Some(
                RadialGradient {
                    fx: cx,
                    fy: cy,
                    fr: rx * from,
                    cx,
                    cy,
                    cr: rx * to,
                    // ellipse: the circle squashed vertically around the center
                    transform: Transform::from_row(1.0, 0.0, 0.0, k, 0.0, cy * (1.0 - k)),
                    spread_method: spread,
                    stops: krilla_stops(&offsets),
                    anti_alias: false,
                }
                .into(),
            )
        }
    }
}

fn fill(c: Rgba) -> Fill {
    Fill {
        paint: rgb::Color::new(c.0, c.1, c.2).into(),
        opacity: NormalizedF32::new(c.3 as f32 / 255.0).unwrap_or(NormalizedF32::ONE),
        rule: FillRule::NonZero,
    }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.push_rect(Rect::from_xywh(x, y, w.max(0.0), h.max(0.0))?);
    pb.finish()
}

/// Rounded rectangle, radii clockwise from top-left, scaled down when they overlap (like CSS).
fn rounded_path(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> Option<Path> {
    if r.iter().all(|r| *r <= 0.0) {
        return rect_path(x, y, w, h);
    }
    let scale = [w / (r[0] + r[1]), h / (r[1] + r[2]), w / (r[2] + r[3]), h / (r[3] + r[0])]
        .into_iter()
        .filter(|s| s.is_finite())
        .fold(1.0f32, f32::min);
    let r = r.map(|v| (v * scale).max(0.0));
    const K: f32 = 0.552_284_8; // cubic approximation of a quarter circle
    let mut pb = PathBuilder::new();
    pb.move_to(x + r[0], y);
    pb.line_to(x + w - r[1], y);
    pb.cubic_to(x + w - r[1] * (1.0 - K), y, x + w, y + r[1] * (1.0 - K), x + w, y + r[1]);
    pb.line_to(x + w, y + h - r[2]);
    pb.cubic_to(x + w, y + h - r[2] * (1.0 - K), x + w - r[2] * (1.0 - K), y + h, x + w - r[2], y + h);
    pb.line_to(x + r[3], y + h);
    pb.cubic_to(x + r[3] * (1.0 - K), y + h, x, y + h - r[3] * (1.0 - K), x, y + h - r[3]);
    pb.line_to(x, y + r[0]);
    pb.cubic_to(x, y + r[0] * (1.0 - K), x + r[0] * (1.0 - K), y, x + r[0], y);
    pb.close();
    pb.finish()
}

fn dash(style: BorderStyle, w: f32) -> Option<StrokeDash> {
    match style {
        BorderStyle::Dashed => Some(StrokeDash { array: vec![w * 3.0, w * 3.0], offset: 0.0 }),
        BorderStyle::Dotted => Some(StrokeDash { array: vec![0.0, w * 2.0], offset: 0.0 }),
        _ => None,
    }
}

fn stroke(c: Rgba, w: f32, style: BorderStyle) -> Stroke {
    Stroke {
        paint: rgb::Color::new(c.0, c.1, c.2).into(),
        width: w,
        opacity: NormalizedF32::new(c.3 as f32 / 255.0).unwrap_or(NormalizedF32::ONE),
        line_cap: if style == BorderStyle::Dotted { LineCap::Round } else { LineCap::Butt },
        dash: dash(style, w),
        ..Default::default()
    }
}

/// Coverage (0..1) of a rounded rectangle at a point, antialiased over 1/k pt.
fn rrect_coverage(px: f32, py: f32, (x, y, w, h): (f32, f32, f32, f32), r: [f32; 4], k: f32) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let (hw, hh) = (w / 2.0, h / 2.0);
    let (dx, dy) = (px - x - hw, py - y - hh);
    let rr = match (dx < 0.0, dy < 0.0) {
        (true, true) => r[0],
        (false, true) => r[1],
        (false, false) => r[2],
        (true, false) => r[3],
    }
    .min(hw)
    .min(hh)
    .max(0.0);
    let (qx, qy) = (dx.abs() - (hw - rr), dy.abs() - (hh - rr));
    let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rr;
    (0.5 - d * k).clamp(0.0, 1.0)
}

/// A blurred shape as an RGBA bitmap covering (x0, y0, w, h) in pt: `coverage` gives the shape,
/// the CSS blur radius is two standard deviations (three box blurs approximate the gaussian).
fn blurred(x0: f32, y0: f32, w: f32, h: f32, blur: f32, c: Rgba, coverage: impl Fn(f32, f32, f32) -> f32) -> Option<(krilla::image::Image, (f32, f32))> {
    // ~2 px per pt, at most ~1.5 M pixels
    let k = (2.0f32).min((1_500_000.0 / (w * h).max(1.0)).sqrt()).max(0.05);
    let (pw, ph) = ((w * k).ceil() as usize, (h * k).ceil() as usize);
    if pw == 0 || ph == 0 || pw > 8192 || ph > 8192 {
        return None;
    }
    let mut a = vec![0f32; pw * ph];
    for j in 0..ph {
        let py = y0 + (j as f32 + 0.5) / k;
        for i in 0..pw {
            a[j * pw + i] = coverage(x0 + (i as f32 + 0.5) / k, py, k);
        }
    }
    if blur > 0.0 {
        // box size per pass for a gaussian of sigma (in px)
        let sp = blur / 2.0 * k;
        let box_r = (((12.0 * sp * sp / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
        let mut tmp = vec![0f32; pw * ph];
        for _ in 0..3 {
            box_blur(&a, &mut tmp, pw, ph, box_r, true);
            box_blur(&tmp, &mut a, pw, ph, box_r, false);
        }
    }
    let alpha = c.3 as f32;
    let mut rgba = Vec::with_capacity(pw * ph * 4);
    for v in a {
        rgba.extend_from_slice(&[c.0, c.1, c.2, (v * alpha).round().clamp(0.0, 255.0) as u8]);
    }
    Some((krilla::image::Image::from_rgba8(rgba, pw as u32, ph as u32), (pw as f32 / k, ph as f32 / k)))
}

fn draw_bitmap(s: &mut Surface, image: krilla::image::Image, x: f32, y: f32, (w, h): (f32, f32)) {
    s.push_transform(&Transform::from_translate(x, y));
    if let Some(size) = Size::from_wh(w, h) {
        s.draw_image(image, size);
    }
    s.pop();
}

/// Outer box shadows, the last one at the bottom (as in CSS).
fn draw_shadows(s: &mut Surface, shadows: &[Shadow], radius: [f32; 4], x: f32, y: f32, w: f32, h: f32) {
    for sh in shadows.iter().rev().filter(|sh| !sh.inset && sh.color.3 > 0) {
        let (sx, sy, sw, sh_h) = (x + sh.dx - sh.spread, y + sh.dy - sh.spread, w + 2.0 * sh.spread, h + 2.0 * sh.spread);
        if sw <= 0.0 || sh_h <= 0.0 {
            continue;
        }
        let r = radius.map(|r| if r > 0.0 { (r + sh.spread).max(0.0) } else { 0.0 });
        if sh.blur <= 0.0 {
            if let Some(p) = rounded_path(sx, sy, sw, sh_h, r) {
                s.set_stroke(None);
                s.set_fill(Some(fill(sh.color)));
                s.draw_path(&p);
            }
        } else {
            let pad = 1.5 * sh.blur;
            let shape = (sx, sy, sw, sh_h);
            if let Some((image, size)) = blurred(sx - pad, sy - pad, sw + 2.0 * pad, sh_h + 2.0 * pad, sh.blur, sh.color, |px, py, k| rrect_coverage(px, py, shape, r, k)) {
                draw_bitmap(s, image, sx - pad, sy - pad, size);
            }
        }
    }
}

/// Inset shadows, inside the padding box: everything but the (offset, shrunk) box casts the shadow.
fn draw_inset_shadows(s: &mut Surface, shadows: &[Shadow], (x, y, w, h): (f32, f32, f32, f32), radius: [f32; 4]) {
    let Some(clip) = rounded_path(x, y, w, h, radius) else { return };
    s.push_clip_path(&clip, &FillRule::NonZero);
    for sh in shadows.iter().rev().filter(|sh| sh.inset && sh.color.3 > 0) {
        let hole = (x + sh.dx + sh.spread, y + sh.dy + sh.spread, (w - 2.0 * sh.spread).max(0.0), (h - 2.0 * sh.spread).max(0.0));
        let r = radius.map(|r| (r - sh.spread).max(0.0));
        let pad = 1.5 * sh.blur + 1.0;
        if let Some((image, size)) = blurred(x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad, sh.blur, sh.color, |px, py, k| 1.0 - rrect_coverage(px, py, hole, r, k)) {
            draw_bitmap(s, image, x - pad, y - pad, size);
        }
    }
    s.pop();
}

/// One running-sum box blur pass of radius `r`, along rows or columns (edges clamped to 0).
fn box_blur(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut sum = 0.0;
        for i in 0..r.min(len) {
            sum += src[at(line, i)];
        }
        for i in 0..len {
            if i + r < len {
                sum += src[at(line, i + r)];
            }
            if i > r {
                sum -= src[at(line, i - r - 1)];
            }
            dst[at(line, i)] = sum * norm;
        }
    }
}

/// The border / padding / content box of a box, with its corner radii.
fn area(which: BoxArea, (x, y, w, h): (f32, f32, f32, f32), d: &Deco, pad: [f32; 4]) -> ((f32, f32, f32, f32), [f32; 4]) {
    let [bt, br, bb, bl] = d.border_width;
    let inset = |rect: (f32, f32, f32, f32), [t, r, b, l]: [f32; 4]| (rect.0 + l, rect.1 + t, (rect.2 - l - r).max(0.0), (rect.3 - t - b).max(0.0));
    let shrink = |radius: [f32; 4], [t, r, b, l]: [f32; 4]| [radius[0] - t.max(l), radius[1] - t.max(r), radius[2] - b.max(r), radius[3] - b.max(l)].map(|v| v.max(0.0));
    let border = (x, y, w, h);
    match which {
        BoxArea::Border => (border, d.radius),
        BoxArea::Padding => (inset(border, d.border_width), shrink(d.radius, d.border_width)),
        BoxArea::Content => {
            let total = [bt + pad[0], br + pad[1], bb + pad[2], bl + pad[3]];
            (inset(border, total), shrink(d.radius, total))
        }
    }
}

/// One background layer: tiles over its origin box, clipped to its clip box.
fn draw_layer(s: &mut Surface, layer: &BgLayer, d: &Deco, rect: (f32, f32, f32, f32), pad: [f32; 4], window: (f32, f32)) {
    let ((ax, ay, aw, ah), _) = area(layer.origin, rect, d, pad);
    let ((cx, cy, cw, ch), cr) = area(layer.clip, rect, d, pad);
    if aw <= 0.0 || ah <= 0.0 || cw <= 0.0 || ch <= 0.0 {
        return;
    }
    // a gradient has no natural size: it fills the positioning area
    let (nw, nh) = match &layer.paint {
        LayerPaint::Image(_, natural) => *natural,
        LayerPaint::Gradient(_) => (aw, ah),
    };
    if nw <= 0.0 || nh <= 0.0 {
        return;
    }
    let (tw, th) = match layer.size {
        BgSize::Auto => (nw, nh),
        BgSize::Cover => {
            let k = (aw / nw).max(ah / nh);
            (nw * k, nh * k)
        }
        BgSize::Contain => {
            let k = (aw / nw).min(ah / nh);
            (nw * k, nh * k)
        }
        BgSize::Size(a, b) => match (a, b) {
            (Len::Auto, Len::Auto) => (nw, nh),
            (a, Len::Auto) => {
                let tw = style::resolve(a, aw);
                (tw, if matches!(layer.paint, LayerPaint::Gradient(_)) { ah } else { tw * nh / nw })
            }
            (Len::Auto, b) => {
                let th = style::resolve(b, ah);
                (if matches!(layer.paint, LayerPaint::Gradient(_)) { aw } else { th * nw / nh }, th)
            }
            (a, b) => (style::resolve(a, aw), style::resolve(b, ah)),
        },
    };
    if tw < 0.5 || th < 0.5 {
        return;
    }
    // percentages position the image's point at the area's point (CSS)
    let px = ax + style::resolve(layer.position.0, aw - tw);
    let py = ay + style::resolve(layer.position.1, ah - th);
    let tiles = |p: f32, t: f32, from: f32, to: f32, repeat: bool| -> Vec<f32> {
        if repeat {
            let first = p - ((p - from) / t).ceil() * t;
            (0..).map(|i| first + i as f32 * t).take_while(|v| *v < to).take(512).collect()
        } else {
            vec![p]
        }
    };
    let xs = tiles(px, tw, cx, cx + cw, layer.repeat.0);
    let ys = tiles(py, th, cy, cy + ch, layer.repeat.1);
    let Some(clip) = rounded_path(cx, cy, cw, ch, cr) else { return };
    s.push_clip_path(&clip, &FillRule::NonZero);
    let mut drawn = 0;
    for &ty in ys.iter().filter(|ty| **ty < window.1 && **ty + th > window.0) {
        for &tx in &xs {
            if drawn >= 4096 {
                break;
            }
            drawn += 1;
            match &layer.paint {
                LayerPaint::Gradient(g) => {
                    if let (Some(paint), Some(p)) = (gradient_paint(g, tx, ty, tw, th), rect_path(tx, ty, tw, th)) {
                        s.set_stroke(None);
                        s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                        s.draw_path(&p);
                    }
                }
                LayerPaint::Image(kind, _) => {
                    let Some(size) = Size::from_wh(tw, th) else { continue };
                    s.push_transform(&Transform::from_translate(tx, ty));
                    match kind {
                        ImageKind::Raster(i) => s.draw_image(i.clone(), size),
                        ImageKind::Svg(tree) => {
                            s.draw_svg(tree, size, SvgSettings::default());
                        }
                    }
                    s.pop();
                }
            }
        }
    }
    s.pop();
}

/// Shadows, background (color, then layers bottom to top), inset shadows, borders.
fn draw_box(s: &mut Surface, d: &Deco, x: f32, y: f32, w: f32, h: f32, pad: [f32; 4], window: (f32, f32)) {
    let rect = (x, y, w, h);
    if let Some(shadows) = &d.shadows {
        draw_shadows(s, shadows, d.radius, x, y, w, h);
    }
    if let Some(bg) = d.background {
        let ((cx, cy, cw, ch), cr) = area(d.color_clip, rect, d, pad);
        if let Some(p) = rounded_path(cx, cy, cw, ch, cr) {
            s.set_stroke(None);
            s.set_fill(Some(fill(bg)));
            s.draw_path(&p);
        }
    }
    for layer in d.layers.iter().rev() {
        draw_layer(s, layer, d, rect, pad, window);
    }
    if let Some(shadows) = d.shadows.as_ref().filter(|sh| sh.iter().any(|s| s.inset)) {
        let (inner, r) = area(BoxArea::Padding, rect, d, pad);
        draw_inset_shadows(s, shadows, inner, r);
    }
    let bw = d.border_width;
    if bw.iter().all(|w| *w <= 0.0) {
        return;
    }
    let uniform = bw.iter().all(|w| *w == bw[0]) && d.border_color.iter().all(|c| *c == d.border_color[0]) && d.border_style.iter().all(|st| *st == d.border_style[0]);
    s.set_fill(None);
    if uniform && d.border_style[0] != Some(BorderStyle::Double) {
        // one stroke along the middle of the border
        let (w0, st) = (bw[0], d.border_style[0].unwrap_or(BorderStyle::Solid));
        let r = d.radius.map(|r| (r - w0 / 2.0).max(0.0));
        if let Some(p) = rounded_path(x + w0 / 2.0, y + w0 / 2.0, w - w0, h - w0, r) {
            s.set_stroke(Some(stroke(d.border_color[0], w0, st)));
            s.draw_path(&p);
        }
        s.set_stroke(None);
        return;
    }
    // per side: solid sides are filled trapezoids (mitred corners), others stroked lines
    let (t, r, b, l) = (bw[0], bw[1], bw[2], bw[3]);
    let outer = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let inner = [(x + l, y + t), (x + w - r, y + t), (x + w - r, y + h - b), (x + l, y + h - b)];
    for side in 0..4 {
        let (Some(st), width) = (d.border_style[side], bw[side]) else { continue };
        if width <= 0.0 {
            continue;
        }
        let (a, bb) = (side, (side + 1) % 4);
        match st {
            BorderStyle::Solid | BorderStyle::Double => {
                s.set_stroke(None);
                s.set_fill(Some(fill(d.border_color[side])));
                let mut pb = PathBuilder::new();
                pb.move_to(outer[a].0, outer[a].1);
                pb.line_to(outer[bb].0, outer[bb].1);
                pb.line_to(inner[bb].0, inner[bb].1);
                pb.line_to(inner[a].0, inner[a].1);
                pb.close();
                if let Some(p) = pb.finish() {
                    s.draw_path(&p);
                }
            }
            _ => {
                s.set_fill(None);
                s.set_stroke(Some(stroke(d.border_color[side], width, st)));
                let mid = |o: (f32, f32), i: (f32, f32)| ((o.0 + i.0) / 2.0, (o.1 + i.1) / 2.0);
                let (p0, p1) = (mid(outer[a], inner[a]), mid(outer[bb], inner[bb]));
                let mut pb = PathBuilder::new();
                pb.move_to(p0.0, p0.1);
                pb.line_to(p1.0, p1.1);
                if let Some(p) = pb.finish() {
                    s.draw_path(&p);
                }
            }
        }
    }
    s.set_fill(None);
    s.set_stroke(None);
}

/// Clusters of the glyph run, in visual order, so every glyph knows its text range (selectable text).
fn glyph_ranges(gr: &parley::GlyphRun<'_, Brush>, nth: usize) -> Vec<std::ops::Range<usize>> {
    // the line's run is split into glyph runs at style changes: take the nth segment of this style
    let mut segments: Vec<Vec<std::ops::Range<usize>>> = Vec::new();
    let mut prev = None;
    for c in gr.run().visual_clusters() {
        if prev != Some(c.style_index()) {
            segments.push(Vec::new());
            prev = Some(c.style_index());
        }
        if c.style_index() == gr.style_index() {
            let r = c.text_range();
            segments.last_mut().unwrap().extend(c.glyphs().map(|_| r.clone()));
        }
    }
    segments.into_iter().filter(|s| !s.is_empty()).nth(nth).unwrap_or_default()
}

pub struct Link {
    pub rect: (f32, f32, f32, f32),
    pub href: u32,
}

fn draw_text(s: &mut Surface, tree: &Tree, t: &TextLeaf, x: f32, y: f32, window: (f32, f32), links: &mut Vec<Link>) {
    for line in t.layout.lines() {
        let m = line.metrics();
        if y + m.block_max_coord <= window.0 || y + m.block_min_coord >= window.1 {
            continue;
        }
        let mut seen: HashMap<(usize, u16), usize> = HashMap::new();
        for item in line.items() {
            let gr = match item {
                parley::PositionedLayoutItem::GlyphRun(gr) => gr,
                parley::PositionedLayoutItem::InlineBox(ib) => {
                    // inline-block / inline image: its own subtree, drawn at the box's place in the line
                    let node = NodeId::from(ib.id);
                    if let Ok(l) = tree.taffy.layout(node) {
                        let (ox, oy) = (x + ib.x + l.margin.left - l.location.x, y + ib.y + l.margin.top - l.location.y);
                        draw(s, tree, node, ox, oy, (f32::NEG_INFINITY, f32::INFINITY), links);
                    }
                    continue;
                }
            };
            let brush = &gr.style().brush;
            let run = gr.run();
            let size = run.font_size();
            let (gx, gy) = (x + gr.offset(), y + gr.baseline());
            if let Some(bg) = brush.background {
                if let Some(p) = rect_path(gx, y + m.block_min_coord, gr.advance(), m.block_max_coord - m.block_min_coord) {
                    s.set_stroke(None);
                    s.set_fill(Some(fill(bg)));
                    s.draw_path(&p);
                }
            }
            if brush.link > 0 {
                links.push(Link { rect: (gx, y + m.block_min_coord, gr.advance(), m.block_max_coord - m.block_min_coord), href: brush.link });
            }
            let Some(font) = krilla_font(run.font()) else { continue };
            let nth = seen.entry((run.index(), gr.style_index())).or_insert(0);
            let ranges = glyph_ranges(&gr, *nth);
            *nth += 1;
            let whole = run.text_range();
            let glyphs: Vec<KrillaGlyph> = gr
                .glyphs()
                .enumerate()
                .map(|(i, g)| KrillaGlyph {
                    glyph_id: GlyphId::new(g.id),
                    text_range: ranges.get(i).cloned().unwrap_or_else(|| whole.clone()),
                    x_advance: g.advance / size,
                    x_offset: g.x / size,
                    y_offset: -g.y / size,
                    y_advance: 0.0,
                    location: None,
                })
                .collect();
            s.set_stroke(None);
            s.set_fill(Some(fill(brush.color)));
            s.draw_glyphs(Point::from_xy(gx, gy), &glyphs, font, &t.text, size, false);
            let fm = run.font_metrics();
            let style = gr.style();
            for (deco, offset, thickness) in [
                (&style.underline, fm.underline_offset, fm.underline_size),
                (&style.strikethrough, fm.strikethrough_offset, fm.strikethrough_size),
            ] {
                if let Some(d) = deco {
                    let off = d.offset.unwrap_or(offset);
                    let th = d.size.unwrap_or(thickness).max(0.5);
                    if let Some(p) = rect_path(gx, gy - off, gr.advance(), th) {
                        s.set_fill(Some(fill(d.brush.color)));
                        s.draw_path(&p);
                    }
                }
            }
        }
    }
}

fn draw_image(s: &mut Surface, img: &ImageLeaf, x: f32, y: f32, w: f32, h: f32) {
    let (nw, nh) = img.natural;
    if w <= 0.0 || h <= 0.0 || nw <= 0.0 || nh <= 0.0 {
        return;
    }
    // object-fit: the drawn size inside the content box, centered
    let (dw, dh) = match img.fit {
        ObjectFit::Fill => (w, h),
        ObjectFit::Contain => {
            let k = (w / nw).min(h / nh);
            (nw * k, nh * k)
        }
        ObjectFit::Cover => {
            let k = (w / nw).max(h / nh);
            (nw * k, nh * k)
        }
        ObjectFit::None => (nw, nh),
    };
    let clip = img.radius.iter().any(|r| *r > 0.0) || dw > w + 0.01 || dh > h + 0.01;
    if clip {
        if let Some(p) = rounded_path(x, y, w, h, img.radius) {
            s.push_clip_path(&p, &FillRule::NonZero);
        }
    }
    s.push_transform(&Transform::from_translate(x + (w - dw) / 2.0, y + (h - dh) / 2.0));
    if let Some(size) = Size::from_wh(dw, dh) {
        match &img.kind {
            ImageKind::Raster(i) => s.draw_image(i.clone(), size),
            ImageKind::Svg(tree) => {
                s.draw_svg(tree, size, SvgSettings::default());
            }
        }
    }
    s.pop();
    if clip {
        s.pop();
    }
}

/// Draw the subtree of `n` whose parent's border box is at (ox, oy); only what crosses `window` (y range).
pub fn draw(s: &mut Surface, tree: &Tree, n: NodeId, ox: f32, oy: f32, window: (f32, f32), links: &mut Vec<Link>) {
    let Ok(l) = tree.taffy.layout(n) else { return };
    let (x, y, w, h) = (ox + l.location.x, oy + l.location.y, l.size.width, l.size.height);
    let d = tree.deco.get(&n);
    let mut pushed = 0;
    if let Some(d) = d {
        if d.opacity < 1.0 {
            s.push_opacity(NormalizedF32::new(d.opacity).unwrap_or(NormalizedF32::ONE));
            pushed += 1;
        }
        // transform around transform-origin (default: the center of the border box)
        if let Some((t, origin, em)) = &d.transform {
            if let Some(m) = style::transform_matrix(t, w, h, *em) {
                let (ox, oy) = origin.as_deref().map_or((Len::Pct(50.0), Len::Pct(50.0)), |o| style::position(o, *em, *em));
                let (ox, oy) = (x + style::resolve(ox, w), y + style::resolve(oy, h));
                s.push_transform(&Transform::from_translate(ox, oy));
                s.push_transform(&Transform::from_row(m[0], m[1], m[2], m[3], m[4], m[5]));
                s.push_transform(&Transform::from_translate(-ox, -oy));
                pushed += 3;
            }
        }
        // shadows reach outside the box
        let reach = d.shadows.as_ref().map_or(0.0, |sh| sh.iter().map(|s| s.dy.abs() + s.spread.max(0.0) + 1.5 * s.blur).fold(0.0, f32::max));
        if y - reach < window.1 && y + h + reach > window.0 {
            draw_box(s, d, x, y, w, h, [l.padding.top, l.padding.right, l.padding.bottom, l.padding.left], window);
        }
        if d.clip {
            if let Some(p) = rounded_path(x, y, w, h, d.radius) {
                s.push_clip_path(&p, &FillRule::NonZero);
                pushed += 1;
            }
        }
    }
    let (cx, cy) = (x + l.padding.left + l.border.left, y + l.padding.top + l.border.top);
    match tree.taffy.get_node_context(n) {
        Some(Leaf::Text(t)) => draw_text(s, tree, t, cx, cy, window, links),
        Some(Leaf::Image(img)) if y < window.1 && y + h > window.0 => {
            draw_image(s, img, cx, cy, l.content_box_width(), l.content_box_height());
        }
        _ => {}
    }
    for c in tree.taffy.child_ids(n) {
        draw(s, tree, c, x, y, window, links);
    }
    for _ in 0..pushed {
        s.pop();
    }
}
