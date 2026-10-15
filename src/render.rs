//! Pagination and PDF drawing (krilla).
//!
//! The document is laid out once as one tall column of the content width, then cut into pages.
//! Cuts never split a text line, an image or a `break-inside: avoid` box shorter than a page
//! (they are moved to the next page); boxes that span a cut are drawn on both pages, clipped.

use crate::layout::{Brush, Deco, ImageKind, ImageLeaf, Leaf, TextLeaf, Tree};
use crate::style::{BorderStyle, ObjectFit, Rgba};
use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Point, Rect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, Stroke, StrokeDash};
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
}

pub fn collect_breaks(tree: &Tree) -> Breaks {
    let mut b = Breaks::default();
    walk_breaks(tree, tree.root, 0.0, &mut b);
    b.bottom = b.atoms.iter().map(|a| a.1).fold(b.bottom, f32::max);
    b.atoms.sort_by(|a, b| a.0.total_cmp(&b.0));
    b.forced.sort_by(f32::total_cmp);
    b
}

fn walk_breaks(tree: &Tree, n: NodeId, oy: f32, b: &mut Breaks) {
    let Ok(l) = tree.taffy.layout(n) else { return };
    let y = oy + l.location.y;
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
        if d.background.is_some() || d.border_width.iter().any(|w| *w > 0.0) {
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
    for c in tree.taffy.child_ids(n) {
        walk_breaks(tree, c, y, b);
    }
}

/// Page cuts [start, end) of the column, pages of `page` high. Always at least one page.
pub fn paginate(page: f32, breaks: &Breaks) -> Vec<(f32, f32)> {
    const EPS: f32 = 0.01;
    let mut pages = Vec::new();
    let mut start = 0.0f32;
    loop {
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
        if cut <= start + EPS {
            cut = limit;
        }
        pages.push((start, cut));
        start = cut;
        // stop when nothing visible remains below the cut (forced breaks at the very end included)
        if breaks.bottom <= start + EPS || pages.len() >= 10_000 {
            break;
        }
    }
    pages
}

thread_local! {
    /// krilla fonts by (blob id, face index): parsed once per thread
    static FONTS: RefCell<HashMap<(u64, u32), Option<Font>>> = RefCell::new(HashMap::new());
}

fn krilla_font(f: &parley::FontData) -> Option<Font> {
    FONTS.with(|c| {
        c.borrow_mut()
            .entry((f.data.id(), f.index))
            .or_insert_with(|| {
                let blob = f.data.clone();
                Font::new(krilla::Data::from(std::sync::Arc::new(blob) as std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>), f.index)
            })
            .clone()
    })
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

fn draw_box(s: &mut Surface, d: &Deco, x: f32, y: f32, w: f32, h: f32) {
    if let Some(bg) = d.background {
        if let Some(p) = rounded_path(x, y, w, h, d.radius) {
            s.set_stroke(None);
            s.set_fill(Some(fill(bg)));
            s.draw_path(&p);
        }
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

fn draw_text(s: &mut Surface, t: &TextLeaf, x: f32, y: f32, window: (f32, f32), links: &mut Vec<Link>) {
    for line in t.layout.lines() {
        let m = line.metrics();
        if y + m.block_max_coord <= window.0 || y + m.block_min_coord >= window.1 {
            continue;
        }
        let mut seen: HashMap<(usize, u16), usize> = HashMap::new();
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(gr) = item else { continue };
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
        if y < window.1 && y + h > window.0 {
            draw_box(s, d, x, y, w, h);
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
        Some(Leaf::Text(t)) => draw_text(s, t, cx, cy, window, links),
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
