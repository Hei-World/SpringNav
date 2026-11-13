// In-memory index: compact HashMaps only. Every navigation relationship is
// precomputed as an undirected edge, so a query is a single span lookup plus a
// HashMap read -- no disk I/O, no file parsing (P-02/P-03).

use crate::config::{glob_match, Config};
use crate::model::*;
use crate::parser::strip_to_simple;
use std::collections::{HashMap, HashSet};

const DEFAULT_IMPL_PATTERNS: &[&str] = &["*ServiceImpl", "*Impl"];

pub struct Index {
    pub root: String,
    pub files: Vec<FileModel>,
    pub by_uri: HashMap<String, usize>,
    pub by_simple: HashMap<String, Vec<usize>>,
    pub by_fqcn: HashMap<String, usize>,
    /// interface simple/FQCN -> implementing class file indices
    pub interface_impls: HashMap<String, Vec<usize>>,
    /// (file idx, method name) -> method indices (overloads)
    pub method_index: HashMap<(usize, String), Vec<usize>>,
    /// full URL -> indices into `path_entries`
    pub path_index: HashMap<String, Vec<usize>>,
    pub path_entries: Vec<PathEntry>,
    /// (namespace, id) -> XML statement locations
    pub xml_index: HashMap<(String, String), Vec<Loc>>,
    pub spans: HashMap<(String, u32), Vec<Span>>,
    /// undirected navigation edges (both endpoints registered)
    pub edges: HashMap<Loc, Vec<Loc>>,
    /// directional edges: down = 控制器→接口→实现→Mapper→XML；up = 反向
    pub down: HashMap<Loc, Vec<Loc>>,
    pub up: HashMap<Loc, Vec<Loc>>,
    /// one-way aliases: call sites / method refs -> declarations
    pub aliases: HashMap<Loc, Vec<Loc>>,
}

pub struct State {
    pub index: Index,
    pub config: Config,
}

impl Index {
    pub fn blank(root: String) -> Index {
        Index {
            root,
            files: Vec::new(),
            by_uri: HashMap::new(),
            by_simple: HashMap::new(),
            by_fqcn: HashMap::new(),
            interface_impls: HashMap::new(),
            method_index: HashMap::new(),
            path_index: HashMap::new(),
            path_entries: Vec::new(),
            xml_index: HashMap::new(),
            spans: HashMap::new(),
            edges: HashMap::new(),
            down: HashMap::new(),
            up: HashMap::new(),
            aliases: HashMap::new(),
        }
    }
}

impl State {
    pub fn empty() -> State {
        State {
            index: Index::blank(String::new()),
            config: Config::default(),
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
    if let Some(r) = cfg.role_override(&m.class_name, &m.fqcn) {
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
    if let Some(a) = cfg.alias(field) {
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
    let mut out: Vec<usize> = Vec::new();
    // 1) explicit alias (highest priority)
    if let Some(a) = cfg.alias(&m.class_name).or_else(|| cfg.alias(&m.fqcn)) {
        for i in resolve_type_files(index, a) {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    // 2) implements / extends clauses
    for k in [&m.class_name, &m.fqcn] {
        if let Some(v) = index.interface_impls.get(k) {
            for &i in v {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    // 3) impl-patterns + naming fallback
    if out.is_empty() {
        let patterns: Vec<String> = cfg
            .impl_patterns_for("service")
            .cloned()
            .unwrap_or_else(|| DEFAULT_IMPL_PATTERNS.iter().map(|s| s.to_string()).collect());
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

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .count()
}

fn fuzzy_xml(index: &Index, ns: &str, name: &str) -> Vec<Loc> {
    let mut out = Vec::new();
    for ((n, id), locs) in &index.xml_index {
        if n == ns
            && (id == name
                || id.contains(name)
                || name.contains(id)
                || common_prefix(id, name) >= 4)
        {
            out.extend(locs.iter().cloned());
        }
    }
    if out.is_empty() {
        let simple = ns.rsplit('.').next().unwrap_or(ns);
        for ((n, id), locs) in &index.xml_index {
            if n.ends_with(simple)
                && (id == name || id.contains(name) || name.contains(id))
            {
                out.extend(locs.iter().cloned());
            }
        }
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut it = s.chars();
    match it.next() {
        Some(c) => c.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
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

fn add_edge(index: &mut Index, a: Loc, b: Loc) {
    if a == b {
        return;
    }
    index.edges.entry(a.clone()).or_default().push(b.clone());
    index.edges.entry(b).or_default().push(a);
}

/// Add a directional relation: `upper` is closer to the controller / top of the
/// chain, `lower` is deeper (toward Mapper / XML / entity field).
fn add_edge_dir(index: &mut Index, upper: Loc, lower: Loc) {
    if upper == lower {
        return;
    }
    add_edge(index, upper.clone(), lower.clone());
    index.down.entry(upper.clone()).or_default().push(lower.clone());
    index.up.entry(lower).or_default().push(upper);
}

/// Same-level peers (e.g. getter <-> setter): both directions resolve to each other.
fn add_peer(index: &mut Index, a: Loc, b: Loc) {
    if a == b {
        return;
    }
    add_edge(index, a.clone(), b.clone());
    index.down.entry(a.clone()).or_default().push(b.clone());
    index.up.entry(b.clone()).or_default().push(a.clone());
    index.down.entry(b.clone()).or_default().push(a.clone());
    index.up.entry(a).or_default().push(b);
}

fn add_alias(index: &mut Index, a: Loc, b: Loc) {
    if a == b {
        return;
    }
    index.aliases.entry(a.clone()).or_default().push(b.clone());
    // Aliases (call sites, method refs, field->type) are reachable from either
    // direction so that both gestures always land on the declaration.
    index.down.entry(a.clone()).or_default().push(b.clone());
    index.up.entry(a).or_default().push(b);
}

/// Derive the Maven/Gradle module name from a file path.
fn module_of(path: &str) -> String {
    let p = path.replace('\\', "/");
    for marker in ["/src/main/java/", "/src/main/kotlin/", "/src/main/resources/", "/src/"] {
        if let Some(i) = p.find(marker) {
            return p[..i].rsplit('/').next().unwrap_or("").to_string();
        }
    }
    String::new()
}

fn push_path_entry(
    index: &mut Index,
    url: &str,
    file: &str,
    method: &str,
    module: &str,
    loc: &Loc,
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
    });
    index.path_index.entry(url.to_string()).or_default().push(idx);
}

fn build_maps(index: &mut Index) {
    // Path entries are collected first (immutable borrow of `files`), then
    // applied after the loop so we never borrow `index` both ways at once.
    let mut path_candidates: Vec<(String, String, String, String, Loc)> = Vec::new();
    for (idx, m) in index.files.iter().enumerate() {
        if m.kind == FileKind::Java {
            index.by_uri.insert(m.uri.clone(), idx);
            index.by_simple.entry(m.class_name.clone()).or_default().push(idx);
            index.by_fqcn.insert(m.fqcn.clone(), idx);
            for s in &m.supers {
                index.interface_impls.entry(s.clone()).or_default().push(idx);
            }
            if m.role == Role::ServiceImpl {
                if let Some(base) = m.class_name.strip_suffix("Impl") {
                    index
                        .interface_impls
                        .entry(base.to_string())
                        .or_default()
                        .push(idx);
                }
            }
            for (mi, md) in m.methods.iter().enumerate() {
                index
                    .method_index
                    .entry((idx, md.name.clone()))
                    .or_default()
                    .push(mi);
            }
            if m.role == Role::Controller {
                let bases: Vec<String> = if m.class_paths.is_empty() {
                    vec![String::new()]
                } else {
                    m.class_paths.clone()
                };
                let file = format!("{}.java", m.class_name);
                let module = module_of(&m.path);
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
                            // Keep only the FULL path: class-level prefix + method path
                            // (e.g. /sys + /list => /sys/list). No bare /list entries.
                            path_candidates.push((
                                join_url_path(b, p),
                                file.clone(),
                                verb.clone(),
                                module.clone(),
                                md.loc.clone(),
                            ));
                        }
                    }
                }
            }
        } else {
            for st in &m.xml_stmts {
                index
                    .xml_index
                    .entry((st.namespace.clone(), st.id.clone()))
                    .or_default()
                    .push(st.loc.clone());
            }
        }
    }

    for (url, file, verb, module, loc) in path_candidates {
        push_path_entry(index, &url, &file, &verb, &module, &loc);
    }
}

fn register_spans(index: &mut Index) {
    // Collect first: `index` is borrowed immutably while walking the files,
    // then mutated in a second pass.
    let mut pending: Vec<(String, u32, u32, u32, Loc)> = Vec::new();
    for m in &index.files {
        let uri = m.uri.clone();
        if m.kind == FileKind::Java {
            pending.push((
                uri.clone(),
                m.class_loc.line,
                m.class_loc.character,
                m.class_loc.character + m.class_name.chars().count() as u32,
                m.class_loc.clone(),
            ));
            for md in &m.methods {
                pending.push((
                    uri.clone(),
                    md.loc.line,
                    md.loc.character,
                    md.loc.character + md.name.chars().count() as u32,
                    md.loc.clone(),
                ));
            }
            for f in &m.fields {
                pending.push((
                    uri.clone(),
                    f.loc.line,
                    f.loc.character,
                    f.loc.character + f.name.chars().count() as u32,
                    f.loc.clone(),
                ));
            }
            for cs in &m.calls {
                let me = cs.loc.character + cs.method.chars().count() as u32;
                pending.push((uri.clone(), cs.loc.line, cs.loc.character, me, cs.loc.clone()));
                let fe = cs.field_loc.character + cs.field.chars().count() as u32;
                pending.push((
                    uri.clone(),
                    cs.field_loc.line,
                    cs.field_loc.character,
                    fe,
                    cs.loc.clone(),
                ));
            }
            for mr in &m.method_refs {
                pending.push((
                    uri.clone(),
                    mr.loc.line,
                    mr.loc.character,
                    mr.loc.character + mr.name.chars().count() as u32,
                    mr.loc.clone(),
                ));
            }
        } else {
            for st in &m.xml_stmts {
                pending.push((
                    uri.clone(),
                    st.loc.line,
                    st.span.0,
                    st.span.1,
                    st.loc.clone(),
                ));
            }
            if let (Some(loc), Some((s, e))) = (&m.namespace_loc, m.namespace_span) {
                pending.push((uri.clone(), loc.line, s, e, loc.clone()));
            }
        }
    }
    for (uri, line, start, end, loc) in pending {
        reg_span(index, &uri, line, start, end, loc);
    }
}

fn build_edges(index: &mut Index, cfg: &Config) {
    let n = index.files.len();

    // (a) call sites: Controller -> Service (J-01), ServiceImpl -> Mapper (J-03)
    for fi in 0..n {
        let calls = index.files[fi].calls.clone();
        for cs in calls {
            let target_type = resolve_field_type(index, fi, &cs.field, cfg);
            let Some(t) = target_type else { continue };
            let tfiles = resolve_type_files(index, &t);
            if tfiles.is_empty() {
                continue;
            }
            let targets = find_methods(index, &tfiles, &cs.method, Some(cs.arg_count));
            if targets.is_empty() {
                continue;
            }
            let owner_loc: Option<Loc> = cs
                .owner_method
                .and_then(|o| index.files[fi].methods.get(o).map(|m| m.loc.clone()));
            for tm in &targets {
                add_alias(index, cs.loc.clone(), tm.clone());
                if let Some(ol) = &owner_loc {
                    // Controller method -> Service method, ServiceImpl -> Mapper: down.
                    add_edge_dir(index, ol.clone(), tm.clone());
                }
            }
        }
    }

    // (b) interface <-> impl (J-02), multi-implementation -> multiple edges
    let services: Vec<usize> = (0..n)
        .filter(|&i| index.files[i].role == Role::Service)
        .collect();
    for si in services {
        let impls = find_impls(index, si, cfg);
        let methods = index.files[si].methods.clone();
        for md in &methods {
            for &ii in &impls {
                let targets =
                    find_methods(index, &[ii], &md.name, Some(md.param_types.len()));
                for tm in targets {
                    // interface method -> impl method: down.
                    add_edge_dir(index, md.loc.clone(), tm);
                }
            }
        }
    }

    // (c) mapper method <-> XML statement (J-04), exact then approximate
    let mappers: Vec<usize> = (0..n)
        .filter(|&i| index.files[i].role == Role::Mapper)
        .collect();
    for mi in mappers {
        let ns = index.files[mi].fqcn.clone();
        let methods = index.files[mi].methods.clone();
        for md in &methods {
            let mut stmts = index
                .xml_index
                .get(&(ns.clone(), md.name.clone()))
                .cloned()
                .unwrap_or_default();
            if stmts.is_empty() {
                stmts = fuzzy_xml(index, &ns, &md.name);
            }
            for st in stmts {
                // Mapper method -> XML statement: down.
                add_edge_dir(index, md.loc.clone(), st);
            }
        }
    }

    // XML namespace -> mapper interface declaration
    for xi in 0..n {
        if index.files[xi].kind != FileKind::Xml {
            continue;
        }
        let ns = index.files[xi].xml_namespace.clone();
        let target = index
            .by_fqcn
            .get(&ns)
            .map(|&mi| index.files[mi].class_loc.clone());
        if let (Some(loc), Some(t)) = (&index.files[xi].namespace_loc, target) {
            add_alias(index, loc.clone(), t);
        }
    }

    // (d) entity field <-> get/set (J-06)
    let entities: Vec<usize> = (0..n)
        .filter(|&i| index.files[i].role == Role::Entity)
        .collect();
    for ei in entities {
        let fields = index.files[ei].fields.clone();
        let methods = index.files[ei].methods.clone();
        for f in &fields {
            let cap = capitalize(&f.name);
            let getter_names = [
                format!("get{}", cap),
                format!("is{}", cap),
                f.name.clone(),
            ];
            let setter = format!("set{}", cap);
            let mut gl = Vec::new();
            let mut sl = Vec::new();
            for m in &methods {
                if getter_names.contains(&m.name) && m.param_types.is_empty() {
                    gl.push(m.loc.clone());
                }
                if m.name == setter && m.param_types.len() == 1 {
                    sl.push(m.loc.clone());
                }
            }
            // Entity: getter/setter -> field is "down"; field -> accessor is "up".
            for g in &gl {
                add_edge_dir(index, g.clone(), f.loc.clone());
            }
            for s in &sl {
                add_edge_dir(index, s.clone(), f.loc.clone());
            }
            for g in &gl {
                for s in &sl {
                    add_peer(index, g.clone(), s.clone());
                }
            }
        }
    }

    // (e) this::method references (J-07), overloads -> list
    for fi in 0..n {
        let refs = index.files[fi].method_refs.clone();
        for mr in refs {
            for m in find_methods(index, &[fi], &mr.name, None) {
                add_alias(index, mr.loc.clone(), m);
            }
        }
    }

    // (f) field declaration -> its type's class declaration (convenience)
    for fi in 0..n {
        let fields = index.files[fi].fields.clone();
        for f in &fields {
            let targets: Vec<Loc> = resolve_type_files(index, &f.type_simple)
                .iter()
                .map(|&ti| index.files[ti].class_loc.clone())
                .collect();
            for t in targets {
                add_alias(index, f.loc.clone(), t);
            }
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

pub fn build_index_from_files(mut files: Vec<FileModel>, cfg: &Config, root: String) -> State {
    for m in &mut files {
        m.role = classify(m, cfg);
    }
    let mut index = Index::blank(root);
    index.files = files;
    build_maps(&mut index);
    register_spans(&mut index);
    build_edges(&mut index, cfg);
    finalize(&mut index);
    State {
        index,
        config: cfg.clone(),
    }
}
