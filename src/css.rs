//! Minimal CSS: stylesheet / declaration parsing, specificity, cascade.
//! Selectors are matched by `scraper` (Servo's `selectors` crate), so anything it parses works.

use scraper::{ElementRef, Selector};

pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

pub struct Rule {
    pub selector: Selector,
    /// (ids, classes/attributes/pseudo-classes, types), compared lexicographically
    pub specificity: (u16, u16, u16),
    pub decls: Vec<Decl>,
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
fn split_top_level(s: &str, sep: char) -> impl Iterator<Item = &str> {
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

/// Rules of a stylesheet, in source order. At-rules (@media, @font-face, @page...) are skipped.
pub fn parse_stylesheet(css: &str, rules: &mut Vec<Rule>) {
    let css = strip_comments(css);
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let prelude = rest[..open].trim();
        // matching close brace (at-rules nest blocks)
        let (mut depth, mut close) = (0, rest.len());
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &rest[open + 1..close.min(rest.len())];
        rest = rest.get(close + 1..).unwrap_or("");
        if prelude.starts_with('@') {
            continue;
        }
        let decls = parse_declarations(body);
        if decls.is_empty() {
            continue;
        }
        // one rule per selector of the list: each has its own specificity
        for sel in split_top_level(prelude, ',') {
            let sel = sel.trim();
            if let Ok(selector) = Selector::parse(sel) {
                let decls = decls.iter().map(|d| Decl { prop: d.prop.clone(), value: d.value.clone(), important: d.important }).collect();
                rules.push(Rule { selector, specificity: specificity(sel), decls });
            }
        }
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

/// Winning declarations for an element: sorted by (important, origin, specificity, source order),
/// inline `style` beats every stylesheet rule. Later entries win.
pub fn matched<'a>(el: ElementRef, ua: &'a [Rule], author: &'a [Rule], inline: &'a [Decl]) -> Vec<&'a Decl> {
    // (important, origin, specificity, order)
    let mut out: Vec<((bool, u8, (u16, u16, u16), usize), &Decl)> = Vec::new();
    let mut order = 0;
    for (origin, rules) in [(0u8, ua), (1, author)] {
        for rule in rules {
            if rule.selector.matches(&el) {
                for d in &rule.decls {
                    out.push(((d.important, origin, rule.specificity, order), d));
                    order += 1;
                }
            }
        }
    }
    for d in inline {
        out.push(((d.important, 2, (u16::MAX, 0, 0), order), d));
        order += 1;
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

    #[test]
    fn stylesheet_skips_at_rules() {
        let mut rules = Vec::new();
        parse_stylesheet("/* c */ @media print { p { color: red } } h1, .a { color: blue } @font-face { x: y }", &mut rules);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].specificity, (0, 0, 1));
        assert_eq!(rules[1].specificity, (0, 1, 0));
    }

    #[test]
    fn specificities() {
        assert_eq!(specificity("#a .b p"), (1, 1, 1));
        assert_eq!(specificity("ul li:first-child"), (0, 1, 2));
        assert_eq!(specificity("a[href^=http]"), (0, 1, 1));
        assert_eq!(specificity("p::before"), (0, 0, 2));
    }
}
