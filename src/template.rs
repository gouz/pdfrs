//! HTML templates: MiniJinja (Jinja / Twig syntax), filled with JSON data before rendering.
//!
//! Strict: a variable missing from the data is an error (use `| default(...)` for optional
//! fields). Every output is HTML-escaped (`| safe` opts out). A template can only read the
//! templates of its directory (`{% extends %}`, `{% include %}`), and its run time is bounded.

use minijinja::value::{Kwargs, Rest};
use minijinja::{AutoEscape, Environment, Error, ErrorKind, State, UndefinedBehavior, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Where the template comes from.
pub enum Source<'a> {
    /// the template's text, sent with the request
    Inline(&'a str),
    /// a file of the templates directory
    Named(&'a str),
}

/// Instructions a template may run (loops, filters...): enough for thousands of table rows,
/// not for an endless loop.
const FUEL: u64 = 10_000_000;

fn environment(dir: Option<&Path>) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.set_fuel(Some(FUEL));
    env.set_recursion_limit(100);
    if let Some(dir) = dir {
        let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let read = minijinja::path_loader(root.clone());
        env.set_loader(move |name| {
            // inside the directory only, symlinks included (path_loader already refuses `..`)
            let inside = root.join(name).canonicalize().is_ok_and(|p| p.starts_with(&root));
            if inside { read(name) } else { Ok(None) }
        });
    }
    env.add_filter("money", money);
    env.add_filter("number", number);
    env.add_filter("date", date);
    env.add_filter("nl2br", nl2br);
    env
}

/// One environment per templates directory: compiled templates are cached (and reloaded when
/// their file changes).
fn shared(dir: Option<&Path>) -> Arc<Environment<'static>> {
    static ENVS: OnceLock<std::sync::Mutex<Vec<(Option<PathBuf>, Arc<Environment<'static>>)>>> = OnceLock::new();
    let envs = ENVS.get_or_init(Default::default);
    let mut envs = envs.lock().unwrap_or_else(|e| e.into_inner());
    let key = dir.map(Path::to_path_buf);
    if let Some((_, env)) = envs.iter().find(|(d, _)| *d == key) {
        return env.clone();
    }
    let env = Arc::new(environment(dir));
    envs.push((key, env.clone()));
    env
}

/// Render a template with `data` (a JSON object), `locale` for the number / money / date filters.
/// The error says what went wrong and where (template name and line).
pub fn render(source: Source, data: &serde_json::Value, dir: Option<&Path>, locale: &str) -> Result<String, String> {
    let env = shared(dir);
    let mut ctx = match data {
        serde_json::Value::Object(_) => Value::from(minijinja::value::Serde(data)),
        serde_json::Value::Null => Value::from(minijinja::value::Serde(serde_json::json!({}))),
        _ => return Err("data: a JSON object is expected".into()),
    };
    // the locale is visible to the filters, not a variable of the data
    ctx = minijinja::context! { _locale => locale, ..ctx };
    let out = match source {
        Source::Inline(text) => env.render_str(text, ctx),
        Source::Named(name) => env.get_template(name).and_then(|t| t.render(ctx)),
    };
    out.map_err(describe)
}

fn describe(e: Error) -> String {
    let mut msg = match e.kind() {
        ErrorKind::UndefinedError => "undefined value (missing from the data? use `| default(...)` for optional fields)".to_string(),
        ErrorKind::TemplateNotFound => format!("template not found: {}", e.detail().unwrap_or("")),
        ErrorKind::OutOfFuel => "template too long to run (endless loop?)".to_string(),
        _ => e.to_string(),
    };
    if !matches!(e.kind(), ErrorKind::TemplateNotFound) {
        if let Some(detail) = e.detail().filter(|d| !msg.contains(*d)) {
            msg.push_str(&format!(": {detail}"));
        }
    }
    if let (Some(name), Some(line)) = (e.name(), e.line()) {
        msg.push_str(&format!(" ({name}, line {line})"));
    }
    msg
}

fn locale(state: &State, kwargs: &Kwargs) -> Result<String, Error> {
    let explicit: Option<String> = kwargs.get("locale")?;
    Ok(explicit.or_else(|| state.lookup("_locale").and_then(|v| v.as_str().map(String::from))).unwrap_or_else(|| "fr".into()))
}

/// Strict mode lets an undefined value reach a filter: report it as the missing variable it is.
fn defined(v: &Value) -> Result<(), Error> {
    if v.is_undefined() {
        Err(Error::new(ErrorKind::UndefinedError, "a filter got an undefined value"))
    } else {
        Ok(())
    }
}

fn as_number(v: &Value) -> Result<f64, Error> {
    defined(v)?;
    if let Some(s) = v.as_str() {
        return s.trim().replace(',', ".").parse().map_err(|_| Error::new(ErrorKind::InvalidOperation, format!("not a number: {s:?}")));
    }
    f64::try_from(v.clone()).map_err(|_| Error::new(ErrorKind::InvalidOperation, format!("not a number: {v}")))
}

/// Group the digits of `n` with `decimals` decimals, in the locale's style:
/// fr `1 234,50`, de `1.234,50`, en `1,234.50`, ch `1’234.50`.
pub fn format_number(n: f64, decimals: usize, locale: &str) -> String {
    let lang = locale.split(['-', '_']).next().unwrap_or("fr").to_ascii_lowercase();
    let (group, point) = match lang.as_str() {
        "en" | "ja" | "zh" | "ko" => (",", "."),
        "de" | "es" | "it" | "nl" | "pt" | "da" | "id" => (".", ","),
        _ if locale.to_ascii_lowercase().ends_with("ch") => ("’", "."),
        _ => ("\u{202f}", ","), // fr and most of Europe: narrow no-break space
    };
    // half away from zero (2.5 -> 3, 2.675 -> 2.68), not to even: what an invoice expects
    let factor = 10f64.powi(decimals.min(12) as i32);
    let rounded = format!("{:.*}", decimals, (n.abs() * factor * (1.0 + 1e-12)).round() / factor);
    let (int, frac) = rounded.split_once('.').map_or((rounded.as_str(), ""), |(i, f)| (i, f));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push_str(group);
        }
        grouped.push(c);
    }
    let sign = if n < 0.0 && rounded.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}{point}{frac}") }
}

/// `{{ total | money }}` → `1 234,50 €`; `money("USD")`, `money("EUR", locale="en")`, `money(decimals=0)`
fn money(state: &State, value: Value, currency: Option<String>, kwargs: Kwargs) -> Result<String, Error> {
    let loc = locale(state, &kwargs)?;
    let decimals: Option<usize> = kwargs.get("decimals")?;
    kwargs.assert_all_used()?;
    let n = as_number(&value)?;
    let code = currency.unwrap_or_else(|| "EUR".into()).to_ascii_uppercase();
    let symbol = match code.as_str() {
        "EUR" => "€",
        "USD" => "$",
        "GBP" => "£",
        "JPY" => "¥",
        "CHF" => "CHF",
        "CAD" => "$ CA",
        other => other,
    };
    let amount = format_number(n, decimals.unwrap_or(if code == "JPY" { 0 } else { 2 }), &loc);
    let lang = loc.split(['-', '_']).next().unwrap_or("fr").to_ascii_lowercase();
    Ok(if lang == "en" {
        // $1,234.50 / -$1,234.50
        match amount.strip_prefix('-') {
            Some(abs) => format!("-{symbol}{abs}"),
            None => format!("{symbol}{amount}"),
        }
    } else {
        format!("{amount}\u{a0}{symbol}")
    })
}

/// `{{ qty | number }}` → `1 234`; `number(2)` → `1 234,50`
fn number(state: &State, value: Value, decimals: Option<usize>, kwargs: Kwargs) -> Result<String, Error> {
    let loc = locale(state, &kwargs)?;
    kwargs.assert_all_used()?;
    Ok(format_number(as_number(&value)?, decimals.unwrap_or(0), &loc))
}

const MONTHS_FR: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS_FR: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const DAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// Day of the week of a date, 0 = Monday (Sakamoto's method).
fn weekday(y: i32, m: u32, d: u32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let sunday_based = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d as i32).rem_euclid(7);
    ((sunday_based + 6) % 7) as usize
}

/// `{{ "2026-10-15" | date }}` → `15/10/2026` (fr) / `10/15/2026` (en);
/// `date("%e %B %Y")` → `15 octobre 2026`. `%d %e %m %B %b %Y %y %A %H %M`.
/// Takes `YYYY-MM-DD`, optionally followed by `THH:MM[:SS]...` (ISO 8601).
fn date(state: &State, value: Value, format: Option<String>, kwargs: Kwargs) -> Result<String, Error> {
    let loc = locale(state, &kwargs)?;
    kwargs.assert_all_used()?;
    defined(&value)?;
    let text = value.as_str().map(String::from).unwrap_or_else(|| value.to_string());
    let bad = || Error::new(ErrorKind::InvalidOperation, format!("not a date (YYYY-MM-DD expected): {text:?}"));
    let (day_part, time_part) = text.split_once(['T', ' ']).map_or((text.as_str(), ""), |(d, t)| (d, t));
    let mut it = day_part.split('-');
    let (y, m, d): (i32, u32, u32) = (
        it.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?,
        it.next().and_then(|v| v.parse().ok()).filter(|m| (1..=12).contains(m)).ok_or_else(bad)?,
        it.next().and_then(|v| v.get(..2).unwrap_or(v).parse().ok()).filter(|d| (1..=31).contains(d)).ok_or_else(bad)?,
    );
    let mut t = time_part.split(':');
    let (hh, mm) = (t.next().and_then(|v| v.get(..2)?.parse::<u32>().ok()).unwrap_or(0), t.next().and_then(|v| v.get(..2)?.parse::<u32>().ok()).unwrap_or(0));
    let en = loc.to_ascii_lowercase().starts_with("en");
    let format = format.unwrap_or_else(|| if en { "%m/%d/%Y".into() } else { "%d/%m/%Y".into() });
    let (months, days) = if en { (MONTHS_EN, DAYS_EN) } else { (MONTHS_FR, DAYS_FR) };
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str(&format!("{d:02}")),
            Some('e') => out.push_str(&d.to_string()),
            Some('m') => out.push_str(&format!("{m:02}")),
            Some('B') => out.push_str(months[m as usize - 1]),
            Some('b') => out.extend(months[m as usize - 1].chars().take(3)),
            Some('Y') => out.push_str(&y.to_string()),
            Some('y') => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some('A') => out.push_str(days[weekday(y, m, d)]),
            Some('H') => out.push_str(&format!("{hh:02}")),
            Some('M') => out.push_str(&format!("{mm:02}")),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    Ok(out)
}

/// `{{ address | nl2br }}`: escaped text, line breaks as `<br>`
fn nl2br(value: Value, _rest: Rest<Value>) -> Result<Value, Error> {
    defined(&value)?;
    let text = value.as_str().map(String::from).unwrap_or_else(|| value.to_string());
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    Ok(Value::from_safe_string(escaped.replace("\r\n", "\n").replace('\n', "<br>\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// rendered, with `/` back from MiniJinja's `&#x2f;` (the HTML parser does the same)
    fn r(t: &str, data: serde_json::Value) -> Result<String, String> {
        render(Source::Inline(t), &data, None, "fr").map(|s| s.replace("&#x2f;", "/"))
    }

    #[test]
    fn variables_loops_and_escaping() {
        let out = r("{% for l in lignes %}<td>{{ l.nom }}</td>{% endfor %}", json!({ "lignes": [{ "nom": "<b>a</b> & b" }, { "nom": "c" }] })).unwrap();
        assert_eq!(out, "<td>&lt;b&gt;a&lt;/b&gt; &amp; b</td><td>c</td>");
        assert_eq!(r("{{ x | safe }}", json!({ "x": "<i>ok</i>" })).unwrap(), "<i>ok</i>");
    }

    #[test]
    fn strict() {
        let e = r("<p>{{ client.nom }}</p>", json!({ "client": {} })).unwrap_err();
        assert!(e.contains("undefined") && e.contains("line 1"), "{e}");
        assert!(r("{{ nom }}", json!({})).is_err());
        assert_eq!(r("{{ nom | default('anonyme') }}", json!({})).unwrap(), "anonyme");
        assert!(r("{% if remise is defined %}x{% endif %}", json!({})).is_ok());
        assert!(r("{{ x }}", json!([1])).is_err()); // data must be an object
        for f in ["money", "number", "date", "nl2br"] {
            let e = r(&format!("{{{{ absent | {f} }}}}"), json!({})).unwrap_err();
            assert!(e.contains("undefined"), "{f}: {e}");
        }
    }

    #[test]
    fn bounded() {
        let e = r("{% for i in range(100000) %}{% for j in range(100000) %}{% endfor %}{% endfor %}", json!({})).unwrap_err();
        assert!(e.contains("too long"), "{e}");
    }

    #[test]
    fn filters() {
        let d = json!({ "n": 1234.5, "neg": -3.456, "s": "2026-10-15", "dt": "2026-10-15T08:30:00Z" });
        assert_eq!(r("{{ n | money }}", d.clone()).unwrap(), "1\u{202f}234,50\u{a0}€");
        assert_eq!(r("{{ n | money('USD', locale='en') }}", d.clone()).unwrap(), "$1,234.50");
        assert_eq!(r("{{ neg | money('USD', locale='en') }}", d.clone()).unwrap(), "-$3.46");
        assert_eq!(r("{{ n | money(decimals=0, locale='de') }}", d.clone()).unwrap(), "1.235\u{a0}€");
        assert_eq!(r("{{ n | number }} {{ n | number(1) }}", d.clone()).unwrap(), "1\u{202f}235 1\u{202f}234,5");
        assert_eq!(r("{{ '12,5' | number(2) }}", d.clone()).unwrap(), "12,50");
        assert_eq!(r("{{ 2.675 | number(2) }} {{ 0.5 | number }} {{ -0.004 | money }}", d.clone()).unwrap(), "2,68 1 0,00\u{a0}€");
        assert_eq!(r("{{ s | date }} {{ s | date('%A %e %B %Y') }}", d.clone()).unwrap(), "15/10/2026 jeudi 15 octobre 2026");
        assert_eq!(r("{{ dt | date('%d/%m %H:%M', locale='en') }} {{ s | date(locale='en') }}", d.clone()).unwrap(), "15/10 08:30 10/15/2026");
        assert!(r("{{ 'hier' | date }}", d.clone()).is_err());
        assert_eq!(r("{{ a | nl2br }}", json!({ "a": "1 rue <X>\n75000 Paris" })).unwrap(), "1 rue &lt;X&gt;<br>\n75000 Paris");
        assert_eq!(r("{{ [1, 2.5] | sum | money }}", d).unwrap(), "3,50\u{a0}€");
    }

    /// the examples of the README's filter table
    #[test]
    fn readme_examples() {
        let en = |t: &str| render(Source::Inline(t), &json!({}), None, "en").map(|s| s.replace("&#x2f;", "/")).unwrap();
        assert_eq!(r("{{ 99 | money('USD') }}", json!({})).unwrap(), "99,00\u{a0}$");
        assert_eq!(en("{{ 1234.5 | money }}"), "€1,234.50");
        assert_eq!(en("{{ 99 | money('USD') }}"), "$99.00");
        assert_eq!(en("{{ 1234.5 | money(decimals=0) }}"), "€1,235");
        assert_eq!(r("{{ 1234.5 | money(decimals=0) }}", json!({})).unwrap(), "1\u{202f}235\u{a0}€");
        assert_eq!(en("{{ 1234.5 | number }} / {{ 1234.5 | number(2) }}"), "1,235 / 1,234.50");
        assert_eq!(en("{{ '2026-10-15' | date }}"), "10/15/2026");
        assert_eq!(en("{{ '2026-10-15' | date('%A %e %B %Y') }}"), "Thursday 15 October 2026");
    }

    #[test]
    fn templates_directory() {
        let root = std::env::temp_dir().join(format!("pdfrs-tpl-{}", std::process::id()));
        std::fs::create_dir_all(root.join("tpl/parts")).unwrap();
        std::fs::write(root.join("tpl/base.html"), "<h1>{% block title %}{% endblock %}</h1>{% include 'parts/foot.html' %}").unwrap();
        std::fs::write(root.join("tpl/parts/foot.html"), "<p>{{ societe }}</p>").unwrap();
        std::fs::write(root.join("tpl/facture.html"), "{% extends 'base.html' %}{% block title %}Facture {{ n }}{% endblock %}").unwrap();
        std::fs::write(root.join("secret.html"), "secret").unwrap();
        let _ = std::os::unix::fs::symlink(root.join("secret.html"), root.join("tpl/link.html"));
        let dir = root.join("tpl");
        let data = json!({ "n": 42, "societe": "ACME" });
        assert_eq!(render(Source::Named("facture.html"), &data, Some(&dir), "fr").unwrap(), "<h1>Facture 42</h1><p>ACME</p>");
        // inline templates can extend the directory's ones
        assert_eq!(render(Source::Inline("{% extends 'base.html' %}{% block title %}T{% endblock %}"), &data, Some(&dir), "fr").unwrap(), "<h1>T</h1><p>ACME</p>");
        for escape in ["../secret.html", "link.html", "/etc/passwd"] {
            let e = render(Source::Named(escape), &data, Some(&dir), "fr").unwrap_err();
            assert!(e.contains("not found"), "{escape}: {e}");
        }
        assert!(render(Source::Named("facture.html"), &data, None, "fr").is_err()); // no directory, no files
        std::fs::remove_dir_all(root).unwrap();
    }
}
