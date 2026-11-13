// Compact metadata-only model. No file contents, no AST, no source text are kept.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum Role {
    Controller,
    Service,
    ServiceImpl,
    Mapper,
    Entity,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum FileKind {
    #[default]
    Java,
    Xml,
}

/// A canonical symbol position. `line` and `character` are 0-based, with
/// `character` measured in UTF-16 code units (VS Code semantics). For code made
/// of identifiers this equals the count of Rust `char`s up to that byte offset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loc {
    pub uri: String,
    pub line: u32,
    pub character: u32,
}

impl Loc {
    pub fn new(uri: &str, line: u32, character: u32) -> Self {
        Loc {
            uri: uri.to_string(),
            line,
            character,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Annotation {
    pub name: String,
    pub paths: Vec<String>,
    /// Raw argument text, used to read `method = RequestMethod.GET`.
    pub raw: String,
}

#[derive(Clone, Debug, Default)]
pub struct MethodDecl {
    pub name: String,
    pub param_types: Vec<String>,
    pub loc: Loc,
    /// Mapping paths declared directly on the method (@GetMapping etc.).
    pub paths: Vec<String>,
    /// HTTP verbs inferred from mapping annotations (GET/POST/...).
    pub http_methods: Vec<String>,
    /// For hand-written accessors: the field this get/set/is method reads or
    /// writes (trivial body detection). None for everything else.
    pub accessor_field: Option<String>,
}

/// One entry of the interface-path index, carrying display metadata.
#[derive(Clone, Debug, Default)]
pub struct PathEntry {
    pub path: String,
    pub file: String,
    pub method: String,
    pub module: String,
    pub loc: Loc,
    /// Index of the owning file in Index.files (for incremental removal).
    pub owner: usize,
}

#[derive(Clone, Debug, Default)]
pub struct FieldDecl {
    pub name: String,
    pub type_simple: String,
    pub loc: Loc,
    pub is_static: bool,
    pub is_final: bool,
    /// Field-level annotation simple names (e.g. @Getter / @Setter).
    pub annotations: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CallSite {
    pub field: String,
    pub method: String,
    pub arg_count: usize,
    /// Position of the invoked method name.
    pub loc: Loc,
    /// Position of the receiver expression (field / variable).
    pub field_loc: Loc,
    /// Index into FileModel.methods of the enclosing method.
    pub owner_method: Option<usize>,
    /// Receiver kind: 0 = explicit `x.y(` (field or class, resolved at index
    /// time), 1 = `this.y(`, 2 = bare `y(` (same class / inherited).
    pub kind: u8,
}

/// A field/constant reference: `Class.CONST` (class = simple name) or
/// `this.field` (class empty). Bare identifiers are not captured (P2).
#[derive(Clone, Debug, Default)]
pub struct FieldRef {
    /// Simple class name; empty = current class (`this.field`).
    pub class: String,
    pub name: String,
    /// Position of the referenced identifier.
    pub loc: Loc,
}

#[derive(Clone, Debug, Default)]
pub struct MethodRef {
    pub name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug, Default)]
pub struct XmlStmt {
    pub namespace: String,
    pub id: String,
    pub loc: Loc,
    /// Span (start,end) of the opening tag on `loc.line`.
    pub span: (u32, u32),
}

#[derive(Clone, Debug, Default)]
pub struct FileModel {
    pub uri: String,
    pub path: String,
    pub kind: FileKind,
    /// Project root this file belongs to (drives per-project config).
    pub project_root: String,
    // Java class metadata
    pub package: String,
    pub class_name: String,
    pub fqcn: String,
    pub is_interface: bool,
    pub class_loc: Loc,
    pub class_annotations: Vec<Annotation>,
    pub class_paths: Vec<String>,
    pub supers: Vec<String>,
    pub methods: Vec<MethodDecl>,
    pub fields: Vec<FieldDecl>,
    pub calls: Vec<CallSite>,
    pub method_refs: Vec<MethodRef>,
    /// Imported fully-qualified class names (drives static-call resolution).
    pub imports: Vec<String>,
    /// Field/constant references (`Class.CONST`, `this.field`).
    pub field_refs: Vec<FieldRef>,
    // Role is assigned while building the index (config may override it).
    pub role: Role,
    // XML mapper metadata
    pub xml_namespace: String,
    pub xml_stmts: Vec<XmlStmt>,
    pub namespace_loc: Option<Loc>,
    pub namespace_span: Option<(u32, u32)>,
}



impl FileModel {
    pub fn new(uri: String, path: String, kind: FileKind, project_root: String) -> Self {
        FileModel {
            uri,
            path,
            kind,
            project_root,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub loc: Loc,
}

/// A per-file edge contribution: kind 0 = undirected(a,b),
/// 1 = directed(a=upper, b=lower), 2 = alias(a=from, b=to).
#[derive(Clone, Debug)]
pub struct Triple {
    pub a: Loc,
    pub b: Loc,
    pub kind: u8,
}
