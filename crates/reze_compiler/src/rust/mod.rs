//! The Rust render target: `reze_routes.rs`, with the params of each route, one render function per
//! route level, and the `App` builder that takes the Rust loaders.

mod ir;
mod lower;
mod nodes;

pub use ir::{CrateIr, FieldDef, TypeDef, TypeRef, VariantDef};

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
    /// `crate::Name`, or `Date`, `Set`, and `Map`, which only client code reads.
    Struct(String),
    Enum(String),
    Opt(Box<Ty>),
    List(Box<Ty>),
    Tuple(Vec<Ty>),
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

/// `addComment` as `add_comment`.
fn snake(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
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
fn node_names(routes: &RouteTable, dir: &str) -> Vec<String> {
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
            TypeRef::Set { .. } => Ty::Struct("Set".into()),
            TypeRef::Map { .. } => Ty::Struct("Map".into()),
            TypeRef::Date => Ty::Struct("Date".into()),
            TypeRef::Tuple { items } => {
                Ty::Tuple(items.iter().map(|item| self.ty_of(item, krate)).collect())
            }
            TypeRef::Defer { of } => self.ty_of(of, krate),
            TypeRef::Named { name } => self.named(&format!("{krate}::{name}")),
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
fn exported_const<'a>(
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

/// Source of `reze_routes.rs`, with the reports of what has no Rust lowering.
pub fn emit(
    modules: &[Module<'_>],
    analysis: &Analysis<'_>,
    routes: &RouteTable,
    plan: &Plan,
    crates: &[CrateIr],
    dir: &str,
) -> (String, Vec<Vec<Report>>) {
    let mut types = HashMap::new();
    for krate in crates {
        for ty in &krate.types {
            types.insert(format!("{}::{}", krate.name, ty.name()), ty);
        }
    }
    let mut emitter = Emitter {
        modules,
        analysis,
        plan,
        routes,
        types,
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
    let names = node_names(routes, dir);
    let module_of = |file: &str| modules.iter().position(|m| m.path == file);

    let mut out = String::from(
        "// Generated by reze-js from the route files. Do not edit.\n\n#[allow(dead_code, unused_variables, unused_mut, unused_braces, unused_parens, unused_labels, clippy::all)]\npub mod routes {\n",
    );
    let mut loaders: Vec<(usize, String)> = Vec::new();
    let mut guards: Vec<usize> = Vec::new();
    for (index, node) in routes.nodes.iter().enumerate() {
        let name = &names[index];
        let module = module_of(&node.file);
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
        out.push_str(&format!(
            "    pub mod {name} {{\n        #[derive(Clone, Debug)]\n        pub struct Params {{\n{fields}        }}\n\n        impl Params {{\n            pub fn from(params: &::reze_server::Params) -> Self {{\n                Self {{ {build}}}\n            }}\n        }}\n    }}\n\n"
        ));

        let depth = routes.chain(index).len() - 1;
        let mut data = Binding::Null;
        let mut preamble = String::new();
        if let Some(module) = module {
            for endpoint in &analysis.facts[module].endpoints {
                let span = endpoint.call;
                match endpoint.kind {
                    EndpointKind::Mutation => {}
                    EndpointKind::Loader if endpoint.export != "load" => {}
                    EndpointKind::Loader => match (endpoint.external, &endpoint.data_type) {
                        (true, Some(path)) => {
                            if !emitter.types.contains_key(path) {
                                emitter.report(
                                    module,
                                    span,
                                    format!("`{path}` is not in the Rust type IR"),
                                );
                            }
                            loaders.push((index, path.clone()));
                            preamble = format!(
                                "        let data: &::{path} = page.data::<::{path}>({depth});\n"
                            );
                            data = Binding::Val(Val::new("data", emitter.named(path)));
                        }
                        (true, None) => emitter.report(
                            module,
                            span,
                            "`loader.external<T>()` takes a type imported from `virtual:reze-types/<crate>`",
                        ),
                        (false, _) => emitter.report(
                            module,
                            span,
                            "A route of the Rust server loads its data with `loader.external<T>()`",
                        ),
                    },
                }
            }
            if let Some((span, init)) = exported_const(&modules[module], "guard") {
                match init {
                    Some(Expression::CallExpression(call))
                        if call
                            .callee
                            .as_member_expression()
                            .and_then(|m| m.static_property_name())
                            == Some("external") =>
                    {
                        guards.push(index);
                    }
                    _ => emitter.report(
                        module,
                        span,
                        "A route of the Rust server guards with `guardRoute.external()`",
                    ),
                }
            }
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
        out.push_str(&format!(
            "    fn level_{index}(\n        out: &mut ::reze_server::Html,\n        cx: &mut ::reze_server::Render,\n        page: &::reze_server::Page<'_>,\n    ) -> ::std::result::Result<(), ::reze_server::RouteError> {{\n{preamble}{body}        Ok(())\n    }}\n\n"
        ));
        emitter.indent = 0;
    }

    let mut mutations: Vec<(String, &str, String, String)> = Vec::new();
    out.push_str("    static MUTATIONS: &[::reze_server::MutationDef] = &[\n");
    for (module, facts) in analysis.facts.iter().enumerate() {
        for endpoint in facts.endpoints.iter().filter(|e| e.kind == EndpointKind::Mutation) {
            let span = endpoint.call;
            let (true, Some(id)) = (endpoint.external, &endpoint.id) else {
                emitter.report(
                    module,
                    span,
                    "A mutation of the Rust server is `mutation.external<Input, Result>()`",
                );
                continue;
            };
            let Some(input) = &endpoint.input_type else {
                emitter.report(
                    module,
                    span,
                    "`mutation.external<Input, Result>()` takes types imported from `virtual:reze-types/<crate>`",
                );
                continue;
            };
            let result = match &endpoint.data_type {
                Some(path) => format!("::{path}"),
                None => "()".into(),
            };
            let setter = rust_ident(&snake(&endpoint.export));
            let taken = ["new", "manifest", "server_timing", "body_limit"]
                .contains(&setter.as_str())
                || loaders.iter().any(|(node, _)| names[*node] == setter)
                || guards.iter().any(|node| format!("guard_{}", names[*node]) == setter)
                || mutations.iter().any(|(name, ..)| *name == setter);
            if taken {
                emitter.report(
                    module,
                    span,
                    format!("The Rust app already has a setter `{setter}`: rename the mutation"),
                );
                continue;
            }
            let revalidate = match &endpoint.revalidate {
                Some(patterns) => {
                    let patterns: Vec<String> = patterns.iter().map(|p| literal(p)).collect();
                    format!("Some(&[{}])", patterns.join(", "))
                }
                None => "None".into(),
            };
            out.push_str(&format!(
                "        ::reze_server::MutationDef {{ id: {}, file: {}, export: {}, revalidate: {revalidate} }},\n",
                literal(id),
                literal(&modules[module].path),
                literal(&endpoint.export)
            ));
            mutations.push((setter, &endpoint.export, format!("::{input}"), result));
        }
    }
    out.push_str("    ];\n\n");

    out.push_str("    static NODES: &[::reze_server::Node] = &[\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        let module = module_of(&node.file);
        let revalidate = module
            .and_then(|m| exported_const(&modules[m], "revalidate"))
            .and_then(|(_, init)| match init {
                Some(Expression::NumericLiteral(n)) => Some(format!("Some({})", n.value as u32)),
                _ => None,
            })
            .unwrap_or_else(|| "None".to_string());
        let loader = loaders.iter().any(|(node, _)| *node == index);
        let guard = guards.contains(&index);
        out.push_str(&format!(
            "        ::reze_server::Node {{ file: {}, pattern: {}, render: level_{index}, loader: {loader}, guard: {guard}, revalidate: {revalidate} }},\n",
            literal(&node.file),
            literal(&node.pattern)
        ));
    }
    out.push_str("    ];\n\n    static ROUTES: &[::reze_server::Route] = &[\n");
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
        out.push_str(&format!(
            "        ::reze_server::Route {{ pattern: {}, segments: {}, chain: &[{}], chunk: {chunk} }},\n",
            literal(&node.pattern),
            segments_code(&node.segments),
            chain.join(", ")
        ));
    }
    let root = routes.root.map(|node| node.to_string()).unwrap_or_default();
    out.push_str(&format!(
        "    ];\n\n    static TABLE: ::reze_server::Table =\n        ::reze_server::Table {{ nodes: NODES, routes: ROUTES, root: &[{root}], mutations: MUTATIONS }};\n\n"
    ));

    out.push_str("    /// The application: the route table and the Rust loaders, guards, and mutations.\n    pub struct App<C>(::reze_server::App<C>);\n\n    impl<C: Send + Sync + 'static> Default for App<C> {\n        fn default() -> Self {\n            Self::new()\n        }\n    }\n\n    impl<C: Send + Sync + 'static> App<C> {\n        pub fn new() -> Self {\n            Self(::reze_server::App::new(&TABLE))\n        }\n\n        /// Sets the URLs of the client build, from `reze.json`.\n        pub fn manifest(mut self, manifest: ::reze_server::Manifest) -> Self {\n            self.0.set_manifest(manifest);\n            self\n        }\n\n        /// Writes loader and render durations into `Server-Timing`, which is on with the `dev` feature.\n        pub fn server_timing(mut self, on: bool) -> Self {\n            self.0.set_server_timing(on);\n            self\n        }\n\n        /// Sets the largest mutation body in bytes, 1 MB by default.\n        pub fn body_limit(mut self, bytes: usize) -> Self {\n            self.0.set_body_limit(bytes);\n            self\n        }\n");
    for (index, path) in &loaders {
        let name = &names[*index];
        out.push_str(&format!(
            "\n        /// Sets the loader of `{}`.\n        pub fn {name}<F>(mut self, loader: F) -> Self\n        where\n            F: for<'a> ::reze_server::Loader<'a, C, {name}::Params, ::{path}> + Send + Sync + 'static,\n        {{\n            self.0.set_loader({index}, {name}::Params::from, loader);\n            self\n        }}\n",
            routes.nodes[*index].pattern
        ));
    }
    for index in &guards {
        let name = &names[*index];
        out.push_str(&format!(
            "\n        /// Sets the guard of `{}`.\n        pub fn guard_{name}<F>(mut self, guard: F) -> Self\n        where\n            F: for<'a> ::reze_server::Guard<'a, C, {name}::Params> + 'static,\n        {{\n            self.0.set_guard({index}, {name}::Params::from, guard);\n            self\n        }}\n",
            routes.nodes[*index].pattern
        ));
    }
    for (index, (setter, export, input, result)) in mutations.iter().enumerate() {
        out.push_str(&format!(
            "\n        /// Sets the mutation `{}`.\n        pub fn {setter}<F>(mut self, mutation: F) -> Self\n        where\n            F: for<'a> ::reze_server::Mutation<'a, C, {input}, {result}> + 'static,\n        {{\n            self.0.set_mutation({index}, mutation);\n            self\n        }}\n",
            export
        ));
    }
    out.push_str("    }\n\n    impl<C> From<App<C>> for ::reze_server::App<C> {\n        fn from(app: App<C>) -> Self {\n            app.0\n        }\n    }\n}\n");
    (out, emitter.reports)
}
