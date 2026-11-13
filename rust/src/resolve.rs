// Pure in-memory query layer. No file reads, no parsing on the query path.

use crate::config::Config;
use crate::index::{
    find_lombok, find_methods, normalize_url_path, resolve_field_type, resolve_type_files, Index,
};
use crate::model::*;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static RE_FALLBACK_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*\.\s*(\w+)\s*\(").unwrap());
static RE_FALLBACK_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bthis\s*::\s*(\w+)").unwrap());

fn dedup_sort(v: &mut Vec<Loc>) {
    v.sort();
    v.dedup();
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Extract the identifier under a UTF-16 column in `s` (fallback resolution).
fn word_at(s: &str, utf16_col: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut idx: Option<usize> = None;
    let mut u = 0u32;
    for (i, c) in chars.iter().enumerate() {
        let w = c.len_utf16() as u32;
        if u <= utf16_col && utf16_col < u + w {
            idx = Some(i);
            break;
        }
        u += w;
    }
    let idx = match idx {
        Some(v) => v,
        None => return String::new(),
    };
    if idx >= chars.len() || !is_word_char(chars[idx]) {
        return String::new();
    }
    let mut l = idx;
    while l > 0 && is_word_char(chars[l - 1]) {
        l -= 1;
    }
    let mut r = idx;
    while r + 1 < chars.len() && is_word_char(chars[r + 1]) {
        r += 1;
    }
    chars[l..=r].iter().collect()
}

fn targets_for(index: &Index, loc: &Loc) -> Vec<Loc> {
    let mut out = Vec::new();
    if let Some(v) = index.edges.get(loc) {
        out.extend(v.iter().cloned());
    }
    if let Some(v) = index.aliases.get(loc) {
        out.extend(v.iter().cloned());
    }
    out
}

fn span_loc(index: &Index, uri: &str, line: u32, character: u32) -> Option<Loc> {
    let spans = index.spans.get(&(uri.to_string(), line))?;
    spans
        .iter()
        .find(|s| s.start <= character && character < s.end)
        .map(|s| s.loc.clone())
}

/// True when `loc` is the declaration site of a method in a ServiceImpl file.
///
/// Clicking such a declaration must NOT follow the call chain down into the
/// Mapper (the calls in the method body carry their own anchors). Its
/// relationship counterpart is the interface method it implements — up.
fn is_impl_method_decl(index: &Index, loc: &Loc) -> bool {
    match index.by_uri.get(&loc.uri) {
        Some(&fi) if index.files[fi].role == Role::ServiceImpl => {
            index.files[fi].methods.iter().any(|md| md.loc == *loc)
        }
        _ => false,
    }
}

/// Fallback resolution from the current line text, used only when the cursor
/// misses every stored span (e.g. a freshly edited line).
fn fallback_targets(
    index: &Index,
    cfg: &Config,
    uri: &str,
    character: u32,
    snippet: &str,
) -> Vec<Loc> {
    let mut out = Vec::new();
    let fi = match index.by_uri.get(uri) {
        Some(&i) => i,
        None => return out,
    };

    for caps in RE_FALLBACK_REF.captures_iter(snippet) {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if !name.is_empty() {
            let mut ms = find_methods(index, &[fi], name, None);
            if ms.is_empty() {
                ms = find_lombok(index, &[fi], name, None);
            }
            out.extend(ms);
        }
    }
    for caps in RE_FALLBACK_CALL.captures_iter(snippet) {
        let field = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let method = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if field.is_empty() || method.is_empty() {
            continue;
        }
        if let Some(t) = resolve_field_type(index, fi, field, cfg) {
            let tfiles = resolve_type_files(index, &t);
            if !tfiles.is_empty() {
                let mut ms = find_methods(index, &tfiles, method, None);
                if ms.is_empty() {
                    ms = find_lombok(index, &tfiles, method, None);
                }
                out.extend(ms);
            }
        } else if field.chars().next().is_some_and(|c| c.is_uppercase()) {
            // Static call on a project class (P1): same package / import /
            // unique simple name — mirrors the (a) index-time resolution.
            let tfiles = crate::index::resolve_class(index, fi, field);
            if !tfiles.is_empty() {
                out.extend(find_methods(index, &tfiles, method, None));
            }
        }
    }

    let word = word_at(snippet, character);
    if !word.is_empty() {
        if let Some(mis) = index.method_index.get(&(fi, word.clone())) {
            for &mi in mis {
                let dl = &index.files[fi].methods[mi].loc;
                if is_impl_method_decl(index, dl) {
                    if let Some(v) = index.up.get(dl) {
                        out.extend(v.iter().cloned());
                    }
                } else {
                    out.extend(targets_for(index, dl));
                }
            }
        }
        for f in &index.files[fi].fields {
            if f.name == word {
                out.extend(targets_for(index, &f.loc));
            }
        }
    }
    out
}

/// Direction-agnostic resolution (kept for compatibility / "both").
pub fn resolve_definition(
    index: &Index,
    cfg: &Config,
    uri: &str,
    line: u32,
    character: u32,
    snippet: &str,
) -> Vec<Loc> {
    let mut out = Vec::new();
    if let Some(loc) = span_loc(index, uri, line, character) {
        out = if is_impl_method_decl(index, &loc) {
            index.up.get(&loc).cloned().unwrap_or_default()
        } else {
            targets_for(index, &loc)
        };
    }
    if out.is_empty() {
        out = fallback_targets(index, cfg, uri, character, snippet);
    }
    dedup_sort(&mut out);
    out
}

/// Directional resolution.
/// - `direction`: "down" (控制器→接口→实现→Mapper→XML, getXX→字段),
///   "up" (reverse), or "both".
/// - `fallback`: when the requested direction has no result, try the opposite
///   direction once ("向下没有匹配项回退向上跳一级" / "已到顶级则向下").
#[allow(clippy::too_many_arguments)] // explicit query parameters from the TS shell
pub fn resolve_directional(
    index: &Index,
    cfg: &Config,
    uri: &str,
    line: u32,
    character: u32,
    snippet: &str,
    direction: &str,
    fallback: bool,
) -> Vec<Loc> {
    let dir = direction.to_ascii_lowercase();
    let mut out = Vec::new();

    if let Some(loc) = span_loc(index, uri, line, character) {
        match dir.as_str() {
            "up" => {
                if let Some(v) = index.up.get(&loc) {
                    out.extend(v.iter().cloned());
                }
                if out.is_empty() && fallback {
                    if let Some(v) = index.down.get(&loc) {
                        out.extend(v.iter().cloned());
                    }
                }
            }
            "down" => {
                if is_impl_method_decl(index, &loc) {
                    // ServiceImpl method declaration: jump to the interface
                    // method it implements (the body's calls are anchored at
                    // their own call sites, not at the declaration).
                    if let Some(v) = index.up.get(&loc) {
                        out.extend(v.iter().cloned());
                    }
                } else {
                    if let Some(v) = index.down.get(&loc) {
                        out.extend(v.iter().cloned());
                    }
                    if out.is_empty() && fallback {
                        if let Some(v) = index.up.get(&loc) {
                            out.extend(v.iter().cloned());
                        }
                    }
                }
            }
            _ => out = targets_for(index, &loc),
        }
    }
    if !out.is_empty() {
        dedup_sort(&mut out);
        return out;
    }

    // Cursor missed a span: use the snippet fallback (direction-agnostic).
    out = fallback_targets(index, cfg, uri, character, snippet);
    dedup_sort(&mut out);
    out
}

pub fn search_by_path(index: &Index, path: &str) -> Vec<Loc> {
    let norm = normalize_url_path(path);
    let mut out = Vec::new();
    if let Some(v) = index.path_index.get(&norm) {
        for &i in v {
            out.push(index.path_entries[i].loc.clone());
        }
    }
    if out.is_empty() {
        for (k, v) in &index.path_index {
            if k == &norm
                || k.ends_with(&norm)
                || norm.ends_with(k.as_str())
                || k.contains(&norm)
            {
                for &i in v {
                    out.push(index.path_entries[i].loc.clone());
                }
            }
        }
    }
    if out.is_empty() {
        let alt = norm.trim_start_matches('/');
        for (k, v) in &index.path_index {
            if k.trim_start_matches('/') == alt {
                for &i in v {
                    out.push(index.path_entries[i].loc.clone());
                }
            }
        }
    }
    dedup_sort(&mut out);
    out
}

/// Real-time path search for the QuickPick list: every whitespace token must
/// appear in path/file/verb/module (AND); if that yields nothing, fall back to
/// OR. Returns display-ready entries (no filesystem paths).
pub fn search_path_hits(index: &Index, query: &str) -> Vec<PathEntry> {
    let q = query.trim();
    let ql = q.to_lowercase();
    // Fuzzy tokens: split on whitespace AND '/', so `/sys/user/list` still
    // matches `/system/user/list`, while the displayed label stays the full path.
    let tokens: Vec<String> = ql
        .split(|c: char| c.is_whitespace() || c == '/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if tokens.is_empty() {
        return Vec::new();
    }

    let hay = |e: &PathEntry| {
        format!(
            "{} {} {} {}",
            e.path.to_lowercase(),
            e.file.to_lowercase(),
            e.method.to_lowercase(),
            e.module.to_lowercase()
        )
    };

    let mut picked: Vec<&PathEntry> = {
        let and: Vec<&PathEntry> = index
            .path_entries
            .iter()
            .filter(|e| tokens.iter().all(|t| hay(e).contains(t.as_str())))
            .collect();
        if and.is_empty() {
            index
                .path_entries
                .iter()
                .filter(|e| tokens.iter().any(|t| hay(e).contains(t.as_str())))
                .collect()
        } else {
            and
        }
    };

    picked.sort_by(|a, b| {
        let ea = a.path == q;
        let eb = b.path == q;
        eb.cmp(&ea)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.loc.cmp(&b.loc))
    });

    let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
    let mut out = Vec::new();
    for e in picked {
        if seen.insert((e.path.clone(), e.loc.line, e.loc.character)) {
            out.push(e.clone());
            if out.len() >= 500 {
                break;
            }
        }
    }
    out
}
