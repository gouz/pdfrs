//! `pdfrs` with no argument (or `pdfrs serve`): HTTP server, POST HTML, get a PDF.
//! `pdfrs input.html [output.pdf] [options]`: one-shot conversion (`--data d.json`: input is a template).

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use pdfrs::{html_to_pdf, page_size, template, ImagePolicy, Options};
use serde::Deserialize;
use std::path::{Path, PathBuf};
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

/// `{"html": ...}`, or a template and its data: `{"template": ..., "data": {...}}` /
/// `{"template_name": "invoice.html", "data": {...}}` (a file of TEMPLATES_DIR)
#[derive(Deserialize)]
struct JsonBody {
    html: Option<String>,
    template: Option<String>,
    #[serde(alias = "templateName")]
    template_name: Option<String>,
    #[serde(default)]
    data: serde_json::Value,
    locale: Option<String>,
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

/// What to render: HTML, or a template with its data.
enum Input {
    Html(String),
    Template { inline: Option<String>, name: Option<String>, data: serde_json::Value, locale: String },
}

/// A failed render: the HTTP status (400: the template or its data, 500: rendering) and why.
type Failure = (StatusCode, String);
type Job = (Input, Options, Option<PathBuf>, oneshot::Sender<Result<Vec<u8>, Failure>>);

/// Fill the template, then render the HTML. The header / footer options are templates too.
fn produce(input: Input, mut opts: Options, templates: Option<&Path>) -> Result<Vec<u8>, Failure> {
    let html = match input {
        Input::Html(html) => html,
        Input::Template { inline, name, data, locale } => {
            fn fill(source: template::Source, data: &serde_json::Value, dir: Option<&Path>, locale: &str) -> Result<String, Failure> {
                template::render(source, data, dir, locale).map_err(|e| (StatusCode::BAD_REQUEST, format!("template: {e}")))
            }
            for band in [&mut opts.header, &mut opts.footer] {
                if let Some(b) = band.take() {
                    *band = Some(fill(template::Source::Inline(&b), &data, templates, &locale)?);
                }
            }
            match (&inline, &name) {
                (Some(t), _) => fill(template::Source::Inline(t), &data, templates, &locale)?,
                (None, Some(n)) => fill(template::Source::Named(n), &data, templates, &locale)?,
                (None, None) => unreachable!(),
            }
        }
    };
    html_to_pdf(&html, &opts).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

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
                let Ok((input, opts, templates, reply)) = job else { break };
                let result = std::panic::catch_unwind(|| produce(input, opts, templates.as_deref()))
                    .unwrap_or_else(|_| Err((StatusCode::INTERNAL_SERVER_ERROR, "render panicked".into())));
                let _ = reply.send(result);
            })
            .expect("spawn render thread");
    }
    tx
}

struct AppState {
    jobs: Mutex<mpsc::Sender<Job>>,
    images: ImagePolicy,
    templates: Option<PathBuf>,
    locale: String,
}

async fn render(State(st): State<Arc<AppState>>, Query(q): Query<Params>, headers: HeaderMap, body: Bytes) -> Response {
    let is_json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    let (input, params) = if is_json {
        match serde_json::from_slice::<JsonBody>(&body) {
            Ok(b) => {
                let input = match (b.html, b.template, b.template_name) {
                    (Some(html), None, None) => Input::Html(html),
                    (None, Some(t), None) => Input::Template { inline: Some(t), name: None, data: b.data, locale: b.locale.unwrap_or_else(|| st.locale.clone()) },
                    (None, None, Some(name)) if st.templates.is_some() => {
                        Input::Template { inline: None, name: Some(name), data: b.data, locale: b.locale.unwrap_or_else(|| st.locale.clone()) }
                    }
                    (None, None, Some(_)) => return (StatusCode::BAD_REQUEST, "template_name: no TEMPLATES_DIR on this server\n").into_response(),
                    _ => return (StatusCode::BAD_REQUEST, "give one of html, template, template_name\n").into_response(),
                };
                (input, b.params)
            }
            Err(e) => return (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}\n")).into_response(),
        }
    } else {
        (Input::Html(String::from_utf8_lossy(&body).into_owned()), q)
    };
    let opts = match options(params, st.images.clone()) {
        Ok(o) => o,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response(),
    };
    let (reply, wait) = oneshot::channel();
    if st.jobs.lock().unwrap().send((input, opts, st.templates.clone(), reply)).is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "render pool stopped\n").into_response();
    }
    match wait.await {
        Ok(Ok(pdf)) => ([(header::CONTENT_TYPE, "application/pdf")], pdf).into_response(),
        Ok(Err((status, e))) => (status, format!("{e}\n")).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "render failed\n").into_response(),
    }
}

const USAGE: &str = "pdfrs: POST HTML to / (or /pdf), get a PDF back.

  curl --data-binary @page.html 'http://localhost:3000/?size=A4&margin=40' -o page.pdf
  curl -H 'content-type: application/json' -d '{\"html\":\"<h1>Hi</h1>\",\"footer\":\"<p>{page}/{pages}</p>\"}' http://localhost:3000/ -o hi.pdf

Options (query string or JSON fields): size, landscape, margin, font, fontSize, header, footer,
title, author, subject, keywords.

Templates (JSON): {\"template\": \"<h1>{{ title }}</h1>\", \"data\": {...}} or {\"template_name\": \"invoice.html\",
\"data\": {...}} (TEMPLATES_DIR), optional \"locale\" (fr, en, de...) for the money / number / date filters.
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
    let templates = std::env::var("TEMPLATES_DIR").ok().filter(|d| !d.is_empty()).map(PathBuf::from);
    let images = ImagePolicy {
        // images and stylesheets next to the templates, unless they have a directory of their own
        base_dir: std::env::var("IMAGES_DIR").ok().filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| templates.clone()),
        remote: remote.then(|| Arc::new(pdfrs::remote::RemoteConfig::from_env())),
    };
    let max_body = std::env::var("MAX_BODY_MB").ok().and_then(|m| m.parse::<usize>().ok()).unwrap_or(20) * 1024 * 1024;
    let locale = std::env::var("LOCALE").ok().filter(|l| !l.is_empty()).unwrap_or_else(|| "fr".into());
    let state = Arc::new(AppState { jobs: Mutex::new(spawn_workers(workers)), images, templates, locale });
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
    let (mut data, mut locale): (Option<String>, Option<String>) = (None, None);
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
            "--data" => data = Some(val()?),
            "--locale" => locale = Some(val()?),
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
    let opts = options(p, ImagePolicy { base_dir: base.clone(), remote: remote.then(|| Arc::new(pdfrs::remote::RemoteConfig::from_env())) })?;
    // --data: the input is a template of its folder, filled with that JSON
    let job = match data {
        None => Input::Html(html),
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            let data = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            let name = input.file_name().map(|n| n.to_string_lossy().into_owned());
            Input::Template { inline: None, name, data, locale: locale.unwrap_or_else(|| "fr".into()) }
        }
    };
    // same stack as the server's render threads
    let pdf = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || produce(job, opts, base.as_deref()))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "render panicked".to_string())?
        .map_err(|(_, e)| e)?;
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
            "Usage: pdfrs                                   HTTP server (PORT, WORKERS, FONTS_DIR, IMAGES_DIR, TEMPLATES_DIR, LOCALE, REMOTE_RESOURCES)
       pdfrs <input.html> [output.pdf] [options]
  --size A4|LETTER|400x600   --landscape   --margin 40 | '60 40 50 40'
  --font Georgia             --font-size 12
  --header '<p>Titre</p>'    --footer '<p>{{page}} / {{pages}}</p>'   --title 'Facture 42'
  --remote                   also load http(s) images, stylesheets and fonts (REMOTE_HOSTS, REMOTE_ALLOW_PRIVATE...)
  --data data.json           the input is a MiniJinja template, filled with this JSON   --locale fr|en|de..."
        );
        std::process::exit(1);
    }
}
