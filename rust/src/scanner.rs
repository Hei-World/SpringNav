use std::collections::HashSet;
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

const MARKERS: &[&str] = &["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"];
/// Maximum depth when discovering project roots inside a workspace folder.
const DISCOVER_DEPTH: u32 = 8;
/// Maximum depth when enumerating source files.
const SCAN_DEPTH: u32 = 24;
/// Files larger than this are skipped (with a counter, surfaced in the UI).
const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub struct Candidate {
    pub path: String,
    pub uri: String,
    pub content: String,
    pub is_xml: bool,
    pub project_root: String,
    /// Parse regardless of the relevance filter (import-closure files,
    /// files already indexed).
    pub force: bool,
}

pub struct ScanResult {
    pub candidates: Vec<Candidate>,
    /// Actual Java project roots discovered (a multi-module project = 1).
    pub project_roots: Vec<String>,
    /// Deduped workspace folders (used for module naming).
    pub workspace_roots: Vec<String>,
    /// Number of files skipped because they exceeded MAX_FILE_BYTES.
    pub skipped_large: u32,
}

fn has_marker(dir: &Path) -> bool {
    MARKERS.iter().any(|m| dir.join(m).is_file())
}

/// Discover Java project roots inside a folder, top-down. When a directory has
/// a build marker it is recorded as ONE project and its subdirectories are not
/// searched for further project roots (so sub-modules of a multi-module project
/// are not mistaken for independent projects). Files are still scanned from the
/// whole folder tree afterwards.
pub fn discover_projects(root: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = vec![(PathBuf::from(root), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > DISCOVER_DEPTH {
            continue;
        }
        if has_marker(&dir) {
            found.push(dir.to_string_lossy().into_owned());
            continue; // prune: sub-modules belong to this project
        }
        let rd = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let n = e.file_name().to_string_lossy().into_owned();
            if SKIP_DIRS.contains(&n.as_str()) {
                continue;
            }
            stack.push((p, depth + 1));
        }
    }
    found.sort();
    found
}

/// F-01: a workspace counts as a Java project if any project root exists
/// inside it (深度可到 DISCOVER_DEPTH，monorepo 深层项目也能识别).
pub fn is_java_project(root: &str) -> bool {
    !discover_projects(root).is_empty()
}

/// Enumerate candidate source files for one project root.
pub fn collect_with_stats(root: &str, project_root: &str) -> (Vec<Candidate>, u32) {
    let mut out = Vec::new();
    let mut skipped = 0u32;
    let mut stack: Vec<(PathBuf, u32)> = vec![(PathBuf::from(root), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > SCAN_DEPTH {
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
            if meta.len() > MAX_FILE_BYTES {
                skipped += 1;
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
                project_root: project_root.to_string(),
                force: false,
            });
        }
    }
    (out, skipped)
}

/// Collect files whose class name (file stem) is in `names` — the import
/// closure: utility/constant classes are not suffix-relevant but must be
/// indexed so static calls and constant references can jump. Files whose
/// stem is in `known` are skipped (already collected).
pub fn collect_by_names(
    roots: &[String],
    names: &HashSet<String>,
    known: &HashSet<String>,
) -> (Vec<Candidate>, u32) {
    let mut out = Vec::new();
    let mut skipped = 0u32;
    for root in roots {
        for pr in discover_projects(root) {
            let mut stack: Vec<(PathBuf, u32)> = vec![(PathBuf::from(&pr), 0)];
            while let Some((dir, depth)) = stack.pop() {
                if depth > SCAN_DEPTH {
                    continue;
                }
                let Ok(rd) = fs::read_dir(&dir) else {
                    continue;
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
                    if !name.ends_with(".java") {
                        continue;
                    }
                    let stem = name.trim_end_matches(".java").to_string();
                    if !names.contains(&stem) || known.contains(&stem) {
                        continue;
                    }
                    let path_str = p.to_string_lossy().into_owned();
                    let Ok(bytes) = fs::read(&path_str) else {
                        continue;
                    };
                    if bytes.len() as u64 > MAX_FILE_BYTES {
                        skipped += 1;
                        continue;
                    }
                    out.push(Candidate {
                        uri: path_to_uri(&path_str),
                        path: path_str.clone(),
                        content: String::from_utf8_lossy(&bytes).into_owned(),
                        is_xml: false,
                        project_root: pr.clone(),
                        force: true,
                    });
                }
            }
        }
    }
    (out, skipped)
}

/// Scan all workspace folders: dedupe nested roots, discover project roots,
/// scan each project once, dedupe files by path.
pub fn scan_workspace(roots_in: &[String]) -> ScanResult {
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

    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates = Vec::new();
    let mut project_roots = Vec::new();
    let mut skipped_large = 0u32;
    for r in &roots {
        for pr in discover_projects(r) {
            let (cs, skipped) = collect_with_stats(&pr, &pr);
            skipped_large += skipped;
            for c in cs {
                if seen.insert(c.path.clone()) {
                    candidates.push(c);
                }
            }
            project_roots.push(pr);
        }
    }
    ScanResult {
        candidates,
        project_roots,
        workspace_roots: roots,
        skipped_large,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_roundtrip() {
        for p in [
            "/tmp/simple/A.java",
            "/tmp/dir with space/My Controller.java",
            "/tmp/目录/文件.java",
        ] {
            assert_eq!(uri_to_path(&path_to_uri(p)), p);
        }
    }

    #[test]
    fn discovers_deep_and_prunes_modules() {
        let base = std::env::temp_dir().join(format!("sn-discover-{}", std::process::id()));
        let deep = base.join("l1/l2/l3/l4/proj");
        fs::create_dir_all(deep.join("src/main/java")).unwrap();
        fs::write(deep.join("pom.xml"), "<project/>").unwrap();
        fs::write(deep.join("src/main/java/A.java"), "class A {}").unwrap();
        // A multi-module project: prune at the outer marker, sub-modules not listed separately.
        let multi = base.join("multi");
        fs::create_dir_all(multi.join("mod-a")).unwrap();
        fs::write(multi.join("pom.xml"), "<project/>").unwrap();
        fs::write(multi.join("mod-a/pom.xml"), "<project/>").unwrap();
        let found = discover_projects(&multi.to_string_lossy());
        assert_eq!(found.len(), 1, "multi-module should be 1 project");
        let found_deep = discover_projects(&base.to_string_lossy());
        assert!(found_deep.iter().any(|p| p.ends_with("proj")), "deep project must be found");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn skips_large_files_with_counter() {
        let base = std::env::temp_dir().join(format!("sn-skip-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let big = base.join("Big.java");
        fs::write(&big, vec![b'x'; 1024 * 1024 + 10]).unwrap();
        let (cs, skipped) = collect_with_stats(&base.to_string_lossy(), &base.to_string_lossy());
        assert_eq!(cs.len(), 0);
        assert_eq!(skipped, 1);
        fs::remove_dir_all(base).ok();
    }
}
