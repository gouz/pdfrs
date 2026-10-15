//! Image policy and loading. HTML may be untrusted, so by default only inline data: images load:
//!   - local files only if `base_dir` is set, and only inside it (no ../, absolute path or symlink escape)
//!   - http(s) never (this build does not fetch: rendering never makes network requests)
//!   - any other scheme (file:, ftp:, ...) never

use base64::Engine;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct ImagePolicy {
    pub base_dir: Option<PathBuf>,
}

pub enum Source {
    Data(Vec<u8>),
    File(PathBuf),
}

/// Resolve an <img src> under the policy; None = blocked.
pub fn resolve(src: &str, policy: &ImagePolicy) -> Option<Source> {
    let src = src.trim();
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("data:image/") {
        let (meta, data) = src.split_once(',')?;
        return if meta.to_ascii_lowercase().ends_with(";base64") {
            let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(clean).ok().map(Source::Data)
        } else {
            Some(Source::Data(percent_decode(data).into_bytes()))
        };
    }
    // any scheme (http:, file:, ...) is refused
    if lower.split_once(':').is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))) {
        return None;
    }
    let root = policy.base_dir.as_ref()?.canonicalize().ok()?;
    let path = percent_decode(src.split(['?', '#']).next()?);
    let file = root.join(Path::new(&path)).canonicalize().ok()?;
    (file.starts_with(&root) && file != root && file.is_file()).then_some(Source::File(file))
}

pub fn load(src: &str, policy: &ImagePolicy) -> Option<Vec<u8>> {
    match resolve(src, policy)? {
        Source::Data(d) => Some(d),
        Source::File(f) => std::fs::read(f).ok(),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn is_svg(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(512)]).to_ascii_lowercase();
    head.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        let root = std::env::temp_dir().join(format!("pdfrs-{}", std::process::id()));
        let site = root.join("site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("logo.png"), b"x").unwrap();
        std::fs::write(root.join("secret.png"), b"x").unwrap();
        let _ = std::os::unix::fs::symlink(root.join("secret.png"), site.join("link.png"));
        let p = ImagePolicy { base_dir: Some(site.clone()) };
        let none = ImagePolicy::default();
        let ok = |s: &str, p: &ImagePolicy| resolve(s, p).is_some();

        assert!(ok("data:image/png;base64,AA==", &none));
        assert!(!ok("logo.png", &none)); // no base dir: no local files
        assert!(ok("logo.png", &p));
        assert!(ok("./logo.png?v=2", &p));
        assert!(!ok("../secret.png", &p)); // ../ escape
        assert!(!ok(root.join("secret.png").to_str().unwrap(), &p)); // absolute path outside
        assert!(!ok("link.png", &p)); // symlink escape
        assert!(!ok("missing.png", &p));
        assert!(!ok("file:///etc/hosts", &p));
        assert!(!ok("https://x.test/a.png", &p));
        assert!(!ok(".", &p));
        std::fs::remove_dir_all(root).unwrap();
    }
}
