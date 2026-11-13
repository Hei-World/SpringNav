use std::fs;
use std::path::{Path, PathBuf};

pub const SKIP_DIRS: &[&str] = &[
    "target",
    "build",
    "out",
    "bin",
    "dist",
    ".git",
    ".idea",
    ".gradle",
    ".mvn",
    ".settings",
    ".vscode-test",
    "node_modules",
];

const MARKERS: &[&str] = &["pom.xml", "build.gradle", "build.gradle.kts"];

/// F-01: a workspace counts as a Java project only if a build marker exists.
/// `workspaceContains:**/pom.xml` may match a nested module, so look a few
/// levels deep (bounded, cheap).
pub fn is_java_project(root: &str) -> bool {
    fn walk(dir: &Path, depth: u32) -> bool {
        if depth > 3 {
            return false;
        }
        for m in MARKERS {
            if dir.join(m).is_file() {
                return true;
            }
        }
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let n = e.file_name().to_string_lossy().into_owned();
                if SKIP_DIRS.contains(&n.as_str()) {
                    continue;
                }
                if walk(&p, depth + 1) {
                    return true;
                }
            }
        }
        false
    }
    walk(Path::new(root), 0)
}

pub struct Candidate {
    pub path: String,
    pub uri: String,
    pub content: String,
    pub is_xml: bool,
}

/// Enumerate candidate source files ourselves (no workspace.findFiles).
pub fn collect(root: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = vec![(PathBuf::from(root), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 24 {
            continue;
        }
        let rd = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let is_xml = name.ends_with(".xml");
            let is_java = name.ends_with(".java");
            if !is_xml && !is_java {
                continue;
            }
            let meta = match fs::metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.len() > 1024 * 1024 {
                continue;
            }
            let bytes = match fs::read(&p) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let path_str = p.to_string_lossy().into_owned();
            out.push(Candidate {
                uri: path_to_uri(&path_str),
                path: path_str,
                content: String::from_utf8_lossy(&bytes).into_owned(),
                is_xml,
            });
        }
    }
    out
}

/// Collect across multiple workspace folders: drop nested roots, index each
/// Java project once, dedupe files by path. A multi-module project is scanned
/// from its top root once (sub-module pom.xml files are not separate projects).
pub fn collect_multi(roots_in: &[String]) -> (Vec<Candidate>, Vec<String>, Vec<String>) {
    let mut roots: Vec<String> = Vec::new();
    for r in roots_in {
        let nr = normalize_path(r);
        if nr.is_empty() {
            continue;
        }
        if roots.iter().any(|x| is_prefix(x, &nr)) {
            continue; // already covered by a parent root
        }
        roots.retain(|x| !is_prefix(&nr, x)); // drop roots nested under this one
        roots.push(nr);
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut projects = Vec::new();
    for r in &roots {
        if !is_java_project(r) {
            continue;
        }
        projects.push(r.clone());
        for c in collect(r) {
            if seen.insert(c.path.clone()) {
                out.push(c);
            }
        }
    }
    (out, projects, roots)
}

pub fn normalize_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

fn is_prefix(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    let mut p = parent.to_string();
    if !p.ends_with('/') {
        p.push('/');
    }
    child.starts_with(&p)
}

pub fn read_one(path: &str) -> String {
    String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned()
}

/// Match VS Code's `Uri.file(path).toString()` for ordinary paths: keep
/// unreserved characters plus '/', percent-encode everything else (UTF-8).
pub fn path_to_uri(p: &str) -> String {
    let mut out = String::with_capacity(p.len() + 8);
    out.push_str("file://");
    for &b in p.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/' | b':') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
            out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> String {
    let s = uri.strip_prefix("file://").unwrap_or(uri);
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = hex(bytes[i + 1]);
            let l = hex(bytes[i + 2]);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
