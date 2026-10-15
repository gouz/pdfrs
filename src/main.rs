//! `pdfrs` with no argument (or `pdfrs serve`): HTTP server, POST HTML, get a PDF.
//! `pdfrs input.html [output.pdf] [options]`: one-shot conversion.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use pdfrs::{html_to_pdf, page_size, ImagePolicy, Options};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::oneshot;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Options shared by the query string, the JSON body and the command line.
#[derive(Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
struct Params {
    size: Option<String>,
    landscape: Option<serde_json::Value>,
    margin: Option<serde_json::Value>,
    font: Option<String>,
    font_size: Option<f32>,
    header: Option<String>,
    footer: Option<String>,
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
}

#[derive(Deserialize)]
struct JsonBody {
    html: String,
    #[serde(flatten)]
    params: Params,
}

fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => !matches!(s.as_str(), "false" | "0" | "no"),
        serde_json::Value::Number(n) => n.as_f64() != Some(0.0),
        _ => false,
    }
}

/// `40`, `"40"`, `"60,40,50,40"` / `"60 40 50 40"` (top right bottom left, CSS order and shorthand)
fn margin(v: &serde_json::Value) -> Result<[f32; 4], String> {
    let s = match v {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "),
        _ => return Err("margin: number or \"top right bottom left\"".into()),
    };
    let n: Vec<f32> = s
        .split([',', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim_end_matches("pt").parse())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("margin: invalid value {s:?}"))?;
    Ok(match n[..] {
        [a] => [a; 4],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return Err(format!("margin: invalid value {s:?}")),
    })
}

fn options(p: Params, images: ImagePolicy) -> Result<Options, String> {
    let mut o = Options { images, ..Options::default() };
    if let Some(s) = p.size {
        o.size = Some(page_size(&s).ok_or(format!("size: unknown page size {s:?} (A4, LETTER... or 400x600)"))?);
    }
    if let Some(l) = p.landscape {
        o.landscape = Some(truthy(&l));
    }
    if let Some(m) = p.margin {
        o.margin = Some(margin(&m)?);
    }
    if let Some(f) = p.font {
        o.font = f;
    }
    if let Some(fs) = p.font_size.filter(|f| *f > 0.0 && *f < 1000.0) {
        o.font_size = fs;
    }
    (o.header, o.footer, o.title, o.author, o.subject, o.keywords) = (p.header, p.footer, p.title, p.author, p.subject, p.keywords);
    Ok(o)
}

type Job = (String, Options, oneshot::Sender<Result<Vec<u8>, String>>);

/// Fixed pool of render threads: each keeps its font and layout caches warm across requests.
/// Requests beyond the pool size wait in the channel (bounded memory, like pdfjs's queue).
fn spawn_workers(n: usize) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..n {
        let rx = rx.clone();
        std::thread::Builder::new()
            .name(format!("render-{i}"))
            // layout and drawing recurse along the box tree (depth is capped, but leave room)
            .stack_size(16 * 1024 * 1024)
            .spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                let Ok((html, opts, reply)) = job else { break };
                let result = std::panic::catch_unwind(|| html_to_pdf(&html, &opts)).unwrap_or_else(|_| Err("render panicked".into()));
                let _ = reply.send(result);
            })
            .expect("spawn render thread");
    }
    tx
}

struct AppState {
    jobs: Mutex<mpsc::Sender<Job>>,
    images: ImagePolicy,
}

async fn render(State(st): State<Arc<AppState>>, Query(q): Query<Params>, headers: HeaderMap, body: Bytes) -> Response {
    let is_json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    let (html, params) = if is_json {
        match serde_json::from_slice::<JsonBody>(&body) {
            Ok(b) => (b.html, b.params),
            Err(e) => return (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}\n")).into_response(),
        }
    } else {
        (String::from_utf8_lossy(&body).into_owned(), q)
    };
    let opts = match options(params, st.images.clone()) {
        Ok(o) => o,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response(),
    };
    let (reply, wait) = oneshot::channel();
    if st.jobs.lock().unwrap().send((html, opts, reply)).is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "render pool stopped\n").into_response();
    }
    match wait.await {
        Ok(Ok(pdf)) => ([(header::CONTENT_TYPE, "application/pdf")], pdf).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\n")).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "render failed\n").into_response(),
    }
}

const USAGE: &str = "pdfrs: POST HTML to / (or /pdf), get a PDF back.

  curl --data-binary @page.html 'http://localhost:3000/?size=A4&margin=40' -o page.pdf
  curl -H 'content-type: application/json' -d '{\"html\":\"<h1>Hi</h1>\",\"footer\":\"<p>{page}/{pages}</p>\"}' http://localhost:3000/ -o hi.pdf

Options (query string or JSON fields): size, landscape, margin, font, fontSize, header, footer,
title, author, subject, keywords.
";

fn env_dirs(var: &str, default: &str) -> Vec<PathBuf> {
    std::env::var(var).unwrap_or_else(|_| default.into()).split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect()
}

fn fonts_init() -> usize {
    let n = pdfrs::fonts::init(&env_dirs("FONTS_DIR", "/usr/share/fonts:/fonts"));
    if n == 0 {
        eprintln!("pdfrs: warning: no font found (set FONTS_DIR), text will not render");
    }
    n
}

async fn serve() {
    let n = fonts_init();
    let workers = std::env::var("WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .filter(|w| *w > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let remote = std::env::var("REMOTE_RESOURCES").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"));
    let images = ImagePolicy {
        base_dir: std::env::var("IMAGES_DIR").ok().filter(|d| !d.is_empty()).map(PathBuf::from),
        remote: remote.then(|| Arc::new(pdfrs::remote::RemoteConfig::from_env())),
    };
    let max_body = std::env::var("MAX_BODY_MB").ok().and_then(|m| m.parse::<usize>().ok()).unwrap_or(20) * 1024 * 1024;
    let state = Arc::new(AppState { jobs: Mutex::new(spawn_workers(workers)), images });
    let app = Router::new()
        .route("/", post(render).get(|| async { USAGE }))
        .route("/pdf", post(render))
        .route("/health", get(|| async { "ok\n" }))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state);
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pdfrs: cannot listen on :{port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("pdfrs listening on :{port} ({workers} render threads, {n} font files, remote resources {})", if remote { "on" } else { "off" });
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        })
        .await
        .expect("serve");
}

fn cli(args: Vec<String>) -> Result<(), String> {
    let mut p = Params::default();
    let mut pos = Vec::new();
    let mut remote = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a}: missing value"));
        match a.as_str() {
            "--size" => p.size = Some(val()?),
            "--landscape" => p.landscape = Some(true.into()),
            "--margin" => p.margin = Some(val()?.into()),
            "--font" => p.font = Some(val()?),
            "--font-size" => p.font_size = Some(val()?.parse().map_err(|_| "--font-size: number expected")?),
            "--header" => p.header = Some(val()?),
            "--footer" => p.footer = Some(val()?),
            "--title" => p.title = Some(val()?),
            "--remote" => remote = true,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => pos.push(a),
        }
    }
    let input = PathBuf::from(pos.first().ok_or("missing input file")?);
    let output = pos.get(1).cloned().unwrap_or_else(|| "out.pdf".into());
    let html = std::fs::read_to_string(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    fonts_init();
    // local images: only from the input file's folder
    let base = std::fs::canonicalize(&input).ok().and_then(|p| p.parent().map(PathBuf::from));
    let opts = options(p, ImagePolicy { base_dir: base, remote: remote.then(|| Arc::new(pdfrs::remote::RemoteConfig::from_env())) })?;
    let pdf = html_to_pdf(&html, &opts)?;
    std::fs::write(&output, pdf).map_err(|e| format!("{output}: {e}"))?;
    println!("✔ {output}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "serve" {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime").block_on(serve());
    } else if let Err(e) = cli(args) {
        if !e.is_empty() {
            eprintln!("pdfrs: {e}");
        }
        eprintln!(
            "Usage: pdfrs                                   HTTP server (PORT, WORKERS, FONTS_DIR, IMAGES_DIR, REMOTE_RESOURCES)
       pdfrs <input.html> [output.pdf] [options]
  --size A4|LETTER|400x600   --landscape   --margin 40 | '60 40 50 40'
  --font Georgia             --font-size 12
  --header '<p>Titre</p>'    --footer '<p>{{page}} / {{pages}}</p>'   --title 'Facture 42'
  --remote                   also load http(s) images, stylesheets and fonts (REMOTE_HOSTS, REMOTE_ALLOW_PRIVATE...)"
        );
        std::process::exit(1);
    }
}
