//! Font registry: every .ttf/.otf/.ttc found in the font directories, loaded once at startup
//! and shared (read-only) by all render threads.
//!
//! The PDF base-14 names and CSS generic families map to metric-compatible fonts:
//! Helvetica/Arial/sans-serif -> Liberation Sans, Times/serif -> Liberation Serif,
//! Courier/monospace -> Liberation Mono. DejaVu Sans is the last-resort fallback for missing glyphs.

use parley::fontique::{Blob, Collection, CollectionOptions, FontInfoOverride, FontStyle, FontWeight};
use parley::FontContext;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub struct Registry {
    blobs: Vec<Blob<u8>>,
    blob_ids: std::collections::HashSet<u64>,
    /// lowercase family name -> family name as registered
    families: HashMap<String, String>,
    /// generic -> registered family
    aliases: HashMap<&'static str, String>,
    pub fallback: Option<String>,
    pub svg_fonts: Arc<usvg::fontdb::Database>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

const SANS: &[&str] = &["Liberation Sans", "Arimo", "Helvetica", "Arial", "DejaVu Sans"];
const SERIF: &[&str] = &["Liberation Serif", "Tinos", "Times New Roman", "DejaVu Serif"];
const MONO: &[&str] = &["Liberation Mono", "Cousine", "Courier New", "DejaVu Sans Mono"];
const FALLBACK: &[&str] = &["DejaVu Sans", "Noto Sans"];

const GENERIC: &[(&str, &[&str])] = &[
    ("sans-serif", SANS), ("system-ui", SANS), ("-apple-system", SANS), ("ui-sans-serif", SANS),
    ("arial", SANS), ("helvetica", SANS), ("helvetica neue", SANS),
    ("serif", SERIF), ("ui-serif", SERIF), ("times", SERIF), ("times-roman", SERIF), ("times new roman", SERIF),
    ("monospace", MONO), ("ui-monospace", MONO), ("courier", MONO), ("courier new", MONO),
];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "woff" | "woff2")) {
            out.push(p);
        }
    }
}

/// Load every font of `dirs` (recursively). Returns the number of font files; call once.
pub fn init(dirs: &[PathBuf]) -> usize {
    let mut files = Vec::new();
    for d in dirs {
        collect(d, &mut files);
    }
    files.sort();
    let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: false });
    let mut svg = usvg::fontdb::Database::new();
    let mut blobs = Vec::new();
    let mut families = HashMap::new();
    for f in &files {
        let Some(data) = std::fs::read(f).ok().and_then(to_sfnt) else { continue };
        svg.load_font_data(data.clone());
        let blob = Blob::new(Arc::new(data));
        for (id, _) in collection.register_fonts(blob.clone(), None) {
            if let Some(name) = collection.family_name(id) {
                families.insert(name.to_lowercase(), name.to_string());
            }
        }
        blobs.push(blob);
    }
    let pick = |cands: &[&str]| cands.iter().find_map(|c| families.get(&c.to_lowercase()).cloned());
    let aliases: HashMap<&'static str, String> = GENERIC.iter().filter_map(|(g, cands)| Some((*g, pick(cands)?))).collect();
    if let Some(f) = aliases.get("sans-serif") {
        svg.set_sans_serif_family(f);
    }
    if let Some(f) = aliases.get("serif") {
        svg.set_serif_family(f);
    }
    if let Some(f) = aliases.get("monospace") {
        svg.set_monospace_family(f);
    }
    let fallback = pick(FALLBACK);
    let n = files.len();
    let blob_ids = blobs.iter().map(|b| b.id()).collect();
    let _ = REGISTRY.set(Registry { blobs, blob_ids, families, aliases, fallback, svg_fonts: Arc::new(svg) });
    n
}

impl Registry {
    /// a font of the font directories (shared by every document), not a document's @font-face
    pub fn is_shared(&self, blob_id: u64) -> bool {
        self.blob_ids.contains(&blob_id)
    }
}

pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| Registry {
        blobs: Vec::new(),
        blob_ids: Default::default(),
        families: HashMap::new(),
        aliases: HashMap::new(),
        fallback: None,
        svg_fonts: Arc::new(usvg::fontdb::Database::new()),
    })
}

#[cfg(test)]
pub fn init_for_tests() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        init(&[PathBuf::from(std::env::var("FONTS_DIR").unwrap_or("/usr/share/fonts".into()))]);
    });
}

/// First family of a CSS stack that can be drawn, as a quoted CSS family name.
/// None when no family is usable: the declaration is then ignored (the parent's family is inherited).
pub fn resolve_stack(stack: &str) -> Option<String> {
    let r = registry();
    DOC_FAMILIES.with(|doc| {
        let doc = doc.borrow();
        stack.replace("&quot;", "").split(',').find_map(|name| {
            let name = name.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_lowercase();
            let family = doc.get(&name).or_else(|| r.families.get(&name)).or_else(|| r.aliases.get(name.as_str()))?;
            Some(format!("\"{family}\""))
        })
    })
}

thread_local! {
    static FONT_CX: RefCell<Option<FontContext>> = const { RefCell::new(None) };
    /// @font-face families of the document being built on this thread (lowercase -> name)
    static DOC_FAMILIES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// A `@font-face` of a document, loaded.
#[derive(Clone)]
pub struct DocFace {
    pub family: String,
    pub data: Blob<u8>,
    pub weight: Option<f32>,
    pub italic: bool,
}

/// sfnt data (TrueType / OpenType / collection)?
pub fn is_sfnt(data: &[u8]) -> bool {
    matches!(data.get(..4), Some(b"\0\x01\0\0" | b"OTTO" | b"true" | b"ttcf"))
}

/// Font file bytes as sfnt: TrueType / OpenType as is, WOFF / WOFF2 decompressed. None if neither.
pub fn to_sfnt(data: Vec<u8>) -> Option<Vec<u8>> {
    match data.get(..4)? {
        b"wOFF" => wuff::decompress_woff1(&data).ok(),
        b"wOF2" => wuff::decompress_woff2(&data).ok(),
        _ => is_sfnt(&data).then_some(data),
    }
    .filter(|d| is_sfnt(d))
}

/// Like with_font_cx, plus the document's @font-face fonts, visible to this call only:
/// they go into a copy of the thread's FontContext, never into the shared one.
pub fn with_doc_font_cx<R>(faces: &[DocFace], f: impl FnOnce(&mut FontContext) -> R) -> R {
    if faces.is_empty() {
        return with_font_cx(f);
    }
    let mut fcx = with_font_cx(|base| base.clone());
    DOC_FAMILIES.with(|doc| {
        let mut doc = doc.borrow_mut();
        for face in faces {
            let o = FontInfoOverride {
                family_name: Some(&face.family),
                weight: face.weight.map(FontWeight::new),
                style: face.italic.then_some(FontStyle::Italic),
                ..Default::default()
            };
            fcx.collection.register_fonts(face.data.clone(), Some(o));
            doc.insert(face.family.to_lowercase(), face.family.clone());
        }
    });
    let r = f(&mut fcx);
    DOC_FAMILIES.with(|doc| doc.borrow_mut().clear());
    r
}

/// The thread's FontContext, built on first use from the shared font blobs.
pub fn with_font_cx<R>(f: impl FnOnce(&mut FontContext) -> R) -> R {
    FONT_CX.with(|cell| {
        let mut cell = cell.borrow_mut();
        let fcx = cell.get_or_insert_with(|| {
            let mut fcx = FontContext {
                collection: Collection::new(CollectionOptions { shared: false, system_fonts: false }),
                source_cache: Default::default(),
            };
            for b in &registry().blobs {
                fcx.collection.register_fonts(b.clone(), None);
            }
            fcx
        });
        f(fcx)
    })
}
