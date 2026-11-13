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
use std::path::Path;
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
    /// Files skipped for exceeding the size limit.
    pub skipped: u32,
    /// Relevant source files that failed to parse.
    pub failed: u32,
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
        skipped: i.skipped_large,
        failed: i.parse_failed,
    }
}

/// Parse one candidate, counting a "relevant but failed" file as parse_failed.
/// Panic containment: panics must NEVER unwind through the libuv/FFI
/// boundary (that aborts the whole extension host). Every napi entry point
/// and the async build task run behind catch_unwind; the panic message is
/// recorded so the user can report it and the root cause gets fixed.
static LAST_PANIC: std::sync::LazyLock<std::sync::Mutex<String>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(String::new()));

fn record_panic(where_: &str, p: Box<dyn std::any::Any + Send>) {
    let msg = p
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    let text = format!("{}: {}", where_, msg);
    eprintln!("SpringNav PANIC ({})", text);
    if let Ok(mut g) = LAST_PANIC.lock() {
        *g = text;
    }
}

fn last_panic_text() -> String {
    LAST_PANIC.lock().map(|g| g.clone()).unwrap_or_default()
}

fn parse_candidate(c: &scanner::Candidate, failed: &mut u32) -> Option<model::FileModel> {
    if c.is_xml {
        let parsed = parser::parse_xml(c);
        if parsed.is_none() && c.content.contains("<mapper") {
            *failed += 1;
        }
        parsed
    } else {
        let name = Path::new(&c.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let relevant = parser::relevant_java(&c.path, &name, &c.content);
        let parsed = parser::parse_java(c);
        if relevant && parsed.is_none() {
            *failed += 1;
        }
        parsed
    }
}

/// F-01: a workspace folder is a Java project if any project root exists
/// inside it (deep monorepo detection).
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
    /// (files, projects, modules, methods, edges, skipped, failed)
    type Output = (u32, u32, u32, u32, u32, u32, u32);
    type JsValue = BuildResult;

    fn compute(&mut self) -> Result<Self::Output> {
        // Panics must not unwind across the libuv thread boundary (process
        // abort). Contain them and surface as a rejected task.
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.compute_inner()
        }));
        match out {
            Ok(r) => r,
            Err(p) => {
                record_panic("buildIndex", p);
                Err(napi::Error::new(
                    napi::Status::GenericFailure,
                    format!("SpringNav buildIndex panicked — {}", last_panic_text()),
                ))
            }
        }
    }

    fn resolve(&mut self, _env: Env, out: Self::Output) -> Result<Self::JsValue> {
        Ok(BuildResult {
            files: out.0,
            projects: out.1,
            modules: out.2,
            methods: out.3,
            edges: out.4,
            skipped: out.5,
            failed: out.6,
        })
    }
}

impl BuildIndexTask {
    fn compute_inner(&mut self) -> Result<(u32, u32, u32, u32, u32, u32, u32)> {
        let roots = self.roots.clone();
        let scan = scanner::scan_workspace(&roots);
        // Config comes from the ACTUAL project roots, so a config placed in a
        // sub-project's `.idea` takes effect (not fixed to the workspace root).
        let cfg = config::Config::load_multi(&scan.project_roots);
        let mut files = Vec::new();
        let mut failed = 0u32;
        for c in &scan.candidates {
            if let Some(m) = parse_candidate(c, &mut failed) {
                files.push(m);
            }
        }
        // P1 import closure: utility/constant classes referenced by imports
        // are not suffix-relevant; parse them too so static calls and
        // constant references can jump (one level, bounded by distinct names).
        let mut wanted: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in &files {
            for imp in &m.imports {
                if let Some(simple) = imp.rsplit('.').next() {
                    wanted.insert(simple.to_string());
                }
            }
        }
        let known: std::collections::HashSet<String> =
            files.iter().map(|f| f.class_name.clone()).collect();
        let (extra, extra_skipped) =
            scanner::collect_by_names(&scan.project_roots, &wanted, &known);
        for c in &extra {
            if let Some(m) = parse_candidate(c, &mut failed) {
                files.push(m);
            }
        }
        let project_count = scan.project_roots.len();
        let mut st = index::build_index_from_files(
            files,
            &cfg,
            scan.workspace_roots,
            scan.project_roots,
            project_count,
            scan.skipped_large + extra_skipped,
            failed,
        );
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
            scan.skipped_large,
            failed,
        );
        if let Ok(mut g) = state().write() {
            if st.gen >= g.gen {
                *g = st;
            }
        }
        Ok(out)
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
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::resolve_definition(&g.index, &g.config, &uri, line, character, &context_snippet)
        .into_iter()
        .map(Location::from_loc)
        .collect()
    }));
    match out {
        Ok(v) => v,
        Err(p) => {
            record_panic("resolve_definition", p);
            Vec::new()
        }
    }
}

/// TEMP debug
#[napi]
pub fn dump_files() -> String {
    let g = match state().read() { Ok(v) => v, Err(_) => return "no state".into() };
    let mut out = String::new();
    for f in &g.index.files {
        out += &format!("[{} role={:?} fields={}] ", f.path.rsplit('/').next().unwrap_or("?"), f.role, f.fields.iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(","));
    }
    out
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
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
    }));
    match out {
        Ok(v) => v,
        Err(p) => {
            record_panic("resolve_directional", p);
            Vec::new()
        }
    }
}

/// Real-time interface-path search: returns display entries, no filesystem paths.
#[napi]
pub fn search_path_hits(query: String) -> Vec<PathHit> {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
    }));
    match out {
        Ok(v) => v,
        Err(p) => {
            record_panic("search_path_hits", p);
            Vec::new()
        }
    }
}

#[napi]
pub fn search_by_path(path: String) -> Vec<Location> {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    resolve::search_by_path(&g.index, &path)
        .into_iter()
        .map(Location::from_loc)
        .collect()
    }));
    match out {
        Ok(v) => v,
        Err(p) => {
            record_panic("search_by_path", p);
            Vec::new()
        }
    }
}

/// Incremental update: re-parse only the given files, then apply a true
/// incremental index update (only changed files + invalidated edges).
fn do_update(uris: &[String]) -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let cfg = g.config.clone();
    let mut failed_delta = 0u32;
    let mut changes: Vec<(String, Option<model::FileModel>)> = Vec::new();
    // Inherit the project root from the existing model (or fall back to the
    // workspace root) so scoped config keeps working after a save.
    for uri in uris {
        let path = scanner::uri_to_path(uri);
        // Normalize: consumers must pass file:// URIs; accept a bare path too
        // (otherwise the change is treated as an unknown file and duplicated).
        let uri = if uri.starts_with("file://") {
            uri.clone()
        } else {
            scanner::path_to_uri(&path)
        };
        let is_xml = path.ends_with(".xml");
        let content = scanner::read_one(&path);
        let project_root = g
            .index
            .by_uri
            .get(&uri)
            .map(|&i| g.index.files[i].project_root.clone())
            .unwrap_or_else(|| {
                g.index
                    .project_roots
                    .iter()
                    .filter(|r| path.starts_with(&format!("{}/", r)))
                    .max_by_key(|r| r.len())
                    .cloned()
                    .unwrap_or_else(|| g.index.roots.first().cloned().unwrap_or_default())
            });
        // Already-indexed files parse regardless of the relevance filter so a
        // save never drops them (e.g. import-closure utility classes).
        let force = g.index.by_uri.contains_key(&uri);
        let c = scanner::Candidate {
            path,
            uri: uri.clone(),
            content,
            is_xml,
            project_root,
            force,
        };
        let prev_failed = failed_delta;
        let parsed = parse_candidate(&c, &mut failed_delta);
        // Only count a NEW failure when the file existed before (avoid double
        // counting on repeated failed saves).
        if g.index.by_uri.contains_key(&uri) && failed_delta > prev_failed {
            // keep the delta
        } else {
            failed_delta = prev_failed;
        }
        changes.push((uri.clone(), parsed));
    }
    index::apply_changes(&mut g, changes, &cfg);
    g.index.parse_failed = g
        .index
        .parse_failed
        .saturating_add(failed_delta);
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    g.gen = gen;
    build_result(&g)
}

/// Batch incremental update (one rebuild for N changed files).
#[napi]
pub fn update_files(uris: Vec<String>) -> BuildResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| do_update(&uris))) {
        Ok(r) => r,
        Err(p) => {
            record_panic("updateFiles", p);
            BuildResult::default()
        }
    }
}

/// Single-file update (kept for tests / simple callers).
#[napi]
pub fn update_file(uri: String) {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = do_update(&[uri]);
    }));
    if let Err(p) = out {
        record_panic("updateFile", p);
    }
}

/// C-04: reload config from every project root and rebuild.
#[napi]
pub fn reload_config() -> BuildResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(reload_config_inner)) {
        Ok(r) => r,
        Err(p) => {
            record_panic("reloadConfig", p);
            BuildResult::default()
        }
    }
}

fn reload_config_inner() -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let cfg = config::Config::load_multi(&g.index.project_roots);
    let files = std::mem::take(&mut g.index.files);
    let roots = g.index.roots.clone();
    let project_roots = g.index.project_roots.clone();
    let pc = g.index.project_count;
    let skipped = g.index.skipped_large;
    let failed = g.index.parse_failed;
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut st =
        index::build_index_from_files(files, &cfg, roots, project_roots, pc, skipped, failed);
    st.gen = gen;
    let res = build_result(&st);
    *g = st;
    res
}

/// Rebuild one project root only: drop its files, rescan that subtree, keep the
/// rest of the workspace index intact.
#[napi]
pub fn reindex_root(root: String) -> BuildResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reindex_root_inner(root))) {
        Ok(r) => r,
        Err(p) => {
            record_panic("reindexRoot", p);
            BuildResult::default()
        }
    }
}

fn reindex_root_inner(root: String) -> BuildResult {
    let mut g = match state().write() {
        Ok(v) => v,
        Err(_) => return BuildResult::default(),
    };
    let cfg = g.config.clone();
    let rn = scanner::normalize_path(&root);
    let mut files = std::mem::take(&mut g.index.files);
    files.retain(|f| {
        let p = f.path.replace('\\', "/");
        !(p == rn || p.starts_with(&format!("{}/", rn)))
    });
    let mut skipped_delta = 0u32;
    let mut failed_delta = 0u32;
    if scanner::is_java_project(&rn) {
        let (cs, skipped) = scanner::collect_with_stats(&rn, &rn);
        skipped_delta = skipped;
        for c in cs {
            if let Some(m) = parse_candidate(&c, &mut failed_delta) {
                files.push(m);
            }
        }
    }
    let roots = g.index.roots.clone();
    let project_roots = g.index.project_roots.clone();
    let pc = g.index.project_count;
    let skipped = g.index.skipped_large.saturating_add(skipped_delta);
    let failed = g.index.parse_failed.saturating_add(failed_delta);
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut st =
        index::build_index_from_files(files, &cfg, roots, project_roots, pc, skipped, failed);
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

/// Filesystem paths of the discovered Java project roots (for the Explorer
/// context menu filter).
#[napi]
pub fn project_roots() -> Vec<String> {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    g.index.project_roots.clone()
}

/// Whether the index has been built at least once (empty-window hint).
#[napi]
pub fn is_ready() -> bool {
    let g = match state().read() {
        Ok(v) => v,
        Err(_) => return false,
    };
    g.gen > 0
}

#[napi]
pub fn index_stats() -> String {
    let lp = last_panic_text();
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
    let s = format!(
        "projects={} modules={} roots={} files={} java={} xml_stmts={} methods={} calls={} paths={} path_entries={} edges={} down={} up={} aliases={} spans={} skipped={} failed={} call_cache={} impls_cache={} xml_cache={} edge_contrib={}",
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
        i.spans.len(),
        i.skipped_large,
        i.parse_failed,
        i.call_cache.len(),
        i.impls_cache.len(),
        i.xml_cache.len(),
        i.edge_contrib.len(),
    );
    if lp.is_empty() {
        s
    } else {
        s.replace(
            "edge_contrib={}",
            &("edge_contrib=0 | lastPanic=".to_string() + &lp),
        )
    }
}
