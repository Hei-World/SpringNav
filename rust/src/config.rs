use crate::model::Role;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Rules loaded from `.idea/spring-nav.toml` (or `.idea/spring-nav.json`).
/// Priority: aliases > type-roles (role override) > default naming conventions.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// field name / type name -> target type (simple or FQCN)
    pub aliases: HashMap<String, String>,
    /// class simple name / FQCN -> role string
    pub type_roles: HashMap<String, String>,
    /// interface role -> implementation filename glob patterns
    pub impl_patterns: HashMap<String, Vec<String>>,
    /// Per-project overrides, keyed by project root. These take precedence over
    /// the merged/global maps so a config in one project never leaks into
    /// another (multi-root workspaces).
    pub project_aliases: HashMap<String, HashMap<String, String>>,
    pub project_roles: HashMap<String, HashMap<String, String>>,
    pub project_impl_patterns: HashMap<String, HashMap<String, Vec<String>>>,
}

pub fn parse_role(s: &str) -> Option<Role> {
    match s.to_ascii_lowercase().replace('-', "_").as_str() {
        "controller" => Some(Role::Controller),
        "service" => Some(Role::Service),
        "service_impl" | "serviceimpl" => Some(Role::ServiceImpl),
        "mapper" => Some(Role::Mapper),
        "entity" => Some(Role::Entity),
        "other" => Some(Role::Other),
        _ => None,
    }
}

impl Config {
    pub fn load(root: &str) -> Config {
        let dir = Path::new(root).join(".idea");
        for name in ["spring-nav.toml", "spring-nav.json"] {
            let p = dir.join(name);
            if !p.is_file() {
                continue;
            }
            let text = fs::read_to_string(&p).unwrap_or_default();
            return if name.ends_with(".toml") {
                parse_toml(&text)
            } else {
                parse_json(&text)
            };
        }
        Config::default()
    }

    /// Load config from every ACTUAL project root. Rules stay strictly scoped
    /// to their project, so one project's config never affects another.
    pub fn load_multi(roots: &[String]) -> Config {
        let mut out = Config::default();
        for r in roots {
            let c = Config::load(r);
            out.project_aliases.insert(r.clone(), c.aliases);
            out.project_roles.insert(r.clone(), c.type_roles);
            out.project_impl_patterns.insert(r.clone(), c.impl_patterns);
        }
        out
    }

    /// Strictly scoped lookup (no cross-project leakage).
    pub fn alias_for(&self, project: &str, key: &str) -> Option<&str> {
        self.project_aliases
            .get(project)
            .and_then(|m| m.get(key))
            .map(|s| s.as_str())
    }

    pub fn role_override_for(&self, project: &str, simple: &str, fqcn: &str) -> Option<Role> {
        self.project_roles
            .get(project)
            .and_then(|m| m.get(fqcn).or_else(|| m.get(simple)))
            .and_then(|v| parse_role(v))
    }

    pub fn impl_patterns_for_project(&self, project: &str, role: &str) -> Option<&Vec<String>> {
        self.project_impl_patterns
            .get(project)
            .and_then(|m| m.get(role))
    }
}

fn parse_toml(text: &str) -> Config {
    let v: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(_) => return Config::default(),
    };
    let mut c = Config::default();
    if let Some(t) = v.get("aliases").and_then(|x| x.as_table()) {
        for (k, val) in t {
            if let Some(s) = val.as_str() {
                c.aliases.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(t) = v.get("type-roles").and_then(|x| x.as_table()) {
        for (k, val) in t {
            if let Some(s) = val.as_str() {
                c.type_roles.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(t) = v.get("impl-patterns").and_then(|x| x.as_table()) {
        for (k, val) in t {
            if let Some(arr) = val.as_array() {
                let list: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
                c.impl_patterns.insert(k.clone(), list);
            }
        }
    }
    c
}

fn parse_json(text: &str) -> Config {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Config::default(),
    };
    let mut c = Config::default();
    if let Some(o) = v.get("aliases").and_then(|x| x.as_object()) {
        for (k, val) in o {
            if let Some(s) = val.as_str() {
                c.aliases.insert(k.clone(), s.to_string());
            }
        }
    }
    let tr = v.get("type-roles").or_else(|| v.get("typeRoles"));
    if let Some(o) = tr.and_then(|x| x.as_object()) {
        for (k, val) in o {
            if let Some(s) = val.as_str() {
                c.type_roles.insert(k.clone(), s.to_string());
            }
        }
    }
    let ip = v.get("impl-patterns").or_else(|| v.get("implPatterns"));
    if let Some(o) = ip.and_then(|x| x.as_object()) {
        for (k, val) in o {
            if let Some(arr) = val.as_array() {
                let list: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
                c.impl_patterns.insert(k.clone(), list);
            }
        }
    }
    c
}

/// Minimal glob matcher supporting `*` and `?`.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn glob_matches() {
        assert!(glob_match("*ServiceImpl", "SysUserServiceImpl"));
        assert!(glob_match("ISys*Service", "ISysUserService"));
        assert!(!glob_match("*ServiceImpl", "SysUserMapper"));
    }

    #[test]
    fn role_parsing() {
        assert_eq!(parse_role("controller"), Some(Role::Controller));
        assert_eq!(parse_role("service_impl"), Some(Role::ServiceImpl));
        assert_eq!(parse_role("nope"), None);
    }

    #[test]
    fn project_scoped_config_is_isolated() {
        let base = std::env::temp_dir().join(format!("sn-cfg-test-{}", std::process::id()));
        let a = base.join("A");
        let b = base.join("B");
        fs::create_dir_all(a.join(".idea")).unwrap();
        fs::create_dir_all(b.join(".idea")).unwrap();
        fs::write(a.join(".idea/spring-nav.toml"), "[aliases]\n\"svc\" = \"AService\"\n").unwrap();
        fs::write(b.join(".idea/spring-nav.toml"), "[aliases]\n\"svc\" = \"BService\"\n").unwrap();
        let cfg = Config::load_multi(&[a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned()]);
        assert_eq!(cfg.alias_for(&a.to_string_lossy(), "svc"), Some("AService"));
        assert_eq!(cfg.alias_for(&b.to_string_lossy(), "svc"), Some("BService"));
        assert_eq!(cfg.alias_for("/some/other", "svc"), None);
        let _: PathBuf = base;
        fs::remove_dir_all(base).ok();
    }
}
