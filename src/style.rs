//! Computed style: cascade result for one element, inheritance, units, shorthands.
//! 1px = 1pt, like react-pdf (so documents render at the same size as with pdfjs).

use crate::css::Decl;
use crate::fonts;
use std::collections::HashMap;
use std::rc::Rc;
use taffy::prelude::TaffyZero;
use taffy::{AlignContent, AlignItems, Dimension, FlexDirection, FlexWrap, LengthPercentage, LengthPercentageAuto};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Auto,
    Px(f32),
    Pct(f32),
    /// `calc()` mixing both: px + pct% of the containing block. Laid out as its px part first,
    /// then resolved against the real containing block (see Tree::compute).
    Calc(f32, f32),
}

impl Len {
    pub fn dim(self) -> Dimension {
        match self {
            Len::Auto => Dimension::auto(),
            Len::Px(v) | Len::Calc(v, _) => Dimension::length(v),
            Len::Pct(v) => Dimension::percent(v / 100.0),
        }
    }
    pub fn lpa(self) -> LengthPercentageAuto {
        match self {
            Len::Auto => LengthPercentageAuto::auto(),
            Len::Px(v) | Len::Calc(v, _) => LengthPercentageAuto::length(v),
            Len::Pct(v) => LengthPercentageAuto::percent(v / 100.0),
        }
    }
    pub fn lp(self) -> LengthPercentage {
        match self {
            Len::Auto => LengthPercentage::ZERO,
            Len::Px(v) | Len::Calc(v, _) => LengthPercentage::length(v),
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
    Sub,
    Super,
    TextTop,
    TextBottom,
    /// raise by this many pt (negative: lower)
    Length(f32),
}

/// `linear-gradient()` / `radial-gradient()` (and `repeating-` variants) of a background
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// color, position (None = spread evenly between neighbours)
    pub stops: Vec<(Rgba, Option<Len>)>,
    pub repeating: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// angle in degrees, CSS convention: 0 = to top, clockwise
    Linear(f32),
    /// `to top right`...: x = -1 left / 1 right / 0, y = -1 top / 1 bottom / 0; the angle depends on the box
    LinearCorner(i8, i8),
    /// circle (else ellipse), center
    Radial { circle: bool, at: (Len, Len) },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgSize {
    Auto,
    Cover,
    Contain,
    /// width, height (each may be Len::Auto: keeps the image's ratio)
    Size(Len, Len),
}

/// The image of a background layer
#[derive(Clone, Debug, PartialEq)]
pub enum BgSource {
    Url(Rc<str>),
    Gradient(Rc<Gradient>),
}

/// `background-origin` / `background-clip` box
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BoxArea {
    #[default]
    Border,
    Padding,
    Content,
    /// `background-clip: text`: the layer paints the text of the element instead of its box
    Text,
}

/// Background layers, first on top. Each list is cycled to the number of images, as in CSS.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Background {
    pub images: Vec<BgSource>,
    pub sizes: Vec<BgSize>,
    pub positions: Vec<(Len, Len)>,
    pub repeats: Vec<(bool, bool)>,
    pub origins: Vec<BoxArea>,
    pub clips: Vec<BoxArea>,
    /// `background-attachment: fixed`: positioned against the page instead of the box
    pub fixed: Vec<bool>,
}

impl Background {
    /// value of layer `i` of a list (cycled), or the initial value
    pub fn nth<T: Copy>(list: &[T], i: usize, initial: T) -> T {
        if list.is_empty() { initial } else { list[i % list.len()] }
    }
}

/// One `box-shadow`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
    pub inset: bool,
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
    pub rtl: bool,
    /// custom properties (`--name`), inherited
    pub vars: Option<Rc<HashMap<String, String>>>,
    // not inherited
    pub display: Display,
    /// `inline-block` / `inline-flex` / `inline-grid` / `inline-table`: `display` is the inner layout,
    /// the box itself sits in a line of text
    pub inline_block: bool,
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
    /// background image layers (`url()`, gradients) and their size / position / repeat / origin / clip
    pub bg: Option<Rc<Background>>,
    pub shadows: Option<Rc<Vec<Shadow>>>,
    /// `counter-reset` / `counter-increment` / `counter-set`: (name, value)
    pub counter_reset: Option<Rc<Vec<(String, i32)>>>,
    pub counter_increment: Option<Rc<Vec<(String, i32)>>>,
    pub counter_set: Option<Rc<Vec<(String, i32)>>>,
    /// `page: name`: the named @page of a top-level element
    pub page: Option<Rc<str>>,
    pub opacity: f32,
    /// `transform` / `transform-origin`, resolved at draw time (they depend on the box size)
    pub transform_fn: Option<Rc<str>>,
    pub transform_origin: Option<Rc<str>>,
    /// `content` of a ::before / ::after
    pub content: Option<Rc<str>>,
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
            rtl: false,
            vars: None,
            display: Display::Block,
            inline_block: false,
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
            bg: None,
            shadows: None,
            counter_reset: None,
            counter_increment: None,
            counter_set: None,
            page: None,
            opacity: 1.0,
            transform_fn: None,
            transform_origin: None,
            content: None,
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
            rtl: self.rtl,
            vars: self.vars.clone(),
            display: Display::Inline,
            ..initial
        }
    }

    /// Apply the winning declarations (in cascade order) on top of `self.inherit()`.
    pub fn compute(parent: &Style, decls: &[&Decl], root_font_size: f32) -> Style {
        let mut s = parent.inherit();
        // custom properties first: every other value may use them
        let custom: Vec<&&Decl> = decls.iter().filter(|d| d.prop.starts_with("--")).collect();
        if !custom.is_empty() {
            let mut vars = s.vars.as_deref().cloned().unwrap_or_default();
            for d in custom {
                vars.insert(d.prop.clone(), d.value.clone());
            }
            s.vars = Some(Rc::new(vars));
        }
        // var() substituted; a reference to an undefined variable without fallback drops the declaration
        let values: Vec<(&str, std::borrow::Cow<str>)> = decls
            .iter()
            .filter(|d| !d.prop.starts_with("--"))
            .filter_map(|d| Some((d.prop.as_str(), substitute_vars(&d.value, s.vars.as_deref(), 0)?)))
            .collect();
        // font-size next: em units of every other property depend on it
        for (prop, value) in values.iter().filter(|(p, _)| *p == "font-size" || *p == "font") {
            if *prop == "font" {
                apply_font_shorthand(&mut s, value, parent.font_size, root_font_size);
            } else if let Some(fs) = font_size(value, parent.font_size, root_font_size) {
                s.font_size = fs;
            }
        }
        let em = s.font_size;
        for (prop, value) in &values {
            apply(&mut s, parent, prop, value, em, root_font_size);
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
    if let Some(inner) = ["calc(", "-webkit-calc("].iter().find_map(|p| v.strip_prefix(p)).and_then(|r| r.strip_suffix(')')) {
        let (px, pct) = calc::eval(inner, em, rem)?;
        return Some(match (px, pct) {
            (px, 0.0) => Len::Px(px),
            (0.0, pct) => Len::Pct(pct),
            (px, pct) => Len::Calc(px, pct),
        });
    }
    if let Some(func) = ["min(", "max(", "clamp("].iter().find(|p| v.starts_with(**p)) {
        // px-only arguments are evaluated; anything relative falls back to the first argument
        let inner = v[func.len()..].strip_suffix(')')?;
        let args: Vec<(f32, f32)> = crate::css::split_top_level(inner, ',').map(|a| calc::eval(a, em, rem)).collect::<Option<_>>()?;
        if args.iter().any(|a| a.1 != 0.0) {
            let (px, pct) = args[0];
            return Some(if pct == 0.0 { Len::Px(px) } else if px == 0.0 { Len::Pct(pct) } else { Len::Calc(px, pct) });
        }
        let px: Vec<f32> = args.iter().map(|a| a.0).collect();
        return Some(Len::Px(match (*func, px.as_slice()) {
            ("min(", p) => p.iter().copied().fold(f32::INFINITY, f32::min),
            ("max(", p) => p.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            (_, [lo, v, hi]) => v.clamp(*lo, hi.max(*lo)),
            _ => return None,
        }));
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
        Len::Calc(px, p) => Some(px + parent * p / 100.0),
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
    split_values(v).iter().map(|p| len(p, em, rem)).collect()
}

/// `var(--name)` / `var(--name, fallback)`, recursively (custom properties may use var() too).
/// None when a variable is undefined and has no fallback.
pub fn substitute_vars<'a>(v: &'a str, vars: Option<&HashMap<String, String>>, depth: u8) -> Option<std::borrow::Cow<'a, str>> {
    let Some(start) = v.find("var(") else { return Some(std::borrow::Cow::Borrowed(v)) };
    if depth > 16 {
        return None; // cycle
    }
    // matching parenthesis
    let mut d = 0;
    let mut end = None;
    for (i, c) in v[start + 3..].char_indices() {
        match c {
            '(' => d += 1,
            ')' => {
                d -= 1;
                if d == 0 {
                    end = Some(start + 3 + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let inner = &v[start + 4..end];
    let (name, fallback) = inner.split_once(',').map_or((inner.trim(), None), |(n, f)| (n.trim(), Some(f.trim())));
    let value = match vars.and_then(|m| m.get(name)) {
        Some(val) => substitute_vars(val, vars, depth + 1)?.into_owned(),
        None => substitute_vars(fallback?, vars, depth + 1)?.into_owned(),
    };
    let rest = substitute_vars(&v[end + 1..], vars, depth + 1)?;
    Some(std::borrow::Cow::Owned(format!("{}{}{}", &v[..start], value, rest)))
}

/// `calc()` arithmetic on lengths: the result is `px + pct%`.
mod calc {
    use super::len;

    pub fn eval(expr: &str, em: f32, rem: f32) -> Option<(f32, f32)> {
        let tokens = tokenize(expr)?;
        let mut pos = 0;
        let v = sum(&tokens, &mut pos, em, rem)?;
        (pos == tokens.len()).then_some(v)
    }

    #[derive(Debug, PartialEq)]
    enum Tok {
        Val(String),
        Op(char),
        Open,
        Close,
    }

    fn tokenize(s: &str) -> Option<Vec<Tok>> {
        let mut out = Vec::new();
        let b: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c.is_whitespace() {
                i += 1;
            } else if c == '(' {
                out.push(Tok::Open);
                i += 1;
            } else if c == ')' {
                out.push(Tok::Close);
                i += 1;
            } else if matches!(c, '*' | '/') || (matches!(c, '+' | '-') && b.get(i + 1).is_some_and(|n| n.is_whitespace())) {
                out.push(Tok::Op(c));
                i += 1;
            } else if s[s.char_indices().nth(i)?.0..].starts_with("calc(") {
                out.push(Tok::Open);
                i += 5;
            } else {
                let start = i;
                while i < b.len() && !b[i].is_whitespace() && !matches!(b[i], '(' | ')' | '*' | '/') {
                    i += 1;
                }
                out.push(Tok::Val(b[start..i].iter().collect()));
            }
        }
        Some(out)
    }

    fn sum(t: &[Tok], pos: &mut usize, em: f32, rem: f32) -> Option<(f32, f32)> {
        let mut acc = product(t, pos, em, rem)?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = t.get(*pos) {
            *pos += 1;
            let r = product(t, pos, em, rem)?;
            acc = if *op == '+' { (acc.0 + r.0, acc.1 + r.1) } else { (acc.0 - r.0, acc.1 - r.1) };
        }
        Some(acc)
    }

    /// (value, is_plain_number)
    fn product(t: &[Tok], pos: &mut usize, em: f32, rem: f32) -> Option<(f32, f32)> {
        let (mut acc, mut plain) = atom(t, pos, em, rem)?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = t.get(*pos) {
            *pos += 1;
            let (r, r_plain) = atom(t, pos, em, rem)?;
            if *op == '*' {
                // one side must be a plain number
                if plain {
                    acc = (r.0 * acc.0, r.1 * acc.0);
                    plain = r_plain;
                } else if r_plain {
                    acc = (acc.0 * r.0, acc.1 * r.0);
                } else {
                    return None;
                }
            } else {
                if !r_plain || r.0 == 0.0 {
                    return None;
                }
                acc = (acc.0 / r.0, acc.1 / r.0);
            }
        }
        Some(acc)
    }

    fn atom(t: &[Tok], pos: &mut usize, em: f32, rem: f32) -> Option<((f32, f32), bool)> {
        match t.get(*pos)? {
            Tok::Open => {
                *pos += 1;
                let v = sum(t, pos, em, rem)?;
                (t.get(*pos)? == &Tok::Close).then_some(())?;
                *pos += 1;
                Some((v, false))
            }
            Tok::Val(v) => {
                *pos += 1;
                if let Ok(n) = v.parse::<f32>() {
                    return Some(((n, 0.0), true));
                }
                match len(v, em, rem)? {
                    super::Len::Px(px) => Some(((px, 0.0), false)),
                    super::Len::Pct(p) => Some(((0.0, p), false)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

fn angle(v: &str) -> Option<f32> {
    let v = v.trim();
    let unit = v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len());
    let n: f32 = v[..unit].parse().ok()?;
    Some(match &v[unit..] {
        "deg" => n,
        "rad" => n.to_degrees(),
        "turn" => n * 360.0,
        "grad" => n * 0.9,
        "" if n == 0.0 => 0.0,
        _ => return None,
    })
}

/// `linear-gradient(...)`, `radial-gradient(...)`, `repeating-*`
pub fn gradient(v: &str, em: f32, rem: f32, current: Rgba) -> Option<Gradient> {
    let lv = v.trim().to_ascii_lowercase();
    let (repeating, rest) = match lv.strip_prefix("repeating-") {
        Some(r) => (true, r),
        None => (false, lv.as_str()),
    };
    let (radial, args) = if let Some(a) = rest.strip_prefix("linear-gradient(") {
        (false, a)
    } else if let Some(a) = rest.strip_prefix("radial-gradient(") {
        (true, a)
    } else {
        return None;
    };
    let args = args.strip_suffix(')')?;
    let mut parts: Vec<&str> = crate::css::split_top_level(args, ',').map(str::trim).collect();
    let first = *parts.first()?;
    let kind = if radial {
        let shape_like = first.split_whitespace().all(|w| {
            matches!(w, "circle" | "ellipse" | "at" | "closest-side" | "closest-corner" | "farthest-side" | "farthest-corner" | "center" | "left" | "right" | "top" | "bottom")
                || len(w, em, rem).is_some()
        }) && color(first).is_none();
        let mut at = (Len::Pct(50.0), Len::Pct(50.0));
        let mut circle = false;
        if shape_like {
            circle = first.contains("circle");
            if let Some((_, pos)) = first.split_once("at ") {
                at = position(pos, em, rem);
            }
            parts.remove(0);
        }
        GradientKind::Radial { circle, at }
    } else if let Some(to) = first.strip_prefix("to ") {
        parts.remove(0);
        let (mut x, mut y) = (0i8, 0i8);
        for w in to.split_whitespace() {
            match w {
                "left" => x = -1,
                "right" => x = 1,
                "top" => y = -1,
                "bottom" => y = 1,
                _ => return None,
            }
        }
        match (x, y) {
            (0, -1) => GradientKind::Linear(0.0),
            (1, 0) => GradientKind::Linear(90.0),
            (0, 1) => GradientKind::Linear(180.0),
            (-1, 0) => GradientKind::Linear(270.0),
            (0, 0) => return None,
            _ => GradientKind::LinearCorner(x, y),
        }
    } else if let Some(a) = angle(first) {
        parts.remove(0);
        GradientKind::Linear(a)
    } else {
        GradientKind::Linear(180.0)
    };
    let mut stops = Vec::new();
    for p in parts {
        let words = split_values(p);
        let c = css_color(words.first()?, current)?;
        let positions: Vec<Len> = words[1..].iter().filter_map(|w| len(w, em, rem)).collect();
        if positions.is_empty() {
            stops.push((c, None));
        }
        for pos in positions.into_iter().take(2) {
            stops.push((c, Some(pos)));
        }
    }
    (stops.len() >= 2).then_some(Gradient { kind, stops, repeating })
}

/// `center`, `left top`, `30% 10px`... -> (x, y)
pub fn position(v: &str, em: f32, rem: f32) -> (Len, Len) {
    let (mut x, mut y) = (Len::Pct(50.0), Len::Pct(50.0));
    let words = split_values(v);
    let kw = |w: &str| match w {
        "left" | "top" => Some(Len::Pct(0.0)),
        "center" => Some(Len::Pct(50.0)),
        "right" | "bottom" => Some(Len::Pct(100.0)),
        _ => None,
    };
    if words.len() >= 3 {
        // `right 10px top`, `left 5% bottom 2px`: edge keywords with offsets
        let mut i = 0;
        while i < words.len() {
            let w = words[i].as_str();
            let off = words.get(i + 1).and_then(|o| len(o, em, rem)).filter(|_| !matches!(words[i + 1].as_str(), "left" | "right" | "top" | "bottom" | "center"));
            let v = match (w, off) {
                ("left" | "top", Some(o)) => o,
                ("right" | "bottom", Some(Len::Px(o))) => Len::Calc(-o, 100.0),
                ("right" | "bottom", Some(Len::Pct(p))) => Len::Pct(100.0 - p),
                (_, _) => kw(w).unwrap_or(Len::Pct(50.0)),
            };
            if matches!(w, "top" | "bottom") { y = v } else if w != "center" || i == 0 { x = v }
            i += if off.is_some() { 2 } else { 1 };
        }
        return (x, y);
    }
    match words.as_slice() {
        [a] if a == "top" || a == "bottom" => y = kw(a).unwrap(),
        [a] => x = kw(a).or_else(|| len(a, em, rem)).unwrap_or(x),
        [a, b] => {
            // `top left` is valid too: vertical keyword first
            let (a, b) = if matches!(a.as_str(), "top" | "bottom") || matches!(b.as_str(), "left" | "right") { (b, a) } else { (a, b) };
            x = kw(a).or_else(|| len(a, em, rem)).unwrap_or(x);
            y = kw(b).or_else(|| len(b, em, rem)).unwrap_or(y);
        }
        _ => {}
    }
    (x, y)
}

fn bg_mut(s: &mut Style) -> &mut Background {
    Rc::make_mut(s.bg.get_or_insert_with(Default::default))
}

fn box_area(v: &str) -> Option<BoxArea> {
    Some(match v {
        "border-box" => BoxArea::Border,
        "padding-box" => BoxArea::Padding,
        "content-box" => BoxArea::Content,
        _ => return None,
    })
}

/// `name [n] name2 [n]...` / `none`
fn counter_list(v: &str, default: i32) -> Option<Rc<Vec<(String, i32)>>> {
    if v.trim() == "none" {
        return Some(Rc::new(Vec::new()));
    }
    let mut out: Vec<(String, i32)> = Vec::new();
    for w in v.split_whitespace() {
        match (w.parse::<i32>(), out.last_mut()) {
            (Ok(n), Some(last)) => last.1 = n,
            (Ok(_), None) => return None,
            (Err(_), _) => out.push((w.to_string(), default)),
        }
    }
    Some(Rc::new(out))
}

/// A counter value in a list style: `decimal`, `lower-alpha`, `upper-roman`, `disc`...
pub fn format_counter(n: i32, style: &str) -> String {
    let alpha = |n: i32, base: u8| {
        if n < 1 {
            return n.to_string();
        }
        let (mut n, mut s) = (n as u32, Vec::new());
        while n > 0 {
            n -= 1;
            s.push((base + (n % 26) as u8) as char);
            n /= 26;
        }
        s.iter().rev().collect::<String>()
    };
    let roman = |n: i32| {
        if !(1..4000).contains(&n) {
            return n.to_string();
        }
        let (mut n, mut s) = (n, String::new());
        for (v, r) in [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
            while n >= v {
                s.push_str(r);
                n -= v;
            }
        }
        s
    };
    match style {
        "none" => String::new(),
        "disc" => "•".into(),
        "circle" => "◦".into(),
        "square" => "▪".into(),
        "decimal-leading-zero" => format!("{n:02}"),
        "lower-alpha" | "lower-latin" => alpha(n, b'a'),
        "upper-alpha" | "upper-latin" => alpha(n, b'A'),
        "lower-greek" if (1..=24).contains(&n) => char::from_u32(0x3B1 + n as u32 - 1 + if n > 17 { 1 } else { 0 }).map(String::from).unwrap_or_default(),
        "lower-roman" => roman(n),
        "upper-roman" => roman(n).to_uppercase(),
        _ => n.to_string(),
    }
}

/// `url(x)`, `url("x")` -> x
pub fn url(v: &str) -> Option<String> {
    let inner = v.trim().strip_prefix("url(").or_else(|| v.trim().strip_prefix("URL("))?.strip_suffix(')')?;
    let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
    (!inner.is_empty()).then(|| inner.to_string())
}

/// `center / cover` -> ("center", Some("cover")), outside parentheses
fn split_slash(layer: &str) -> (&str, Option<&str>) {
    let mut depth = 0;
    for (i, c) in layer.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '/' if depth == 0 => return (&layer[..i], Some(&layer[i + 1..])),
            _ => {}
        }
    }
    (layer, None)
}

fn repeat(v: &str) -> Option<(bool, bool)> {
    Some(match v {
        "repeat" | "space" | "round" => (true, true),
        "no-repeat" => (false, false),
        "repeat-x" => (true, false),
        "repeat-y" => (false, true),
        _ => return None,
    })
}

fn bg_size(v: &str, em: f32, rem: f32) -> Option<BgSize> {
    let words = split_values(&v.trim().to_ascii_lowercase());
    Some(match words.as_slice() {
        [w] if w == "cover" => BgSize::Cover,
        [w] if w == "contain" => BgSize::Contain,
        [w] if w == "auto" => BgSize::Auto,
        [w] => BgSize::Size(len(w, em, rem)?, Len::Auto),
        [w, h] => BgSize::Size(len(w, em, rem)?, len(h, em, rem)?),
        _ => return None,
    })
}

/// `box-shadow`: comma separated `[inset] dx dy [blur [spread]] [color]`
fn shadows(v: &str, em: f32, rem: f32, current: Rgba) -> Option<Vec<Shadow>> {
    let mut out = Vec::new();
    for layer in crate::css::split_top_level(v, ',') {
        let (mut lens, mut color, mut inset) = (Vec::new(), current, false);
        for w in split_values(layer) {
            if w.eq_ignore_ascii_case("inset") {
                inset = true;
            } else if let Some(l) = len(&w, em, rem).and_then(Len::px) {
                lens.push(l);
            } else {
                color = css_color(&w, current)?;
            }
        }
        let [dx, dy] = [*lens.first()?, *lens.get(1)?];
        out.push(Shadow { dx, dy, blur: lens.get(2).copied().unwrap_or(0.0).max(0.0), spread: lens.get(3).copied().unwrap_or(0.0), color, inset });
    }
    (!out.is_empty()).then_some(out)
}

/// Resolve a length against a basis (percentages, calc)
pub fn resolve(l: Len, basis: f32) -> f32 {
    match l {
        Len::Px(v) => v,
        Len::Pct(p) => basis * p / 100.0,
        Len::Calc(px, p) => px + basis * p / 100.0,
        Len::Auto => 0.0,
    }
}

/// `transform` functions -> affine matrix (a, b, c, d, e, f) for a box of w x h. None = identity / invalid.
pub fn transform_matrix(v: &str, w: f32, h: f32, em: f32) -> Option<[f32; 6]> {
    let mul = |m: [f32; 6], n: [f32; 6]| {
        [
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ]
    };
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let lv = v.to_ascii_lowercase();
    if lv.trim() == "none" {
        return None;
    }
    let mut rest = lv.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let close = open + rest[open..].find(')')?;
        let args: Vec<&str> = rest[open + 1..close].split([',', ' ']).filter(|a| !a.is_empty()).collect();
        rest = &rest[close + 1..];
        let l = |i: usize, basis: f32| args.get(i).and_then(|a| len(a, em, em)).map(|l| resolve(l, basis));
        let n = |i: usize| args.get(i).and_then(|a| a.trim().parse::<f32>().ok());
        let a = |i: usize| args.get(i).and_then(|v| angle(v)).map(f32::to_radians);
        let t = match name {
            "translate" => [1.0, 0.0, 0.0, 1.0, l(0, w)?, l(1, h).unwrap_or(0.0)],
            "translatex" => [1.0, 0.0, 0.0, 1.0, l(0, w)?, 0.0],
            "translatey" => [1.0, 0.0, 0.0, 1.0, 0.0, l(0, h)?],
            "scale" => {
                let sx = n(0)?;
                [sx, 0.0, 0.0, n(1).unwrap_or(sx), 0.0, 0.0]
            }
            "scalex" => [n(0)?, 0.0, 0.0, 1.0, 0.0, 0.0],
            "scaley" => [1.0, 0.0, 0.0, n(0)?, 0.0, 0.0],
            "rotate" => {
                let r = a(0)?;
                [r.cos(), r.sin(), -r.sin(), r.cos(), 0.0, 0.0]
            }
            "skew" => [1.0, a(1).unwrap_or(0.0).tan(), a(0)?.tan(), 1.0, 0.0, 0.0],
            "skewx" => [1.0, 0.0, a(0)?.tan(), 1.0, 0.0, 0.0],
            "skewy" => [1.0, a(0)?.tan(), 0.0, 1.0, 0.0, 0.0],
            "matrix" => [n(0)?, n(1)?, n(2)?, n(3)?, n(4)?, n(5)?],
            _ => return None,
        };
        m = mul(m, t);
    }
    Some(m)
}

/// `content` of a pseudo-element: strings, `attr(x)`, quotes. None = no box (`none`, `normal`, invalid).
pub fn content(v: &str, attr: impl Fn(&str) -> Option<String>, counter: impl Fn(&str, Option<&str>, &str) -> String) -> Option<String> {
    let mut out = String::new();
    let mut chars = v.trim().chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                any = true;
                while let Some(ch) = chars.next() {
                    match ch {
                        q if q == c => break,
                        '\\' => {
                            // \201C, \a (newline), \"
                            let mut hex = String::new();
                            while hex.len() < 6 && chars.peek().is_some_and(|h| h.is_ascii_hexdigit()) {
                                hex.push(chars.next().unwrap());
                            }
                            if hex.is_empty() {
                                if let Some(e) = chars.next() {
                                    out.push(e);
                                }
                            } else {
                                if chars.peek() == Some(&' ') {
                                    chars.next();
                                }
                                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                            }
                        }
                        ch => out.push(ch),
                    }
                }
            }
            c if c.is_whitespace() => {}
            _ => {
                // a keyword or function
                let mut word = String::from(c);
                while let Some(&ch) = chars.peek() {
                    if ch.is_whitespace() && !word.contains('(') || word.ends_with(')') {
                        break;
                    }
                    word.push(ch);
                    chars.next();
                }
                let w = word.to_ascii_lowercase();
                match w.as_str() {
                    "none" | "normal" => return None,
                    "open-quote" => out.push('“'),
                    "close-quote" => out.push('”'),
                    "no-open-quote" | "no-close-quote" => {}
                    _ if w.starts_with("attr(") => out.push_str(&attr(w[5..].trim_end_matches(')').trim()).unwrap_or_default()),
                    _ if w.starts_with("counter(") || w.starts_with("counters(") => {
                        // counter(name[, style]) / counters(name, "sep"[, style])
                        let multi = w.starts_with("counters(");
                        let inner = word[if multi { 9 } else { 8 }..].trim_end_matches(')');
                        let args: Vec<&str> = crate::css::split_top_level(inner, ',').map(str::trim).collect();
                        let name = args.first().copied().unwrap_or("");
                        let (sep, style) = if multi {
                            (args.get(1).map(|s| s.trim_matches(|c| c == '"' || c == '\'')), args.get(2).copied().unwrap_or("decimal"))
                        } else {
                            (None, args.get(1).copied().unwrap_or("decimal"))
                        };
                        out.push_str(&counter(name, sep, &style.to_ascii_lowercase()));
                    }
                    _ => return None,
                }
                any = true;
            }
        }
    }
    any.then_some(out)
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
        Len::Calc(px, p) => Some(LineHeight::Px(px + em * p / 100.0)),
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
            s.inline_block = matches!(lv.as_str(), "inline-block" | "inline-flex" | "inline-grid" | "inline-table");
            s.display = match lv.as_str() {
                "block" | "flow-root" | "inline-block" => Display::Block,
                "inline" => Display::Inline,
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
            // layers: image (url / gradient), position / size, repeat, origin / clip; the color in the last one
            s.background = None;
            let mut bg = Background::default();
            for layer in crate::css::split_top_level(v, ',') {
                let (pos_part, size_part) = split_slash(layer);
                // after the `/`: one or two size words, then the layer goes on (`/ cover no-repeat`)
                let (mut size_words, mut rest) = (Vec::new(), Vec::new());
                for w in size_part.map(split_values).unwrap_or_default() {
                    let lw = w.to_ascii_lowercase();
                    let is_size = matches!(lw.as_str(), "cover" | "contain" | "auto") || len(&lw, em, rem).is_some();
                    if is_size && rest.is_empty() && size_words.len() < 2 && !(size_words.first().is_some_and(|f: &String| f == "cover" || f == "contain")) {
                        size_words.push(lw);
                    } else {
                        rest.push(w);
                    }
                }
                let (mut pos_words, mut image, mut rep, mut boxes, mut fixed) = (Vec::new(), None, (true, true), Vec::new(), false);
                for word in split_values(pos_part).into_iter().chain(rest) {
                    let lw = word.to_ascii_lowercase();
                    if lw.contains("gradient(") {
                        image = gradient(&word, em, rem, s.color).map(|g| BgSource::Gradient(Rc::new(g)));
                    } else if lw.starts_with("url(") {
                        image = url(&word).map(|u| BgSource::Url(u.into()));
                    } else if let Some(r) = repeat(&lw) {
                        rep = r;
                    } else if let Some(b) = box_area(&lw) {
                        boxes.push(b);
                    } else if lw == "fixed" {
                        fixed = true;
                    } else if lw == "text" {
                        boxes.push(BoxArea::Text);
                    } else if matches!(lw.as_str(), "scroll" | "local" | "none") {
                    } else if let Some(c) = css_color(&word, s.color) {
                        s.background = Some(c).filter(|c| c.3 > 0);
                    } else {
                        pos_words.push(word);
                    }
                }
                if let Some(image) = image {
                    bg.images.push(image);
                    bg.positions.push(if pos_words.is_empty() { (Len::Pct(0.0), Len::Pct(0.0)) } else { position(&pos_words.join(" "), em, rem) });
                    bg.sizes.push((!size_words.is_empty()).then(|| bg_size(&size_words.join(" "), em, rem)).flatten().unwrap_or(BgSize::Auto));
                    bg.repeats.push(rep);
                    bg.origins.push(boxes.first().copied().unwrap_or(BoxArea::Padding));
                    bg.clips.push(boxes.get(1).or(boxes.first()).copied().unwrap_or(BoxArea::Border));
                    bg.fixed.push(fixed);
                }
            }
            s.bg = (!bg.images.is_empty()).then(|| Rc::new(bg));
        }
        "background-image" => {
            let images: Vec<BgSource> = crate::css::split_top_level(v, ',')
                .filter_map(|l| {
                    let l = l.trim();
                    if l.to_ascii_lowercase().contains("gradient(") {
                        gradient(l, em, rem, s.color).map(|g| BgSource::Gradient(Rc::new(g)))
                    } else {
                        url(l).map(|u| BgSource::Url(u.into()))
                    }
                })
                .collect();
            bg_mut(s).images = images;
        }
        "background-size" => bg_mut(s).sizes = crate::css::split_top_level(v, ',').filter_map(|l| bg_size(l, em, rem)).collect(),
        "background-position" => bg_mut(s).positions = crate::css::split_top_level(v, ',').map(|l| position(l, em, rem)).collect(),
        "background-repeat" => {
            bg_mut(s).repeats = crate::css::split_top_level(&lv, ',')
                .filter_map(|l| match l.split_whitespace().collect::<Vec<_>>().as_slice() {
                    [one] => repeat(one),
                    [x, y] => Some((*x != "no-repeat", *y != "no-repeat")),
                    _ => None,
                })
                .collect()
        }
        "background-origin" => bg_mut(s).origins = crate::css::split_top_level(&lv, ',').filter_map(|l| box_area(l.trim())).collect(),
        "background-clip" | "-webkit-background-clip" => {
            bg_mut(s).clips = crate::css::split_top_level(&lv, ',').filter_map(|l| if l.trim() == "text" { Some(BoxArea::Text) } else { box_area(l.trim()) }).collect()
        }
        "background-attachment" => bg_mut(s).fixed = crate::css::split_top_level(&lv, ',').map(|l| l.trim() == "fixed").collect(),
        // the text color, over `color` (gradient text: `-webkit-text-fill-color: transparent`)
        "-webkit-text-fill-color" => {
            if let Some(c) = css_color(v, s.color) {
                s.color = c;
            }
        }
        "counter-reset" => s.counter_reset = counter_list(&lv, 0),
        "counter-increment" => s.counter_increment = counter_list(&lv, 1),
        "counter-set" => s.counter_set = counter_list(&lv, 0),
        "page" => s.page = (lv != "auto").then(|| v.trim().into()),
        "box-shadow" => s.shadows = if lv == "none" { None } else { shadows(v, em, rem, s.color).map(Rc::new) },
        "direction" => s.rtl = lv == "rtl",
        "content" => s.content = Some(v.trim().into()),
        "transform" => s.transform_fn = (lv != "none").then(|| v.trim().into()),
        "transform-origin" => s.transform_origin = Some(v.trim().into()),
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
            // a string marker (`"→ "`) keeps its spaces and case
            if let Some(q) = v.find(['"', '\'']) {
                if let Some(end) = v[q + 1..].find(&v[q..q + 1]) {
                    s.list_style = v[q..q + end + 2].into();
                }
            } else if let Some(t) = split_values(&lv).into_iter().find(|p| !matches!(p.as_str(), "inside" | "outside") && !p.starts_with("url(")) {
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
                "top" => VAlign::Top,
                "middle" => VAlign::Middle,
                "bottom" => VAlign::Bottom,
                "sub" => VAlign::Sub,
                "super" => VAlign::Super,
                "text-top" => VAlign::TextTop,
                "text-bottom" => VAlign::TextBottom,
                _ => match len(&lv, em, rem) {
                    Some(Len::Px(px)) => VAlign::Length(px),
                    Some(Len::Pct(p)) => VAlign::Length(s.line_height_px() * p / 100.0),
                    _ => VAlign::Baseline,
                },
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
    fn custom_properties() {
        crate::fonts::init_for_tests();
        let root = Style::root("Helvetica", 12.0);
        let decls = parse_declarations("--gap: 4px; --pad: calc(var(--gap) * 2); padding: var(--pad); margin: var(--nope, 3px); width: var(--nope)");
        let refs: Vec<&Decl> = decls.iter().collect();
        let s = Style::compute(&root, &refs, 12.0);
        assert_eq!(s.padding[0], Len::Px(8.0));
        assert_eq!(s.margin[0], Len::Px(3.0)); // fallback
        assert_eq!(s.size[0], Len::Auto); // undefined, no fallback: declaration dropped
        // inherited by children
        let child = Style::compute(&s, &[], 12.0);
        assert_eq!(child.vars.as_ref().unwrap().get("--gap").map(String::as_str), Some("4px"));
        // a cycle does not hang
        let decls = parse_declarations("--a: var(--b); --b: var(--a); width: var(--a)");
        let refs: Vec<&Decl> = decls.iter().collect();
        assert_eq!(Style::compute(&root, &refs, 12.0).size[0], Len::Auto);
    }

    #[test]
    fn calc_and_friends() {
        let l = |v| len(v, 10.0, 12.0);
        assert_eq!(l("calc(10px + 2em)"), Some(Len::Px(30.0)));
        assert_eq!(l("calc(50% / 2)"), Some(Len::Pct(25.0)));
        assert_eq!(l("calc(100% - 20px)"), Some(Len::Calc(-20.0, 100.0)));
        assert_eq!(l("calc((1rem + 3px) * 2)"), Some(Len::Px(30.0)));
        assert_eq!(l("calc(2 * -5px)"), Some(Len::Px(-10.0)));
        assert_eq!(l("calc(10px * 2px)"), None);
        assert_eq!(l("min(10px, 2em, 50px)"), Some(Len::Px(10.0)));
        assert_eq!(l("clamp(5px, 100px, 30px)"), Some(Len::Px(30.0)));
        assert_eq!(resolve(Len::Calc(-20.0, 100.0), 200.0), 180.0);
    }

    #[test]
    fn gradients() {
        let black = Rgba(0, 0, 0, 255);
        let g = gradient("linear-gradient(to right, red, rgb(0, 0, 255) 80%)", 12.0, 12.0, black).unwrap();
        assert_eq!(g.kind, GradientKind::Linear(90.0));
        assert_eq!(g.stops, vec![(Rgba(255, 0, 0, 255), None), (Rgba(0, 0, 255, 255), Some(Len::Pct(80.0)))]);
        assert_eq!(gradient("linear-gradient(45deg, red, blue)", 12.0, 12.0, black).unwrap().kind, GradientKind::Linear(45.0));
        assert_eq!(gradient("linear-gradient(to top left, red, blue)", 12.0, 12.0, black).unwrap().kind, GradientKind::LinearCorner(-1, -1));
        let r = gradient("radial-gradient(circle at top left, red, blue)", 12.0, 12.0, black).unwrap();
        assert_eq!(r.kind, GradientKind::Radial { circle: true, at: (Len::Pct(0.0), Len::Pct(0.0)) });
        assert!(gradient("repeating-linear-gradient(red 0, blue 10px)", 12.0, 12.0, black).unwrap().repeating);
        assert!(gradient("linear-gradient(red)", 12.0, 12.0, black).is_none());
        let s = computed("background: #eee linear-gradient(red, blue)");
        assert!(matches!(s.bg.as_ref().unwrap().images[0], BgSource::Gradient(_)) && s.background == Some(Rgba(238, 238, 238, 255)));
    }

    #[test]
    fn transforms() {
        let m = transform_matrix("translate(10px, 50%) scale(2)", 100.0, 40.0, 12.0).unwrap();
        assert_eq!(m, [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
        let r = transform_matrix("rotate(90deg)", 10.0, 10.0, 12.0).unwrap();
        assert!((r[0]).abs() < 1e-6 && (r[1] - 1.0).abs() < 1e-6 && (r[2] + 1.0).abs() < 1e-6);
        assert_eq!(transform_matrix("none", 1.0, 1.0, 12.0), None);
        assert_eq!(transform_matrix("wobble(3)", 1.0, 1.0, 12.0), None);
        assert_eq!(position("right bottom", 12.0, 12.0), (Len::Pct(100.0), Len::Pct(100.0)));
        assert_eq!(position("top", 12.0, 12.0), (Len::Pct(50.0), Len::Pct(0.0)));
    }

    #[test]
    fn counters_and_vertical_align() {
        let s = computed("counter-reset: chapter section 3; counter-increment: section -1; vertical-align: 3px");
        assert_eq!(s.counter_reset.as_deref(), Some(&vec![("chapter".to_string(), 0), ("section".to_string(), 3)]));
        assert_eq!(s.counter_increment.as_deref(), Some(&vec![("section".to_string(), -1)]));
        assert_eq!(s.vertical_align, VAlign::Length(3.0));
        assert_eq!(format_counter(4, "upper-roman"), "IV");
        assert_eq!(format_counter(28, "lower-alpha"), "ab");
        assert_eq!(format_counter(7, "decimal-leading-zero"), "07");
        assert_eq!(computed("list-style-type: '→ '").list_style.as_ref(), "'→ '");
    }

    #[test]
    fn generated_content() {
        let attr = |a: &str| (a == "href").then(|| "https://x.test".to_string());
        let counter = |name: &str, sep: Option<&str>, style: &str| match sep {
            Some(sep) => [1, 2].map(|n| format_counter(n, style)).join(sep),
            None => format!("{name}:{}", format_counter(3, style)),
        };
        assert_eq!(content(r#""(" attr(href) ")""#, attr, counter).as_deref(), Some("(https://x.test)"));
        assert_eq!(content(r#""\201C" open-quote 'a\'b'"#, attr, counter).as_deref(), Some("\u{201C}“a'b"));
        assert_eq!(content("\"\"", attr, counter).as_deref(), Some(""));
        assert_eq!(content("none", attr, counter), None);
        assert_eq!(content("counter(item) \". \"", attr, counter).as_deref(), Some("item:3. "));
        assert_eq!(content("counter(item, upper-roman)", attr, counter).as_deref(), Some("item:III"));
        assert_eq!(content("counters(item, \".\") \" \"", attr, counter).as_deref(), Some("1.2 "));
    }

    #[test]
    fn backgrounds_and_shadows() {
        let s = computed("background: #fff url('img/a.png') no-repeat right 10px top / 50% auto");
        let bg = s.bg.clone().unwrap();
        assert_eq!(bg.images, vec![BgSource::Url("img/a.png".into())]);
        assert_eq!(s.background, Some(Rgba(255, 255, 255, 255)));
        assert_eq!(bg.repeats, vec![(false, false)]);
        assert_eq!(bg.sizes, vec![BgSize::Size(Len::Pct(50.0), Len::Auto)]);
        assert_eq!(bg.positions, vec![(Len::Calc(-10.0, 100.0), Len::Pct(0.0))]);
        let s = computed("background: #222 url(a.png) center / contain no-repeat");
        let bg = s.bg.clone().unwrap();
        assert_eq!((bg.sizes[0], bg.repeats[0], s.background), (BgSize::Contain, (false, false), Some(Rgba(34, 34, 34, 255))));
        let s = computed("background-image: url(a.png); background-size: cover; background-position: center; background-repeat: repeat-x");
        let bg = s.bg.clone().unwrap();
        assert_eq!((bg.sizes[0], bg.positions[0], bg.repeats[0]), (BgSize::Cover, (Len::Pct(50.0), Len::Pct(50.0)), (true, false)));
        // several layers, cycled lists, origin / clip
        let s = computed("background: url(a.png) no-repeat content-box, linear-gradient(red, blue) padding-box border-box, #eee");
        let bg = s.bg.clone().unwrap();
        assert_eq!(bg.images.len(), 2);
        assert_eq!((bg.origins.clone(), bg.clips.clone()), (vec![BoxArea::Content, BoxArea::Padding], vec![BoxArea::Content, BoxArea::Border]));
        let s = computed("background: linear-gradient(red, blue) fixed; -webkit-background-clip: text");
        let bg = s.bg.clone().unwrap();
        assert_eq!((bg.clips.clone(), bg.fixed.clone()), (vec![BoxArea::Text], vec![true]));
        let s = computed("background-image: url(a.png), url(b.png), url(c.png); background-repeat: no-repeat, repeat");
        let bg = s.bg.clone().unwrap();
        assert_eq!(Background::nth(&bg.repeats, 2, (true, true)), (false, false));
        let s = computed("color: red; box-shadow: 0 2px 4px rgba(0, 0, 0, .5), inset 1px 1px blue, 3px 3px 0 2px");
        let sh = s.shadows.unwrap();
        assert_eq!(sh[0], Shadow { dx: 0.0, dy: 2.0, blur: 4.0, spread: 0.0, color: Rgba(0, 0, 0, 128), inset: false });
        assert!(sh[1].inset && sh[1].color == Rgba(0, 0, 255, 255));
        assert_eq!((sh[2].spread, sh[2].color), (2.0, Rgba(255, 0, 0, 255)));
        assert!(computed("box-shadow: 1px").shadows.is_none());
    }

    #[test]
    fn direction() {
        assert!(computed("direction: rtl").rtl);
    }

    #[test]
    fn units_for_taffy() {
        assert_eq!(px_units("1fr 2em repeat(2, 10pt)", 10.0, 10.0), "1fr 20px repeat(2, 10px)");
    }
}
