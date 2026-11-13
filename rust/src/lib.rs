// napi-rs entry point. All matching logic lives in Rust; this file only
// exposes synchronous / asynchronous bindings to the TS shell.

mod config;
mod index;
mod model;
mod parser;
mod resolve;
mod scanner;

use napi::bindgen_prelude::*;
use napi::{Env, Result, Task};
use napi_derive::napi;
use std::sync::{OnceLock, RwLock};

#[napi(object)]
#[derive(Clone)]
pub struct Location {
    pub uri: String,
    pub line: u32,
    pub character: u32,
}

impl Location {
    fn from_loc(l: model::Loc) -> Location {
        Location {
            uri: l.uri,
            line: l.line,
            character: l.character,
        }
    }
}

static STATE: OnceLock<RwLock<index::State>> = OnceLock::new();

fn state() -> &'static RwLock<index::State> {
    STATE.get_or_init(|| RwLock::new(index::State::empty()))
}

/// F-01: re-validate that the workspace root really is a Java project.
#[napi]
pub fn is_java_project(root: String) -> bool {
    scanner::is_java_project(&root)
}

/// Build the URI exactly the way VS Code does, for tooling/test callers.
#[napi]
pub fn uri_for_path(path: String) -> String {
    scanner::path_to_uri(&path)
}

pub struct BuildIndexTask {
    root: String,
}

impl Task for BuildIndexTask {
    type Output = u32;
    type JsValue = u32;

    fn compute(&mut self) -> Result<Self::Output> {
        let root = self.root.clone();
        let cfg = config::Config::load(&root);
        let mut files = Vec::new();
        // F-02: lazy, off the JS thread (AsyncTask runs in the libuv pool).
        if scanner::is_java_project(&root) {
            for c in scanner::collect(&root) {
                let parsed = if c.is_xml {
                    parser::parse_xml(&c)
                } else {
                    parser::parse_java(&c)
                };
                if let Some(m) = parsed {
                    files.push(m);
                }
            }
        }
        let st = index::build_index_from_files(files, &cfg, root);
        let count = st.index.files.len() as u32;
        if let Ok(mut g) = state().write() {
            *g = st;
        }
        Ok(count)
    }

    fn resolve(&mut self, _env: Env, out: Self::Output) -> Result<Self::JsValue> {
        Ok(out)
    }
}

#[napi]
pub fn build_index(root: String) -> AsyncTask<BuildIndexTask> {
    AsyncTask::new(BuildIndexTask { root })
}

/// P-05: TS passes (uri, line, character, current line text); Rust does all
/// the matching. Returns a Vec so VS Code shows a peek list for multi-hits.
#[napi]
pub fn resolve_definition(
    uri: String,
    line: u32,
    character: u32,
    context_snippet: String,
) -> Vec<Location> {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::resolve_definition(&g.index, &g.config, &uri, line, character, &context_snippet)
        .into_iter()
        .map(Location::from_loc)
        .collect()
}

#[napi]
pub fn search_by_path(path: String) -> Vec<Location> {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::search_by_path(&g.index, &path)
        .into_iter()
        .map(Location::from_loc)
        .collect()
}

/// F-03: re-parse only the saved file, then recompute the in-memory index
/// (no re-scan of other files, no query-time disk I/O).
#[napi]
pub fn update_file(uri: String) {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return,
    };
    let root = g.index.root.clone();
    let cfg = g.config.clone();
    let existing: Option<usize> = g.index.by_uri.get(&uri).copied();
    let path = existing
        .map(|i| g.index.files[i].path.clone())
        .unwrap_or_else(|| scanner::uri_to_path(&uri));
    let is_xml = path.ends_with(".xml");
    let content = scanner::read_one(&path);
    let c = scanner::Candidate {
        path,
        uri: uri.clone(),
        content,
        is_xml,
    };
    let parsed = if c.is_xml {
        parser::parse_xml(&c)
    } else {
        parser::parse_java(&c)
    };
    let mut files = std::mem::take(&mut g.index.files);
    match (existing, parsed) {
        (Some(i), Some(m)) => {
            if i < files.len() {
                files[i] = m;
            }
        }
        (Some(i), None) => {
            if i < files.len() {
                files.remove(i);
            }
        }
        (None, Some(m)) => files.push(m),
        (None, None) => {}
    }
    *g = index::build_index_from_files(files, &cfg, root);
}

/// C-04: reload config and rebuild relationships from cached parse models.
#[napi]
pub fn reload_config() {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return,
    };
    let root = g.index.root.clone();
    let cfg = config::Config::load(&root);
    let files = std::mem::take(&mut g.index.files);
    *g = index::build_index_from_files(files, &cfg, root);
}

#[napi]
pub fn index_stats() -> String {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return "state poisoned".to_string(),
    };
    let i = &g.index;
    let java = i
        .files
        .iter()
        .filter(|f| f.kind == model::FileKind::Java)
        .count();
    let methods: usize = i.files.iter().map(|f| f.methods.len()).sum();
    let calls: usize = i.files.iter().map(|f| f.calls.len()).sum();
    let xml: usize = i.files.iter().map(|f| f.xml_stmts.len()).sum();
    format!(
        "files={} java={} xml_stmts={} methods={} calls={} paths={} edges={} aliases={} spans={}",
        i.files.len(),
        java,
        xml,
        methods,
        calls,
        i.path_index.len(),
        i.edges.len(),
        i.aliases.len(),
        i.spans.len()
    )
}
