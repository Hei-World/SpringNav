// Pure in-memory query layer. No file reads, no parsing on the query path.

use crate::config::Config;
use crate::index::{
    find_methods, normalize_url_path, resolve_field_type, resolve_type_files, Index,
};
use crate::model::*;
use regex::Regex;
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

pub fn resolve_definition(
    index: &Index,
    cfg: &Config,
    uri: &str,
    line: u32,
    character: u32,
    snippet: &str,
) -> Vec<Loc> {
    let mut out = Vec::new();

    // Primary path: exact span hit -> O(1) adjacency lookup.
    if let Some(spans) = index.spans.get(&(uri.to_string(), line)) {
        if let Some(s) = spans
            .iter()
            .find(|s| s.start <= character && character < s.end)
        {
            out = targets_for(index, &s.loc);
        }
    }
    if !out.is_empty() {
        dedup_sort(&mut out);
        return out;
    }

    // Fallback: use the passed line snippet when the cursor misses a stored
    // span (e.g. between tokens, or a freshly edited line).
    let fi = match index.by_uri.get(uri) {
        Some(&i) => i,
        None => {
            dedup_sort(&mut out);
            return out;
        }
    };

    for caps in RE_FALLBACK_REF.captures_iter(snippet) {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if !name.is_empty() {
            out.extend(find_methods(index, &[fi], name, None));
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
                out.extend(find_methods(index, &tfiles, method, None));
            }
        }
    }

    let word = word_at(snippet, character);
    if !word.is_empty() {
        if let Some(mis) = index.method_index.get(&(fi, word.clone())) {
            for &mi in mis {
                out.extend(targets_for(index, &index.files[fi].methods[mi].loc));
            }
        }
        for f in &index.files[fi].fields {
            if f.name == word {
                out.extend(targets_for(index, &f.loc));
            }
        }
    }

    dedup_sort(&mut out);
    out
}

pub fn search_by_path(index: &Index, path: &str) -> Vec<Loc> {
    let norm = normalize_url_path(path);
    let mut out = Vec::new();
    if let Some(v) = index.path_index.get(&norm) {
        out.extend(v.iter().cloned());
    }
    if out.is_empty() {
        // Approximate: suffix, prefix or substring match of the indexed URL.
        for (k, v) in &index.path_index {
            if k == &norm
                || k.ends_with(&norm)
                || norm.ends_with(k.as_str())
                || k.contains(&norm)
            {
                out.extend(v.iter().cloned());
            }
        }
    }
    if out.is_empty() {
        let alt = norm.trim_start_matches('/');
        for (k, v) in &index.path_index {
            if k.trim_start_matches('/') == alt {
                out.extend(v.iter().cloned());
            }
        }
    }
    dedup_sort(&mut out);
    out
}
