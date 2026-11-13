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

    pub fn role_override(&self, simple: &str, fqcn: &str) -> Option<Role> {
        self.type_roles
            .get(fqcn)
            .or_else(|| self.type_roles.get(simple))
            .and_then(|v| parse_role(v))
    }

    pub fn alias(&self, key: &str) -> Option<&str> {
        self.aliases.get(key).map(|s| s.as_str())
    }

    pub fn impl_patterns_for(&self, role: &str) -> Option<&Vec<String>> {
        self.impl_patterns.get(role)
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
