// In-memory index: compact HashMaps only. Every navigation relationship is
// precomputed as an edge, so a query is a single span lookup plus a HashMap
// read -- no disk I/O, no file parsing (P-02/P-03).
//
// Incremental design: every derived structure is maintained per file, so saving
// one file only touches that file's contributions (O(changed)) plus the edges of
// files whose cached resolutions were invalidated (O(affected)). The expensive
// resolution work (field/type/inheritance walks, glob impl matching, fuzzy XML)
// is cached and only re-run for invalidated entries.

use crate::config::{glob_match, Config};
use crate::model::*;
use crate::parser::strip_to_simple;
use std::collections::{HashMap, HashSet};

const DEFAULT_IMPL_PATTERNS: &[&str] = &["*ServiceImpl", "*Impl"];

pub struct Index {
    /// All indexed workspace folders (multi-root supported).
    pub roots: Vec<String>,
    /// Discovered Java project roots (config is loaded from each project's .idea).
    pub project_roots: Vec<String>,
    /// Number of Java projects scanned (a multi-module project = 1).
    pub project_count: usize,
    /// Number of distinct modules detected across all roots.
    pub module_count: usize,
    pub files: Vec<FileModel>,
    pub by_uri: HashMap<String, usize>,
    pub by_simple: HashMap<String, Vec<usize>>,
    pub by_fqcn: HashMap<String, usize>,
    /// interface simple/FQCN -> implementing class file indices
    pub interface_impls: HashMap<String, Vec<usize>>,
    /// (file idx, method name) -> method indices (overloads)
    pub method_index: HashMap<(usize, String), Vec<usize>>,
    /// Lombok-synthesized accessors: (file idx, accessor) -> (field loc, param count)
    pub lombok: HashMap<(usize, String), (Loc, usize)>,
    /// full URL -> indices into `path_entries`
    pub path_index: HashMap<String, Vec<usize>>,
    pub path_entries: Vec<PathEntry>,
    /// (namespace, id) -> XML statement locations
    pub xml_index: HashMap<(String, String), Vec<Loc>>,
    /// (namespace, id) -> owner file indices (parallel to xml_index)
    pub xml_owner: HashMap<(String, String), Vec<usize>>,
    pub spans: HashMap<(String, u32), Vec<Span>>,
    /// undirected navigation edges (both endpoints registered)
    pub edges: HashMap<Loc, Vec<Loc>>,
    /// directional edges: down = 控制器→接口→实现→Mapper→XML；up = 反向
    pub down: HashMap<Loc, Vec<Loc>>,
    pub up: HashMap<Loc, Vec<Loc>>,
    /// one-way aliases: call sites / method refs -> declarations
    pub aliases: HashMap<Loc, Vec<Loc>>,
    /// Cached call-site resolution (call-site Loc -> (targets, from_lombok,
    /// field_based)). field_based=true when resolved through an injected
    /// field, i.e. the only case that gets an owner-declaration down edge.
    pub call_cache: HashMap<Loc, CallCacheEntry>,
    /// Java file indices grouped by package (same-package class resolution).
    pub by_package: HashMap<String, Vec<usize>>,
    /// call-site Loc -> owning file idx (for incremental invalidation).
    pub call_owner: HashMap<Loc, usize>,
    /// Cached interface -> impl file indices, keyed by interface FQCN.
    pub impls_cache: HashMap<String, Vec<usize>>,
    /// Cached (namespace, method) -> XML statement Locs.
    pub xml_cache: HashMap<(String, String), Vec<Loc>>,
    /// Per-file edge contributions (source file idx -> triples).
    pub edge_contrib: HashMap<usize, Vec<Triple>>,
    /// Files skipped because they exceeded the size limit.
    pub skipped_large: u32,
    /// Relevant source files that failed to parse.
    pub parse_failed: u32,
}

pub struct State {
    pub index: Index,
    pub config: Config,
    /// Build generation: an older build finishing late must not overwrite a
    /// newer one.
    pub gen: u64,
}

impl Index {
    pub fn blank(roots: Vec<String>) -> Index {
        Index {
            roots,
            project_roots: Vec::new(),
            project_count: 0,
            module_count: 0,
            files: Vec::new(),
            by_uri: HashMap::new(),
            by_simple: HashMap::new(),
            by_fqcn: HashMap::new(),
            by_package: HashMap::new(),
            interface_impls: HashMap::new(),
            method_index: HashMap::new(),
            lombok: HashMap::new(),
            path_index: HashMap::new(),
            path_entries: Vec::new(),
            xml_index: HashMap::new(),
            xml_owner: HashMap::new(),
            spans: HashMap::new(),
            edges: HashMap::new(),
            down: HashMap::new(),
            up: HashMap::new(),
            aliases: HashMap::new(),
            call_cache: HashMap::new(),
            call_owner: HashMap::new(),
            impls_cache: HashMap::new(),
            xml_cache: HashMap::new(),
            edge_contrib: HashMap::new(),
            skipped_large: 0,
            parse_failed: 0,
        }
    }
}

impl State {
    pub fn empty() -> State {
        State {
            index: Index::blank(Vec::new()),
            config: Config::default(),
            gen: 0,
        }
    }
}

pub fn normalize_url_path(s: &str) -> String {
    let mut t = s.trim().replace('\\', "/");
    if !t.starts_with('/') {
        t.insert(0, '/');
    }
    while t.contains("//") {
        t = t.replace("//", "/");
    }
    if t.len() > 1 {
        while t.ends_with('/') {
            t.pop();
        }
    }
    t
}

fn join_url_path(base: &str, path: &str) -> String {
    let b = normalize_url_path(base);
    let p = normalize_url_path(path);
    if b == "/" || b.is_empty() {
        p
    } else if p == "/" || p.is_empty() {
        b
    } else {
        format!("{}{}", b.trim_end_matches('/'), p)
    }
}

fn classify(m: &FileModel, cfg: &Config) -> Role {
    if m.kind == FileKind::Xml {
        return Role::Other;
    }
    if let Some(r) = cfg.role_override_for(&m.project_root, &m.class_name, &m.fqcn) {
        return r;
    }
    for a in &m.class_annotations {
        match a.name.as_str() {
            "RestController" | "Controller" => return Role::Controller,
            "Mapper" | "Repository" => return Role::Mapper,
            "Service" => return Role::ServiceImpl,
            _ => {}
        }
    }
    let stem = m.class_name.as_str();
    if stem.ends_with("Controller") {
        return Role::Controller;
    }
    if stem.ends_with("ServiceImpl") || stem.ends_with("Impl") {
        return Role::ServiceImpl;
    }
    if stem.ends_with("Mapper") || stem.ends_with("Dao") || stem.ends_with("Repository") {
        return Role::Mapper;
    }
    if stem.ends_with("Service") {
        return Role::Service;
    }
    for a in &m.class_annotations {
        if matches!(a.name.as_str(), "TableName" | "Entity" | "Data") {
            return Role::Entity;
        }
    }
    let p = m.path.replace('\\', "/");
    for seg in [
        "/domain/", "/entity/", "/entities/", "/model/", "/pojo/", "/dto/", "/vo/", "/bean/",
    ] {
        if p.contains(seg) {
            return Role::Entity;
        }
    }
    Role::Other
}

pub(crate) fn resolve_type_files(index: &Index, t: &str) -> Vec<usize> {
    let simple = strip_to_simple(t);
    let mut out: Vec<usize> = Vec::new();
    if let Some(&i) = index.by_fqcn.get(t) {
        out.push(i);
    }
    if let Some(v) = index.by_simple.get(&simple) {
        for &i in v {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    if out.is_empty() {
        if let Some(v) = index.by_simple.get(t) {
            out.extend(v.iter().copied());
        }
    }
    out
}

/// Find methods by name in `start` files; if none, walk up the inheritance
/// chain (J-09) level by level. `arg_count` narrows overloads (J-08); when
/// several remain they are all returned (M-01).
pub(crate) fn find_methods(
    index: &Index,
    start: &[usize],
    name: &str,
    arg_count: Option<usize>,
) -> Vec<Loc> {
    let mut visited: HashSet<usize> = HashSet::new();
    let mut level: Vec<usize> = start.to_vec();
    for _ in 0..8 {
        if level.is_empty() {
            break;
        }
        let mut out = Vec::new();
        let mut next = Vec::new();
        for fi in level {
            if !visited.insert(fi) {
                continue;
            }
            if let Some(mis) = index.method_index.get(&(fi, name.to_string())) {
                let all: Vec<&MethodDecl> = mis
                    .iter()
                    .map(|&mi| &index.files[fi].methods[mi])
                    .collect();
                let chosen: Vec<&MethodDecl> = match arg_count {
                    Some(n) => {
                        let exact: Vec<&MethodDecl> = all
                            .iter()
                            .copied()
                            .filter(|m| m.param_types.len() == n)
                            .collect();
                        if exact.is_empty() {
                            all
                        } else {
                            exact
                        }
                    }
                    None => all,
                };
                for m in chosen {
                    out.push(m.loc.clone());
                }
            }
            for s in &index.files[fi].supers {
                for si in resolve_type_files(index, s) {
                    if !visited.contains(&si) {
                        next.push(si);
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
        level = next;
    }
    Vec::new()
}

/// Resolve a receiver name to its declared type (J-09 walks up supers).
pub(crate) fn resolve_field_type(
    index: &Index,
    fi: usize,
    field: &str,
    cfg: &Config,
) -> Option<String> {
    if field == "this" {
        return Some(index.files[fi].class_name.clone());
    }
    if field == "super" {
        return index.files[fi].supers.first().cloned();
    }
    if let Some(a) = cfg.alias_for(&index.files[fi].project_root, field) {
        return Some(a.to_string());
    }
    let mut visited: HashSet<usize> = HashSet::new();
    let mut level = vec![fi];
    for _ in 0..8 {
        let mut next = Vec::new();
        for cur in level {
            if !visited.insert(cur) {
                continue;
            }
            for f in &index.files[cur].fields {
                if f.name == field {
                    return Some(f.type_simple.clone());
                }
            }
            for s in &index.files[cur].supers {
                for si in resolve_type_files(index, s) {
                    next.push(si);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    None
}

fn name_related(interface: &str, impl_name: &str) -> bool {
    let base = interface
        .trim_start_matches('I')
        .trim_end_matches("Service")
        .trim_end_matches("Mapper")
        .to_string();
    !base.is_empty() && impl_name.contains(&base)
}

fn find_impls(index: &Index, si: usize, cfg: &Config) -> Vec<usize> {
    let m = &index.files[si];
    let key = m.fqcn.clone();
    if let Some(v) = index.impls_cache.get(&key) {
        return v.clone();
    }
    let mut out: Vec<usize> = Vec::new();
    let project = m.project_root.clone();
    if let Some(alias) = cfg
        .alias_for(&project, &m.class_name)
        .or_else(|| cfg.alias_for(&project, &m.fqcn)) {
        for i in resolve_type_files(index, alias) {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    for k in [&m.class_name, &m.fqcn] {
        if let Some(v) = index.interface_impls.get(k) {
            for &i in v {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    if out.is_empty() {
        let patterns: Vec<String> = cfg
            .impl_patterns_for_project(&project, "service")
            .cloned()
            .unwrap_or_else(|| {
                DEFAULT_IMPL_PATTERNS.iter().map(|s| s.to_string()).collect()
            });
        for (i, f) in index.files.iter().enumerate() {
            if f.role == Role::ServiceImpl
                && patterns.iter().any(|p| glob_match(p, &f.class_name))
                && name_related(&m.class_name, &f.class_name)
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
        if out.is_empty() {
            for (i, f) in index.files.iter().enumerate() {
                if f.role == Role::ServiceImpl
                    && name_related(&m.class_name, &f.class_name)
                    && !out.contains(&i)
                {
                    out.push(i);
                }
            }
        }
    }
    out
}

fn cache_impls(index: &mut Index, si: usize, cfg: &Config) {
    let key = index.files[si].fqcn.clone();
    let v = find_impls(index, si, cfg);
    index.impls_cache.insert(key, v);
}

/// XML statement lookup for a mapper method. Accuracy-first: EXACT
/// (namespace, id) only — loose prefix/contains matching produced wrong
/// edges (e.g. selectUserList matching selectUserIdByPhonenumber).
fn cached_fuzzy_xml(index: &Index, ns: &str, name: &str) -> Vec<Loc> {
    let key = (ns.to_string(), name.to_string());
    if let Some(v) = index.xml_cache.get(&key) {
        return v.clone();
    }
    let mut out = Vec::new();
    if let Some(locs) = index.xml_index.get(&(ns.to_string(), name.to_string())) {
        out.extend(locs.iter().cloned());
    }
    if out.is_empty() {
        // Namespace suffix mismatch fallback (mapper interface in a
        // different package than the XML namespace), still exact on id.
        let simple = ns.rsplit('.').next().unwrap_or(ns);
        for ((n, id), locs) in &index.xml_index {
            if n.ends_with(simple) && id == name {
                out.extend(locs.iter().cloned());
            }
        }
    }
    out
}

fn cache_xml(index: &mut Index, ns: &str, name: &str) {
    let key = (ns.to_string(), name.to_string());
    let v = cached_fuzzy_xml(index, ns, name);
    index.xml_cache.insert(key, v);
}

/// Detect Lombok class-level accessor generation.
fn lombok_modes(m: &FileModel) -> (bool, bool) {
    let mut getter = false;
    let mut setter = false;
    for a in &m.class_annotations {
        match a.name.as_str() {
            "Data" => {
                getter = true;
                setter = true;
            }
            "Getter" => getter = true,
            "Setter" => setter = true,
            // Lombok @Value: getters only; ignore Spring's @Value("${...}").
            "Value" if !a.raw.contains("${") => getter = true,
            _ => {}
        }
    }
    (getter, setter)
}

fn capitalize(s: &str) -> String {
    let mut it = s.chars();
    match it.next() {
        Some(c) => c.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

/// Lombok-generated accessors for ONE file.
fn add_file_lombok(index: &mut Index, fi: usize) {
    let m = &index.files[fi];
    let (class_getter, class_setter) = lombok_modes(m);
    if !class_getter && !class_setter {
        return;
    }
    let mut entries: Vec<((usize, String), (Loc, usize))> = Vec::new();
    for f in &m.fields {
        if f.is_static || f.name == "serialVersionUID" {
            continue;
        }
        let getter = class_getter || f.annotations.iter().any(|a| a == "Getter");
        let setter = class_setter || f.annotations.iter().any(|a| a == "Setter");
        let cap = capitalize(&f.name);
        if getter {
            let g = format!("get{}", cap);
            if !index.method_index.contains_key(&(fi, g.clone())) {
                entries.push(((fi, g), (f.loc.clone(), 0)));
            }
            if f.type_simple == "boolean" {
                let ig = format!("is{}", cap);
                if !index.method_index.contains_key(&(fi, ig.clone())) {
                    entries.push(((fi, ig), (f.loc.clone(), 0)));
                }
            }
        }
        if setter && !f.is_final {
            let s = format!("set{}", cap);
            if !index.method_index.contains_key(&(fi, s.clone())) {
                entries.push(((fi, s), (f.loc.clone(), 1)));
            }
        }
    }
    for (k, v) in entries {
        index.lombok.insert(k, v);
    }
}

fn remove_file_lombok(index: &mut Index, fi: usize) {
    index.lombok.retain(|k, _| k.0 != fi);
}

/// Resolve a Lombok-generated accessor to its field location (walks supers too).
pub(crate) fn find_lombok(
    index: &Index,
    start: &[usize],
    name: &str,
    arg_count: Option<usize>,
) -> Vec<Loc> {
    let mut visited: HashSet<usize> = HashSet::new();
    let mut level: Vec<usize> = start.to_vec();
    for _ in 0..8 {
        if level.is_empty() {
            break;
        }
        let mut out = Vec::new();
        let mut next = Vec::new();
        for fi in level {
            if !visited.insert(fi) {
                continue;
            }
            if let Some((loc, argc)) = index.lombok.get(&(fi, name.to_string())) {
                if arg_count.is_none_or(|n| n == *argc) {
                    out.push(loc.clone());
                }
            }
            for s in &index.files[fi].supers {
                for si in resolve_type_files(index, s) {
                    next.push(si);
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
        level = next;
    }
    Vec::new()
}

fn reg_span(index: &mut Index, uri: &str, line: u32, start: u32, end: u32, loc: Loc) {
    if end <= start {
        return;
    }
    index
        .spans
        .entry((uri.to_string(), line))
        .or_default()
        .push(Span { start, end, loc });
}

/// Module name relative to the owning workspace root: `proj-a/mod-web`, or the
/// project directory name for a single-module project.
fn module_of(path: &str, roots: &[String]) -> String {
    let p = path.replace('\\', "/");
    let mut best: Option<String> = None;
    for r in roots {
        let rn = r.replace('\\', "/");
        let rn = rn.trim_end_matches('/');
        if (p == rn || p.starts_with(&format!("{}/", rn)))
            && best.as_ref().is_none_or(|b| rn.len() > b.len()) {
                best = Some(rn.to_string());
            }
    }
    let root = best.unwrap_or_default();
    let rel = if root.is_empty() {
        p.trim_start_matches('/').to_string()
    } else {
        p.strip_prefix(&root)
            .unwrap_or(&p)
            .trim_start_matches('/')
            .to_string()
    };
    let cut = if rel.starts_with("src/") {
        0
    } else {
        rel.find("/src/").unwrap_or(rel.len())
    };
    let m = rel[..cut].trim_matches('/').to_string();
    if m.is_empty() {
        root.rsplit('/').next().unwrap_or("").to_string()
    } else {
        m
    }
}

// ---------------------------------------------------------------------------
// Per-file map maintenance (incremental)
// ---------------------------------------------------------------------------

fn add_file_maps(index: &mut Index, fi: usize) {
    let m = index.files[fi].clone();
    let uri = m.uri.clone();
    if m.kind == FileKind::Java {
        index.by_uri.insert(uri.clone(), fi);
        let pkg = m
            .fqcn
            .rsplit_once('.')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        index.by_package.entry(pkg).or_default().push(fi);
        index
            .by_simple
            .entry(m.class_name.clone())
            .or_default()
            .push(fi);
        index.by_fqcn.insert(m.fqcn.clone(), fi);
        for s in &m.supers {
            index.interface_impls.entry(s.clone()).or_default().push(fi);
        }
        if m.role == Role::ServiceImpl {
            if let Some(base) = m.class_name.strip_suffix("Impl") {
                index
                    .interface_impls
                    .entry(base.to_string())
                    .or_default()
                    .push(fi);
            }
        }
        for (mi, md) in m.methods.iter().enumerate() {
            index
                .method_index
                .entry((fi, md.name.clone()))
                .or_default()
                .push(mi);
        }
        add_file_lombok(index, fi);

        // Register call-site owners for incremental invalidation.
        for cs in &m.calls {
            index.call_owner.insert(cs.loc.clone(), fi);
        }

        if m.role == Role::Controller {
            let bases: Vec<String> = if m.class_paths.is_empty() {
                vec![String::new()]
            } else {
                m.class_paths.clone()
            };
            let file = format!("{}.java", m.class_name);
            let module = module_of(&m.path, &index.roots);
            for md in &m.methods {
                if md.paths.is_empty() {
                    continue;
                }
                let verb = md
                    .http_methods
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "ANY".to_string());
                for b in &bases {
                    for p in &md.paths {
                        let url = join_url_path(b, p);
                        push_path_entry(index, &url, &file, &verb, &module, &md.loc, fi);
                    }
                }
            }
        }
    } else {
        for st in &m.xml_stmts {
            let key = (st.namespace.clone(), st.id.clone());
            index.xml_index.entry(key.clone()).or_default().push(st.loc.clone());
            index.xml_owner.entry(key).or_default().push(fi);
        }
    }
    add_file_spans(index, fi, &uri);
}

fn push_path_entry(
    index: &mut Index,
    url: &str,
    file: &str,
    method: &str,
    module: &str,
    loc: &Loc,
    owner: usize,
) {
    if let Some(list) = index.path_index.get(url) {
        if list
            .iter()
            .any(|&i| index.path_entries[i].loc == *loc)
        {
            return;
        }
    }
    let idx = index.path_entries.len();
    index.path_entries.push(PathEntry {
        path: url.to_string(),
        file: file.to_string(),
        method: method.to_string(),
        module: module.to_string(),
        loc: loc.clone(),
        owner,
    });
    index.path_index.entry(url.to_string()).or_default().push(idx);
}

fn add_file_spans(index: &mut Index, fi: usize, uri: &str) {
    let m = &index.files[fi];
    let mut pending: Vec<(String, u32, u32, u32, Loc)> = Vec::new();
    if m.kind == FileKind::Java {
        pending.push((
            uri.to_string(),
            m.class_loc.line,
            m.class_loc.character,
            m.class_loc.character + m.class_name.chars().count() as u32,
            m.class_loc.clone(),
        ));
        for md in &m.methods {
            pending.push((
                uri.to_string(),
                md.loc.line,
                md.loc.character,
                md.loc.character + md.name.chars().count() as u32,
                md.loc.clone(),
            ));
        }
        for f in &m.fields {
            pending.push((
                uri.to_string(),
                f.loc.line,
                f.loc.character,
                f.loc.character + f.name.chars().count() as u32,
                f.loc.clone(),
            ));
        }
        for cs in &m.calls {
            let me = cs.loc.character + cs.method.chars().count() as u32;
            pending.push((
                uri.to_string(),
                cs.loc.line,
                cs.loc.character,
                me,
                cs.loc.clone(),
            ));
            // Receiver click: an injected-field receiver anchors at its own
            // position and resolves via the field-declaration alias (k2 above);
            // this./bare receivers resolve like the call itself.
            let tgt = if cs.kind == 0 { cs.field_loc.clone() } else { cs.loc.clone() };
            let fe = cs.field_loc.character + cs.field.chars().count() as u32;
            pending.push((
                uri.to_string(),
                cs.field_loc.line,
                cs.field_loc.character,
                fe,
                tgt,
            ));
        }
        for fr in &m.field_refs {
            let end = fr.loc.character + fr.name.chars().count() as u32;
            pending.push((uri.to_string(), fr.loc.line, fr.loc.character, end, fr.loc.clone()));
        }
        for tr in &m.type_refs {
            let end = tr.loc.character + tr.name.chars().count() as u32;
            pending.push((uri.to_string(), tr.loc.line, tr.loc.character, end, tr.loc.clone()));
        }
        for mr in &m.method_refs {
            pending.push((
                uri.to_string(),
                mr.loc.line,
                mr.loc.character,
                mr.loc.character + mr.name.chars().count() as u32,
                mr.loc.clone(),
            ));
        }
    } else {
        for st in &m.xml_stmts {
            pending.push((
                uri.to_string(),
                st.loc.line,
                st.span.0,
                st.span.1,
                st.loc.clone(),
            ));
        }
        if let (Some(loc), Some((s, e))) = (&m.namespace_loc, m.namespace_span) {
            pending.push((uri.to_string(), loc.line, s, e, loc.clone()));
        }
    }
    for (u, line, start, end, loc) in pending {
        reg_span(index, &u, line, start, end, loc);
    }
}

fn remove_one(map: &mut HashMap<Loc, Vec<Loc>>, a: &Loc, b: &Loc) {
    if let Some(v) = map.get_mut(a) {
        if let Some(pos) = v.iter().position(|x| x == b) {
            v.remove(pos);
        }
        if v.is_empty() {
            map.remove(a);
        }
    }
}

fn remove_file_maps(index: &mut Index, fi: usize) {
    let uri = index.files[fi].uri.clone();
    index.by_uri.remove(&uri);
    let empty_keys: Vec<String> = index
        .by_simple
        .iter_mut()
        .filter_map(|(k, v)| {
            v.retain(|x| *x != fi);
            if v.is_empty() {
                Some(k.clone())
            } else {
                None
            }
        })
        .collect();
    for k in empty_keys {
        index.by_simple.remove(&k);
    }
    index.by_fqcn.retain(|_, v| *v != fi);
    for v in index.by_package.values_mut() {
        v.retain(|x| *x != fi);
    }
    index.by_package.retain(|_, v| !v.is_empty());
    for v in index.interface_impls.values_mut() {
        v.retain(|x| *x != fi);
    }
    index.interface_impls.retain(|_, v| !v.is_empty());
    index.method_index.retain(|k, _| k.0 != fi);
    remove_file_lombok(index, fi);
    index.spans.retain(|k, _| k.0 != uri);
    index.call_owner.retain(|_, v| *v != fi);

    // path entries by owner
    let kept: Vec<PathEntry> = index
        .path_entries
        .drain(..)
        .filter(|e| e.owner != fi)
        .collect();
    index.path_entries = kept;
    index.path_index.clear();
    for (i, e) in index.path_entries.iter().enumerate() {
        index
            .path_index
            .entry(e.path.clone())
            .or_default()
            .push(i);
    }

    // xml entries by owner (parallel vectors)
    let keys: Vec<(String, String)> = index.xml_owner.keys().cloned().collect();
    for key in keys {
        let owners = index.xml_owner.get_mut(&key).unwrap();
        let mut new_locs = Vec::new();
        if let Some(locs) = index.xml_index.get(&key) {
            for (i, loc) in locs.iter().enumerate() {
                if owners.get(i).copied() != Some(fi) {
                    new_locs.push(loc.clone());
                }
            }
        }
        *owners = owners.iter().copied().filter(|x| *x != fi).collect();
        if new_locs.is_empty() {
            index.xml_index.remove(&key);
            index.xml_owner.remove(&key);
        } else {
            *index.xml_index.get_mut(&key).unwrap() = new_locs;
        }
    }
}

// ---------------------------------------------------------------------------
// Edge computation as per-file Triple contributions
// ---------------------------------------------------------------------------

fn apply_triple(index: &mut Index, t: &Triple) {
    if t.a == t.b {
        return;
    }
    match t.kind {
        0 => {
            index.edges.entry(t.a.clone()).or_default().push(t.b.clone());
            index.edges.entry(t.b.clone()).or_default().push(t.a.clone());
        }
        1 => {
            index.edges.entry(t.a.clone()).or_default().push(t.b.clone());
            index
                .edges
                .entry(t.b.clone())
                .or_default()
                .push(t.a.clone());
            index.down.entry(t.a.clone()).or_default().push(t.b.clone());
            index.up.entry(t.b.clone()).or_default().push(t.a.clone());
        }
        _ => {
            index
                .aliases
                .entry(t.a.clone())
                .or_default()
                .push(t.b.clone());
            index.down.entry(t.a.clone()).or_default().push(t.b.clone());
            index.up.entry(t.a.clone()).or_default().push(t.b.clone());
        }
    }
}

fn unapply_triple(index: &mut Index, t: &Triple) {
    match t.kind {
        0 => {
            remove_one(&mut index.edges, &t.a, &t.b);
            remove_one(&mut index.edges, &t.b, &t.a);
        }
        1 => {
            remove_one(&mut index.edges, &t.a, &t.b);
            remove_one(&mut index.edges, &t.b, &t.a);
            remove_one(&mut index.down, &t.a, &t.b);
            remove_one(&mut index.up, &t.b, &t.a);
        }
        _ => {
            remove_one(&mut index.aliases, &t.a, &t.b);
            remove_one(&mut index.down, &t.a, &t.b);
            remove_one(&mut index.up, &t.a, &t.b);
        }
    }
}

fn k0(a: Loc, b: Loc) -> Triple {
    Triple { a, b, kind: 0 }
}
fn k1(a: Loc, b: Loc) -> Triple {
    Triple { a, b, kind: 1 }
}
fn k2(a: Loc, b: Loc) -> Triple {
    Triple { a, b, kind: 2 }
}

/// Compute all edge contributions owned by ONE file. Uses the resolution cache
/// for call sites; caches impl/fuzzy-xml lookups to avoid O(N) scans per file.
/// Cached call-site resolution entry: (targets, from_lombok, field_based).
type CallCacheEntry = (Vec<Loc>, bool, bool);

fn compute_file_edges(index: &mut Index, fi: usize, cfg: &Config) -> Vec<Triple> {
    let m = index.files[fi].clone();
    let mut out = Vec::new();
    let mut cache_writes: Vec<(Loc, CallCacheEntry)> = Vec::new();

    // (a) call sites: Controller -> Service (J-01), ServiceImpl -> Mapper (J-03),
    //     this./bare -> current class (P1), static -> imported/same-package class (P1).
    for cs in &m.calls {
        index.call_owner.insert(cs.loc.clone(), fi);
        let (targets, _from_lombok, _field_based) = match index.call_cache.get(&cs.loc).cloned() {
            Some(v) => v,
            None => {
                let mut targets = Vec::new();
                let mut from_lombok = false;
                let mut field_based = false;
                match cs.kind {
                    1 | 2 => {
                        // this.x() / bare x(): current class, then supers.
                        let mut scope = vec![fi];
                        for s in &m.supers {
                            if let Some(&sf) = index.by_fqcn.get(s) {
                                scope.push(sf);
                            }
                        }
                        targets = find_methods(index, &scope, &cs.method, Some(cs.arg_count));
                    }
                    _ => {
                        if let Some(t) = resolve_field_type(index, fi, &cs.field, cfg) {
                            field_based = true;
                            let tfiles = resolve_type_files(index, &t);
                            if !tfiles.is_empty() {
                                targets = find_methods(index, &tfiles, &cs.method, Some(cs.arg_count));
                                if targets.is_empty() {
                                    let lt = find_lombok(index, &tfiles, &cs.method, Some(cs.arg_count));
                                    if !lt.is_empty() {
                                        targets = lt;
                                        from_lombok = true;
                                    }
                                }
                            }
                        } else {
                            // P2: parameter / local variable receiver
                            // (`body.getNote()` where body is a param).
                            let mut hit = false;
                            if let Some(owner) = cs.owner_method {
                                let ty = m.var_decls.iter().find(|v| {
                                    v.method == Some(owner) && v.name == cs.field
                                }).map(|v| v.type_simple.clone());
                                if let Some(t) = ty {
                                    let mut tfiles = resolve_type_files(index, &t);
                                    if tfiles.is_empty() {
                                        tfiles = resolve_class(index, fi, &t);
                                    }
                                    if !tfiles.is_empty() {
                                        targets = find_methods(index, &tfiles, &cs.method, Some(cs.arg_count));
                                        if targets.is_empty() {
                                            let lt = find_lombok(index, &tfiles, &cs.method, Some(cs.arg_count));
                                            if !lt.is_empty() {
                                                targets = lt;
                                                from_lombok = true;
                                            }
                                        }
                                        hit = !targets.is_empty();
                                    }
                                }
                            }
                            if !hit && cs.field.chars().next().is_some_and(|c| c.is_uppercase()) {
                                // static call on a project class (same package / import / unique simple name)
                                let tfiles = resolve_class(index, fi, &cs.field);
                                if !tfiles.is_empty() {
                                    targets = find_methods(index, &tfiles, &cs.method, Some(cs.arg_count));
                                }
                            }
                        }
                    }
                }
                cache_writes.push((cs.loc.clone(), (targets.clone(), from_lombok, field_based)));
                (targets, from_lombok, field_based)
            }
        };
        // Receiver click -> field declaration ("变量支持跳转").
        if cs.kind == 0 {
            if let Some(f) = m.fields.iter().find(|x| x.name == cs.field) {
                out.push(k2(cs.field_loc.clone(), f.loc.clone()));
            }
        }
        if targets.is_empty() {
            continue;
        }
        // 统一语义：所有调用点锚定自身 —— down = 定义，up = 引用。
        // 不再产生 声明->声明 的 owner 链（调用点已携带引用关系）。
        for tm in &targets {
            out.push(k1(cs.loc.clone(), tm.clone()));
        }
    }
    for (k, v) in cache_writes {
        index.call_cache.insert(k, v);
    }

    // (b) interface <-> impl (J-02), multi-implementation -> multiple edges
    if m.role == Role::Service {
        cache_impls(index, fi, cfg);
        let impls = index
            .impls_cache
            .get(&m.fqcn)
            .cloned()
            .unwrap_or_default();
        for md in &m.methods {
            for &ii in &impls {
                let targets = find_methods(index, &[ii], &md.name, Some(md.param_types.len()));
                for tm in targets {
                    out.push(k1(md.loc.clone(), tm));
                }
            }
        }
    }

    // (c) mapper method <-> XML statement (J-04), exact then approximate
    if m.role == Role::Mapper {
        let ns = m.fqcn.clone();
        for md in &m.methods {
            let mut stmts = index
                .xml_index
                .get(&(ns.clone(), md.name.clone()))
                .cloned()
                .unwrap_or_default();
            if stmts.is_empty() {
                cache_xml(index, &ns, &md.name);
                stmts = index
                    .xml_cache
                    .get(&(ns.clone(), md.name.clone()))
                    .cloned()
                    .unwrap_or_default();
            }
            for st in stmts {
                out.push(k1(md.loc.clone(), st));
            }
        }
    }

    // XML namespace -> mapper interface declaration
    if m.kind == FileKind::Xml {
        let ns = m.xml_namespace.clone();
        let target = index
            .by_fqcn
            .get(&ns)
            .map(|&mi| index.files[mi].class_loc.clone());
        if let (Some(loc), Some(t)) = (&m.namespace_loc, target) {
            out.push(k2(loc.clone(), t));
        }
    }

    // (d) entity field <-> get/set (J-06)
    if m.role == Role::Entity {
        for f in &m.fields {
            let cap = capitalize(&f.name);
            let getter_names = [
                format!("get{}", cap),
                format!("is{}", cap),
                f.name.clone(),
            ];
            let setter = format!("set{}", cap);
            let mut gl = Vec::new();
            let mut sl = Vec::new();
            for x in &m.methods {
                if getter_names.contains(&x.name) && x.param_types.is_empty() {
                    gl.push(x.loc.clone());
                }
                if x.name == setter && x.param_types.len() == 1 {
                    sl.push(x.loc.clone());
                }
            }
            for g in &gl {
                out.push(k1(g.clone(), f.loc.clone()));
            }
            for s in &sl {
                out.push(k1(s.clone(), f.loc.clone()));
            }
            for g in &gl {
                for s in &sl {
                    out.push(k0(g.clone(), s.clone()));
                }
            }
        }
    }

    // (e) X::method references (J-07 + P2 `Class::method`): overloads -> list;
    // Lombok accessors resolve to the FIELD declaration.
    for mr in &m.method_refs {
        let scope: Vec<usize> = if mr.class.is_empty() {
            let mut s = vec![fi];
            for sup in &m.supers {
                if let Some(&sf) = index.by_fqcn.get(sup) {
                    s.push(sf);
                }
            }
            s
        } else {
            resolve_class(index, fi, &mr.class)
        };
        if scope.is_empty() {
            continue;
        }
        let mut ms = find_methods(index, &scope, &mr.name, None);
        if ms.is_empty() {
            ms = find_lombok(index, &scope, &mr.name, None);
        }
        for x in ms {
            // k1：引用 -> 定义（down），定义 -> 引用（up 反查）
            out.push(k1(mr.loc.clone(), x));
        }
    }

    // (f) field declaration -> its type's class declaration (convenience)
    for f in &m.fields {
        let targets: Vec<Loc> = resolve_type_files(index, &f.type_simple)
            .iter()
            .map(|&ti| index.files[ti].class_loc.clone())
            .collect();
        for t in targets {
            out.push(k2(f.loc.clone(), t));
        }
    }

    // (g) field/constant references -> field declaration (P1):
    //     `Class.CONST` resolves via same-package/import/unique-simple-name,
    //     `this.field` resolves in the current file.
    for fr in &m.field_refs {
        if fr.class.is_empty() {
            if let Some(f) = m.fields.iter().find(|x| x.name == fr.name) {
                out.push(k1(fr.loc.clone(), f.loc.clone()));
            } else {
                // static import: `import static pkg.C.MEMBER;` then bare MEMBER
                if let Some((cls_fqcn, _)) = m
                    .static_imports
                    .iter()
                    .find(|(_, mem)| mem == &fr.name)
                {
                    if let Some(&cf) = index.by_fqcn.get(cls_fqcn) {
                        if let Some(fd) = index.files[cf].fields.iter().find(|x| x.name == fr.name) {
                            out.push(k1(fr.loc.clone(), fd.loc.clone()));
                        }
                    }
                }
            }
        } else {
            for tf in resolve_class(index, fi, &fr.class) {
                if let Some(fd) = index.files[tf].fields.iter().find(|x| x.name == fr.name) {
                    out.push(k1(fr.loc.clone(), fd.loc.clone()));
                }
            }
        }
    }

    // (h) hand-written trivial accessors -> field (P1), any role
    for md in &m.methods {
        if let Some(fname) = &md.accessor_field {
            if let Some(f) = m.fields.iter().find(|x| x.name == *fname) {
                out.push(k1(md.loc.clone(), f.loc.clone()));
            }
        }
    }

    // (i) type references -> class declaration (click type -> entity class).
    for tr in &m.type_refs {
        let mut tfiles = resolve_type_files(index, &tr.name);
        if tfiles.is_empty() {
            tfiles = resolve_class(index, fi, &tr.name);
        }
        for tf in tfiles {
            out.push(k1(tr.loc.clone(), index.files[tf].class_loc.clone()));
        }
    }

    out
}

/// Resolve a simple class name to project file indices for `fi`'s context:
/// same package first, then explicit imports, then a unique project-wide
/// simple name. Empty when the name is external (JDK / third-party).
pub(crate) fn resolve_class(index: &Index, fi: usize, simple: &str) -> Vec<usize> {
    let m = &index.files[fi];
    let mut out: Vec<usize> = Vec::new();
    let pkg = m
        .fqcn
        .rsplit_once('.')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default();
    if let Some(files) = index.by_package.get(&pkg) {
        for &tf in files {
            if tf != fi && index.files[tf].class_name == simple && !out.contains(&tf) {
                out.push(tf);
            }
        }
    }
    if out.is_empty() {
        let suffix = format!(".{}", simple);
        for imp in &m.imports {
            if imp == simple || imp.ends_with(&suffix) {
                if let Some(&tf) = index.by_fqcn.get(imp.as_str()) {
                    if !out.contains(&tf) {
                        out.push(tf);
                    }
                }
            }
        }
    }
    if out.is_empty() {
        if let Some(files) = index.by_simple.get(simple) {
            if files.len() == 1 {
                out.push(files[0]);
            }
        }
    }
    out
}

fn apply_file_edges(index: &mut Index, fi: usize, cfg: &Config) {
    let triples = compute_file_edges(index, fi, cfg);
    for t in &triples {
        apply_triple(index, t);
    }
    index.edge_contrib.insert(fi, triples);
}

fn unapply_file_edges(index: &mut Index, fi: usize) {
    if let Some(triples) = index.edge_contrib.remove(&fi) {
        for t in &triples {
            unapply_triple(index, t);
        }
    }
}

fn dedup_sort(v: &mut Vec<Loc>) {
    v.sort();
    v.dedup();
}

fn finalize(index: &mut Index) {
    for v in index.edges.values_mut() {
        dedup_sort(v);
    }
    for v in index.aliases.values_mut() {
        dedup_sort(v);
    }
    for v in index.down.values_mut() {
        dedup_sort(v);
    }
    for v in index.up.values_mut() {
        dedup_sort(v);
    }
    for v in index.path_index.values_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in index.xml_index.values_mut() {
        dedup_sort(v);
    }
    for v in index.spans.values_mut() {
        v.sort_by_key(|s| (s.start, s.end));
    }
}

fn recompute_module_count(index: &mut Index) {
    let mut mods: HashSet<String> = HashSet::new();
    for m in &index.files {
        if m.kind == FileKind::Java {
            mods.insert(module_of(&m.path, &index.roots));
        }
    }
    index.module_count = mods.len();
}

/// Full build from scratch.
pub fn build_index_from_files(
    mut files: Vec<FileModel>,
    cfg: &Config,
    roots: Vec<String>,
    project_roots: Vec<String>,
    project_count: usize,
    skipped_large: u32,
    parse_failed: u32,
) -> State {
    for m in &mut files {
        m.role = classify(m, cfg);
    }
    let mut index = Index::blank(roots);
    index.project_roots = project_roots;
    index.project_count = project_count;
    index.skipped_large = skipped_large;
    index.parse_failed = parse_failed;
    index.files = files;
    let n = index.files.len();
    for fi in 0..n {
        add_file_maps(&mut index, fi);
    }
    for fi in 0..n {
        apply_file_edges(&mut index, fi, cfg);
    }
    finalize(&mut index);
    recompute_module_count(&mut index);
    State {
        index,
        config: cfg.clone(),
        gen: 0,
    }
}

/// True incremental update. `changes`: (uri, Some(model)) replace/add,
/// (uri, None) remove. Structural changes (add/remove/identity change) fall
/// back to a full rebuild; pure content edits touch only affected files.
pub fn apply_changes(
    state: &mut State,
    changes: Vec<(String, Option<FileModel>)>,
    cfg: &Config,
) {
    let mut dirty_uris: HashSet<String> = HashSet::new();
    let mut dirty_fis: Vec<usize> = Vec::new();
    let mut structural = false;
    for (uri, model) in &changes {
        dirty_uris.insert(uri.clone());
        match model {
            None => structural = true,
            Some(_) => {
                if let Some(&fi) = state.index.by_uri.get(uri) {
                    dirty_fis.push(fi);
                } else {
                    structural = true;
                }
            }
        }
    }
    if structural {
        let mut files = std::mem::take(&mut state.index.files);
        // Build a uri->model map for changes.
        let mut by_uri_changes: HashMap<String, Option<FileModel>> = HashMap::new();
        for (uri, model) in changes {
            by_uri_changes.insert(uri, model);
        }
        files.retain(|f| !by_uri_changes.contains_key(&f.uri));
        for (uri, model) in by_uri_changes {
            if let Some(mut m) = model {
                m.uri = uri;
                files.push(m);
            }
        }
        full_rebuild(state, files, cfg);
        return;
    }

    // Identity change detection -> full rebuild for safety.
    for (uri, model) in &changes {
        if let (Some(&fi), Some(m)) = (state.index.by_uri.get(uri), model) {
            let old = &state.index.files[fi];
            if old.class_name != m.class_name
                || old.fqcn != m.fqcn
                || old.supers != m.supers
                || old.class_annotations.len() != m.class_annotations.len()
            {
                // Conservative fallback: rebuild fully.
                let mut files = state.index.files.clone();
                for (u, mm) in &changes {
                    if let Some(&i2) = state.index.by_uri.get(u) {
                        if let Some(newm) = mm {
                            files[i2] = newm.clone();
                        }
                    }
                }
                full_rebuild(state, files, cfg);
                return;
            }
        }
    }

    // ---- pure content edit: incremental ----
    // Capture (fi, new model) BEFORE removing by_uri entries. The parsed
    // model has no role yet — classify it here, otherwise role-dependent
    // edge sections ((b) interface→impl, (c) mapper→XML, Controller path
    // entries) are silently dropped on every save.
    let mut replacements: Vec<(usize, FileModel)> = Vec::new();
    for (uri, model) in &changes {
        if let (Some(&fi), Some(m)) = (state.index.by_uri.get(uri), model) {
            let mut m = m.clone();
            m.role = classify(&m, cfg);
            replacements.push((fi, m));
        }
    }

    // 1. identify which cached resolutions become invalid, and who owns them.
    //    A call site is invalid when its own file changed OR one of its cached
    //    targets lives in a changed file.
    let mut affected: HashSet<usize> = dirty_fis.iter().copied().collect();
    let cache_snapshot: Vec<(Loc, CallCacheEntry)> = state
        .index
        .call_cache
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut invalidated: Vec<Loc> = Vec::new();
    for (k, (targets, _, _)) in &cache_snapshot {
        if dirty_uris.contains(&k.uri) || targets.iter().any(|t| dirty_uris.contains(&t.uri)) {
            invalidated.push(k.clone());
            if let Some(&owner) = state.index.call_owner.get(k) {
                affected.insert(owner);
            }
        }
    }
    for loc in invalidated {
        state.index.call_cache.remove(&loc);
    }
    // Cached global lookups (impl patterns / fuzzy XML) may depend on the
    // changed files; clear them so affected files recompute from fresh maps.
    state.index.impls_cache.clear();
    state.index.xml_cache.clear();

    // Any file whose existing edge target lies in a changed file must also
    // recompute (method lines may have moved).
    let contrib_snapshot: Vec<(usize, Vec<Triple>)> = state
        .index
        .edge_contrib
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for (fi, triples) in &contrib_snapshot {
        if triples.iter().any(|t| dirty_uris.contains(&t.b.uri)) {
            affected.insert(*fi);
        }
    }

    // 2. unapply edges + remove maps for dirty files only.
    for &fi in &dirty_fis {
        unapply_file_edges(&mut state.index, fi);
        remove_file_maps(&mut state.index, fi);
    }

    // 3. replace models, add their maps back.
    for (fi, m) in replacements {
        state.index.files[fi] = m;
    }
    for &fi in &dirty_fis {
        add_file_maps(&mut state.index, fi);
    }

    // 4. recompute edges for affected files only (unapply previous, compute new).
    let mut affected: Vec<usize> = affected.into_iter().collect();
    affected.sort_unstable();
    for &fi in &affected {
        unapply_file_edges(&mut state.index, fi);
    }
    for fi in affected {
        apply_file_edges(&mut state.index, fi, cfg);
    }

    finalize(&mut state.index);
    recompute_module_count(&mut state.index);
    state.config = cfg.clone();
}

fn full_rebuild(state: &mut State, files: Vec<FileModel>, cfg: &Config) {
    let roots = state.index.roots.clone();
    let project_roots = state.index.project_roots.clone();
    let pc = state.index.project_count;
    let skipped = state.index.skipped_large;
    let failed = state.index.parse_failed;
    *state = build_index_from_files(files, cfg, roots, project_roots, pc, skipped, failed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(model: &mut FileModel) {
        model.role = Role::Controller;
    }

    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url_path("list"), "/list");
        assert_eq!(normalize_url_path("/sys/"), "/sys");
        assert_eq!(normalize_url_path("//a//b"), "/a/b");
        assert_eq!(join_url_path("/sys", "/list"), "/sys/list");
        assert_eq!(join_url_path("/sys", ""), "/sys");
    }

    #[test]
    fn module_name_relative_to_root() {
        let roots = vec!["/ws".to_string()];
        assert_eq!(module_of("/ws/proj-a/mod-web/src/main/java/A.java", &roots), "proj-a/mod-web");
        assert_eq!(module_of("/ws/src/main/java/A.java", &roots), "ws");
        let roots2 = vec!["/ws/proj-a".to_string()];
        assert_eq!(module_of("/ws/proj-a/mod-web/src/main/java/A.java", &roots2), "mod-web");
    }

    #[test]
    fn full_build_and_directional_edges() {
        let f1 = FileModel {
            uri: "file:///p/A.java".into(),
            path: "/p/src/main/java/x/AController.java".into(),
            kind: FileKind::Java,
            project_root: "/p".into(),
            class_name: "AController".into(),
            fqcn: "x.AController".into(),
            class_loc: Loc::new("file:///p/A.java", 2, 13),
            methods: vec![MethodDecl {
                name: "go".into(),
                param_types: vec![],
                loc: Loc::new("file:///p/A.java", 5, 16),
                paths: vec!["/go".into()],
                http_methods: vec!["GET".into()],
                accessor_field: None,
            }],
            fields: vec![FieldDecl {
                name: "svc".into(),
                type_simple: "AService".into(),
                loc: Loc::new("file:///p/A.java", 3, 20),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut f2 = f1.clone();
        f2.uri = "file:///p/S.java".into();
        f2.path = "/p/src/main/java/x/AService.java".into();
        f2.class_name = "AService".into();
        f2.fqcn = "x.AService".into();
        f2.fields.clear();
        f2.methods = vec![MethodDecl {
            name: "run".into(),
            param_types: vec![],
            loc: Loc::new("file:///p/S.java", 4, 16),
            ..Default::default()
        }];
        let mut fm = f1.clone();
        file(&mut fm);
        fm.calls.push(CallSite {
            field: "svc".into(),
            method: "run".into(),
            arg_count: 0,
            loc: Loc::new("file:///p/A.java", 6, 8),
            field_loc: Loc::new("file:///p/A.java", 6, 8),
            owner_method: Some(0),
            kind: 0,
        });
        let mut fs = f2.clone();
        fs.role = Role::Service;
        let st = build_index_from_files(
            vec![fm, fs],
            &Config::default(),
            vec!["/p".into()],
            vec!["/p".into()],
            1,
            0,
            0,
        );
        assert_eq!(st.index.files.len(), 2);
        // A call site resolves to the service method (down direction).
        let c = Loc::new("file:///p/A.java", 6, 8);
        assert!(st.index.down.contains_key(&c), "call site should have a down edge");
        assert_eq!(st.index.path_index.get("/go").map(|v| v.len()), Some(1));
    }

    #[test]
    fn incremental_update_is_idempotent() {
        // Build twice from the same models; counts must not grow.
        let base = FileModel {
            uri: "file:///p/A.java".into(),
            path: "/p/A.java".into(),
            kind: FileKind::Java,
            project_root: "/p".into(),
            class_name: "A".into(),
            fqcn: "x.A".into(),
            ..Default::default()
        };
        let mut st = build_index_from_files(
            vec![base.clone()],
            &Config::default(),
            vec!["/p".into()],
            vec!["/p".into()],
            1,
            0,
            0,
        );
        let before = st.index.edges.len();
        apply_changes(
            &mut st,
            vec![("file:///p/A.java".into(), Some(base))],
            &Config::default(),
        );
        assert_eq!(st.index.files.len(), 1);
        assert_eq!(st.index.edges.len(), before);
        assert!(st.index.by_uri.contains_key("file:///p/A.java"));
    }

    #[test]
    fn incremental_update_preserves_role_edges() {
        // Save a mapper file (updateFiles path) — its mapper->XML statement
        // edges must survive the incremental rebuild (role re-classified).
        let mut mapper = FileModel {
            uri: "file:///p/M.java".into(),
            path: "/p/src/main/java/x/SysUserMapper.java".into(),
            kind: FileKind::Java,
            project_root: "/p".into(),
            class_name: "SysUserMapper".into(),
            fqcn: "x.SysUserMapper".into(),
            ..Default::default()
        };
        let md = MethodDecl {
            name: "selectUserList".into(),
            param_types: vec!["SysUser".into()],
            loc: Loc::new("file:///p/M.java", 20, 25),
            ..Default::default()
        };
        mapper.methods = vec![md];
        let mut xml = FileModel {
            uri: "file:///p/M.xml".into(),
            path: "/p/src/main/resources/SysUserMapper.xml".into(),
            kind: FileKind::Xml,
            project_root: "/p".into(),
            ..Default::default()
        };
        xml.project_root = "/p".into();
        xml.xml_namespace = "x.SysUserMapper".into();
        xml.xml_stmts = vec![XmlStmt {
            namespace: "x.SysUserMapper".into(),
            id: "selectUserList".into(),
            loc: Loc::new("file:///p/M.xml", 3, 10),
            span: (10, 24),
        }];
        let cfg = Config::default();
        let mut st = build_index_from_files(
            vec![mapper.clone(), xml],
            &cfg,
            vec!["/p".into()],
            vec!["/p".into()],
            1,
            0,
            0,
        );
        let dl = Loc::new("file:///p/M.java", 20, 25);
        assert_eq!(st.index.down.get(&dl).map(|v| v.len()), Some(1));
        // Simulate a save of the mapper file (same content).
        let mut again = mapper;
        again.role = Role::Mapper;
        apply_changes(&mut st, vec![("file:///p/M.java".into(), Some(again))], &cfg);
        assert_eq!(
            st.index.down.get(&dl).map(|v| v.len()),
            Some(1),
            "mapper->XML edges must survive a save"
        );
    }

    #[test]
    fn add_and_remove_edges_are_symmetric() {
        let a = Loc::new("file:///p/A.java", 1, 0);
        let b = Loc::new("file:///p/B.java", 2, 0);
        let mut idx = Index::blank(vec!["/p".into()]);
        let t = k1(a.clone(), b.clone());
        apply_triple(&mut idx, &t);
        assert_eq!(idx.edges.get(&a).map(|v| v.len()), Some(1));
        assert_eq!(idx.edges.get(&b).map(|v| v.len()), Some(1));
        unapply_triple(&mut idx, &t);
        assert!(idx.edges.is_empty());
        assert!(idx.down.is_empty());
        assert!(idx.up.is_empty());
    }
}
