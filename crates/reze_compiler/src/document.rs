use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionBody, ArrowFunctionExpression, BindingPattern, Declaration,
    ExportDefaultDeclaration, ExportDefaultDeclarationKind, Expression, FormalParameters, Function,
    ImportDeclarationSpecifier, ImportOrExportKind, JSXAttribute, JSXAttributeItem,
    JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement, JSXElementName, ObjectPropertyKind,
    Program, PropertyKey, PropertyKind, Statement, VariableDeclarationKind,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

use crate::diagnostic::{self, Code, Diagnostic, Report};
use crate::html::{clean_jsx_text, decode_entities, escape_text, is_svg_element, is_void};

const DEFAULT_ROOT_ID: &str = "app";
const MAX_IMPORT_DEPTH: u32 = 64;

/// A document module rendered to HTML.
pub struct Document {
    /// The whole page, `<!doctype html>` included, with an empty root element.
    pub html: String,
    /// `id` of the element that holds the app.
    pub root_id: String,
    /// Every imported module the page was compiled from.
    pub files: Vec<String>,
}

pub enum DocumentOutcome {
    Html(Document),
    /// Compile again once each request is recorded in the [`DocumentModules`].
    Resolve(Vec<ImportRequest>),
    /// Part of the page is not static, so the module must run to render it; the `info`
    /// diagnostics name each part.
    Render(Vec<Diagnostic>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImportRequest {
    pub importer: String,
    pub specifier: String,
}

/// The imports a document compile has followed so far.
#[derive(Default)]
pub struct DocumentModules {
    resolutions: HashMap<ImportRequest, Option<String>>,
    sources: HashMap<String, String>,
    defines: Vec<(String, String)>,
}

impl DocumentModules {
    /// Records that `request` resolves to `file`, or to no source module when `file` is `None`.
    pub fn resolve(&mut self, request: ImportRequest, file: Option<String>) {
        self.resolutions.insert(request, file);
    }

    pub fn add_source(&mut self, file: String, source: String) {
        self.sources.insert(file, source);
    }

    /// Records that the bundler replaces every unbound reference to the dotted `key` with
    /// `code`, as Vite's `define` does.
    pub fn define(&mut self, key: String, code: String) {
        self.defines.push((key, code));
    }

    fn lookup(&self, importer: &str, specifier: &str) -> Lookup<'_> {
        let request =
            ImportRequest { importer: importer.to_string(), specifier: specifier.to_string() };
        match self.resolutions.get(&request) {
            None => Lookup::Pending(request),
            Some(None) => Lookup::External,
            Some(Some(file)) => match self.sources.get(file) {
                Some(source) => Lookup::Module(file, source),
                None => Lookup::External,
            },
        }
    }
}

enum Lookup<'m> {
    Pending(ImportRequest),
    External,
    Module(&'m str, &'m str),
}

/// Renders the default-exported document component of `source` to static HTML, following its
/// imports through `modules` and inlining every component and literal constant it reaches.
/// `Err` holds the diagnostics of a page that cannot be valid however it renders.
pub fn compile_document(
    source: &str,
    filename: &str,
    modules: &DocumentModules,
) -> Result<DocumentOutcome, Vec<Diagnostic>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let program = crate::parse_program(&allocator, source, filename, source_type)?;
    let mut renderer = Renderer::new(&allocator, modules);
    renderer.modules.push(Module::collect(program, filename, source));
    let mut export = None;
    for statement in &program.body {
        match statement {
            Statement::ExportDefaultDeclaration(declaration) if export.is_none() => {
                export = Some(declaration.as_ref());
            }
            statement if renderer.is_static_statement(statement) => {}
            statement => renderer.dynamic(statement.span(), "a runtime statement"),
        }
    }
    match export {
        Some(export) => renderer.document(export),
        None => renderer.shape(Span::empty(0), "has no default export"),
    }
    renderer.finish()
}

#[derive(Clone, Copy)]
enum Callable<'a> {
    Function(&'a Function<'a>),
    Arrow(&'a ArrowFunctionExpression<'a>),
}

#[derive(Clone, Copy)]
enum Imported<'a> {
    Named(&'a str),
    Namespace,
}

#[derive(Clone, Copy)]
enum Local<'a> {
    Component(Callable<'a>),
    Const(&'a Expression<'a>),
    Import(&'a str, Imported<'a>),
    Opaque,
}

#[derive(Clone, Copy)]
enum Export<'a> {
    Local(&'a str),
    From(&'a str, Imported<'a>),
    Anonymous(Callable<'a>),
    Value(&'a Expression<'a>),
}

struct Module<'a> {
    file: &'a str,
    source: &'a str,
    locals: HashMap<&'a str, Local<'a>>,
    exports: HashMap<&'a str, Export<'a>>,
    stars: Vec<&'a str>,
}

impl<'a> Module<'a> {
    fn collect(program: &'a Program<'a>, file: &'a str, source: &'a str) -> Self {
        let mut module = Module {
            file,
            source,
            locals: HashMap::new(),
            exports: HashMap::new(),
            stars: Vec::new(),
        };
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(import)
                    if import.import_kind != ImportOrExportKind::Type =>
                {
                    let from = import.source.value.as_str();
                    for specifier in import.specifiers.iter().flatten() {
                        let (local, imported) = match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                                if specifier.import_kind == ImportOrExportKind::Type {
                                    continue;
                                }
                                (
                                    &specifier.local,
                                    Imported::Named(specifier.imported.name().as_str()),
                                )
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                                (&specifier.local, Imported::Named("default"))
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                                (&specifier.local, Imported::Namespace)
                            }
                        };
                        module.locals.insert(local.name.as_str(), Local::Import(from, imported));
                    }
                }
                Statement::ExportDeclaration(export) => {
                    for name in module.declare(&export.declaration) {
                        module.exports.insert(name, Export::Local(name));
                    }
                }
                Statement::ExportNamedDeclaration(export)
                    if export.export_kind != ImportOrExportKind::Type =>
                {
                    for specifier in &export.specifiers {
                        module.exports.insert(
                            specifier.exported.name().as_str(),
                            Export::Local(specifier.local.name().as_str()),
                        );
                    }
                }
                Statement::ExportFromDeclaration(export)
                    if export.export_kind != ImportOrExportKind::Type =>
                {
                    let from = export.source.value.as_str();
                    for specifier in &export.specifiers {
                        module.exports.insert(
                            specifier.exported.name().as_str(),
                            Export::From(from, Imported::Named(specifier.local.name().as_str())),
                        );
                    }
                }
                Statement::ExportAllDeclaration(export)
                    if export.export_kind != ImportOrExportKind::Type =>
                {
                    let from = export.source.value.as_str();
                    match &export.exported {
                        Some(name) => {
                            module.exports.insert(
                                name.name().as_str(),
                                Export::From(from, Imported::Namespace),
                            );
                        }
                        None => module.stars.push(from),
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let default = match &export.declaration {
                        ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                            match &function.id {
                                Some(id) => {
                                    let name = id.name.as_str();
                                    module.locals.insert(
                                        name,
                                        Local::Component(Callable::Function(function)),
                                    );
                                    Export::Local(name)
                                }
                                None => Export::Anonymous(Callable::Function(function)),
                            }
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(_)
                        | ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => continue,
                        declaration => match declaration.as_expression().map(unparenthesized) {
                            Some(Expression::Identifier(identifier)) => {
                                Export::Local(identifier.name.as_str())
                            }
                            Some(Expression::ArrowFunctionExpression(arrow)) => {
                                Export::Anonymous(Callable::Arrow(arrow))
                            }
                            Some(Expression::FunctionExpression(function)) => {
                                Export::Anonymous(Callable::Function(function))
                            }
                            Some(expression) => Export::Value(expression),
                            None => continue,
                        },
                    };
                    module.exports.insert("default", default);
                }
                statement => {
                    if let Some(declaration) = statement.as_declaration() {
                        module.declare(declaration);
                    }
                }
            }
        }
        module
    }

    fn declare(&mut self, declaration: &'a Declaration<'a>) -> Vec<&'a str> {
        let mut names = Vec::new();
        match declaration {
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    let name = id.name.as_str();
                    let local = if is_component_name(name) {
                        Local::Component(Callable::Function(function))
                    } else {
                        Local::Opaque
                    };
                    self.locals.insert(name, local);
                    names.push(name);
                }
            }
            Declaration::VariableDeclaration(variables) => {
                for declarator in &variables.declarations {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
                    let name = id.name.as_str();
                    let local =
                        match (variables.kind, declarator.init.as_ref().map(unparenthesized)) {
                            (
                                VariableDeclarationKind::Const,
                                Some(Expression::ArrowFunctionExpression(arrow)),
                            ) if is_component_name(name) => {
                                Local::Component(Callable::Arrow(arrow))
                            }
                            (
                                VariableDeclarationKind::Const,
                                Some(Expression::FunctionExpression(function)),
                            ) if is_component_name(name) => {
                                Local::Component(Callable::Function(function))
                            }
                            (VariableDeclarationKind::Const, Some(init)) => Local::Const(init),
                            _ => Local::Opaque,
                        };
                    self.locals.insert(name, local);
                    names.push(name);
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    self.locals.insert(id.name.as_str(), Local::Opaque);
                    names.push(id.name.as_str());
                }
            }
            _ => {}
        }
        names
    }
}

fn is_component_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_uppercase())
}

enum Unreadable {
    Suspends,
    Statements,
}

fn returned<'a>(
    callable: Callable<'a>,
) -> Result<(&'a Expression<'a>, &'a FormalParameters<'a>), (Span, Unreadable)> {
    let (suspends, span, statements, params) = match callable {
        Callable::Function(function) => {
            let Some(body) = &function.body else {
                return Err((function.span, Unreadable::Statements));
            };
            (function.r#async || function.generator, body.span, &body.statements, &function.params)
        }
        Callable::Arrow(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                (arrow.r#async, body.span, &body.statements, &arrow.params)
            }
            body if arrow.r#async => return Err((body.span(), Unreadable::Suspends)),
            body => {
                return body
                    .as_expression()
                    .map(|body| (body, &*arrow.params))
                    .ok_or((arrow.span, Unreadable::Statements));
            }
        },
    };
    if suspends {
        return Err((span, Unreadable::Suspends));
    }
    match statements.as_slice() {
        [Statement::ReturnStatement(statement)] => match &statement.argument {
            Some(argument) => Ok((argument, params)),
            None => Err((span, Unreadable::Statements)),
        },
        _ => Err((span, Unreadable::Statements)),
    }
}

#[derive(Clone)]
enum StaticValue {
    Text(String),
    Flag(bool),
}

struct Field<'a> {
    key: &'a str,
    local: &'a str,
    default: Option<StaticValue>,
}

#[derive(Default)]
enum Params<'a> {
    #[default]
    Absent,
    Object(&'a str),
    Fields(Vec<Field<'a>>),
}

impl<'a> Params<'a> {
    fn binds(&self, name: &str) -> bool {
        match self {
            Params::Absent => false,
            Params::Object(props) => *props == name,
            Params::Fields(fields) => fields.iter().any(|field| field.local == name),
        }
    }

    fn prop(&self, expression: &'a Expression<'a>) -> Option<(&'a str, Option<StaticValue>)> {
        match (unparenthesized(expression), self) {
            (Expression::StaticMemberExpression(member), Params::Object(props))
                if !member.optional
                    && matches!(&member.object, Expression::Identifier(object) if object.name == *props) =>
            {
                Some((member.property.name.as_str(), None))
            }
            (Expression::Identifier(identifier), Params::Fields(fields)) => fields
                .iter()
                .find(|field| field.local == identifier.name)
                .map(|field| (field.key, field.default.clone())),
            _ => None,
        }
    }
}

struct Component<'a> {
    name: &'a str,
    module: usize,
    params: Params<'a>,
    body: Option<&'a Expression<'a>>,
}

enum Binding {
    Component(usize),
    Value(StaticValue),
    Pending,
    Missing,
    Opaque(String),
}

enum Load {
    Pending,
    Unreadable(String),
}

#[derive(Clone, Copy)]
struct Scope<'a> {
    component: usize,
    call: Option<&'a JSXElement<'a>>,
    caller: usize,
}

enum Value<'a> {
    Absent,
    Static(StaticValue),
    Children(&'a [JSXChild<'a>], usize),
    Slot,
}

struct Renderer<'a> {
    allocator: &'a Allocator,
    input: &'a DocumentModules,
    modules: Vec<Module<'a>>,
    module_by_file: HashMap<&'a str, usize>,
    requests: Vec<ImportRequest>,
    components: Vec<Component<'a>>,
    component_ids: HashMap<(usize, &'a str), usize>,
    html: String,
    reports: Vec<(usize, Report)>,
    root: Option<(String, Option<Span>)>,
    slots: u32,
    ids: Vec<(String, Span)>,
    scopes: Vec<Scope<'a>>,
    scope: usize,
    root_span: Span,
    defines: Vec<Define<'a>>,
}

struct Define<'a> {
    path: Vec<&'a str>,
    value: Option<&'a Expression<'a>>,
}

impl<'a> Renderer<'a> {
    fn new(allocator: &'a Allocator, input: &'a DocumentModules) -> Self {
        let defines = input
            .defines
            .iter()
            .map(|(key, code)| Define {
                path: key.split('.').collect(),
                value: Parser::new(allocator, code, SourceType::mjs())
                    .parse_expression()
                    .ok()
                    .map(|value| &*allocator.alloc(value)),
            })
            .collect();
        Renderer {
            allocator,
            input,
            defines,
            modules: Vec::new(),
            module_by_file: HashMap::new(),
            requests: Vec::new(),
            components: Vec::new(),
            component_ids: HashMap::new(),
            html: String::new(),
            reports: Vec::new(),
            root: None,
            slots: 0,
            ids: Vec::new(),
            scopes: Vec::new(),
            scope: 0,
            root_span: Span::empty(0),
        }
    }

    fn is_static_statement(&self, statement: &'a Statement<'a>) -> bool {
        match statement {
            Statement::ImportDeclaration(import) => {
                import.import_kind == ImportOrExportKind::Type
                    || import.specifiers.as_ref().is_some_and(|specifiers| !specifiers.is_empty())
            }
            Statement::ExportNamedDeclaration(_) => true,
            Statement::ExportDeclaration(export) => self.is_static_declaration(&export.declaration),
            statement => statement
                .as_declaration()
                .is_some_and(|declaration| self.is_static_declaration(declaration)),
        }
    }

    fn is_static_declaration(&self, declaration: &'a Declaration<'a>) -> bool {
        match declaration {
            Declaration::FunctionDeclaration(_) => true,
            Declaration::VariableDeclaration(variables) => {
                variables.kind == VariableDeclarationKind::Const
                    && variables.declarations.iter().all(|declarator| {
                        declarator.init.as_ref().is_some_and(|init| {
                            matches!(
                                unparenthesized(init),
                                Expression::ArrowFunctionExpression(_)
                                    | Expression::FunctionExpression(_)
                            ) || self.constant(0, init).is_some()
                        })
                    })
            }
            declaration => declaration.is_typescript_syntax(),
        }
    }

    fn constant(&self, module: usize, expression: &'a Expression<'a>) -> Option<StaticValue> {
        static_value(expression).or_else(|| self.defined(module, &Params::Absent, expression))
    }

    /// The value of `expression` when it reads a define through names `module` and `params`
    /// leave unbound.
    fn defined(
        &self,
        module: usize,
        params: &Params<'a>,
        expression: &'a Expression<'a>,
    ) -> Option<StaticValue> {
        let mut path = Vec::new();
        let mut node = unparenthesized(expression);
        loop {
            match node {
                Expression::StaticMemberExpression(member) => {
                    path.push(member.property.name.as_str());
                    node = unparenthesized(&member.object);
                }
                Expression::Identifier(identifier) => {
                    let name = identifier.name.as_str();
                    if self.modules[module].locals.contains_key(name) || params.binds(name) {
                        return None;
                    }
                    path.push(name);
                    break;
                }
                Expression::ImportMeta(_) => {
                    path.extend(["meta", "import"]);
                    break;
                }
                _ => return None,
            }
        }
        path.reverse();
        let define = self
            .defines
            .iter()
            .filter(|define| path.starts_with(&define.path))
            .max_by_key(|define| define.path.len())?;
        let mut value = define.value?;
        for key in &path[define.path.len()..] {
            value = own_property(value, key)?;
        }
        static_value(value)
    }

    fn current_module(&self) -> usize {
        self.scopes.get(self.scope).map_or(0, |scope| self.components[scope.component].module)
    }

    fn report_in(&mut self, module: usize, code: Code, span: Span, detail: &str) {
        if self
            .reports
            .iter()
            .any(|(other, report)| *other == module && report.code == code && report.span == span)
        {
            return;
        }
        let mut report = Report::new(code, span).arg("detail", detail);
        report.path = self
            .scopes
            .iter()
            .skip(1)
            .map(|scope| format!("<{}>", self.components[scope.component].name))
            .collect();
        self.reports.push((module, report));
    }

    fn shape(&mut self, span: Span, detail: &str) {
        self.report_in(self.current_module(), Code::DocumentShape, span, detail);
    }

    fn dynamic(&mut self, span: Span, detail: &str) {
        self.report_in(self.current_module(), Code::DocumentDynamic, span, detail);
    }

    fn misplaced_root(&mut self, span: Span, detail: &str) {
        self.report_in(self.current_module(), Code::DocumentRoot, span, detail);
    }

    fn load(&mut self, importer: usize, specifier: &'a str) -> Result<usize, Load> {
        let (file, source) = match self.input.lookup(self.modules[importer].file, specifier) {
            Lookup::Pending(request) => {
                if !self.requests.contains(&request) {
                    self.requests.push(request);
                }
                return Err(Load::Pending);
            }
            Lookup::External => {
                return Err(Load::Unreadable(format!(
                    "`{specifier}`, which is not a source module"
                )));
            }
            Lookup::Module(file, source) => (file, source),
        };
        if let Some(&module) = self.module_by_file.get(file) {
            return Ok(module);
        }
        let source_type = SourceType::from_path(file).unwrap_or_else(|_| SourceType::tsx());
        let Ok(program) = crate::parse_program(self.allocator, source, file, source_type) else {
            return Err(Load::Unreadable(format!("`{specifier}`, which does not parse")));
        };
        self.modules.push(Module::collect(program, file, source));
        self.module_by_file.insert(file, self.modules.len() - 1);
        Ok(self.modules.len() - 1)
    }

    fn binding(&mut self, module: usize, name: &'a str, depth: u32) -> Binding {
        let Some(local) = self.modules[module].locals.get(name).copied() else {
            return Binding::Opaque(format!("`{name}`, which its module does not declare"));
        };
        match local {
            Local::Component(callable) => {
                Binding::Component(self.component(module, name, callable))
            }
            Local::Const(init) => self.constant(module, init).map_or_else(
                || Binding::Opaque(format!("`{name}`, which is not a literal constant")),
                Binding::Value,
            ),
            Local::Import(specifier, imported) => match self.load(module, specifier) {
                Ok(target) => match self.export(target, imported, depth + 1) {
                    Binding::Missing => {
                        Binding::Opaque(format!("`{name}`, which `{specifier}` does not export"))
                    }
                    binding => binding,
                },
                Err(Load::Pending) => Binding::Pending,
                Err(Load::Unreadable(detail)) => Binding::Opaque(format!("`{name}` from {detail}")),
            },
            Local::Opaque => Binding::Opaque(format!(
                "`{name}`, which is neither a component nor a literal constant"
            )),
        }
    }

    fn export(&mut self, module: usize, imported: Imported<'a>, depth: u32) -> Binding {
        if depth > MAX_IMPORT_DEPTH {
            return Binding::Opaque("an import cycle".to_string());
        }
        let Imported::Named(name) = imported else {
            return Binding::Opaque("a namespace import".to_string());
        };
        match self.modules[module].exports.get(name).copied() {
            Some(Export::Local(local)) => self.binding(module, local, depth),
            Some(Export::From(specifier, imported)) => match self.load(module, specifier) {
                Ok(target) => self.export(target, imported, depth + 1),
                Err(Load::Pending) => Binding::Pending,
                Err(Load::Unreadable(detail)) => Binding::Opaque(format!("`{name}` from {detail}")),
            },
            Some(Export::Anonymous(callable)) => {
                Binding::Component(self.component(module, name, callable))
            }
            Some(Export::Value(expression)) => self.constant(module, expression).map_or_else(
                || Binding::Opaque(format!("`{name}`, which is not a literal constant")),
                Binding::Value,
            ),
            None => {
                let stars = self.modules[module].stars.clone();
                let mut pending = false;
                for specifier in stars {
                    match self.load(module, specifier) {
                        Ok(target) => match self.export(target, imported, depth + 1) {
                            Binding::Missing => {}
                            Binding::Pending => pending = true,
                            binding => return binding,
                        },
                        Err(Load::Pending) => pending = true,
                        Err(Load::Unreadable(detail)) => {
                            return Binding::Opaque(format!("`{name}` from {detail}"));
                        }
                    }
                }
                if pending { Binding::Pending } else { Binding::Missing }
            }
        }
    }

    fn component(&mut self, module: usize, name: &'a str, callable: Callable<'a>) -> usize {
        if let Some(&id) = self.component_ids.get(&(module, name)) {
            return id;
        }
        let (body, params) = match returned(callable) {
            Ok((body, params))
                if matches!(
                    unparenthesized(body),
                    Expression::JSXElement(_) | Expression::JSXFragment(_)
                ) =>
            {
                (Some(body), self.params(params, module))
            }
            Ok((body, _)) => {
                self.report_in(
                    module,
                    Code::DocumentDynamic,
                    body.span(),
                    &format!("`{name}`, which returns something other than JSX"),
                );
                (None, Params::Absent)
            }
            Err((span, Unreadable::Suspends)) => {
                self.report_in(
                    module,
                    Code::DocumentDynamic,
                    span,
                    &format!("`{name}`, which is async or a generator"),
                );
                (None, Params::Absent)
            }
            Err((span, Unreadable::Statements)) => {
                self.report_in(
                    module,
                    Code::DocumentDynamic,
                    span,
                    &format!("`{name}`, which runs statements besides its `return`"),
                );
                (None, Params::Absent)
            }
        };
        self.components.push(Component { name, module, params, body });
        let id = self.components.len() - 1;
        self.component_ids.insert((module, name), id);
        id
    }

    fn params(&mut self, params: &'a FormalParameters<'a>, module: usize) -> Params<'a> {
        let Some(first) = params.items.first() else { return Params::Absent };
        let opaque = |renderer: &mut Self, span: Span, detail: &str| {
            renderer.report_in(module, Code::DocumentDynamic, span, detail);
        };
        let object = match &first.pattern {
            BindingPattern::BindingIdentifier(identifier) => {
                return Params::Object(identifier.name.as_str());
            }
            BindingPattern::ObjectPattern(object) => object,
            other => {
                opaque(
                    self,
                    other.span(),
                    "props taken as something other than `props` or `{ … }`",
                );
                return Params::Absent;
            }
        };
        if let Some(rest) = &object.rest {
            opaque(self, rest.span, "props collected with a rest element");
        }
        let mut fields = Vec::new();
        for property in &object.properties {
            let key = match &property.key {
                PropertyKey::StaticIdentifier(key) if !property.computed => key.name.as_str(),
                PropertyKey::StringLiteral(key) => key.value.as_str(),
                _ => {
                    opaque(self, property.key.span(), "a prop read with a computed key");
                    continue;
                }
            };
            match &property.value {
                BindingPattern::BindingIdentifier(local) => {
                    fields.push(Field { key, local: local.name.as_str(), default: None });
                }
                BindingPattern::AssignmentPattern(assignment) => {
                    match (&assignment.left, static_value(&assignment.right)) {
                        (BindingPattern::BindingIdentifier(local), Some(default)) => {
                            fields.push(Field {
                                key,
                                local: local.name.as_str(),
                                default: Some(default),
                            });
                        }
                        (left, _) => {
                            opaque(self, assignment.span, "a prop default other than a literal");
                            if let BindingPattern::BindingIdentifier(local) = left {
                                fields.push(Field {
                                    key,
                                    local: local.name.as_str(),
                                    default: None,
                                });
                            }
                        }
                    }
                }
                other => opaque(self, other.span(), "a prop destructured further"),
            }
        }
        Params::Fields(fields)
    }

    fn document(&mut self, export: &'a ExportDefaultDeclaration<'a>) {
        let callable = match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                Callable::Function(function)
            }
            declaration => match declaration.as_expression().map(unparenthesized) {
                Some(Expression::FunctionExpression(function)) => Callable::Function(function),
                Some(Expression::ArrowFunctionExpression(arrow)) => Callable::Arrow(arrow),
                Some(Expression::Identifier(identifier)) => {
                    match self.binding(0, identifier.name.as_str(), 0) {
                        Binding::Component(id) => {
                            self.components[id].name = "";
                            self.enter_document(id, export.span);
                        }
                        Binding::Pending => {}
                        Binding::Value(_) => self
                            .shape(export.span, "default-exports something other than a function"),
                        Binding::Missing | Binding::Opaque(_) => {
                            self.dynamic(
                                identifier.span,
                                "a default export the compiler cannot follow",
                            );
                        }
                    }
                    return;
                }
                Some(
                    Expression::StringLiteral(_)
                    | Expression::NumericLiteral(_)
                    | Expression::BooleanLiteral(_)
                    | Expression::NullLiteral(_)
                    | Expression::ObjectExpression(_)
                    | Expression::ArrayExpression(_)
                    | Expression::TemplateLiteral(_)
                    | Expression::JSXElement(_)
                    | Expression::JSXFragment(_),
                )
                | None => {
                    self.shape(export.span, "default-exports something other than a function");
                    return;
                }
                Some(expression) => {
                    self.dynamic(expression.span(), "a default export the compiler cannot follow");
                    return;
                }
            },
        };
        let (body, params) = match returned(callable) {
            Ok(returned) => returned,
            Err((span, Unreadable::Suspends)) => {
                self.dynamic(span, "a document that is async or a generator");
                return;
            }
            Err((span, Unreadable::Statements)) => {
                self.dynamic(span, "statements besides `return <html>…</html>`");
                return;
            }
        };
        let params = self.params(params, 0);
        self.components.push(Component { name: "", module: 0, params, body: Some(body) });
        self.enter_document(self.components.len() - 1, export.span);
    }

    fn enter_document(&mut self, component: usize, span: Span) {
        self.root_span = span;
        self.scopes.push(Scope { component, call: None, caller: 0 });
        self.scope = 0;
        let Some(body) = self.components[component].body else { return };
        match unparenthesized(body) {
            Expression::JSXElement(element)
                if tag_name(&element.opening_element.name) == Some("html") =>
            {
                self.html.push_str("<!doctype html>");
                self.page(element);
            }
            Expression::JSXElement(_) | Expression::JSXFragment(_) => {
                self.shape(body.span(), "returns something other than an `<html>` element");
            }
            other => self.dynamic(other.span(), "a document that returns something other than JSX"),
        }
    }

    fn page(&mut self, html: &'a JSXElement<'a>) {
        self.open(html, "html");
        let parts: Vec<&'a JSXElement<'a>> = html
            .children
            .iter()
            .filter(|child| is_meaningful(child))
            .filter_map(|child| match child {
                JSXChild::Element(element) => Some(element.as_ref()),
                other => {
                    self.shape(other.span(), "`<html>` holds only `<head>` and `<body>`");
                    None
                }
            })
            .collect();
        let tags: Vec<Option<&str>> =
            parts.iter().map(|part| tag_name(&part.opening_element.name)).collect();
        if tags != [Some("head"), Some("body")] {
            self.shape(html.opening_element.span, "`<html>` holds `<head>` and then `<body>`");
        }
        for part in parts {
            self.element(
                part,
                part.opening_element.name.span(),
                tag_name(&part.opening_element.name) == Some("body"),
            );
        }
        self.html.push_str("</html>");
    }

    fn element(&mut self, element: &'a JSXElement<'a>, span: Span, in_body: bool) {
        let Some(tag) = tag_name(&element.opening_element.name) else {
            let JSXElementName::IdentifierReference(name) = &element.opening_element.name else {
                self.dynamic(span, "a member or namespaced component");
                return;
            };
            let module = self.current_module();
            match self.binding(module, name.name.as_str(), 0) {
                Binding::Component(component) => self.call(component, element, span, in_body),
                Binding::Pending => {}
                Binding::Value(_) => {
                    self.dynamic(span, &format!("`{}`, a constant used as a component", name.name))
                }
                Binding::Missing => self
                    .dynamic(span, &format!("`{}`, which its module does not declare", name.name)),
                Binding::Opaque(detail) => self.dynamic(span, &detail),
            }
            return;
        };
        if let Some(slot_span) = self.sole_slot(&element.children, self.scope) {
            self.root_element(element, tag, slot_span, in_body);
            return;
        }
        self.open(element, tag);
        if is_void(tag) {
            if element.children.iter().any(is_meaningful) {
                self.shape(element.span, "gives a void element children");
            }
            return;
        }
        let raw = matches!(tag, "script" | "style");
        let text_start = self.html.len();
        for child in &element.children {
            self.child(child, raw, in_body);
        }
        if raw {
            let closing = format!("</{tag}");
            if self.html[text_start..].to_ascii_lowercase().contains(&closing) {
                self.shape(element.span, "puts the closing tag text inside a raw-text element");
            }
        }
        self.html.push_str("</");
        self.html.push_str(tag);
        self.html.push('>');
    }

    fn call(&mut self, component: usize, element: &'a JSXElement<'a>, span: Span, in_body: bool) {
        let name = self.components[component].name;
        if self.scopes.iter().any(|scope| scope.component == component) {
            self.shape(span, &format!("renders `{name}` inside itself"));
            return;
        }
        for item in &element.opening_element.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    self.dynamic(spread.span, "a spread attribute");
                }
                JSXAttributeItem::Attribute(attribute) => match &attribute.value {
                    Some(JSXAttributeValue::ExpressionContainer(container)) => {
                        if let Some(expression) = container.expression.as_expression()
                            && self.resolve(expression, self.scope).is_none()
                            && static_value(expression).is_none()
                        {
                            self.dynamic(container.span, "an attribute expression");
                        }
                    }
                    Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => {
                        self.dynamic(attribute.span, "an element-valued attribute");
                    }
                    _ => {}
                },
            }
        }
        let Some(body) = self.components[component].body else { return };
        let caller = self.scope;
        let depth = self.scopes.len();
        self.scopes.push(Scope { component, call: Some(element), caller });
        self.scope = depth;
        match unparenthesized(body) {
            Expression::JSXElement(root) => {
                self.element(root, root.opening_element.name.span(), in_body)
            }
            Expression::JSXFragment(fragment) => {
                for child in &fragment.children {
                    self.child(child, false, in_body);
                }
            }
            _ => {}
        }
        self.scopes.truncate(depth);
        self.scope = caller;
    }

    fn resolve(&mut self, expression: &'a Expression<'a>, scope: usize) -> Option<Value<'a>> {
        let Scope { component, call, caller } = self.scopes[scope];
        let Some((key, default)) = self.components[component].params.prop(expression) else {
            let module = self.components[component].module;
            if let Expression::Identifier(identifier) = unparenthesized(expression)
                && self.modules[module].locals.contains_key(identifier.name.as_str())
            {
                return match self.binding(module, identifier.name.as_str(), 0) {
                    Binding::Value(value) => Some(Value::Static(value)),
                    Binding::Pending => Some(Value::Absent),
                    Binding::Component(_) | Binding::Missing | Binding::Opaque(_) => None,
                };
            }
            return self
                .defined(module, &self.components[component].params, expression)
                .map(Value::Static);
        };
        let Some(call) = call else { return (key == "children").then_some(Value::Slot) };
        if key == "children" && call.children.iter().any(is_meaningful) {
            return Some(Value::Children(&call.children, caller));
        }
        let attribute = call.opening_element.attributes.iter().rev().find_map(|item| match item {
            JSXAttributeItem::Attribute(attribute)
                if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == key) =>
            {
                Some(attribute.as_ref())
            }
            _ => None,
        });
        let value =
            attribute.map_or(Value::Absent, |attribute| self.attribute_value(attribute, caller));
        Some(match (value, default) {
            (Value::Absent, Some(default)) => Value::Static(default),
            (value, _) => value,
        })
    }

    fn attribute_value(&mut self, attribute: &'a JSXAttribute<'a>, scope: usize) -> Value<'a> {
        match &attribute.value {
            None => Value::Static(StaticValue::Flag(true)),
            Some(JSXAttributeValue::StringLiteral(literal)) => Value::Static(StaticValue::Text(
                decode_entities(literal.value.as_str()).into_owned(),
            )),
            Some(JSXAttributeValue::ExpressionContainer(container)) => {
                let Some(expression) = container.expression.as_expression() else {
                    return Value::Absent;
                };
                self.resolve(expression, scope)
                    .or_else(|| static_value(expression).map(Value::Static))
                    .unwrap_or(Value::Absent)
            }
            Some(_) => Value::Absent,
        }
    }

    fn sole_slot(&mut self, children: &'a [JSXChild<'a>], scope: usize) -> Option<Span> {
        let mut meaningful = children.iter().filter(|child| is_meaningful(child));
        let (Some(JSXChild::ExpressionContainer(container)), None) =
            (meaningful.next(), meaningful.next())
        else {
            return None;
        };
        match self.resolve(container.expression.as_expression()?, scope)? {
            Value::Slot => Some(container.span),
            Value::Children(children, scope) => self.sole_slot(children, scope),
            Value::Absent | Value::Static(_) => None,
        }
    }

    fn root_element(
        &mut self,
        element: &'a JSXElement<'a>,
        tag: &str,
        slot_span: Span,
        in_body: bool,
    ) {
        self.slots += 1;
        if self.slots > 1 {
            self.misplaced_root(slot_span, "renders its `children` more than once");
        }
        if !in_body {
            self.misplaced_root(slot_span, "renders its `children` outside `<body>`");
        } else if matches!(
            tag,
            "body" | "script" | "style" | "title" | "textarea" | "template" | "noscript"
        ) || is_void(tag)
            || tag == "svg"
            || tag == "math"
            || is_svg_element(tag)
        {
            self.misplaced_root(
                slot_span,
                "renders its `children` directly in a `<body>`, raw-text, template or SVG element; wrap them in an element such as `<div>`",
            );
        }
        let root = match self.open(element, tag) {
            Some((id, span)) => (id, Some(span)),
            None => {
                self.html.insert_str(self.html.len() - 1, &format!(" id={DEFAULT_ROOT_ID}"));
                (DEFAULT_ROOT_ID.to_string(), None)
            }
        };
        self.root.get_or_insert(root);
        self.html.push_str("</");
        self.html.push_str(tag);
        self.html.push('>');
    }

    fn child(&mut self, child: &'a JSXChild<'a>, raw: bool, in_body: bool) {
        match child {
            JSXChild::Text(text) => {
                let cleaned = clean_jsx_text(&decode_entities(text.value.as_str()));
                self.text(&cleaned, raw);
            }
            JSXChild::Element(element) => {
                if raw {
                    self.shape(element.span, "puts an element inside a raw-text element");
                    return;
                }
                let span = element.opening_element.name.span();
                self.element(element, span, in_body);
            }
            JSXChild::Fragment(fragment) => {
                for child in &fragment.children {
                    self.child(child, raw, in_body);
                }
            }
            JSXChild::ExpressionContainer(container) => {
                let Some(expression) = container.expression.as_expression() else { return };
                let value = self
                    .resolve(expression, self.scope)
                    .or_else(|| static_value(expression).map(Value::Static));
                match value {
                    Some(Value::Slot) => {
                        self.slots += 1;
                        self.misplaced_root(
                            container.span,
                            "renders its `children` next to other content; give them an element of their own",
                        );
                    }
                    Some(Value::Children(children, scope)) => {
                        let own = self.scope;
                        self.scope = scope;
                        for child in children {
                            self.child(child, raw, in_body);
                        }
                        self.scope = own;
                    }
                    Some(Value::Static(StaticValue::Text(text))) => self.text(&text, raw),
                    Some(Value::Static(StaticValue::Flag(_)) | Value::Absent) => {}
                    None => self.dynamic(container.span, "an expression"),
                }
            }
            JSXChild::Spread(spread) => self.dynamic(spread.span, "a spread child"),
        }
    }

    fn text(&mut self, text: &str, raw: bool) {
        if raw {
            self.html.push_str(text);
        } else {
            escape_text(&mut self.html, text);
        }
    }

    /// Writes the opening tag; returns its literal `id` and the attribute's span.
    fn open(&mut self, element: &'a JSXElement<'a>, tag: &str) -> Option<(String, Span)> {
        self.html.push('<');
        self.html.push_str(tag);
        let mut id = None;
        for item in &element.opening_element.attributes {
            let attribute = match item {
                JSXAttributeItem::Attribute(attribute) => attribute,
                JSXAttributeItem::SpreadAttribute(spread) => {
                    self.dynamic(spread.span, "a spread attribute");
                    continue;
                }
            };
            let name = match &attribute.name {
                JSXAttributeName::Identifier(name) => name.name.to_string(),
                JSXAttributeName::NamespacedName(name) => {
                    format!("{}:{}", name.namespace.name, name.name.name)
                }
            };
            let value = match &attribute.value {
                None => StaticValue::Flag(true),
                Some(JSXAttributeValue::StringLiteral(literal)) => {
                    StaticValue::Text(decode_entities(literal.value.as_str()).into_owned())
                }
                Some(JSXAttributeValue::ExpressionContainer(container)) => {
                    let value = container.expression.as_expression().and_then(|expression| {
                        match self.resolve(expression, self.scope) {
                            Some(Value::Static(value)) => Some(Some(value)),
                            Some(Value::Absent) => Some(None),
                            Some(Value::Children(..) | Value::Slot) => None,
                            None => static_value(expression).map(Some),
                        }
                    });
                    match value {
                        Some(Some(value)) => value,
                        Some(None) => continue,
                        None => {
                            self.dynamic(container.span, "an attribute expression");
                            continue;
                        }
                    }
                }
                Some(other) => {
                    self.dynamic(other.span(), "an element-valued attribute");
                    continue;
                }
            };
            match value {
                StaticValue::Flag(false) => {}
                StaticValue::Flag(true) => {
                    self.html.push(' ');
                    self.html.push_str(&name);
                }
                StaticValue::Text(text) => {
                    self.html.push(' ');
                    self.html.push_str(&name);
                    crate::html::push_attribute_value(&mut self.html, &text);
                    if name == "id" {
                        self.ids.push((text.clone(), attribute.span));
                        id = Some((text, attribute.span));
                    }
                }
            }
        }
        self.html.push('>');
        id
    }

    fn finish(mut self) -> Result<DocumentOutcome, Vec<Diagnostic>> {
        let has_reasons =
            self.reports.iter().any(|(_, report)| report.code == Code::DocumentDynamic);
        if !has_reasons && !self.requests.is_empty() {
            return Ok(DocumentOutcome::Resolve(self.requests));
        }
        if !has_reasons {
            self.check_root();
        }
        let (reasons, errors): (Vec<_>, Vec<_>) =
            self.reports.drain(..).partition(|(_, report)| report.code == Code::DocumentDynamic);
        if !errors.is_empty() {
            return Err(resolve_reports(&self.modules, errors));
        }
        if !reasons.is_empty() {
            return Ok(DocumentOutcome::Render(resolve_reports(&self.modules, reasons)));
        }
        let (root_id, _) = self.root.expect("a document without errors has a root");
        let files = self.modules.iter().skip(1).map(|module| module.file.to_string()).collect();
        Ok(DocumentOutcome::Html(Document { html: self.html, root_id, files }))
    }

    fn check_root(&mut self) {
        if self.slots == 0 {
            self.report_in(
                0,
                Code::DocumentRoot,
                self.root_span,
                "never renders its `children`; place them in the element the app renders into",
            );
        }
        let Some((id, own)) = self.root.clone() else { return };
        let duplicates: Vec<Span> = self
            .ids
            .iter()
            .filter(|(other, span)| *other == id && Some(*span) != own)
            .map(|(_, span)| *span)
            .collect();
        for duplicate in duplicates {
            self.misplaced_root(duplicate, "gives another element the root element's `id`");
        }
    }
}

fn static_value(expression: &Expression<'_>) -> Option<StaticValue> {
    match unparenthesized(expression) {
        Expression::StringLiteral(literal) => Some(StaticValue::Text(literal.value.to_string())),
        Expression::NumericLiteral(literal) => Some(StaticValue::Text(literal.value.to_string())),
        Expression::BooleanLiteral(literal) => Some(StaticValue::Flag(literal.value)),
        Expression::TemplateLiteral(literal) if literal.expressions.is_empty() => literal
            .quasis
            .first()?
            .value
            .cooked
            .as_ref()
            .map(|cooked| StaticValue::Text(cooked.to_string())),
        _ => None,
    }
}

fn own_property<'a>(object: &'a Expression<'a>, key: &str) -> Option<&'a Expression<'a>> {
    let Expression::ObjectExpression(object) = unparenthesized(object) else { return None };
    let mut found = None;
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
        if property.computed || property.key.static_name().is_none_or(|name| name != key) {
            continue;
        }
        found = (property.kind == PropertyKind::Init && !property.method && key != "__proto__")
            .then_some(&property.value);
    }
    found
}

fn unparenthesized<'b, 'a>(mut expression: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(inner) = expression {
        expression = &inner.expression;
    }
    expression
}

fn tag_name<'a>(name: &'a JSXElementName<'a>) -> Option<&'a str> {
    match name {
        JSXElementName::Identifier(identifier) => Some(identifier.name.as_str()),
        _ => None,
    }
}

fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(container) => container.expression.as_expression().is_some(),
        _ => true,
    }
}

fn resolve_reports(modules: &[Module<'_>], reports: Vec<(usize, Report)>) -> Vec<Diagnostic> {
    let mut by_module: Vec<Vec<Report>> = modules.iter().map(|_| Vec::new()).collect();
    for (module, report) in reports {
        by_module[module].push(report);
    }
    modules
        .iter()
        .zip(by_module)
        .filter(|(_, reports)| !reports.is_empty())
        .flat_map(|(module, reports)| diagnostic::resolve(reports, module.source, module.file))
        .collect()
}
