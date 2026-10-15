//! Minimal CSS: stylesheet / declaration parsing, specificity, cascade.
//! Selectors are matched by `scraper` (Servo's `selectors` crate), so anything it parses works.

use scraper::{ElementRef, Selector};

#[derive(Clone, Debug)]
pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pseudo {
    Before,
    After,
    Marker,
    FirstLetter,
    FirstLine,
}

/// An `@page` rule: page properties, and its margin boxes (`@top-center { content: ... }`).
#[derive(Clone, Debug, Default)]
pub struct PageRule {
    /// `@page wide` (pages of elements with `page: wide`); None for every page
    pub name: Option<String>,
    /// `:first`, `:left`, `:right`; None for `@page` itself
    pub pseudo: Option<String>,
    pub decls: Vec<Decl>,
    /// (`top-left`, `top-center`, `bottom-right`...) -> declarations
    pub boxes: Vec<(String, Vec<Decl>)>,
}

pub struct Rule {
    pub selector: Selector,
    /// (ids, classes/attributes/pseudo-classes, types), compared lexicographically
    pub specificity: (u16, u16, u16),
    /// rule of `sel::before` / `sel::after`: matches `sel`, styles its generated content
    pub pseudo: Option<Pseudo>,
    pub decls: Vec<Decl>,
}

/// What a stylesheet contributes: style rules, `@page` and `@font-face` declarations.
#[derive(Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    pub pages: Vec<PageRule>,
    pub font_faces: Vec<Vec<Decl>>,
    pub has_pseudo: bool,
}

/// The medium @media queries are evaluated against: print, a page area of this size (pt).
#[derive(Clone, Copy, Debug)]
pub struct Media {
    pub width: f32,
    pub height: f32,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i + 2..].find("*/").map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    out.push_str(rest);
    out
}

/// `color: red; margin: 0 !important` -> declarations, property names lowercased
pub fn parse_declarations(css: &str) -> Vec<Decl> {
    split_top_level(css, ';')
        .filter_map(|d| {
            let (prop, value) = d.split_once(':')?;
            let prop = prop.trim().to_ascii_lowercase();
            let mut value = value.trim();
            let important = value.to_ascii_lowercase().ends_with("!important");
            if important {
                value = value[..value.len() - "!important".len()].trim_end();
            }
            (!prop.is_empty() && !value.is_empty()).then(|| Decl { prop, value: value.to_string(), important })
        })
        .collect()
}

/// split on `sep` outside parentheses and quotes (`url(a;b)`, `"a;b"`)
pub fn split_top_level(s: &str, sep: char) -> impl Iterator<Item = &str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, _) if c == sep && depth <= 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts.into_iter()
}

/// Rules of a stylesheet, in source order. `@media` blocks that match `media` are included,
/// `@page` / `@font-face` collected, other at-rules (`@import`, `@keyframes`...) skipped.
pub fn parse_stylesheet(css: &str, media: &Media, sheet: &mut Sheet) {
    parse_block(&strip_comments(css), media, sheet, 0);
}

fn parse_block(css: &str, media: &Media, sheet: &mut Sheet, depth: usize) {
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        // statements before the block (`@import url(x);`) are not part of the prelude
        let prelude = rest[..open].rsplit(';').next().unwrap_or("").trim();
        // matching close brace (at-rules nest blocks)
        let (mut d, mut close) = (0, rest.len());
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => d += 1,
                '}' => {
                    d -= 1;
                    if d == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &rest[open + 1..close.min(rest.len())];
        rest = rest.get(close + 1..).unwrap_or("");
        if let Some(at) = prelude.strip_prefix('@') {
            let (name, args) = at.split_once(|c: char| c.is_whitespace()).unwrap_or((at, ""));
            match name.to_ascii_lowercase().as_str() {
                "media" if depth < 8 && media_matches(args, media) => parse_block(body, media, sheet, depth + 1),
                "supports" if depth < 8 => parse_block(body, media, sheet, depth + 1),
                // the default page and `:first`; `:left` / `:right` / named pages are not supported
                "page" => {
                    // `@page`, `@page :first`, `@page wide`, `@page wide:left`
                    let args = args.trim();
                    let (name, pseudo) = args.split_once(':').map_or((args, None), |(n, p)| (n, Some(format!(":{}", p.trim().to_ascii_lowercase()))));
                    if pseudo.as_deref().is_none_or(|p| matches!(p, ":first" | ":left" | ":right")) {
                        let mut rule = parse_page(body, pseudo);
                        rule.name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
                        sheet.pages.push(rule);
                    }
                }
                "font-face" => sheet.font_faces.push(parse_declarations(body)),
                _ => {}
            }
            continue;
        }
        let decls = parse_declarations(body);
        if decls.is_empty() {
            continue;
        }
        // one rule per selector of the list: each has its own specificity
        for sel in split_top_level(prelude, ',') {
            let sel = sel.trim();
            let lower = sel.to_ascii_lowercase();
            let (base, pseudo) = if let Some(b) = lower.strip_suffix("::before").or(lower.strip_suffix(":before")) {
                (&sel[..b.len()], Some(Pseudo::Before))
            } else if let Some(b) = lower.strip_suffix("::after").or(lower.strip_suffix(":after")) {
                (&sel[..b.len()], Some(Pseudo::After))
            } else if let Some(b) = lower.strip_suffix("::marker") {
                (&sel[..b.len()], Some(Pseudo::Marker))
            } else if let Some(b) = lower.strip_suffix("::first-letter").or(lower.strip_suffix(":first-letter")) {
                (&sel[..b.len()], Some(Pseudo::FirstLetter))
            } else if let Some(b) = lower.strip_suffix("::first-line").or(lower.strip_suffix(":first-line")) {
                (&sel[..b.len()], Some(Pseudo::FirstLine))
            } else if lower.contains("::") {
                continue; // ::marker, ::first-line, ::placeholder...: not generated
            } else {
                (sel, None)
            };
            let base = if base.trim().is_empty() || base.ends_with(' ') || base.ends_with('>') { format!("{base}*") } else { base.to_string() };
            if let Ok(selector) = Selector::parse(&base) {
                let mut spec = specificity(&base);
                if pseudo.is_some() {
                    spec.2 += 1;
                    sheet.has_pseudo = true;
                }
                sheet.rules.push(Rule { selector, specificity: spec, pseudo, decls: decls.clone() });
            }
        }
    }
}

/// `@page` body: declarations, and nested margin boxes `@top-center { ... }`
fn parse_page(body: &str, pseudo: Option<String>) -> PageRule {
    let mut rule = PageRule { pseudo, ..PageRule::default() };
    let mut decls = String::new();
    let mut rest = body;
    while let Some(at) = rest.find('@') {
        decls.push_str(&rest[..at]);
        let Some(open) = rest[at..].find('{').map(|o| at + o) else { break };
        let Some(close) = rest[open..].find('}').map(|c| open + c) else { break };
        let name = rest[at + 1..open].trim().to_ascii_lowercase();
        rule.boxes.push((name, parse_declarations(&rest[open + 1..close])));
        rest = &rest[close + 1..];
    }
    decls.push_str(rest);
    rule.decls = parse_declarations(&decls);
    rule
}

/// `print`, `screen and (min-width: 600px)`, `(orientation: landscape)`, `not print`, lists with commas.
pub fn media_matches(query: &str, m: &Media) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    split_top_level(&q, ',').any(|one| {
        let one = one.trim();
        let (not, one) = match one.strip_prefix("not ") {
            Some(rest) => (true, rest),
            None => (false, one.strip_prefix("only ").unwrap_or(one)),
        };
        let mut ok = true;
        for part in one.split(" and ") {
            let part = part.trim();
            ok &= if let Some(feature) = part.strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
                media_feature(feature.trim(), m)
            } else {
                matches!(part, "all" | "print" | "")
            };
        }
        ok != not
    })
}

fn media_feature(f: &str, m: &Media) -> bool {
    let px = |v: &str| -> Option<f32> {
        let v = v.trim();
        let unit = v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len());
        let n: f32 = v[..unit].trim().parse().ok()?;
        Some(match &v[unit..] {
            "em" | "rem" => n * 16.0,
            "in" => n * 72.0,
            "cm" => n * 72.0 / 2.54,
            "mm" => n * 72.0 / 25.4,
            _ => n,
        })
    };
    // range syntax: `width >= 600px`
    for op in [">=", "<=", ">", "<"] {
        if let Some((name, v)) = f.split_once(op) {
            let actual = if name.trim() == "width" { m.width } else if name.trim() == "height" { m.height } else { return false };
            let Some(v) = px(v) else { return false };
            return match op {
                ">=" => actual >= v,
                "<=" => actual <= v,
                ">" => actual > v,
                _ => actual < v,
            };
        }
    }
    let (name, value) = f.split_once(':').map_or((f, ""), |(a, b)| (a.trim(), b.trim()));
    match name {
        "min-width" => px(value).is_some_and(|v| m.width >= v),
        "max-width" => px(value).is_some_and(|v| m.width <= v),
        "width" => px(value).is_some_and(|v| (m.width - v).abs() < 0.5),
        "min-height" => px(value).is_some_and(|v| m.height >= v),
        "max-height" => px(value).is_some_and(|v| m.height <= v),
        "orientation" => (value == "landscape") == (m.width > m.height),
        "color" | "monochrome" | "hover" | "pointer" | "prefers-color-scheme" | "prefers-reduced-motion" => {
            matches!((name, value), ("color", _) | ("prefers-color-scheme", "light") | ("prefers-reduced-motion", "reduce") | ("hover", "none") | ("pointer", "none"))
        }
        _ => false,
    }
}

/// Approximate specificity from the selector text (good enough for tag / .class / #id / [attr] / :pseudo).
fn specificity(sel: &str) -> (u16, u16, u16) {
    let (mut a, mut b, mut c) = (0, 0, 0);
    let bytes = sel.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let counted = match bytes[i] {
            b'#' => {
                a += 1;
                true
            }
            b'.' => {
                b += 1;
                true
            }
            b'[' => {
                b += 1;
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                false
            }
            b':' if bytes.get(i + 1) == Some(&b':') => {
                c += 1;
                i += 1;
                true
            }
            b':' => {
                b += 1;
                true
            }
            ch if ch.is_ascii_alphabetic() && (i == 0 || matches!(bytes[i - 1], b' ' | b'>' | b'+' | b'~' | b'(' | b',')) => {
                c += 1;
                true
            }
            _ => false,
        };
        i += 1;
        if counted {
            // skip the rest of the name
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_')) {
                i += 1;
            }
        }
    }
    (a, b, c)
}

/// Winning declarations for an element (or its `::before` / `::after`): sorted by
/// (important, origin, specificity, source order), inline `style` beats every stylesheet rule.
/// Later entries win.
pub fn matched<'a>(el: ElementRef, pseudo: Option<Pseudo>, ua: &'a [Rule], author: &'a [Rule], inline: &'a [Decl]) -> Vec<&'a Decl> {
    // (important, origin, specificity, order)
    let mut out: Vec<((bool, u8, (u16, u16, u16), usize), &Decl)> = Vec::new();
    let mut order = 0;
    for (origin, rules) in [(0u8, ua), (1, author)] {
        for rule in rules {
            if rule.pseudo == pseudo && rule.selector.matches(&el) {
                for d in &rule.decls {
                    out.push(((d.important, origin, rule.specificity, order), d));
                    order += 1;
                }
            }
        }
    }
    if pseudo.is_none() {
        for d in inline {
            out.push(((d.important, 2, (u16::MAX, 0, 0), order), d));
            order += 1;
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out.into_iter().map(|(_, d)| d).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations() {
        let d = parse_declarations("color: red; background: url(a;b) no-repeat ; margin:0 !important;;");
        assert_eq!(d.len(), 3);
        assert_eq!(d[1].value, "url(a;b) no-repeat");
        assert!(d[2].important && d[2].value == "0");
    }

    const A4: Media = Media { width: 515.0, height: 762.0 };

    #[test]
    fn at_rules() {
        let mut s = Sheet::default();
        parse_stylesheet(
            "@import url(x.css); /* c */ @media print { p { color: red } } @media screen { p { color: blue } }
             h1, .a { color: blue } @font-face { font-family: X; src: url(x.ttf) } @page { size: A5 } @page :first { margin: 0 }
             @keyframes k { from { color: red } } p::before { content: 'x' } li:after { content: '' } ::marker { color: red }",
            &A4,
            &mut s,
        );
        assert_eq!(s.rules.len(), 6); // p (print), h1, .a, p::before, li::after, *::marker
        assert_eq!(s.rules[5].pseudo, Some(Pseudo::Marker));
        assert_eq!(s.rules[1].specificity, (0, 0, 1));
        assert_eq!(s.rules[2].specificity, (0, 1, 0));
        assert_eq!(s.rules[3].pseudo, Some(Pseudo::Before));
        assert_eq!(s.rules[4].pseudo, Some(Pseudo::After));
        assert!(s.has_pseudo);
        assert_eq!((s.pages.len(), s.font_faces.len()), (2, 1));
        assert!(s.pages[0].pseudo.is_none() && s.pages[1].pseudo.as_deref() == Some(":first"));
        let mut s = Sheet::default();
        parse_stylesheet("@page { margin: 1cm; @top-center { content: 'T'; color: red } @bottom-right { content: counter(page) } size: A5 } li::marker { color: red } p::first-letter { font-size: 2em }", &A4, &mut s);
        let page = &s.pages[0];
        assert_eq!(page.decls.iter().map(|d| d.prop.as_str()).collect::<Vec<_>>(), ["margin", "size"]);
        assert_eq!(page.boxes.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), ["top-center", "bottom-right"]);
        assert_eq!(s.rules.iter().map(|r| r.pseudo).collect::<Vec<_>>(), [Some(Pseudo::Marker), Some(Pseudo::FirstLetter)]);
    }

    #[test]
    fn media_queries() {
        assert!(media_matches("print", &A4));
        assert!(media_matches("all and (min-width: 400px)", &A4));
        assert!(!media_matches("screen", &A4));
        assert!(media_matches("not screen", &A4));
        assert!(media_matches("screen, print", &A4));
        assert!(!media_matches("(max-width: 400px)", &A4));
        assert!(media_matches("(width >= 500px)", &A4));
        assert!(media_matches("(orientation: portrait)", &A4));
        assert!(!media_matches("(min-resolution: 2dppx)", &A4));
    }

    #[test]
    fn specificities() {
        assert_eq!(specificity("#a .b p"), (1, 1, 1));
        assert_eq!(specificity("ul li:first-child"), (0, 1, 2));
        assert_eq!(specificity("a[href^=http]"), (0, 1, 1));
        assert_eq!(specificity("p::before"), (0, 0, 2));
    }
}
