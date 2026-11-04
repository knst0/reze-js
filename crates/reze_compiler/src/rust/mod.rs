//! The Rust render target: `.reze/main.rs`, the crate root that includes the `.rs` file of each
//! route, with one render function per route level, the route table, and `app()`.

mod ir;
mod lower;
mod nodes;
pub mod source;
pub mod typescript;

pub use ir::{CrateIr, FieldDef, TypeDef, TypeRef, VariantDef};
use source::{CRATE, Context, Project};

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use oxc_ast::ast::{BindingPattern, Declaration, Expression, Statement};
use oxc_span::Span;

use crate::analyze::{Analysis, EndpointKind, Export, ImportSource};
use crate::client::{Plan, RouteChunk};
use crate::diagnostic::{Code, Report};
use crate::project::Module;
use crate::routes::{RouteTable, Segment};
use crate::view::{ComponentRef, Content};

/// A value's type as the Rust render sees it.
#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Str,
    /// A Rust number type: `f64`, `u32`, `usize`.
    Num(String),
    Bool,
    Null,
    /// `crate::Name`.
    Struct(String),
    Enum(String),
    Opt(Box<Ty>),
    List(Box<Ty>),
    Tuple(Vec<Ty>),
    /// `SystemTime`.
    Date,
    /// `BTreeSet<T>` or `HashSet<T>`.
    Set(Box<Ty>),
    /// `BTreeMap<String, T>` or `HashMap<String, T>`.
    Map(Box<Ty>),
}

impl Ty {
    fn is_float(&self) -> bool {
        matches!(self, Ty::Num(rust) if rust.starts_with('f'))
    }

    /// `u64` and `i64`, which reach JS as decimal strings.
    fn is_big(&self) -> bool {
        matches!(self, Ty::Num(rust) if rust == "u64" || rust == "i64")
    }
}

/// A Rust expression of a value's view type: `&str`, a number, `bool`, `&T` for a struct, enum,
/// or list, and `Option` of those. An owned string is a `String`.
#[derive(Clone, Debug)]
struct Val {
    code: String,
    ty: Ty,
    owned: bool,
}

impl Val {
    fn new(code: impl Into<String>, ty: Ty) -> Self {
        Self { code: code.into(), ty, owned: false }
    }

    fn owned(code: impl Into<String>) -> Self {
        Self { code: code.into(), ty: Ty::Str, owned: true }
    }

    fn str(&self) -> String {
        if self.owned { format!("({}).as_str()", self.code) } else { self.code.clone() }
    }
}

#[derive(Clone)]
enum Binding<'e, 'a> {
    Val(Val),
    /// A component-level value, lowered where it is read.
    Lazy(&'a Expression<'a>, Rc<Frame<'e, 'a>>),
    /// An object whose fields are known, in insertion order.
    Props(Rc<Vec<(String, Binding<'e, 'a>)>>),
    /// JSX passed as a value, rendered in the frame that wrote it.
    Content(&'e Content<'a>, Rc<Frame<'e, 'a>>),
    /// The `children` of a route level: the next level.
    Children,
    /// The params of the route node.
    Params(usize),
    /// `useSearchParams()`.
    Search,
    /// A `RouteError` in the named variable.
    Error(String),
    Null,
    /// A value the server render cannot read, with the reason.
    Invalid(String),
}

/// One lexical layer of names: a component's props, one local, or a callback's parameters.
struct Frame<'e, 'a> {
    module: usize,
    names: HashMap<String, Binding<'e, 'a>>,
    /// The Rust variables of client scopes, by plan scope.
    scopes: HashMap<usize, String>,
    parent: Option<Rc<Frame<'e, 'a>>>,
}

impl<'e, 'a> Frame<'e, 'a> {
    fn root(module: usize) -> Rc<Self> {
        Rc::new(Frame { module, names: HashMap::new(), scopes: HashMap::new(), parent: None })
    }

    fn child(
        parent: &Rc<Self>,
        names: HashMap<String, Binding<'e, 'a>>,
        scopes: HashMap<usize, String>,
    ) -> Rc<Self> {
        Rc::new(Frame { module: parent.module, names, scopes, parent: Some(parent.clone()) })
    }

    fn lookup(&self, name: &str) -> Option<Binding<'e, 'a>> {
        match self.names.get(name) {
            Some(binding) => Some(binding.clone()),
            None => self.parent.as_ref()?.lookup(name),
        }
    }

    fn scope(&self, scope: usize) -> Option<String> {
        match self.scopes.get(&scope) {
            Some(var) => Some(var.clone()),
            None => self.parent.as_ref()?.scope(scope),
        }
    }
}

/// The type of `useSubmission().issues` fields, which reads as a struct with a `message`.
const ISSUE: &str = "reze::Issue";

type Lowered<'e, 'a> = Result<Binding<'e, 'a>, String>;

/// Rust source of `text` as a string literal.
fn literal(text: &str) -> String {
    format!("{text:?}")
}

fn float(value: f64) -> String {
    format!("{value:?}_f64")
}

fn rust_ident(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("r_{out}")
    } else {
        out
    }
}

const KEYWORDS: [&str; 16] = [
    "as", "build", "crate", "fn", "impl", "in", "let", "loop", "match", "mod", "move", "new",
    "ref", "self", "super", "type",
];

/// The name of each route node in the generated code, from its file: `posts/[id].tsx` is
/// `posts_id`, and the root layout is `root`.
pub fn node_names(routes: &RouteTable, dir: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    routes
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let relative = node.file.strip_prefix(dir).unwrap_or(&node.file);
            let relative = relative.trim_start_matches('/');
            let stem = relative.rsplit_once('.').map_or(relative, |(stem, _)| stem);
            let stem = stem.strip_suffix("/index").unwrap_or(stem);
            let mut name =
                if routes.root == Some(index) { "root".to_string() } else { rust_ident(stem) };
            if KEYWORDS.contains(&name.as_str()) {
                name.push('_');
            }
            if !seen.insert(name.clone()) {
                name = format!("{name}_{index}");
            }
            name
        })
        .collect()
}

struct Emitter<'e, 'a> {
    modules: &'e [Module<'a>],
    analysis: &'e Analysis<'a>,
    plan: &'e Plan,
    routes: &'e RouteTable,
    types: HashMap<String, &'e TypeDef>,
    paths: &'e HashMap<String, String>,
    code: String,
    pending: String,
    /// The `Html` variable markup goes to.
    out: String,
    indent: usize,
    counter: usize,
    /// Labels of the enclosing `<Errored>` and `<Loading>` blocks, which an error breaks out of.
    labels: Vec<String>,
    /// Components being inlined, which a recursive call would repeat forever.
    inlining: Vec<ComponentRef>,
    /// The scope variable and site of the call that links the next inlined component.
    owner: Option<(String, usize)>,
    /// Lazy values being lowered, which a cycle would repeat forever.
    lowering: usize,
    reports: Vec<Vec<Report>>,
    /// What is reported, once for every place a component is inlined.
    reported: HashSet<(usize, Span, String)>,
}

impl<'e, 'a> Emitter<'e, 'a> {
    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn report(&mut self, module: usize, span: Span, reason: impl Into<String>) {
        let reason = reason.into();
        if self.reported.insert((module, span, reason.clone())) {
            self.reports[module]
                .push(Report::new(Code::ServerHoleUnsupported, span).arg("reason", reason));
        }
    }

    fn raw(&mut self, html: &str) {
        self.pending.push_str(html);
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let html = std::mem::take(&mut self.pending);
        let out = self.out.clone();
        self.line(&format!("{out}.raw({});", literal(&html)));
    }

    /// A statement, after the pending markup.
    fn stmt(&mut self, text: &str) {
        self.flush();
        self.line(text);
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.code.push_str("    ");
        }
        self.code.push_str(text);
        self.code.push('\n');
    }

    /// Opens a Rust block with `head`, after the pending markup.
    fn open(&mut self, head: &str) {
        self.flush();
        if head.is_empty() {
            self.line("{");
        } else {
            self.line(&format!("{head} {{"));
        }
        self.indent += 1;
    }

    fn close(&mut self) {
        self.flush();
        self.indent -= 1;
        self.line("}");
    }

    /// Closes a block and opens the next one: `} else {`.
    fn reopen(&mut self, head: &str) {
        self.flush();
        self.indent -= 1;
        self.line(&format!("}} {head} {{"));
        self.indent += 1;
    }

    /// A call that fails with a `RouteError`: it breaks out of the nearest `<Errored>` or
    /// `<Loading>`, or returns.
    fn fallible(&mut self, call: &str) {
        match self.labels.last().cloned() {
            Some(label) => {
                self.stmt(&format!("if let Err(e) = {call} {{ break '{label} Err(e); }}"))
            }
            None => self.stmt(&format!("{call}?;")),
        }
    }

    /// Stops with the error `error`: breaks out of the nearest `<Errored>` or `<Loading>`, or
    /// returns.
    fn throw(&mut self, error: &str) {
        match self.labels.last().cloned() {
            Some(label) => self.stmt(&format!("break '{label} Err({error});")),
            None => self.stmt(&format!("return Err({error});")),
        }
    }

    fn ty_of(&self, ty: &TypeRef, krate: &str) -> Ty {
        match ty {
            TypeRef::String => Ty::Str,
            TypeRef::Boolean => Ty::Bool,
            TypeRef::Number { rust } | TypeRef::Bigint { rust } => Ty::Num(rust.clone()),
            TypeRef::Option { of } => Ty::Opt(Box::new(self.ty_of(of, krate))),
            TypeRef::List { of } => Ty::List(Box::new(self.ty_of(of, krate))),
            TypeRef::Set { of } => Ty::Set(Box::new(self.ty_of(of, krate))),
            TypeRef::Map { value } => Ty::Map(Box::new(self.ty_of(value, krate))),
            TypeRef::Date => Ty::Date,
            TypeRef::Tuple { items } => {
                Ty::Tuple(items.iter().map(|item| self.ty_of(item, krate)).collect())
            }
            TypeRef::Defer { of } => self.ty_of(of, krate),
            TypeRef::Named { name } => self.named(&format!("{krate}::{name}")),
        }
    }

    fn rust_path(&self, key: &str) -> String {
        let name = key.strip_prefix("crate::").unwrap_or(key);
        match self.paths.get(name) {
            Some(module) => format!("crate::{module}::{name}"),
            None => key.to_string(),
        }
    }

    fn named(&self, path: &str) -> Ty {
        match self.types.get(path) {
            Some(TypeDef::Enum { .. }) => Ty::Enum(path.to_string()),
            _ => Ty::Struct(path.to_string()),
        }
    }

    /// The component a route module exports as default.
    fn default_export(&self, module: usize) -> Option<ComponentRef> {
        let mut module = module;
        let mut name = "default".to_string();
        for _ in 0..32 {
            let facts = &self.analysis.facts[module];
            match facts.exports.get(&name)? {
                Export::Component(index) => return Some(ComponentRef { module, index: *index }),
                Export::Reexport { module: target, name: next } => {
                    module = *target;
                    name = next.clone();
                }
                Export::Local(symbol) => {
                    if let Some(index) =
                        facts.components.iter().position(|c| c.symbol == Some(*symbol))
                    {
                        return Some(ComponentRef { module, index });
                    }
                    let import = facts.imports.get(symbol)?;
                    let ImportSource::Module(target) = import.source else { return None };
                    module = target;
                    name = import.name.clone();
                }
            }
        }
        None
    }

    /// The initializer of a module-level `const`.
    fn constant(&self, module: usize, name: &str) -> Option<&'a Expression<'a>> {
        let program = self.modules[module].program;
        program.body.iter().find_map(|statement| {
            let declaration = match statement {
                Statement::VariableDeclaration(declaration) => declaration,
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(declaration) => declaration,
                    _ => return None,
                },
                _ => return None,
            };
            if !declaration.kind.is_const() {
                return None;
            }
            declaration.declarations.iter().find_map(|declarator| match &declarator.id {
                BindingPattern::BindingIdentifier(id) if id.name == name => {
                    declarator.init.as_ref()
                }
                _ => None,
            })
        })
    }
}

/// The span of `export const <name>` in `module`, with its initializer.
pub(crate) fn exported_const<'a>(
    module: &Module<'a>,
    name: &str,
) -> Option<(Span, Option<&'a Expression<'a>>)> {
    module.program.body.iter().find_map(|statement| {
        let Statement::ExportDeclaration(export) = statement else { return None };
        let Declaration::VariableDeclaration(declaration) = &export.declaration else {
            return None;
        };
        declaration.declarations.iter().find_map(|declarator| match &declarator.id {
            BindingPattern::BindingIdentifier(id) if id.name == name => {
                Some((declarator.span, declarator.init.as_ref()))
            }
            _ => None,
        })
    })
}

fn segments_code(segments: &[Segment]) -> String {
    let items: Vec<String> = segments
        .iter()
        .map(|segment| {
            let (kind, name) = match segment {
                Segment::Static(name) => ("Static", name),
                Segment::Param(name) => ("Param", name),
                Segment::Optional(name) => ("Optional", name),
                Segment::Splat(name) => ("Splat", name),
            };
            format!("::reze_server::Segment::{kind}({})", literal(name))
        })
        .collect();
    format!("&[{}]", items.join(", "))
}

fn params_struct(node: &crate::routes::RouteNode) -> (String, String) {
    let mut fields = String::new();
    let mut build = String::new();
    for segment in &node.segments {
        let field = segment.name().map(rust_ident);
        match (segment, field) {
            (Segment::Param(param) | Segment::Splat(param), Some(field)) => {
                fields.push_str(&format!("            pub {field}: String,\n"));
                build.push_str(&format!(
                    "{field}: params.get({}).unwrap_or_default().to_string(), ",
                    literal(param)
                ));
            }
            (Segment::Optional(param), Some(field)) => {
                fields.push_str(&format!("            pub {field}: Option<String>,\n"));
                build.push_str(&format!(
                    "{field}: params.get({}).map(str::to_string), ",
                    literal(param)
                ));
            }
            _ => {}
        }
    }
    (fields, build)
}

fn up_to_root(path: &str) -> String {
    format!("../{path}")
}

pub fn emit(
    modules: &[Module<'_>],
    analysis: &Analysis<'_>,
    routes: &RouteTable,
    plan: &Plan,
    project: &Project,
    is_stub: bool,
) -> (String, Vec<Vec<Report>>) {
    let types: HashMap<String, &TypeDef> =
        project.ir.types.iter().map(|ty| (format!("{CRATE}::{}", ty.name()), ty)).collect();
    let mut emitter = Emitter {
        modules,
        analysis,
        plan,
        routes,
        types,
        paths: &project.paths,
        code: String::new(),
        pending: String::new(),
        out: "out".into(),
        indent: 0,
        counter: 0,
        labels: Vec::new(),
        inlining: Vec::new(),
        owner: None,
        lowering: 0,
        reports: modules.iter().map(|_| Vec::new()).collect(),
        reported: HashSet::new(),
    };
    let module_of = |file: &str| modules.iter().position(|m| m.path == file);

    let mut out = String::from("// Generated by reze-js from the route files. Do not edit.\n\n");
    if let Some(server) = &project.server {
        let dir = server.rsplit_once('/').map_or("", |(dir, _)| dir);
        out.push_str(&format!(
            "#[path = {}]\nmod __src {{\n    pub mod server;\n}}\n\nuse __src::server;\n\n",
            literal(up_to_root(dir).trim_end_matches('/'))
        ));
    }
    let context = match project.context {
        Context::Unit => "    pub type Context = ();\n",
        Context::Sync | Context::Async => "    pub use crate::server::Context;\n",
    };
    out.push_str(&format!(
        "#[rustfmt::skip]\nmod __reze {{\n    pub use ::reze::{{Data, Defer}};\n    pub use ::reze_server::http::request::Parts;\n    pub use ::reze_server::{{Result, RouteError}};\n{context}}}\n\n"
    ));

    let mut setup = String::new();
    if !project.routes.is_empty() {
        out.push_str("#[rustfmt::skip]\npub mod routes {\n");
    }
    for (index, route) in &project.routes {
        let node = &routes.nodes[*index];
        let (fields, build) = params_struct(node);
        let constructor = if route.load.is_some() || route.has_guard {
            let argument = if fields.is_empty() { "_" } else { "params" };
            format!(
                "\n        impl Params {{\n            pub(crate) fn from({argument}: &::reze_server::Params) -> Self {{\n                Self {{ {build}}}\n            }}\n        }}\n"
            )
        } else {
            String::new()
        };
        out.push_str(&format!(
            "    pub mod {name} {{\n        use crate::__reze::*;\n\n        #[allow(dead_code)]\n        #[derive(Clone, Debug)]\n        pub struct Params {{\n{fields}        }}\n{constructor}\n        include!({file});\n    }}\n",
            name = route.module,
            file = literal(&up_to_root(&route.file)),
        ));
        if route.load.is_some() {
            setup.push_str(&format!(
                "    app.set_loader({index}, routes::{name}::Params::from, routes::{name}::load);\n",
                name = route.module
            ));
        }
        if route.has_guard {
            setup.push_str(&format!(
                "    app.set_guard({index}, routes::{name}::Params::from, routes::{name}::guard);\n",
                name = route.module
            ));
        }
    }
    if !project.routes.is_empty() {
        out.push_str("}\n\n");
    }

    let mut render = String::new();
    for (index, node) in routes.nodes.iter().enumerate() {
        let module = module_of(&node.file);
        let route = project.routes.get(&index);
        let depth = routes.chain(index).len() - 1;
        let mut data = Binding::Null;
        let mut preamble = String::new();
        if let Some(data_type) = route.and_then(|r| r.load.as_ref()) {
            let key = format!("{CRATE}::{data_type}");
            let path = emitter.rust_path(&key);
            preamble = format!("        let data: &{path} = page.data::<{path}>({depth});\n");
            data = Binding::Val(Val::new("data", emitter.named(&key)));
        }
        emitter.indent = 2;
        if let Some(module) = module
            && let Some(component) = emitter.default_export(module)
        {
            let props = vec![
                ("params".to_string(), Binding::Params(index)),
                ("data".to_string(), data),
                ("children".to_string(), Binding::Children),
            ];
            emitter.inline(component, Rc::new(props), Frame::root(module), Span::empty(0));
        }
        emitter.flush();
        let body = std::mem::take(&mut emitter.code);
        let (preamble, body) =
            if is_stub { (String::new(), String::new()) } else { (preamble, body) };
        render.push_str(&format!(
            "    fn level_{index}(\n        out: &mut ::reze_server::Html,\n        cx: &mut ::reze_server::Render,\n        page: &::reze_server::Page<'_>,\n    ) -> ::std::result::Result<(), ::reze_server::RouteError> {{\n{preamble}{body}        Ok(())\n    }}\n\n"
        ));
        emitter.indent = 0;
    }

    let mut count = 0;
    render.push_str("    static MUTATIONS: &[::reze_server::MutationDef] = &[\n");
    for (module, facts) in analysis.facts.iter().enumerate() {
        for endpoint in facts.endpoints.iter().filter(|e| e.kind == EndpointKind::Mutation) {
            let target = project.routes.values().find(|route| route.file == modules[module].path);
            let (true, Some(id), Some(route)) = (endpoint.external, &endpoint.id, target) else {
                continue;
            };
            let Some(mutation) = route.mutations.iter().find(|m| m.export == endpoint.export)
            else {
                continue;
            };
            let revalidate = match &mutation.revalidate {
                Some(patterns) => {
                    let patterns: Vec<String> = patterns.iter().map(|p| literal(p)).collect();
                    format!("Some(&[{}])", patterns.join(", "))
                }
                None => "None".into(),
            };
            render.push_str(&format!(
                "        ::reze_server::MutationDef {{ id: {}, file: {}, export: {}, revalidate: {revalidate} }},\n",
                literal(id),
                literal(&modules[module].path),
                literal(&endpoint.export)
            ));
            setup.push_str(&format!(
                "    app.set_mutation({count}, routes::{}::{});\n",
                route.module, mutation.name
            ));
            count += 1;
        }
    }
    render.push_str("    ];\n\n");

    render.push_str("    static NODES: &[::reze_server::Node] = &[\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        let module = module_of(&node.file);
        let revalidate = module
            .and_then(|m| exported_const(&modules[m], "revalidate"))
            .and_then(|(_, init)| match init {
                Some(Expression::NumericLiteral(n)) => Some(format!("Some({})", n.value as u32)),
                _ => None,
            })
            .unwrap_or_else(|| "None".to_string());
        let route = project.routes.get(&index);
        let loader = route.is_some_and(|route| route.load.is_some());
        let guard = route.is_some_and(|route| route.has_guard);
        render.push_str(&format!(
            "        ::reze_server::Node {{ file: {}, pattern: {}, render: level_{index}, loader: {loader}, guard: {guard}, revalidate: {revalidate} }},\n",
            literal(&node.file),
            literal(&node.pattern)
        ));
    }
    render.push_str("    ];\n\n    static ROUTES: &[::reze_server::Route] = &[\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        if node.is_layout {
            continue;
        }
        let chain: Vec<String> = routes.chain(index).iter().map(usize::to_string).collect();
        let chunk = match plan.routes.get(&index) {
            Some(RouteChunk::Route(key)) => {
                format!("Some(::reze_server::Chunk::Route({}))", literal(key))
            }
            Some(RouteChunk::Factories(keys)) => {
                let keys: Vec<String> =
                    keys.iter().map(|(id, key)| format!("({id}, {})", literal(key))).collect();
                format!("Some(::reze_server::Chunk::Factories(&[{}]))", keys.join(", "))
            }
            None => "None".into(),
        };
        render.push_str(&format!(
            "        ::reze_server::Route {{ pattern: {}, segments: {}, chain: &[{}], chunk: {chunk} }},\n",
            literal(&node.pattern),
            segments_code(&node.segments),
            chain.join(", ")
        ));
    }
    let root = routes.root.map(|node| node.to_string()).unwrap_or_default();
    render.push_str(&format!(
        "    ];\n\n    pub static TABLE: ::reze_server::Table =\n        ::reze_server::Table {{ nodes: NODES, routes: ROUTES, root: &[{root}], mutations: MUTATIONS }};\n"
    ));
    out.push_str(&format!(
        "#[rustfmt::skip]\n#[allow(dead_code, unused_variables, unused_mut, unused_braces, unused_parens, unused_labels, clippy::all)]\nmod __render {{\n{render}}}\n\n"
    ));

    out.push_str(&format!(
        "#[rustfmt::skip]\npub fn app() -> ::reze_server::App<__reze::Context> {{\n    let mut app = ::reze_server::App::new(&__render::TABLE);\n{setup}    if let Some(manifest) = ::reze_server::Manifest::embedded(include_str!(\"manifest.json\")) {{\n        app.set_manifest(manifest);\n    }}\n    app\n}}\n\n"
    ));
    out.push_str(match (project.has_main, project.context) {
        (true, _) => "use server::main;\n",
        (false, Context::Unit) => "fn main() -> ::std::io::Result<()> {\n    ::reze_server::ntex::run(app(), async {})\n}\n",
        (false, Context::Sync) => "fn main() -> ::std::io::Result<()> {\n    ::reze_server::ntex::run(app(), async { server::context() })\n}\n",
        (false, Context::Async) => "fn main() -> ::std::io::Result<()> {\n    ::reze_server::ntex::run(app(), server::context())\n}\n",
    });
    (out, emitter.reports)
}
