//! Remote resources (images, stylesheets, fonts) over http(s): off unless enabled.
//!
//! The HTML may be untrusted, so a fetch is the server making a request on behalf of whoever
//! wrote it (SSRF). Guards: http(s) only, addresses resolved to loopback / private / link-local /
//! unique-local / multicast are refused (checked on every connection, so redirects and DNS
//! rebinding too), optional host allow-list, timeout, size limit, at most 3 redirects.

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

#[derive(Clone, Debug)]
pub struct RemoteConfig {
    /// `example.com`, `*.example.com`; empty: any public host
    pub hosts: Vec<String>,
    /// allow loopback / private networks (tests, or a trusted internal network)
    pub allow_private: bool,
    pub timeout: Duration,
    pub max_bytes: u64,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig { hosts: Vec::new(), allow_private: false, timeout: Duration::from_secs(5), max_bytes: 10 * 1024 * 1024 }
    }
}

impl RemoteConfig {
    /// From `REMOTE_HOSTS` (comma separated), `REMOTE_ALLOW_PRIVATE=1`, `REMOTE_TIMEOUT_MS`, `REMOTE_MAX_MB`.
    pub fn from_env() -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let d = RemoteConfig::default();
        RemoteConfig {
            hosts: env("REMOTE_HOSTS").map(|h| h.split(',').map(|h| h.trim().to_ascii_lowercase()).filter(|h| !h.is_empty()).collect()).unwrap_or_default(),
            allow_private: env("REMOTE_ALLOW_PRIVATE").is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes")),
            timeout: env("REMOTE_TIMEOUT_MS").and_then(|v| v.parse().ok()).map_or(d.timeout, Duration::from_millis),
            max_bytes: env("REMOTE_MAX_MB").and_then(|v| v.parse::<u64>().ok()).map_or(d.max_bytes, |m| m * 1024 * 1024),
        }
    }

    fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.hosts.is_empty()
            || self.hosts.iter().any(|h| match h.strip_prefix("*.") {
                Some(domain) => host.ends_with(&format!(".{domain}")) || host == domain,
                None => host == *h,
            })
    }
}

/// Addresses a document must never make the server talk to.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] == 0x2001 && s[1] == 0xdb8)) // documentation
        }
    }
}

/// DNS resolution keeping only public addresses (unless private ones are allowed).
#[derive(Debug)]
struct GuardedResolver {
    inner: DefaultResolver,
    allow_private: bool,
}

impl Resolver for GuardedResolver {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addrs = self.inner.resolve(uri, config, timeout)?;
        if self.allow_private {
            return Ok(addrs);
        }
        let mut public = ResolvedSocketAddrs::from_fn(|_| std::net::SocketAddr::from(([0, 0, 0, 0], 0)));
        for a in addrs.iter().filter(|a| is_public(a.ip())) {
            public.push(*a);
        }
        if public.len() == 0 {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(public)
    }
}

fn agent(c: &RemoteConfig) -> &'static ureq::Agent {
    // one agent per private-address policy, built on first use (connection pooling across documents)
    static PUBLIC: OnceLock<ureq::Agent> = OnceLock::new();
    static PRIVATE: OnceLock<ureq::Agent> = OnceLock::new();
    let build = |allow_private: bool| {
        let config = ureq::Agent::config_builder()
            .max_redirects(0) // followed by hand, each one checked
            .http_status_as_error(false)
            .user_agent("pdfrs")
            .build();
        ureq::Agent::with_parts(config, DefaultConnector::default(), GuardedResolver { inner: DefaultResolver::default(), allow_private })
    };
    if c.allow_private { PRIVATE.get_or_init(|| build(true)) } else { PUBLIC.get_or_init(|| build(false)) }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') { host.split(']').next()?.trim_start_matches('[') } else { host.split(':').next()? };
    (!host.is_empty()).then(|| host.to_string())
}

/// GET `url` under the config: the body, or None (blocked, error, too big, not 2xx).
pub fn fetch(url: &str, c: &RemoteConfig) -> Option<Vec<u8>> {
    let mut url = url.to_string();
    for _ in 0..4 {
        let lower = url.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) || !c.host_allowed(&host_of(&url)?) {
            eprintln!("pdfrs: remote resource refused: {}", url.chars().take(100).collect::<String>());
            return None;
        }
        let mut res = match agent(c).get(&url).config().timeout_global(Some(c.timeout)).build().call() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("pdfrs: remote resource failed: {} ({e})", url.chars().take(100).collect::<String>());
                return None;
            }
        };
        let status = res.status().as_u16();
        if (300..400).contains(&status) {
            let location = res.headers().get("location")?.to_str().ok()?;
            url = if location.contains("://") {
                location.to_string()
            } else if location.starts_with('/') {
                let (scheme, rest) = url.split_once("://")?;
                format!("{scheme}://{}{location}", rest.split('/').next()?)
            } else {
                format!("{}/{location}", url.rsplit_once('/')?.0)
            };
            continue;
        }
        if !(200..300).contains(&status) {
            eprintln!("pdfrs: remote resource {status}: {}", url.chars().take(100).collect::<String>());
            return None;
        }
        return res.body_mut().with_config().limit(c.max_bytes).read_to_vec().ok();
    }
    None
}

thread_local! {
    /// fetched resources of the document being rendered on this thread
    static CACHE: RefCell<HashMap<String, Option<Arc<Vec<u8>>>>> = RefCell::new(HashMap::new());
}

/// Fetch once per document: the same URL used twice is downloaded once.
pub fn cached(url: &str, c: &RemoteConfig) -> Option<Vec<u8>> {
    if let Some(hit) = CACHE.with(|m| m.borrow().get(url).cloned()) {
        return hit.map(|d| (*d).clone());
    }
    let data = fetch(url, c).map(Arc::new);
    CACHE.with(|m| m.borrow_mut().insert(url.to_string(), data.clone()));
    data.map(|d| (*d).clone())
}

/// Download these URLs in parallel (8 at a time) into the document cache.
pub fn prefetch(urls: Vec<String>, c: &RemoteConfig) {
    let mut todo: Vec<String> = urls.into_iter().filter(|u| CACHE.with(|m| !m.borrow().contains_key(u))).collect();
    todo.sort();
    todo.dedup();
    for chunk in todo.chunks(8) {
        let fetched: Vec<(String, Option<Vec<u8>>)> =
            std::thread::scope(|s| chunk.iter().map(|u| s.spawn(move || (u.clone(), fetch(u, c)))).collect::<Vec<_>>().into_iter().filter_map(|h| h.join().ok()).collect());
        CACHE.with(|m| {
            let mut m = m.borrow_mut();
            for (u, d) in fetched {
                m.insert(u, d.map(Arc::new));
            }
        });
    }
}

/// Forget the document's downloads.
pub fn end_document() {
    CACHE.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_addresses() {
        for private in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn hosts() {
        let c = RemoteConfig { hosts: vec!["cdn.example.com".into(), "*.img.test".into()], ..RemoteConfig::default() };
        assert!(c.host_allowed("cdn.example.com") && c.host_allowed("a.img.test") && c.host_allowed("img.test"));
        assert!(!c.host_allowed("example.com") && !c.host_allowed("evilimg.test"));
        assert!(RemoteConfig::default().host_allowed("anything.test"));
        assert_eq!(host_of("https://user@Host.test:8080/a?b").as_deref(), Some("Host.test"));
        assert_eq!(host_of("http://[::1]:80/").as_deref(), Some("::1"));
    }

    #[test]
    fn refused_without_network() {
        let c = RemoteConfig::default();
        assert!(fetch("ftp://x.test/a", &c).is_none());
        assert!(fetch("http://127.0.0.1:9/a.png", &c).is_none()); // loopback refused before connecting
        let only = RemoteConfig { hosts: vec!["good.test".into()], ..RemoteConfig::default() };
        assert!(fetch("https://bad.test/a.png", &only).is_none());
    }
}
