//! Computed style: cascade result for one element, inheritance, units, shorthands.
//! 1px = 1pt, like react-pdf (so documents render at the same size as with pdfjs).

use crate::css::Decl;
use crate::fonts;
use std::rc::Rc;
use taffy::prelude::TaffyZero;
use taffy::{AlignContent, AlignItems, Dimension, FlexDirection, FlexWrap, LengthPercentage, LengthPercentageAuto};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Auto,
    Px(f32),
    Pct(f32),
}

impl Len {
    pub fn dim(self) -> Dimension {
        match self {
            Len::Auto => Dimension::auto(),
            Len::Px(v) => Dimension::length(v),
            Len::Pct(v) => Dimension::percent(v / 100.0),
        }
    }
    pub fn lpa(self) -> LengthPercentageAuto {
        match self {
            Len::Auto => LengthPercentageAuto::auto(),
            Len::Px(v) => LengthPercentageAuto::length(v),
            Len::Pct(v) => LengthPercentageAuto::percent(v / 100.0),
        }
    }
    pub fn lp(self) -> LengthPercentage {
        match self {
            Len::Auto => LengthPercentage::ZERO,
            Len::Px(v) => LengthPercentage::length(v),
            Len::Pct(v) => LengthPercentage::percent(v / 100.0),
        }
    }
    pub fn px(self) -> Option<f32> {
        match self {
            Len::Px(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

pub fn color(v: &str) -> Option<Rgba> {
    let c = csscolorparser::parse(v.trim()).ok()?.to_rgba8();
    Some(Rgba(c[0], c[1], c[2], c[3]))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Display {
    Inline,
    Block,
    Flex,
    Grid,
    ListItem,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    Contents,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAlign {
    Start,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    Factor(f32),
    Px(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    None,
    Upper,
    Lower,
    Capitalize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjectFit {
    Contain,
    Cover,
    Fill,
    None,
}

#[derive(Clone, Debug)]
pub struct Style {
    // inherited
    pub font_family: Rc<str>,
    pub font_size: f32,
    pub font_weight: f32,
    pub italic: bool,
    pub color: Rgba,
    pub text_align: TextAlign,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub transform: Transform,
    pub white_space: WhiteSpace,
    pub list_style: Rc<str>,
    pub border_spacing: f32,
    // text-decoration is not inherited in CSS but propagates to descendants' text: same effect here
    pub underline: bool,
    pub strike: bool,
    // not inherited
    pub display: Display,
    pub position_absolute: bool,
    pub float: taffy::Float,
    pub clear: taffy::Clear,
    pub border_box: bool,
    pub size: [Len; 2],
    pub min_size: [Len; 2],
    pub max_size: [Len; 2],
    /// top, right, bottom, left
    pub margin: [Len; 4],
    pub padding: [Len; 4],
    pub inset: [Len; 4],
    pub border_width: [f32; 4],
    pub border_style: [BorderStyle; 4],
    /// None = currentColor
    pub border_color: [Option<Rgba>; 4],
    /// top-left, top-right, bottom-right, bottom-left
    pub radius: [f32; 4],
    pub background: Option<Rgba>,
    pub opacity: f32,
    pub overflow_hidden: bool,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Len,
    pub justify_content: Option<AlignContent>,
    pub align_content: Option<AlignContent>,
    pub align_items: Option<AlignItems>,
    pub align_self: Option<AlignItems>,
    pub justify_items: Option<AlignItems>,
    pub justify_self: Option<AlignItems>,
    /// row, column
    pub gap: [Len; 2],
    pub order: i32,
    pub grid_template_columns: Option<Rc<str>>,
    pub grid_template_rows: Option<Rc<str>>,
    pub grid_auto_flow: Option<Rc<str>>,
    pub grid_column: Option<Rc<str>>,
    pub grid_row: Option<Rc<str>>,
    pub aspect_ratio: Option<f32>,
    pub vertical_align: VAlign,
    pub object_fit: ObjectFit,
    pub border_collapse: bool,
    pub break_before: bool,
    pub break_after: bool,
    pub break_inside_avoid: bool,
}

impl Style {
    /// Initial style of the root: `font` / `font_size` are the document defaults.
    pub fn root(font: &str, font_size: f32) -> Style {
        Style {
            font_family: fonts::resolve_stack(font).unwrap_or_else(|| fonts::resolve_stack("sans-serif").unwrap_or_default()).into(),
            font_size,
            font_weight: 400.0,
            italic: false,
            color: Rgba(0, 0, 0, 255),
            text_align: TextAlign::Start,
            line_height: LineHeight::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            transform: Transform::None,
            white_space: WhiteSpace::Normal,
            list_style: "disc".into(),
            border_spacing: 2.0,
            underline: false,
            strike: false,
            display: Display::Block,
            position_absolute: false,
            float: taffy::Float::None,
            clear: taffy::Clear::None,
            border_box: false,
            size: [Len::Auto; 2],
            min_size: [Len::Auto; 2],
            max_size: [Len::Auto; 2],
            margin: [Len::Px(0.0); 4],
            padding: [Len::Px(0.0); 4],
            inset: [Len::Auto; 4],
            border_width: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            border_color: [None; 4],
            radius: [0.0; 4],
            background: None,
            opacity: 1.0,
            overflow_hidden: false,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Len::Auto,
            justify_content: None,
            align_content: None,
            align_items: None,
            align_self: None,
            justify_items: None,
            justify_self: None,
            gap: [Len::Px(0.0); 2],
            order: 0,
            grid_template_columns: None,
            grid_template_rows: None,
            grid_auto_flow: None,
            grid_column: None,
            grid_row: None,
            aspect_ratio: None,
            vertical_align: VAlign::Baseline,
            object_fit: ObjectFit::Fill,
            border_collapse: false,
            break_before: false,
            break_after: false,
            break_inside_avoid: false,
        }
    }

    /// Child style: inherited properties from `self`, the rest initial.
    pub fn inherit(&self) -> Style {
        let initial = Style::root("", self.font_size);
        Style {
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            font_weight: self.font_weight,
            italic: self.italic,
            color: self.color,
            text_align: self.text_align,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            word_spacing: self.word_spacing,
            transform: self.transform,
            white_space: self.white_space,
            list_style: self.list_style.clone(),
            border_spacing: self.border_spacing,
            underline: self.underline,
            strike: self.strike,
            display: Display::Inline,
            ..initial
        }
    }

    /// Apply the winning declarations (in cascade order) on top of `self.inherit()`.
    pub fn compute(parent: &Style, decls: &[&Decl], root_font_size: f32) -> Style {
        let mut s = parent.inherit();
        // font-size first: em units of every other property depend on it
        for d in decls.iter().filter(|d| d.prop == "font-size" || d.prop == "font") {
            if d.prop == "font" {
                apply_font_shorthand(&mut s, &d.value, parent.font_size, root_font_size);
            } else if let Some(fs) = font_size(&d.value, parent.font_size, root_font_size) {
                s.font_size = fs;
            }
        }
        let em = s.font_size;
        for d in decls {
            apply(&mut s, parent, &d.prop, &d.value, em, root_font_size);
        }
        s
    }

    pub fn line_height_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Normal => self.font_size * 1.15,
            LineHeight::Factor(f) => self.font_size * f,
            LineHeight::Px(v) => v,
        }
    }
}

/// `12px`, `1.5em`, `50%`, `auto`, `0` -> Len (em relative to `em`, rem to `rem`)
pub fn len(v: &str, em: f32, rem: f32) -> Option<Len> {
    let v = v.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(Len::Auto);
    }
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f32 = num.parse().ok()?;
    let px = match unit.trim() {
        "" if n == 0.0 => 0.0,
        "px" | "pt" | "" => n, // unitless non-zero: lenient, like react-pdf
        "%" => return Some(Len::Pct(n)),
        "em" => n * em,
        "rem" => n * rem,
        "pc" => n * 12.0,
        "in" => n * 72.0,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "q" => n * 72.0 / 101.6,
        "ex" | "ch" => n * em * 0.5,
        _ => return None,
    };
    Some(Len::Px(px))
}

fn font_size(v: &str, parent: f32, rem: f32) -> Option<f32> {
    let v = v.trim().to_ascii_lowercase();
    let kw = match v.as_str() {
        "xx-small" => Some(9.0 / 16.0),
        "x-small" => Some(10.0 / 16.0),
        "small" => Some(13.0 / 16.0),
        "medium" => Some(1.0),
        "large" => Some(18.0 / 16.0),
        "x-large" => Some(1.5),
        "xx-large" => Some(2.0),
        "smaller" => return Some(parent / 1.2),
        "larger" => return Some(parent * 1.2),
        _ => None,
    };
    if let Some(k) = kw {
        return Some(rem * k);
    }
    match len(&v, parent, rem)? {
        Len::Px(px) => Some(px),
        Len::Pct(p) => Some(parent * p / 100.0),
        Len::Auto => None,
    }
}

/// 1 to 4 values, CSS order: top right bottom left
fn four<T: Copy>(vals: &[T]) -> Option<[T; 4]> {
    Some(match *vals {
        [a] => [a, a, a, a],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return None,
    })
}

fn lens(v: &str, em: f32, rem: f32) -> Option<Vec<Len>> {
    v.split_whitespace().map(|p| len(p, em, rem)).collect()
}

fn border_width(v: &str, em: f32, rem: f32) -> Option<f32> {
    match v.to_ascii_lowercase().as_str() {
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        v => len(v, em, rem)?.px(),
    }
}

fn border_style(v: &str) -> Option<BorderStyle> {
    Some(match v.to_ascii_lowercase().as_str() {
        "none" | "hidden" => BorderStyle::None,
        "solid" | "groove" | "ridge" | "inset" | "outset" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        _ => return None,
    })
}

fn css_color(v: &str, current: Rgba) -> Option<Rgba> {
    if v.eq_ignore_ascii_case("currentcolor") {
        Some(current)
    } else {
        color(v)
    }
}

/// `1px solid red`: any order, missing parts reset (width medium = 3, style none, color currentColor)
fn border_shorthand(v: &str, em: f32, rem: f32) -> (f32, BorderStyle, Option<Rgba>) {
    let (mut w, mut st, mut c) = (3.0, BorderStyle::None, None);
    for part in split_values(v) {
        if let Some(s) = border_style(&part) {
            st = s;
        } else if let Some(x) = border_width(&part, em, rem) {
            w = x;
        } else if !part.eq_ignore_ascii_case("currentcolor") {
            c = color(&part);
        }
    }
    (w, st, c)
}

/// whitespace split that keeps `rgb(1, 2, 3)` together
pub fn split_values(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut depth) = (String::new(), 0);
    for ch in v.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn font_weight(v: &str, parent: f32) -> Option<f32> {
    Some(match v.to_ascii_lowercase().as_str() {
        "normal" => 400.0,
        "bold" => 700.0,
        "bolder" => (parent + 300.0).min(900.0),
        "lighter" => (parent - 300.0).max(100.0),
        n => n.parse().ok()?,
    })
}

/// `font: italic bold 12px/1.5 Georgia, serif` (size and family required)
fn apply_font_shorthand(s: &mut Style, v: &str, parent_fs: f32, rem: f32) {
    let parts = split_values(v);
    let Some(i) = parts.iter().position(|p| font_size(p.split('/').next().unwrap_or(""), parent_fs, rem).is_some() && p.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')) else {
        return;
    };
    s.italic = false;
    s.font_weight = 400.0;
    for p in &parts[..i] {
        match p.to_ascii_lowercase().as_str() {
            "italic" | "oblique" => s.italic = true,
            w => {
                if let Some(w) = font_weight(w, 400.0) {
                    s.font_weight = w;
                }
            }
        }
    }
    let (size, lh) = parts[i].split_once('/').map_or((parts[i].as_str(), None), |(a, b)| (a, Some(b)));
    if let Some(fs) = font_size(size, parent_fs, rem) {
        s.font_size = fs;
    }
    if let Some(lh) = lh.and_then(|lh| line_height(lh, s.font_size, rem)) {
        s.line_height = lh;
    }
    if let Some(stack) = fonts::resolve_stack(&parts[i + 1..].join(" ")) {
        s.font_family = stack.into();
    }
}

fn line_height(v: &str, em: f32, rem: f32) -> Option<LineHeight> {
    if v.eq_ignore_ascii_case("normal") {
        return Some(LineHeight::Normal);
    }
    if let Ok(f) = v.trim().parse::<f32>() {
        return Some(LineHeight::Factor(f));
    }
    match len(v, em, rem)? {
        Len::Px(px) => Some(LineHeight::Px(px)),
        Len::Pct(p) => Some(LineHeight::Factor(p / 100.0)),
        Len::Auto => None,
    }
}

fn align<T: std::str::FromStr>(v: &str) -> Option<T> {
    // taffy parses the keyword forms; `left`/`right` are the physical aliases of start/end
    let v = v.trim().to_ascii_lowercase();
    let v = match v.as_str() {
        "left" => "start",
        "right" => "end",
        v => v,
    };
    v.parse().ok()
}

fn apply(s: &mut Style, parent: &Style, prop: &str, v: &str, em: f32, rem: f32) {
    let lv = v.trim().to_ascii_lowercase();
    if lv == "inherit" {
        apply_inherit(s, parent, prop);
        return;
    }
    let one = |v: &str| len(v, em, rem);
    match prop {
        "display" => {
            s.display = match lv.as_str() {
                "block" | "flow-root" => Display::Block,
                "inline" | "inline-block" => Display::Inline,
                "flex" | "inline-flex" => Display::Flex,
                "grid" | "inline-grid" => Display::Grid,
                "list-item" => Display::ListItem,
                "table" | "inline-table" => Display::Table,
                "table-row-group" | "table-header-group" | "table-footer-group" => Display::TableRowGroup,
                "table-row" => Display::TableRow,
                "table-cell" => Display::TableCell,
                "table-caption" => Display::TableCaption,
                "contents" => Display::Contents,
                "none" => Display::None,
                _ => s.display,
            }
        }
        "position" => s.position_absolute = matches!(lv.as_str(), "absolute" | "fixed"),
        "float" => {
            s.float = match lv.as_str() {
                "left" => taffy::Float::Left,
                "right" => taffy::Float::Right,
                _ => taffy::Float::None,
            }
        }
        "clear" => {
            s.clear = match lv.as_str() {
                "left" => taffy::Clear::Left,
                "right" => taffy::Clear::Right,
                "both" => taffy::Clear::Both,
                _ => taffy::Clear::None,
            }
        }
        "box-sizing" => s.border_box = lv == "border-box",
        "width" => set(&mut s.size[0], one(v)),
        "height" => set(&mut s.size[1], one(v)),
        "min-width" => set(&mut s.min_size[0], one(v)),
        "min-height" => set(&mut s.min_size[1], one(v)),
        "max-width" => set(&mut s.max_size[0], if lv == "none" { Some(Len::Auto) } else { one(v) }),
        "max-height" => set(&mut s.max_size[1], if lv == "none" { Some(Len::Auto) } else { one(v) }),
        "margin" => {
            if let Some(m) = lens(v, em, rem).and_then(|l| four(&l)) {
                s.margin = m
            }
        }
        "padding" => {
            if let Some(m) = lens(v, em, rem).and_then(|l| four(&l)) {
                s.padding = m
            }
        }
        "inset" => {
            if let Some(m) = lens(v, em, rem).and_then(|l| four(&l)) {
                s.inset = m
            }
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => set(&mut s.margin[side(prop)], one(v)),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => set(&mut s.padding[side(prop)], one(v)),
        "top" | "right" | "bottom" | "left" => set(&mut s.inset[side(prop)], one(v)),
        "border" => {
            let (w, st, c) = border_shorthand(v, em, rem);
            s.border_width = [w; 4];
            s.border_style = [st; 4];
            s.border_color = [c; 4];
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let i = side(prop);
            (s.border_width[i], s.border_style[i], s.border_color[i]) = border_shorthand(v, em, rem);
        }
        "border-width" => {
            if let Some(w) = split_values(v).iter().map(|p| border_width(p, em, rem)).collect::<Option<Vec<_>>>().and_then(|w| four(&w)) {
                s.border_width = w
            }
        }
        "border-style" => {
            if let Some(st) = split_values(v).iter().map(|p| border_style(p)).collect::<Option<Vec<_>>>().and_then(|w| four(&w)) {
                s.border_style = st
            }
        }
        "border-color" => {
            if let Some(c) = split_values(v).iter().map(|p| css_color(p, s.color).map(Some)).collect::<Option<Vec<_>>>().and_then(|w| four(&w)) {
                s.border_color = c
            }
        }
        p if p.starts_with("border-") && p.ends_with("-width") && p.matches('-').count() == 2 => {
            if let Some(w) = border_width(v, em, rem) {
                s.border_width[side(p)] = w
            }
        }
        p if p.starts_with("border-") && p.ends_with("-style") && p.matches('-').count() == 2 => {
            if let Some(st) = border_style(v) {
                s.border_style[side(p)] = st
            }
        }
        p if p.starts_with("border-") && p.ends_with("-color") && p.matches('-').count() == 2 => {
            if let Some(c) = css_color(v, s.color) {
                s.border_color[side(p)] = Some(c)
            }
        }
        "border-radius" => {
            // `a b c d / e f g h`: elliptical radii unsupported, horizontal values used
            let h = v.split('/').next().unwrap_or("");
            if let Some(r) = lens(h, em, rem).and_then(|l| four(&l)) {
                s.radius = r.map(|l| l.px().unwrap_or(0.0));
            }
        }
        "border-top-left-radius" | "border-top-right-radius" | "border-bottom-right-radius" | "border-bottom-left-radius" => {
            let i = ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]
                .iter()
                .position(|p| *p == prop)
                .unwrap_or(0);
            if let Some(Len::Px(r)) = v.split_whitespace().next().and_then(one) {
                s.radius[i] = r;
            }
        }
        "border-collapse" => s.border_collapse = lv == "collapse",
        "border-spacing" => {
            if let Some(Len::Px(px)) = v.split_whitespace().next().and_then(one) {
                s.border_spacing = px
            }
        }
        "background-color" => s.background = css_color(v, s.color).filter(|c| c.3 > 0),
        "background" => {
            // only the color layer is drawn
            if lv == "none" || lv == "transparent" {
                s.background = None;
            } else if let Some(c) = split_values(v).iter().rev().find_map(|p| css_color(p, s.color)) {
                s.background = Some(c).filter(|c| c.3 > 0);
            }
        }
        "opacity" => {
            if let Ok(o) = lv.trim_end_matches('%').parse::<f32>() {
                s.opacity = if lv.ends_with('%') { o / 100.0 } else { o }.clamp(0.0, 1.0);
            }
        }
        "overflow" => s.overflow_hidden = matches!(lv.as_str(), "hidden" | "clip" | "auto" | "scroll"),
        "color" => {
            if let Some(c) = color(v) {
                s.color = c
            }
        }
        "font-family" => {
            // first usable family of the stack, like a browser; none -> inherited
            if let Some(stack) = fonts::resolve_stack(v) {
                s.font_family = stack.into();
            }
        }
        "font-weight" => {
            if let Some(w) = font_weight(&lv, parent.font_weight) {
                s.font_weight = w
            }
        }
        "font-style" => s.italic = lv == "italic" || lv.starts_with("oblique"),
        "text-align" => {
            s.text_align = match lv.as_str() {
                "left" => TextAlign::Left,
                "right" | "end" => TextAlign::Right,
                "center" => TextAlign::Center,
                "justify" => TextAlign::Justify,
                _ => TextAlign::Start,
            }
        }
        "line-height" => {
            if let Some(lh) = line_height(v, em, rem) {
                s.line_height = lh
            }
        }
        "letter-spacing" | "word-spacing" => {
            let px = if lv == "normal" { Some(0.0) } else { one(v).and_then(Len::px) };
            if let Some(px) = px {
                if prop == "letter-spacing" { s.letter_spacing = px } else { s.word_spacing = px }
            }
        }
        "text-transform" => {
            s.transform = match lv.as_str() {
                "uppercase" => Transform::Upper,
                "lowercase" => Transform::Lower,
                "capitalize" => Transform::Capitalize,
                _ => Transform::None,
            }
        }
        "white-space" => {
            s.white_space = match lv.as_str() {
                "nowrap" => WhiteSpace::NoWrap,
                "pre" => WhiteSpace::Pre,
                "pre-wrap" | "break-spaces" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => WhiteSpace::Normal,
            }
        }
        "text-decoration" | "text-decoration-line" => {
            if lv.contains("none") {
                s.underline = false;
                s.strike = false;
            }
            if lv.contains("underline") {
                s.underline = true;
            }
            if lv.contains("line-through") {
                s.strike = true;
            }
        }
        "list-style" | "list-style-type" => {
            if let Some(t) = split_values(&lv).into_iter().find(|p| !matches!(p.as_str(), "inside" | "outside") && !p.starts_with("url(")) {
                s.list_style = t.into();
            }
        }
        "flex" => {
            // none | auto | <grow> [<shrink>] [<basis>] | <basis>
            let parts = split_values(&lv);
            match parts.as_slice() {
                [k] if k == "none" => (s.flex_grow, s.flex_shrink, s.flex_basis) = (0.0, 0.0, Len::Auto),
                [k] if k == "auto" => (s.flex_grow, s.flex_shrink, s.flex_basis) = (1.0, 1.0, Len::Auto),
                [k] if k == "initial" => (s.flex_grow, s.flex_shrink, s.flex_basis) = (0.0, 1.0, Len::Auto),
                _ => {
                    let nums: Vec<f32> = parts.iter().map_while(|p| p.parse().ok()).collect();
                    let basis = parts.get(nums.len()).and_then(|p| one(p));
                    s.flex_grow = nums.first().copied().unwrap_or(if basis.is_some() { 1.0 } else { 0.0 });
                    s.flex_shrink = nums.get(1).copied().unwrap_or(1.0);
                    // a bare number means basis 0 (flex: 1 -> 1 1 0%)
                    s.flex_basis = basis.unwrap_or(if nums.is_empty() { Len::Auto } else { Len::Px(0.0) });
                }
            }
        }
        "flex-grow" => s.flex_grow = lv.parse().unwrap_or(s.flex_grow),
        "flex-shrink" => s.flex_shrink = lv.parse().unwrap_or(s.flex_shrink),
        "flex-basis" => set(&mut s.flex_basis, if lv == "content" { Some(Len::Auto) } else { one(v) }),
        "flex-direction" => s.flex_direction = lv.parse().unwrap_or(s.flex_direction),
        "flex-wrap" => s.flex_wrap = lv.parse().unwrap_or(s.flex_wrap),
        "flex-flow" => {
            for p in lv.split_whitespace() {
                if let Ok(d) = p.parse() {
                    s.flex_direction = d;
                } else if let Ok(w) = p.parse() {
                    s.flex_wrap = w;
                }
            }
        }
        "justify-content" => s.justify_content = align(v),
        "align-content" => s.align_content = align(v),
        "align-items" => s.align_items = align(v),
        "align-self" => s.align_self = if lv == "auto" { None } else { align(v) },
        "justify-items" => s.justify_items = align(v),
        "justify-self" => s.justify_self = if lv == "auto" { None } else { align(v) },
        "place-items" => {
            let p = split_values(v);
            s.align_items = p.first().and_then(|a| align(a));
            s.justify_items = p.get(1).or(p.first()).and_then(|a| align(a));
        }
        "place-content" => {
            let p = split_values(v);
            s.align_content = p.first().and_then(|a| align(a));
            s.justify_content = p.get(1).or(p.first()).and_then(|a| align(a));
        }
        "gap" | "grid-gap" => {
            if lv == "normal" {
                s.gap = [Len::Px(0.0); 2];
            } else if let Some(g) = lens(v, em, rem) {
                match g.as_slice() {
                    [a] => s.gap = [*a, *a],
                    [a, b] => s.gap = [*a, *b],
                    _ => {}
                }
            }
        }
        "row-gap" | "grid-row-gap" => set(&mut s.gap[0], if lv == "normal" { Some(Len::Px(0.0)) } else { one(v) }),
        "column-gap" | "grid-column-gap" => set(&mut s.gap[1], if lv == "normal" { Some(Len::Px(0.0)) } else { one(v) }),
        "order" => s.order = lv.parse().unwrap_or(0),
        "grid-template-columns" => s.grid_template_columns = Some(px_units(v, em, rem).into()),
        "grid-template-rows" => s.grid_template_rows = Some(px_units(v, em, rem).into()),
        "grid-auto-flow" => s.grid_auto_flow = Some(lv.into()),
        "grid-column" => s.grid_column = Some(lv.into()),
        "grid-row" => s.grid_row = Some(lv.into()),
        "aspect-ratio" => {
            let r = lv.split('/').map(|p| p.trim().parse::<f32>().ok()).collect::<Option<Vec<_>>>();
            s.aspect_ratio = match r.as_deref() {
                Some([a]) => Some(*a),
                Some([a, b]) if *b != 0.0 => Some(a / b),
                _ => None,
            }
        }
        "vertical-align" => {
            s.vertical_align = match lv.as_str() {
                "top" | "text-top" => VAlign::Top,
                "middle" => VAlign::Middle,
                "bottom" | "text-bottom" => VAlign::Bottom,
                _ => VAlign::Baseline,
            }
        }
        "object-fit" => {
            s.object_fit = match lv.as_str() {
                "contain" | "scale-down" => ObjectFit::Contain,
                "cover" => ObjectFit::Cover,
                "none" => ObjectFit::None,
                _ => ObjectFit::Fill,
            }
        }
        "page-break-before" | "break-before" => s.break_before = matches!(lv.as_str(), "always" | "page" | "left" | "right"),
        "page-break-after" | "break-after" => s.break_after = matches!(lv.as_str(), "always" | "page" | "left" | "right"),
        "page-break-inside" | "break-inside" => s.break_inside_avoid = lv.starts_with("avoid"),
        _ => {}
    }
}

fn apply_inherit(s: &mut Style, p: &Style, prop: &str) {
    match prop {
        "background-color" | "background" => s.background = p.background,
        "border" => (s.border_width, s.border_style, s.border_color) = (p.border_width, p.border_style, p.border_color),
        "width" => s.size[0] = p.size[0],
        "height" => s.size[1] = p.size[1],
        "margin" => s.margin = p.margin,
        "padding" => s.padding = p.padding,
        "display" => s.display = p.display,
        _ => {} // inherited properties already are
    }
}

fn set(slot: &mut Len, v: Option<Len>) {
    if let Some(v) = v {
        *slot = v;
    }
}

fn side(prop: &str) -> usize {
    ["top", "right", "bottom", "left"].iter().position(|s| prop.contains(s)).unwrap_or(0)
}

/// rewrite every em/rem/pt/mm... length of a value to px, for taffy's parser (it only takes px)
fn px_units(v: &str, em: f32, rem: f32) -> String {
    let mut out = String::new();
    for tok in v.split_inclusive(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let (word, sep) = tok.split_at(tok.trim_end_matches(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',').len());
        match len(word, em, rem) {
            Some(Len::Px(px)) if word.ends_with(|c: char| c.is_ascii_alphabetic()) && !word.ends_with("px") && !word.ends_with("fr") => out.push_str(&format!("{px}px")),
            _ => out.push_str(word),
        }
        out.push_str(sep);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::parse_declarations;

    fn computed(css: &str) -> Style {
        crate::fonts::init_for_tests();
        let decls = parse_declarations(css);
        let refs: Vec<&Decl> = decls.iter().collect();
        Style::compute(&Style::root("Helvetica", 12.0), &refs, 12.0)
    }

    #[test]
    fn lengths() {
        assert_eq!(len("10px", 12.0, 12.0), Some(Len::Px(10.0)));
        assert_eq!(len("2em", 12.0, 12.0), Some(Len::Px(24.0)));
        assert_eq!(len("50%", 12.0, 12.0), Some(Len::Pct(50.0)));
        assert_eq!(len("1in", 12.0, 12.0), Some(Len::Px(72.0)));
        assert_eq!(len("0", 12.0, 12.0), Some(Len::Px(0.0)));
        assert_eq!(len("red", 12.0, 12.0), None);
    }

    #[test]
    fn shorthands() {
        let s = computed("margin: 1px 2px; padding: 1em; border: medium dashed #f00; border-left: none; flex: 1");
        assert_eq!(s.margin, [Len::Px(1.0), Len::Px(2.0), Len::Px(1.0), Len::Px(2.0)]);
        assert_eq!(s.padding[3], Len::Px(12.0));
        assert_eq!(s.border_width[0], 3.0);
        assert_eq!(s.border_style[1], BorderStyle::Dashed);
        assert_eq!(s.border_style[3], BorderStyle::None);
        assert_eq!(s.border_color[0], Some(Rgba(255, 0, 0, 255)));
        assert_eq!((s.flex_grow, s.flex_shrink, s.flex_basis), (1.0, 1.0, Len::Px(0.0)));
    }

    #[test]
    fn font_size_before_em() {
        // em lengths use the element's own font-size, whatever the declaration order
        let s = computed("padding: 1em; font-size: 20px");
        assert_eq!(s.padding[0], Len::Px(20.0));
        let s = computed("font: italic bold 10px/2 serif");
        assert!(s.italic && s.font_weight == 700.0 && s.font_size == 10.0 && s.line_height == LineHeight::Factor(2.0));
    }

    #[test]
    fn normal_keywords() {
        // `normal` spacings crashed react-pdf; here they are their default
        let s = computed("gap: normal; letter-spacing: normal; line-height: normal");
        assert_eq!(s.gap, [Len::Px(0.0); 2]);
        assert_eq!(s.letter_spacing, 0.0);
    }

    #[test]
    fn units_for_taffy() {
        assert_eq!(px_units("1fr 2em repeat(2, 10pt)", 10.0, 10.0), "1fr 20px repeat(2, 10px)");
    }
}
