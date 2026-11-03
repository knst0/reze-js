mod api;
mod build;
mod data;
mod holes;

use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::{
    ArrowFunctionBody, ArrowFunctionExpression, BindingPattern, Declaration,
    ExportDefaultDeclarationKind, Expression, Function, FunctionBody, IdentifierReference,
    ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

pub use api::Api;
pub use data::{Endpoint, EndpointKind, field_type, restore};

use crate::diagnostic::{Code, Report};
use crate::project::Module;
use crate::routes::RouteTable;
use crate::view::{ComponentRef, Content};
use crate::{Options, Target};

pub enum ImportSource {
    Module(usize),
    Package(String),
}

pub struct Import {
    pub source: ImportSource,
    /// The imported name, `default`, or `*` for a namespace.
    pub name: String,
}

pub enum Export {
    Local(SymbolId),
    /// `export default function () {}`.
    Component(usize),
    Reexport {
        module: usize,
        name: String,
    },
}

#[derive(Clone, Copy)]
pub enum ComponentFunction<'a> {
    Function(&'a Function<'a>),
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> ComponentFunction<'a> {
    pub fn span(self) -> Span {
        match self {
            ComponentFunction::Function(f) => f.span,
            ComponentFunction::Arrow(f) => f.span,
        }
    }

    /// The block body; `None` for an arrow with an expression body.
    pub fn body(self) -> Option<&'a FunctionBody<'a>> {
        match self {
            ComponentFunction::Function(f) => f.body.as_deref(),
            ComponentFunction::Arrow(f) => match &f.body {
                ArrowFunctionBody::FunctionBody(body) => Some(body),
                _ => None,
            },
        }
    }

    /// The expression the function returns: an arrow's expression body, or the last statement's
    /// `return`.
    pub fn returned(self) -> Option<&'a Expression<'a>> {
        if let ComponentFunction::Arrow(f) = self
            && let Some(expression) = f.body.as_expression()
        {
            return Some(expression);
        }
        match self.body()?.statements.last()? {
            Statement::ReturnStatement(statement) => statement.argument.as_ref(),
            _ => None,
        }
    }

    pub fn params(self) -> &'a oxc_ast::ast::FormalParameters<'a> {
        match self {
            ComponentFunction::Function(f) => &f.params,
            ComponentFunction::Arrow(f) => &f.params,
        }
    }
}

pub struct Component<'a> {
    pub name: String,
    pub symbol: Option<SymbolId>,
    pub function: ComponentFunction<'a>,
    /// The returned JSX.
    pub returned: &'a Expression<'a>,
    pub template: Option<Content<'a>>,
    /// The type of the props, with `data` typed by the loader of a route component.
    pub props: Option<data::Ty>,
}

#[derive(Default)]
pub struct ModuleFacts<'a> {
    pub imports: HashMap<SymbolId, Import>,
    pub exports: HashMap<String, Export>,
    pub components: Vec<Component<'a>>,
    pub apis: HashMap<SymbolId, Api>,
    /// JSX in value positions outside templates, such as the return of a render prop, by span
    /// start.
    pub values: HashMap<u32, Content<'a>>,
    /// Exported loaders and mutations.
    pub endpoints: Vec<Endpoint>,
}

pub struct Analysis<'a> {
    pub facts: Vec<ModuleFacts<'a>>,
    pub reports: Vec<Vec<Report>>,
}

pub struct Context<'m, 'a> {
    pub modules: &'m [Module<'a>],
    pub facts: Vec<ModuleFacts<'a>>,
}

impl<'a> Context<'_, 'a> {
    pub fn symbol(&self, module: usize, reference: &IdentifierReference<'a>) -> Option<SymbolId> {
        let id = reference.reference_id.get()?;
        self.modules[module].semantic.scoping().get_reference(id).symbol_id()
    }

    /// The framework call that initializes the variable `reference` names: `useSearchParams` for
    /// `const params = useSearchParams()`.
    pub fn initializer_api(
        &self,
        module: usize,
        reference: &IdentifierReference<'a>,
    ) -> Option<Api> {
        let semantic = &self.modules[module].semantic;
        let declaration = semantic.scoping().symbol_declaration(self.symbol(module, reference)?);
        let AstKind::VariableDeclarator(declarator) = semantic.nodes().get_node(declaration).kind()
        else {
            return None;
        };
        match declarator.init.as_ref().map(Expression::without_parentheses) {
            Some(Expression::CallExpression(call)) => self.api(module, &call.callee),
            _ => None,
        }
    }

    pub fn api(&self, module: usize, expression: &Expression<'a>) -> Option<Api> {
        match expression.without_parentheses() {
            Expression::Identifier(reference) => {
                self.facts[module].apis.get(&self.symbol(module, reference)?).copied()
            }
            _ => None,
        }
    }

    pub fn api_of_reference(
        &self,
        module: usize,
        reference: &IdentifierReference<'a>,
    ) -> Option<Api> {
        self.facts[module].apis.get(&self.symbol(module, reference)?).copied()
    }

    /// The component a local binding names, following imports.
    pub fn component(&self, module: usize, symbol: SymbolId) -> Option<ComponentRef> {
        self.component_at(module, symbol, 0)
    }

    fn component_at(&self, module: usize, symbol: SymbolId, depth: u32) -> Option<ComponentRef> {
        if depth > 32 {
            return None;
        }
        let facts = &self.facts[module];
        if let Some(index) = facts.components.iter().position(|c| c.symbol == Some(symbol)) {
            return Some(ComponentRef { module, index });
        }
        let import = facts.imports.get(&symbol)?;
        match &import.source {
            ImportSource::Module(target) => self.exported_at(*target, &import.name, depth + 1),
            ImportSource::Package(_) => None,
        }
    }

    pub fn exported(&self, module: usize, name: &str) -> Option<ComponentRef> {
        self.exported_at(module, name, 0)
    }

    fn exported_at(&self, module: usize, name: &str, depth: u32) -> Option<ComponentRef> {
        if depth > 32 {
            return None;
        }
        match self.facts[module].exports.get(name)? {
            Export::Local(symbol) => self.component_at(module, *symbol, depth + 1),
            Export::Component(index) => Some(ComponentRef { module, index: *index }),
            Export::Reexport { module: target, name } => self.exported_at(*target, name, depth + 1),
        }
    }
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    name.name().to_string()
}

fn is_component_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

fn returned_jsx<'a>(function: ComponentFunction<'a>) -> Option<&'a Expression<'a>> {
    let expression = function.returned()?;
    matches!(
        expression.without_parentheses(),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
    .then_some(expression)
}

/// `name` must be capitalized unless the function is the default export.
fn component<'a>(
    name: &str,
    symbol: Option<SymbolId>,
    function: ComponentFunction<'a>,
    default_export: bool,
) -> Option<Component<'a>> {
    if !default_export && !is_component_name(name) {
        return None;
    }
    let returned = returned_jsx(function)?;
    Some(Component {
        name: name.to_string(),
        symbol,
        function,
        returned,
        template: None,
        props: None,
    })
}

fn collect_facts<'a>(modules: &[Module<'a>], module: usize) -> ModuleFacts<'a> {
    let current = &modules[module];
    let mut facts = ModuleFacts::default();
    let symbol_of = |id: &oxc_ast::ast::BindingIdentifier<'a>| id.symbol_id.get();
    for statement in &current.program.body {
        match statement {
            Statement::ImportDeclaration(decl) => {
                let package = decl.source.value.as_str();
                let source = match current.resolved.get(package) {
                    Some(&target) => ImportSource::Module(target),
                    None => ImportSource::Package(package.to_string()),
                };
                for specifier in decl.specifiers.iter().flatten() {
                    let (local, name) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            (&s.local, export_name(&s.imported))
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            (&s.local, "default".to_string())
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            (&s.local, "*".to_string())
                        }
                    };
                    let Some(symbol) = symbol_of(local) else { continue };
                    if let ImportSource::Package(package) = &source
                        && let Some(api) = Api::from_import(package, &name)
                    {
                        facts.apis.insert(symbol, api);
                    }
                    let source = match &source {
                        ImportSource::Module(m) => ImportSource::Module(*m),
                        ImportSource::Package(p) => ImportSource::Package(p.clone()),
                    };
                    facts.imports.insert(symbol, Import { source, name });
                }
            }
            Statement::ExportDeclaration(decl) => {
                collect_declaration(&decl.declaration, &mut facts, true);
            }
            Statement::ExportNamedDeclaration(decl) => {
                for specifier in &decl.specifiers {
                    if let ModuleExportName::IdentifierReference(reference) = &specifier.local
                        && let Some(id) = reference.reference_id.get()
                        && let Some(symbol) =
                            current.semantic.scoping().get_reference(id).symbol_id()
                    {
                        facts
                            .exports
                            .insert(export_name(&specifier.exported), Export::Local(symbol));
                    }
                }
            }
            Statement::ExportFromDeclaration(decl) => {
                let Some(&module) = current.resolved.get(decl.source.value.as_str()) else {
                    continue;
                };
                for specifier in &decl.specifiers {
                    facts.exports.insert(
                        export_name(&specifier.exported),
                        Export::Reexport { module, name: export_name(&specifier.local) },
                    );
                }
            }
            Statement::ExportDefaultDeclaration(decl) => match &decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    let symbol = function.id.as_ref().and_then(symbol_of);
                    let name = function.id.as_ref().map_or("default", |id| id.name.as_str());
                    if let Some(c) =
                        component(name, symbol, ComponentFunction::Function(function), true)
                    {
                        facts.components.push(c);
                        match symbol {
                            Some(symbol) => {
                                facts.exports.insert("default".into(), Export::Local(symbol));
                            }
                            None => {
                                let index = facts.components.len() - 1;
                                facts.exports.insert("default".into(), Export::Component(index));
                            }
                        }
                    }
                }
                ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                    if let Some(c) =
                        component("default", None, ComponentFunction::Arrow(arrow), true)
                    {
                        facts.components.push(c);
                        let index = facts.components.len() - 1;
                        facts.exports.insert("default".into(), Export::Component(index));
                    }
                }
                ExportDefaultDeclarationKind::Identifier(reference) => {
                    if let Some(id) = reference.reference_id.get()
                        && let Some(symbol) =
                            current.semantic.scoping().get_reference(id).symbol_id()
                    {
                        facts.exports.insert("default".into(), Export::Local(symbol));
                    }
                }
                _ => {}
            },
            Statement::FunctionDeclaration(_) | Statement::VariableDeclaration(_) => {
                if let Some(declaration) = statement.as_declaration() {
                    collect_declaration(declaration, &mut facts, false);
                }
            }
            _ => {}
        }
    }
    facts
}

fn collect_declaration<'a>(
    declaration: &'a Declaration<'a>,
    facts: &mut ModuleFacts<'a>,
    export: bool,
) {
    match declaration {
        Declaration::FunctionDeclaration(function) => {
            let Some(id) = &function.id else { return };
            let symbol = id.symbol_id.get();
            if export && let Some(symbol) = symbol {
                facts.exports.insert(id.name.to_string(), Export::Local(symbol));
            }
            if let Some(c) =
                component(&id.name, symbol, ComponentFunction::Function(function), false)
            {
                facts.components.push(c);
            }
        }
        Declaration::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
                let symbol = id.symbol_id.get();
                if export && let Some(symbol) = symbol {
                    facts.exports.insert(id.name.to_string(), Export::Local(symbol));
                }
                let function = match declarator.init.as_ref().map(Expression::without_parentheses) {
                    Some(Expression::ArrowFunctionExpression(arrow)) => {
                        ComponentFunction::Arrow(arrow)
                    }
                    Some(Expression::FunctionExpression(function)) => {
                        ComponentFunction::Function(function)
                    }
                    _ => continue,
                };
                if let Some(c) = component(&id.name, symbol, function, false) {
                    facts.components.push(c);
                }
            }
        }
        Declaration::ClassDeclaration(class) => {
            if export
                && let Some(id) = &class.id
                && let Some(symbol) = id.symbol_id.get()
            {
                facts.exports.insert(id.name.to_string(), Export::Local(symbol));
            }
        }
        _ => {}
    }
}

pub fn run<'a>(modules: &[Module<'a>], routes: &RouteTable, options: &Options) -> Analysis<'a> {
    let facts = (0..modules.len()).map(|m| collect_facts(modules, m)).collect();
    let mut cx = Context { modules, facts };
    let mut reports: Vec<Vec<Report>> = modules.iter().map(|_| Vec::new()).collect();

    for module in 0..modules.len() {
        let mut consumed = HashSet::new();
        let mut values = HashMap::new();
        let templates: Vec<_> = (0..cx.facts[module].components.len())
            .map(|index| {
                let component = &cx.facts[module].components[index];
                let mut builder = build::Builder::new(
                    &cx,
                    module,
                    &mut reports[module],
                    &mut consumed,
                    &mut values,
                    component.name.clone(),
                );
                check_returns(component, builder.reports, builder.consumed);
                builder.template(component.returned)
            })
            .collect();
        for (component, template) in cx.facts[module].components.iter_mut().zip(templates) {
            component.template = template;
        }
        cx.facts[module].values = values;
        check_tag_values(&cx, module, &mut reports[module]);
        let mut outside = OutsideTemplate { consumed: &consumed, reports: &mut reports[module] };
        outside.visit_program(modules[module].program);
    }

    for node in &routes.nodes {
        let Some(module) = modules.iter().position(|m| m.path == node.file) else { continue };
        if cx.exported(module, "default").is_none() {
            reports[module].push(Report::new(Code::RouteDefaultExport, Span::empty(0)));
        }
    }

    for module in 0..modules.len() {
        let route = routes.nodes.iter().find(|node| node.file == modules[module].path);
        let pattern = route.map(|node| {
            (node.pattern.as_str(), node.segments.iter().filter_map(|s| s.name()).collect())
        });
        cx.facts[module].endpoints = data::endpoints(&cx, module, pattern, &mut reports[module]);
        if options.target == Target::Spa {
            for endpoint in &cx.facts[module].endpoints {
                reports[module].push(
                    Report::new(Code::ServerInSpa, endpoint.call).arg("name", &endpoint.export),
                );
            }
        }
    }
    for module in 0..modules.len() {
        let props: Vec<_> = (0..cx.facts[module].components.len())
            .map(|index| data::props_type(&cx, module, index))
            .collect();
        for (component, props) in cx.facts[module].components.iter_mut().zip(props) {
            component.props = props;
        }
    }
    check_mutation_ids(&cx, &mut reports);
    for (module, reports) in reports.iter_mut().enumerate() {
        data::check_forms(&cx, module, reports);
    }

    Analysis { facts: cx.facts, reports }
}

fn check_mutation_ids(cx: &Context<'_, '_>, reports: &mut [Vec<Report>]) {
    let mut seen: HashMap<&str, String> = HashMap::new();
    for (module, facts) in cx.facts.iter().enumerate() {
        for endpoint in &facts.endpoints {
            let Some(id) = &endpoint.id else { continue };
            let name = format!("{}#{}", cx.modules[module].path, endpoint.export);
            if let Some(other) = seen.insert(id, name.clone()) {
                reports[module].push(
                    Report::new(Code::MutationIdCollision, endpoint.call)
                        .arg("name", name)
                        .arg("other", other)
                        .arg("id", id.clone()),
                );
            }
        }
    }
}

fn check_tag_values(cx: &Context<'_, '_>, module: usize, reports: &mut Vec<Report>) {
    let semantic = &cx.modules[module].semantic;
    let scoping = semantic.scoping();
    for (&symbol, api) in &cx.facts[module].apis {
        if !api.is_tag() {
            continue;
        }
        for reference in scoping.get_resolved_references(symbol) {
            if !reference.is_value() {
                continue;
            }
            let node = reference.node_id();
            if matches!(
                semantic.nodes().parent_kind(node),
                AstKind::JSXOpeningElement(_) | AstKind::JSXClosingElement(_)
            ) {
                continue;
            }
            let span = semantic.nodes().get_node(node).span();
            reports.push(
                Report::new(Code::ControlFlowAsValue, span)
                    .arg("name", scoping.symbol_name(symbol)),
            );
        }
    }
}

fn check_returns(
    component: &Component<'_>,
    reports: &mut Vec<Report>,
    consumed: &mut HashSet<u32>,
) {
    let Some(body) = component.function.body() else { return };
    let mut returns = Returns { spans: Vec::new(), consumed };
    returns.visit_function_body(body);
    let last = returns.spans.len().saturating_sub(1);
    for span in returns.spans.into_iter().take(last) {
        reports.push(
            Report::new(Code::ComponentReturn, span)
                .label(component.returned.span(), "the template"),
        );
    }
}

/// Return statements of a component body; their JSX counts as reported.
struct Returns<'c> {
    spans: Vec<Span>,
    consumed: &'c mut HashSet<u32>,
}

impl<'a> Visit<'a> for Returns<'_> {
    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        self.spans.push(it.span);
        if let Some(argument) = &it.argument {
            holes::consume_expression(self.consumed, argument);
        }
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

struct OutsideTemplate<'r> {
    consumed: &'r HashSet<u32>,
    reports: &'r mut Vec<Report>,
}

impl<'a> Visit<'a> for OutsideTemplate<'_> {
    fn visit_jsx_element(&mut self, it: &oxc_ast::ast::JSXElement<'a>) {
        if self.consumed.contains(&it.span.start) {
            walk::walk_jsx_element(self, it);
        } else {
            self.reports.push(Report::new(Code::JsxOutsideTemplate, it.span));
        }
    }

    fn visit_jsx_fragment(&mut self, it: &oxc_ast::ast::JSXFragment<'a>) {
        if self.consumed.contains(&it.span.start) {
            walk::walk_jsx_fragment(self, it);
        } else {
            self.reports.push(Report::new(Code::JsxOutsideTemplate, it.span));
        }
    }
}
