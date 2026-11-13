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
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

/// Monotonic build generation (older builds must not overwrite newer ones).
static GEN: AtomicU64 = AtomicU64::new(0);

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

/// A display-ready interface-path hit for the real-time search list.
#[napi(object)]
#[derive(Clone)]
pub struct PathHit {
    pub path: String,
    pub file: String,
    pub method: String,
    pub module: String,
    pub uri: String,
    pub line: u32,
    pub character: u32,
}

/// Index counters returned to the TS shell (status bar / logs).
#[napi(object)]
#[derive(Clone, Default)]
pub struct BuildResult {
    pub files: u32,
    pub projects: u32,
    pub modules: u32,
    pub methods: u32,
    pub edges: u32,
}

static STATE: OnceLock<RwLock<index::State>> = OnceLock::new();

fn state() -> &'static RwLock<index::State> {
    STATE.get_or_init(|| RwLock::new(index::State::empty()))
}

fn build_result(st: &index::State) -> BuildResult {
    let i = &st.index;
    BuildResult {
        files: i.files.len() as u32,
        projects: i.project_count as u32,
        modules: i.module_count as u32,
        methods: i.files.iter().map(|f| f.methods.len() as u32).sum(),
        edges: i.edges.len() as u32,
    }
}

/// F-01: re-validate that a workspace folder is (or contains) a Java project.
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
    roots: Vec<String>,
    gen: u64,
}

impl Task for BuildIndexTask {
    /// (files, projects, modules, methods, edges)
    type Output = (u32, u32, u32, u32, u32);
    type JsValue = BuildResult;

    fn compute(&mut self) -> Result<Self::Output> {
        let roots = self.roots.clone();
        let cfg = config::Config::load_multi(&roots);
        // Multi-root + multi-module: nested roots are dropped, each project is
        // scanned once from its top folder, files are deduped by path.
        let (cands, projects, workspace_roots) = scanner::collect_multi(&roots);
        let mut files = Vec::new();
        for c in cands {
            let parsed = if c.is_xml {
                parser::parse_xml(&c)
            } else {
                parser::parse_java(&c)
            };
            if let Some(m) = parsed {
                files.push(m);
            }
        }
        let mut st = index::build_index_from_files(files, &cfg, workspace_roots, projects.len());
        st.gen = self.gen;
        let out = (
            st.index.files.len() as u32,
            st.index.project_count as u32,
            st.index.module_count as u32,
            st.index
                .files
                .iter()
                .map(|f| f.methods.len() as u32)
                .sum::<u32>(),
            st.index.edges.len() as u32,
        );
        if let Ok(mut g) = state().write() {
            if st.gen >= g.gen {
                *g = st;
            }
        }
        Ok(out)
    }

    fn resolve(&mut self, _env: Env, out: Self::Output) -> Result<Self::JsValue> {
        Ok(BuildResult {
            files: out.0,
            projects: out.1,
            modules: out.2,
            methods: out.3,
            edges: out.4,
        })
    }
}

/// Build the index for all workspace folders. Async (libuv thread pool) so the
/// extension host stays responsive.
#[napi]
pub fn build_index(roots: Vec<String>) -> AsyncTask<BuildIndexTask> {
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    AsyncTask::new(BuildIndexTask { roots, gen })
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

/// Directional resolution used by Ctrl/Cmd+Click (down) and F12 (up).
#[napi]
pub fn resolve_directional(
    uri: String,
    line: u32,
    character: u32,
    context_snippet: String,
    direction: String,
    fallback: bool,
) -> Vec<Location> {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::resolve_directional(
        &g.index,
        &g.config,
        &uri,
        line,
        character,
        &context_snippet,
        &direction,
        fallback,
    )
    .into_iter()
    .map(Location::from_loc)
    .collect()
}

/// Real-time interface-path search: returns display entries, no filesystem paths.
#[napi]
pub fn search_path_hits(query: String) -> Vec<PathHit> {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::search_path_hits(&g.index, &query)
        .into_iter()
        .map(|e| PathHit {
            path: e.path,
            file: e.file,
            method: e.method,
            module: e.module,
            uri: e.loc.uri,
            line: e.loc.line,
            character: e.loc.character,
        })
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

/// Incremental update: re-parse only the given files, then recompute the
/// derived index in memory (no disk re-scan, no other files re-read).
fn do_update(uris: &[String]) -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let roots = g.index.roots.clone();
    let project_count = g.index.project_count;
    let cfg = g.config.clone();
    let mut files = std::mem::take(&mut g.index.files);

    let mut pos: HashMap<String, usize> = HashMap::new();
    for (i, f) in files.iter().enumerate() {
        pos.insert(f.uri.clone(), i);
    }
    let mut removed: Vec<usize> = Vec::new();
    for uri in uris {
        let existing = pos.get(uri).copied();
        let path = existing
            .map(|i| files[i].path.clone())
            .unwrap_or_else(|| scanner::uri_to_path(uri));
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
        match (existing, parsed) {
            (Some(i), Some(m)) => {
                if i < files.len() {
                    files[i] = m;
                }
            }
            (Some(i), None) => removed.push(i),
            (None, Some(m)) => {
                pos.insert(uri.clone(), files.len());
                files.push(m);
            }
            (None, None) => {}
        }
    }
    removed.sort_unstable();
    removed.dedup();
    for i in removed.into_iter().rev() {
        files.remove(i);
    }

    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut st = index::build_index_from_files(files, &cfg, roots, project_count);
    st.gen = gen;
    let res = build_result(&st);
    *g = st;
    res
}

/// Batch incremental update (one rebuild for N changed files).
#[napi]
pub fn update_files(uris: Vec<String>) -> BuildResult {
    do_update(&uris)
}

/// Single-file update (kept for tests / simple callers).
#[napi]
pub fn update_file(uri: String) {
    let _ = do_update(&[uri]);
}

/// C-04: reload config and rebuild relationships from cached parse models.
#[napi]
pub fn reload_config() -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let roots = g.index.roots.clone();
    let project_count = g.index.project_count;
    let cfg = config::Config::load_multi(&roots);
    let files = std::mem::take(&mut g.index.files);
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut st = index::build_index_from_files(files, &cfg, roots, project_count);
    st.gen = gen;
    let res = build_result(&st);
    *g = st;
    res
}

/// Rebuild one project root only: drop its files, rescan that subtree, keep the
/// rest of the workspace index intact.
#[napi]
pub fn reindex_root(root: String) -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let roots = g.index.roots.clone();
    let project_count = g.index.project_count;
    let cfg = g.config.clone();
    let rn = scanner::normalize_path(&root);

    let mut files = std::mem::take(&mut g.index.files);
    files.retain(|f| {
        let p = f.path.replace('\\', "/");
        !(p == rn || p.starts_with(&format!("{}/", rn)))
    });
    if scanner::is_java_project(&rn) {
        for c in scanner::collect(&rn) {
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

    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut st = index::build_index_from_files(files, &cfg, roots, project_count);
    st.gen = gen;
    let res = build_result(&st);
    *g = st;
    res
}

/// Current counters without rebuilding (status bar refresh).
#[napi]
pub fn get_status() -> BuildResult {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    build_result(&g)
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
        "projects={} modules={} roots={} files={} java={} xml_stmts={} methods={} calls={} paths={} path_entries={} edges={} down={} up={} aliases={} spans={}",
        i.project_count,
        i.module_count,
        i.roots.len(),
        i.files.len(),
        java,
        xml,
        methods,
        calls,
        i.path_index.len(),
        i.path_entries.len(),
        i.edges.len(),
        i.down.len(),
        i.up.len(),
        i.aliases.len(),
        i.spans.len()
    )
}
