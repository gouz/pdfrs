// The HTTP API end to end: the real binary, raw HTTP over a socket.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command};

struct Server(Child, u16);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn start(templates: Option<&std::path::Path>) -> Server {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdfrs"));
    cmd.env("PORT", port.to_string()).env("WORKERS", "2").env_remove("TEMPLATES_DIR").env_remove("IMAGES_DIR");
    if let Some(t) = templates {
        cmd.env("TEMPLATES_DIR", t);
    }
    let server = Server(cmd.spawn().unwrap(), port);
    for _ in 0..200 {
        if request(port, "GET", "/health", "", "").0 == 200 {
            return server;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("server did not start");
}

/// (status, body)
fn request(port: u16, method: &str, path: &str, content_type: &str, body: &str) -> (u16, Vec<u8>) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return (0, Vec::new()) };
    let _ = write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let head_end = out.windows(4).position(|w| w == b"\r\n\r\n").map_or(out.len(), |p| p + 4);
    let status = String::from_utf8_lossy(&out[..head_end]).split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, out[head_end..].to_vec())
}

#[test]
fn templates_over_http() {
    let dir = std::env::temp_dir().join(format!("pdfrs-srv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("base.html"), "<h1>{% block t %}{% endblock %}</h1>").unwrap();
    std::fs::write(dir.join("facture.html"), "{% extends 'base.html' %}{% block t %}Facture {{ n }} : {{ total | money }}{% endblock %}").unwrap();
    let srv = start(Some(&dir));
    let json = "application/json";

    let (status, body) = request(srv.1, "POST", "/", json, r#"{"template_name": "facture.html", "data": {"n": 42, "total": 1234.5}, "size": "A5"}"#);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert!(body.starts_with(b"%PDF-"));

    let (status, body) = request(srv.1, "POST", "/pdf", json, r#"{"template": "<p>{% for x in items %}{{ x }} {% endfor %}</p>", "data": {"items": [1, 2]}, "footer": "<p>{{ items | length }} / {page}</p>"}"#);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

    // strict: a missing variable is a 400 that says where
    let (status, body) = request(srv.1, "POST", "/", json, r#"{"template_name": "facture.html", "data": {"n": 1}}"#);
    let msg = String::from_utf8_lossy(&body);
    assert_eq!(status, 400);
    assert!(msg.contains("undefined") && msg.contains("facture.html"), "{msg}");

    for bad in [
        r#"{"template_name": "../etc/passwd", "data": {}}"#,
        r#"{"template": "{% for %}", "data": {}}"#,
        r#"{"html": "<p>x</p>", "template": "<p>y</p>"}"#,
        r#"{"template": "{{ x }}", "data": [1]}"#,
        r#"{}"#,
    ] {
        assert_eq!(request(srv.1, "POST", "/", json, bad).0, 400, "{bad}");
    }
    // plain HTML still works
    let (status, body) = request(srv.1, "POST", "/?size=A6", "text/html", "<h1>ok</h1>");
    assert!(status == 200 && body.starts_with(b"%PDF-"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn template_name_needs_a_templates_dir() {
    let srv = start(None);
    let (status, body) = request(srv.1, "POST", "/", "application/json", r#"{"template_name": "a.html", "data": {}}"#);
    assert_eq!(status, 400);
    assert!(String::from_utf8_lossy(&body).contains("TEMPLATES_DIR"));
}
