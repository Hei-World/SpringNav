// Lightweight hand-written parser: line scanning + a small state machine + a
// handful of regular expressions. No tree-sitter, no full AST (P-04).
// Only metadata is emitted; the source text is never retained.

use crate::model::*;
use crate::scanner::Candidate;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static RE_PACKAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*package\s+([\w.]+)\s*;").unwrap());

static RE_CLASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(class|interface|enum|record)\s+(\w+)\b(.*)$").unwrap());

static RE_KW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:extends|implements)\b").unwrap());

static RE_ANNOTATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@([\w.]+)(?:\s*\((.*)\))?\s*$").unwrap());

static RE_QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["']([^"']*)["']"#).unwrap());

static RE_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?:(?:public|private|protected|static|final|abstract|synchronized|native|default|strictfp|transient|volatile)\s+|<[^>]+>\s+)*([A-Za-z_$][\w$<>\[\],.?]*)\s+(\w+)\s*\(",
    )
    .unwrap()
});

static RE_PARAM_ANN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@[\w.]+(?:\([^)]*\))?").unwrap());

static RE_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*\.\s*(\w+)\s*\(").unwrap());

static RE_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*import\s+(?:static\s+)?([\w.]+)\s*;").unwrap());

static RE_BARE_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-zA-Z_$][\w$]*)\s*\(").unwrap());

/// `Class.CONST` — uppercase constant reference.
static RE_CONST_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z][\w$]*)\s*\.\s*([A-Z][A-Z0-9_$]*)\b").unwrap());

/// `this.field` reference (read or write).
static RE_THIS_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bthis\s*\.\s*([a-zA-Z_$][\w$]*)").unwrap());

static RE_METHOD_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bthis\s*::\s*(\w+)").unwrap());

static RE_MAPPER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<mapper\b[^>]*\bnamespace\s*=\s*["']([^"']+)["']"#).unwrap()
});

static RE_STMT_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<(select|insert|update|delete)\b").unwrap());

static RE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bid\s*=\s*["']([^"']+)["']"#).unwrap());

const CONTROL: &[&str] = &[
    "if", "for", "while", "switch", "catch", "try", "finally", "do", "else", "return",
    "throw", "new", "case", "assert", "yield", "break", "continue", "synchronized",
    "instanceof", "super", "this", "import", "package",
];

const MODIFIERS: &[&str] = &[
    "public", "private", "protected", "static", "final", "transient", "volatile",
    "default", "abstract", "synchronized", "native", "strictfp",
];

/// UTF-16-ish column for a byte offset inside `line`.
pub fn col(line: &str, byte: usize) -> u32 {
    let b = byte.min(line.len());
    // `b` is always a char boundary (offsets come from ASCII tokens / regex).
    line[..b].chars().count() as u32
}

fn is_ident(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    it.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn paren_balance(s: &str) -> i32 {
    let mut d = 0i32;
    let mut quote: Option<u8> = None;
    for &b in s.as_bytes() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => d += 1,
            b')' => d -= 1,
            _ => {}
        }
    }
    d
}

/// Byte index of the ')' matching the '(' at `open`.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut d = 1i32;
    let mut quote: Option<u8> = None;
    for (i, &c) in b.iter().enumerate().skip(open + 1) {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => quote = Some(c),
            b'(' => d += 1,
            b')' => {
                d -= 1;
                if d == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split by `delim` at top level (ignore delimiters nested in <>, (), []).
fn split_top_level(s: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut d = 0i32;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '<' | '(' | '[' => {
                d += 1;
                cur.push(c);
            }
            '>' | ')' | ']' => {
                d -= 1;
                cur.push(c);
            }
            _ if c == delim && d == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Reduce a type spelling (`List<String>`, `String[]`, `java.lang.Long`) to its
/// simple name. Generics/arrays/variadics/wildcards are intentionally discarded.
pub fn strip_to_simple(s: &str) -> String {
    let mut t = s.trim().trim_matches(|c: char| {
        c == '{' || c == '}' || c == ';' || c.is_whitespace()
    });
    if let Some(i) = t.find('<') {
        t = &t[..i];
    }
    let mut t = t.trim();
    while t.ends_with("[]") {
        t = t[..t.len() - 2].trim_end();
    }
    if let Some(rest) = t.strip_suffix("...") {
        t = rest.trim_end();
    }
    if let Some(rest) = t.strip_prefix("? extends ").or_else(|| t.strip_prefix("? super ")) {
        t = rest.trim();
    }
    let last = t.rsplit('.').next().unwrap_or(t).trim();
    last.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

fn find_top_level_eq(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut d = 0i32;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '<' | '(' | '[' => d += 1,
            '>' | ')' | ']' => d -= 1,
            '=' if d == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Byte length of a leading `@Annotation(...)` prefix (balanced parens).
fn annotation_prefix_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'@') {
        return None;
    }
    let mut i = 1usize;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
        i += 1;
    }
    if i == 1 {
        return None;
    }
    let mut j = i;
    while j < b.len() && b[j].is_ascii_whitespace() {
        j += 1;
    }
    if j < b.len() && b[j] == b'(' {
        let mut d = 1i32;
        let mut k = j + 1;
        let mut quote: Option<u8> = None;
        while k < b.len() {
            let c = b[k];
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == b'"' || c == b'\'' {
                quote = Some(c);
            } else if c == b'(' {
                d += 1;
            } else if c == b')' {
                d -= 1;
                if d == 0 {
                    return Some(k + 1);
                }
            }
            k += 1;
        }
        return None;
    }
    Some(i)
}

fn parse_annotation(t: &str) -> Option<Annotation> {
    let caps = RE_ANNOTATION.captures(t)?;
    let name = caps.get(1)?.as_str().rsplit('.').next()?.to_string();
    let mut paths = Vec::new();
    let mut raw = String::new();
    if let Some(args) = caps.get(2) {
        raw = args.as_str().to_string();
        for q in RE_QUOTED.captures_iter(args.as_str()) {
            if let Some(m) = q.get(1) {
                paths.push(m.as_str().to_string());
            }
        }
    }
    Some(Annotation { name, paths, raw })
}

/// Infer HTTP verbs from mapping annotations.
fn collect_http_methods(anns: &[Annotation]) -> Vec<String> {
    let mut out = Vec::new();
    for a in anns {
        let m = match a.name.as_str() {
            "GetMapping" => Some("GET"),
            "PostMapping" => Some("POST"),
            "PutMapping" => Some("PUT"),
            "DeleteMapping" => Some("DELETE"),
            "PatchMapping" => Some("PATCH"),
            _ => None,
        };
        if let Some(m) = m {
            if !out.iter().any(|x: &String| x == m) {
                out.push(m.to_string());
            }
            continue;
        }
        if a.name == "RequestMapping" {
            for verb in ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"] {
                if a.raw.contains(&format!("RequestMethod.{}", verb)) && !out.iter().any(|x| x == verb) {
                    out.push(verb.to_string());
                }
            }
            if out.is_empty() {
                out.push("REQUEST".to_string());
            }
        }
    }
    out
}

fn collect_paths(anns: &[Annotation]) -> Vec<String> {
    let mut out = Vec::new();
    for a in anns {
        let n = a.name.as_str();
        if !matches!(
            n,
            "RequestMapping"
                | "GetMapping"
                | "PostMapping"
                | "PutMapping"
                | "DeleteMapping"
                | "PatchMapping"
        ) {
            continue;
        }
        for p in &a.paths {
            if p.starts_with('/') || p.starts_with("${") || p.contains('{') {
                out.push(p.clone());
            }
        }
    }
    out
}

fn parse_class_decl(line: &str) -> Option<(String, bool, Vec<String>, usize)> {
    let caps = RE_CLASS.captures(line)?;
    let kind = caps.get(1)?.as_str();
    let name = caps.get(2)?.as_str().to_string();
    let tail = caps.get(3).map(|m| m.as_str()).unwrap_or("");
    let cleaned = RE_KW.replace_all(tail, " ");
    let mut supers = Vec::new();
    for part in cleaned.split(',') {
        let s = strip_to_simple(part);
        if !s.is_empty() && s != name {
            supers.push(s);
        }
    }
    Some((name, kind == "interface", supers, caps.get(2)?.start()))
}

fn parse_params(s: &str) -> Vec<String> {
    let mut types = Vec::new();
    for part in split_top_level(s, ',') {
        let cleaned = RE_PARAM_ANN.replace_all(part.trim(), "");
        let words: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|w| !MODIFIERS.contains(w))
            .collect();
        if words.is_empty() {
            continue;
        }
        let ty = if words.len() >= 2 {
            words[words.len() - 2]
        } else {
            words[0]
        };
        types.push(strip_to_simple(ty));
    }
    types
}

fn count_args(line: &str, open: usize) -> usize {
    let b = line.as_bytes();
    let mut d = 1i32;
    let mut commas = 0usize;
    let mut saw = false;
    let mut quote: Option<u8> = None;
    for &c in b.iter().skip(open + 1) {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => {
                quote = Some(c);
                saw = true;
            }
            b'(' | b'<' | b'[' => {
                d += 1;
                saw = true;
            }
            b')' | b'>' | b']' => {
                d -= 1;
                if d == 0 {
                    break;
                }
            }
            b',' if d == 1 => {
                commas += 1;
                saw = true;
            }
            _ => {
                if !c.is_ascii_whitespace() {
                    saw = true;
                }
            }
        }
    }
    if saw {
        commas + 1
    } else {
        0
    }
}

fn try_method_decl(
    first_line: &str,
    lines: &[&str],
    i: usize,
    uri: &str,
    class_name: &str,
) -> Option<(MethodDecl, usize)> {
    let caps = RE_METHOD.captures(first_line)?;
    let name = caps.get(2)?.as_str().to_string();
    let open = caps.get(0)?.end().checked_sub(1)?;
    let prefix = &first_line[..open];
    for w in prefix.split_whitespace() {
        if CONTROL.contains(&w) {
            return None;
        }
    }
    if name == class_name {
        return None; // constructor
    }

    let mut joined = first_line.to_string();
    let mut consumed = 1usize;
    while paren_balance(&joined) > 0 && i + consumed < lines.len() {
        joined.push(' ');
        joined.push_str(lines[i + consumed].trim_end_matches('\r'));
        consumed += 1;
    }
    let close = matching_paren(&joined, open)?;
    let params = &joined[open + 1..close];
    let after = joined[close + 1..].trim();
    if !(after.is_empty()
        || after.starts_with('{')
        || after.starts_with(';')
        || after.starts_with("throws"))
    {
        return None;
    }
    let param_types = parse_params(params);
    let name_byte = caps.get(2)?.start();
    Some((
        MethodDecl {
            name,
            param_types,
            loc: Loc::new(uri, i as u32, col(first_line, name_byte)),
            paths: Vec::new(),
            http_methods: Vec::new(),
            accessor_field: None,
        },
        consumed,
    ))
}

fn try_field_decl(line: &str, line_no: u32, uri: &str) -> Option<FieldDecl> {
    let t = line.trim();
    if !t.ends_with(';') || t.contains('(') || t.contains(')') {
        return None;
    }
    if t.starts_with("return") || t.starts_with("throw") || t.starts_with("import") {
        return None;
    }
    let body = t.trim_end_matches(';');
    let cut = match find_top_level_eq(body) {
        Some(i) => &body[..i],
        None => body,
    };
    let words: Vec<&str> = cut.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let name = *words.last()?;
    if !is_ident(name) || CONTROL.contains(&words[0]) {
        return None;
    }
    let type_words: Vec<&str> = words[..words.len() - 1]
        .iter()
        .copied()
        .filter(|w| !MODIFIERS.contains(w))
        .collect();
    if type_words.is_empty() {
        return None;
    }
    let type_simple = strip_to_simple(&type_words.join(" "));
    if type_simple.is_empty() {
        return None;
    }
    let byte = line.rfind(name).unwrap_or(0);
    Some(FieldDecl {
        name: name.to_string(),
        type_simple,
        loc: Loc::new(uri, line_no, col(line, byte)),
        is_static: words.contains(&"static"),
        is_final: words.contains(&"final"),
        annotations: Vec::new(),
    })
}

/// Uppercase the first character (for get/set name matching).
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn scan_calls(line: &str, line_no: u32, uri: &str, model: &mut FileModel) {
    for caps in RE_CALL.captures_iter(line) {
        let field = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let method = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if field.is_empty() || method.is_empty() {
            continue;
        }
        let open = caps.get(0).unwrap().end().saturating_sub(1);
        let method_start = caps.get(2).unwrap().start();
        let field_start = caps.get(1).unwrap().start();
        let kind = if field == "this" { 1 } else { 0 };
        model.calls.push(CallSite {
            field: field.to_string(),
            method: method.to_string(),
            arg_count: count_args(line, open),
            loc: Loc::new(uri, line_no, col(line, method_start)),
            field_loc: Loc::new(uri, line_no, col(line, field_start)),
            owner_method: None,
            kind,
        });
    }
}

/// Bare same-class calls `foo(...)` — skips keywords, annotations, `new X(...)`
/// and declaration lines (they start with visibility modifiers).
fn scan_bare_calls(line: &str, line_no: u32, uri: &str, model: &mut FileModel) {
    let t = line.trim_start();
    for w in t.split_whitespace().take(3) {
        if MODIFIERS.contains(&w) {
            return; // declaration line
        }
    }
    if t.starts_with('@') {
        return;
    }
    for caps in RE_BARE_CALL.captures_iter(line) {
        let m = caps.get(1).unwrap();
        let name = m.as_str();
        if CONTROL.contains(&name) {
            continue;
        }
        let rest = &line[caps.get(0).unwrap().end()..];
        let before = &line[..m.start()];
        if before.ends_with("new ") || before.ends_with("new\t") {
            continue; // constructor
        }
        if before.ends_with('.') {
            continue; // x.foo( already captured by RE_CALL
        }
        let open = caps.get(0).unwrap().end().saturating_sub(1);
        model.calls.push(CallSite {
            field: String::new(),
            method: name.to_string(),
            arg_count: count_args(line, open),
            loc: Loc::new(uri, line_no, col(line, m.start())),
            field_loc: Loc::new(uri, line_no, col(line, m.start())),
            owner_method: None,
            kind: 2,
        });
        let _ = rest;
    }
}

/// `Class.CONST` constants and `this.field` references.
fn scan_field_refs(line: &str, line_no: u32, uri: &str, model: &mut FileModel) {
    for caps in RE_CONST_REF.captures_iter(line) {
        let cls = caps.get(1).unwrap();
        let name = caps.get(2).unwrap();
        model.field_refs.push(FieldRef {
            class: cls.as_str().to_string(),
            name: name.as_str().to_string(),
            loc: Loc::new(uri, line_no, col(line, name.start())),
        });
    }
    for caps in RE_THIS_FIELD.captures_iter(line) {
        let m = caps.get(1).unwrap();
        model.field_refs.push(FieldRef {
            class: String::new(),
            name: m.as_str().to_string(),
            loc: Loc::new(uri, line_no, col(line, m.start())),
        });
    }
}

/// Trivial accessor body detection: scan a few body lines for
/// `return this.x;` / `return x;` (get/is) or `this.x = ...;` (set).
fn detect_accessor(body: &[&str], name: &str) -> Option<String> {
    let (prefix, stem) = match () {
        _ if name.starts_with("get") => ("get", &name["get".len()..]),
        _ if name.starts_with("is") => ("is", &name["is".len()..]),
        _ if name.starts_with("set") => ("set", &name["set".len()..]),
        _ => return None,
    };
    if stem.is_empty() {
        return None;
    }
    let mut seen = 0usize;
    for raw in body {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("//") || l.starts_with('*') || l.starts_with("/*") {
            continue;
        }
        seen += 1;
        if seen > 8 {
            return None;
        }
        // stop at the next member declaration
        if l.starts_with("public ") || l.starts_with("private ") || l.starts_with("protected ")
            || l == "}"
        {
            return None;
        }
        if prefix == "set" {
            if let Some(rest) = l.strip_prefix("this.") {
                if let Some((field, _)) = rest.split_once('=') {
                    let f = field.trim();
                    if capitalize(f) == stem {
                        return Some(f.to_string());
                    }
                }
            }
        } else {
            if let Some(ret) = l.strip_prefix("return ") {
                let target = ret.trim_end_matches(';').trim();
                let target = target.strip_prefix("this.").unwrap_or(target);
                let matches = target == stem.to_lowercase()
                    || (target.len() == stem.len()
                        && target
                            .chars()
                            .zip(stem.chars())
                            .all(|(a, b)| a.eq_ignore_ascii_case(&b)));
                if matches {
                    return Some(target.to_string());
                }
            }
        }
    }
    None
}

fn scan_method_refs(line: &str, line_no: u32, uri: &str, model: &mut FileModel) {
    for caps in RE_METHOD_REF.captures_iter(line) {
        let m = caps.get(1).unwrap();
        model.method_refs.push(MethodRef {
            name: m.as_str().to_string(),
            loc: Loc::new(uri, line_no, col(line, m.start())),
        });
    }
}

fn assign_owners(model: &mut FileModel) {
    for cs in &mut model.calls {
        let mut best: Option<usize> = None;
        for (idx, m) in model.methods.iter().enumerate() {
            if m.loc.line <= cs.loc.line {
                match best {
                    Some(b) if model.methods[b].loc.line >= m.loc.line => {}
                    _ => best = Some(idx),
                }
            }
        }
        cs.owner_method = best;
    }
}

pub fn relevant_java(path: &str, file_name: &str, content: &str) -> bool {
    let stem = file_name.trim_end_matches(".java");
    for suf in [
        "Controller",
        "Service",
        "ServiceImpl",
        "Impl",
        "Mapper",
        "Dao",
        "Repository",
        "Client",
        "Feign",
    ] {
        if stem.ends_with(suf) {
            return true;
        }
    }
    for a in [
        "@RestController",
        "@Controller",
        "@Service",
        "@Mapper",
        "@Repository",
        "@TableName",
        "@Entity",
        "@Data",
    ] {
        if content.contains(a) {
            return true;
        }
    }
    let p = path.replace('\\', "/");
    for seg in [
        "/domain/",
        "/entity/",
        "/entities/",
        "/model/",
        "/pojo/",
        "/dto/",
        "/vo/",
        "/bean/",
    ] {
        if p.contains(seg) {
            return true;
        }
    }
    false
}

pub fn parse_java(c: &Candidate) -> Option<FileModel> {
    let file_name = Path::new(&c.path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !c.force && !relevant_java(&c.path, &file_name, &c.content) {
        return None;
    }

    let lines: Vec<&str> = c.content.lines().collect();
    let mut model = FileModel::new(
        c.uri.clone(),
        c.path.clone(),
        FileKind::Java,
        c.project_root.clone(),
    );
    let mut pending: Vec<Annotation> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let raw_line = lines[i].trim_end_matches('\r');
        // Strip any leading annotation(s) even when a declaration follows on the
        // SAME line. Blank them with equal-length spaces so all byte offsets
        // (and therefore VS Code columns) stay identical to the original line.
        let mut owned_line = raw_line.to_string();
        loop {
            let trimmed = owned_line.trim_start();
            if !trimmed.starts_with('@') {
                break;
            }
            let leading = owned_line.len() - trimmed.len();
            match annotation_prefix_len(trimmed) {
                Some(n) => {
                    if let Some(a) = parse_annotation(&trimmed[..n]) {
                        pending.push(a);
                    }
                    let end = leading + n;
                    let blanks = " ".repeat(end);
                    owned_line = format!("{}{}", blanks, &owned_line[end..]);
                    if owned_line.trim().is_empty() {
                        break;
                    }
                }
                None => break,
            }
        }
        let line = owned_line.as_str();
        let t = line.trim();

        if t.is_empty()
            || t.starts_with("//")
            || t.starts_with('*')
            || t.starts_with("/*")
            || t.starts_with("*/")
        {
            i += 1;
            continue;
        }
        if let Some(caps) = RE_PACKAGE.captures(line) {
            model.package = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            i += 1;
            continue;
        }
        if t.starts_with("import ") {
            if let Some(caps) = RE_IMPORT.captures(line) {
                if let Some(fqcn) = caps.get(1) {
                    model.imports.push(fqcn.as_str().to_string());
                }
            }
            i += 1;
            continue;
        }

        if let Some((name, is_iface, supers, name_byte)) = parse_class_decl(line) {
            if model.class_name.is_empty() {
                model.class_name = name;
                model.is_interface = is_iface;
                model.supers = supers;
                model.class_annotations = std::mem::take(&mut pending);
                model.class_paths = collect_paths(&model.class_annotations);
                model.class_loc = Loc::new(&c.uri, i as u32, col(line, name_byte));
            } else {
                pending.clear();
            }
            i += 1;
            continue;
        }

        if let Some((mut md, consumed)) =
            try_method_decl(line, &lines, i, &c.uri, &model.class_name)
        {
            md.paths = collect_paths(&pending);
            md.http_methods = collect_http_methods(&pending);
            md.accessor_field = detect_accessor(&lines[i + consumed..], &md.name);
            model.methods.push(md);
            pending.clear();
            // Calls / refs may appear on the declaration line itself.
            scan_calls(line, i as u32, &c.uri, &mut model);
            scan_method_refs(line, i as u32, &c.uri, &mut model);
            i += consumed;
            continue;
        }

        if let Some(mut f) = try_field_decl(line, i as u32, &c.uri) {
            f.annotations = pending.iter().map(|a| a.name.clone()).collect();
            model.fields.push(f);
            pending.clear();
            // The line may also carry calls / field references
            // (e.g. `String p = C.TOKEN_PREFIX;`).
            scan_calls(line, i as u32, &c.uri, &mut model);
            scan_bare_calls(line, i as u32, &c.uri, &mut model);
            scan_field_refs(line, i as u32, &c.uri, &mut model);
            scan_method_refs(line, i as u32, &c.uri, &mut model);
            i += 1;
            continue;
        }

        scan_calls(line, i as u32, &c.uri, &mut model);
        scan_bare_calls(line, i as u32, &c.uri, &mut model);
        scan_field_refs(line, i as u32, &c.uri, &mut model);
        scan_method_refs(line, i as u32, &c.uri, &mut model);
        i += 1;
    }

    if model.class_name.is_empty() {
        return None;
    }
    model.fqcn = if model.package.is_empty() {
        model.class_name.clone()
    } else {
        format!("{}.{}", model.package, model.class_name)
    };
    assign_owners(&mut model);
    Some(model)
}

fn byte_to_lc(content: &str, b: usize) -> (u32, u32) {
    let mut start = 0usize;
    let mut ln = 0u32;
    for line in content.split('\n') {
        let end = start + line.len();
        if b <= end + 1 {
            let rel = b.saturating_sub(start).min(line.len());
            return (ln, line[..rel].chars().count() as u32);
        }
        start = end + 1;
        ln += 1;
    }
    (ln, 0)
}

pub fn parse_xml(c: &Candidate) -> Option<FileModel> {
    if !c.content.contains("<mapper") {
        return None;
    }
    let mut model = FileModel::new(
        c.uri.clone(),
        c.path.clone(),
        FileKind::Xml,
        c.project_root.clone(),
    );

    if let Some(caps) = RE_MAPPER.captures(&c.content) {
        model.xml_namespace = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let start = caps.get(0).unwrap().start();
        let end = c.content[start..]
            .find('>')
            .map(|d| start + d + 1)
            .unwrap_or(c.content.len().min(start + caps.get(0).unwrap().len()));
        let (sl, sc) = byte_to_lc(&c.content, start);
        let (el, ec) = byte_to_lc(&c.content, end);
        model.namespace_loc = Some(Loc::new(&c.uri, sl, sc));
        model.namespace_span = Some((sc, if el == sl { ec } else { u32::MAX / 4 }));
    }

    let lines: Vec<&str> = c.content.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim_end_matches('\r');
        if let Some(caps) = RE_STMT_OPEN.captures(line) {
            let start_byte = caps.get(0).unwrap().start();
            let start_line = i;
            let mut buf = line.to_string();
            let mut j = i;
            while !buf.contains('>') && j + 1 < lines.len() {
                j += 1;
                buf.push(' ');
                buf.push_str(lines[j].trim_end_matches('\r'));
            }
            let tag_end = buf.find('>').unwrap_or(buf.len().saturating_sub(1));
            let tag = &buf[..tag_end.min(buf.len())];
            if let Some(idc) = RE_ID.captures(tag) {
                let id = idc.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                let start_col = col(line, start_byte);
                let end_col = if j == start_line {
                    col(line, (tag_end + 1).min(line.len()))
                } else {
                    line.chars().count() as u32
                };
                model.xml_stmts.push(XmlStmt {
                    namespace: model.xml_namespace.clone(),
                    id,
                    loc: Loc::new(&c.uri, start_line as u32, start_col),
                    span: (start_col, end_col.max(start_col + 1)),
                });
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }

    if model.xml_stmts.is_empty() && model.xml_namespace.is_empty() {
        return None;
    }
    Some(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(content: &str) -> Candidate {
        Candidate {
            path: "/p/src/main/java/x/Foo.java".to_string(),
            uri: "file:///p/src/main/java/x/Foo.java".to_string(),
            content: content.to_string(),
            is_xml: false,
            project_root: "/p".to_string(),
            force: false,
        }
    }

    #[test]
    fn strip_simple_types() {
        assert_eq!(strip_to_simple("List<String>"), "List");
        assert_eq!(strip_to_simple("String[]"), "String");
        assert_eq!(strip_to_simple("java.lang.Long"), "Long");
        assert_eq!(strip_to_simple("Map<String, Object>"), "Map");
    }

    #[test]
    fn annotation_prefix_len_balanced() {
        assert!(annotation_prefix_len("@RestController rest").is_some());
        assert!(annotation_prefix_len("@GetMapping(\"/x\") public void a() {}").is_some());
        assert!(annotation_prefix_len("@RequestMapping(value = {\"/\", \"/{id}\"}) public void a(){}").is_some());
        assert_eq!(annotation_prefix_len("public class X"), None);
    }

    #[test]
    fn single_line_annotation_with_class_is_parsed() {
        let m = parse_java(&cand(
            "package x;\n@RestController public class OneLineController {\n    @GetMapping(\"/one\")\n    public void one() {}\n}\n",
        ))
        .expect("should parse");
        assert_eq!(m.class_name, "OneLineController");
        assert_eq!(m.methods.len(), 1);
        assert_eq!(m.methods[0].paths, vec!["/one"]);
        // Column must be correct on the masked line (name follows the space).
        assert!(m.class_loc.character > 0);
    }

    #[test]
    fn crlf_and_inline_comment_do_not_break_offsets() {
        let c = cand("package x;\r\n@RestController\r\npublic class C {\r\n    public void go() {}\r\n}\r\n");
        let m = parse_java(&c).unwrap();
        assert_eq!(m.class_name, "C");
        assert_eq!(m.methods.len(), 1);
    }

    #[test]
    fn lombok_field_metadata() {
        let m = parse_java(&cand(
            "package x;\n@Data\npublic class D {\n    private static final long serialVersionUID = 1L;\n    private final String code = \"x\";\n    private String name;\n}\n",
        ))
        .unwrap();
        assert_eq!(m.fields.len(), 3);
        assert!(m.fields.iter().any(|f| f.is_static && f.is_final));
        assert!(m.fields.iter().any(|f| f.is_final && !f.is_static));
    }
}
