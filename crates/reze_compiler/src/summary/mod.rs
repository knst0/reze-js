//! `ModuleSummary`: everything `link` needs to know about one module, without its AST (SPEC §15.2,
//! §15.4). Independent of target and `optimize`; built from any dialect, JSX or not.

mod inert;
mod slots;

use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{AstNodes, Scoping, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::symbol::SymbolId;
use serde::{Deserialize, Serialize};

use crate::analyze::{
    self, computed, is_foldable_signal_shape, is_runtime_module, static_text_alone,
};
use crate::diagnostic::{self, Code, Diagnostic, Report};
use crate::facts::{self, ImportRef};
use crate::lower::{self, has_jsx};
use crate::namer::Namer;
use crate::usage::{self, Context};
use crate::{Target, emit};

pub use inert::{Boundary, Dep, DepKind, Violation, island_load_mode};
pub use slots::SlotUses;

pub struct SummaryOptions {
    /// Module the runtime is imported from, next to the built-in runtime modules (SPEC §8.0).
    pub module_name: String,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self { module_name: "reze-js".to_string() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModuleSummary {
    pub version: String,
    pub source_hash: String,
    /// Static imports, re-exports and literal `import()`, in order of first appearance.
    pub specifiers: Vec<String>,
    /// Indexes into `specifiers` of runtime modules: never resolved, recognized by name.
    pub runtime_specifiers: Vec<u32>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub dynamic: Dynamic,
    /// Every reference to a top-level binding or an import, in source order.
    pub uses: Vec<Use>,
    /// Starts of calls `x()` of imports standing in a reactive JSX expression (bind, insert,
    /// getter prop), where an inlined computed keeps its semantics (§8 O4, §16.5).
    pub reactive_reads: Vec<u32>,
    /// Top-level computeds whose body can move to another module as text (§16.5).
    pub computeds: Vec<ComputedSummary>,
    /// Top-level `const [a, b] = f(…)` and `const a = f(…)`: signal, computed and store
    /// candidates once `f` resolves to a primitive.
    pub declarations: Vec<CallDeclaration>,
    pub components: Vec<ComponentSummary>,
    pub constants: Vec<ConstantSummary>,
    pub roots: Vec<RootSummary>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Import {
    /// Start of the local binding identifier.
    pub binding: u32,
    pub specifier: u32,
    pub name: ImportName,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ImportName {
    Named(String),
    Default,
    Namespace,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Export {
    /// `export const x`, `export function x`, `export { x as name }`, `export default x`;
    /// `binding` is a top-level binding or an import binding.
    Local { name: String, binding: u32 },
    /// `export { imported as name } from "…"`.
    Reexport { name: String, specifier: u32, imported: String },
    /// `export * from "…"`.
    Star { specifier: u32 },
    /// `export * as name from "…"`.
    StarAs { name: String, specifier: u32 },
    /// `export default <expression>`: not a binding.
    DefaultExpression,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Dynamic {
    /// Specifiers of literal `import()`.
    pub imports: Vec<u32>,
    /// Literal `import.meta.glob` patterns, relative to the module.
    pub globs: Vec<String>,
    /// A non-literal `import()` or `import.meta.glob`: every module may be loaded.
    pub opens_all: bool,
}

/// A binding of this module: top-level, an import, or `member` of a namespace import.
pub type Ref = ImportRef;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Use {
    pub target: Ref,
    pub class: UseClass,
    pub start: u32,
    pub end: u32,
    /// The component or constant (binding start) whose inert position holds this use.
    pub site: Option<u32>,
    /// The top-level component (binding start) the use is in.
    pub component: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UseClass {
    /// `x()`: no arguments, not optional, no type arguments.
    Call0,
    /// `x.k₁…kₙ` read as a value.
    Path(Vec<String>),
    Tag,
    /// `x((d) => …)` in the shape of a store setter call, with the draft paths it touches.
    StoreSet(Vec<DraftUse>),
    /// An array-leaf use: an index tail, `.length`, `each`, spread or a method call (§16.6).
    Array(ArrayUse),
    Other,
}

/// `state.a` with an index tail, a `.length`, or in an array position.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArrayUse {
    /// Static keys before the first index.
    pub path: Vec<String>,
    /// Index and key steps after them.
    pub tail: Vec<ArrayTail>,
    pub site: ArraySite,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ArrayTail {
    /// `[i]`: any index expression without JSX.
    Index,
    /// `.length` or a static key after an index.
    Key(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ArraySite {
    /// Read as a value.
    Read,
    /// `each={…}` of a JSX element with this tag.
    Each(String),
    /// `[...…]` of an array literal.
    Spread,
    /// Callee of a call; `statement` when the call discards its value.
    Call { statement: bool },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DraftUse {
    pub path: Vec<String>,
    /// The path ends at an array leaf, read or written through `[i]`.
    pub index: bool,
    /// Static keys after the index.
    pub tail: Vec<String>,
    /// A statement-position method call on the array leaf at `path` (§16.6).
    pub method: Option<String>,
    /// A write of an object literal to the non-leaf form at `path`, with the literal's
    /// leaf-relative key paths (§16.6).
    pub form: Option<Vec<Vec<String>>>,
    /// A static read in an array position (`each`, spread): whole array leaves need it.
    pub array_site: bool,
    pub is_write: bool,
}

/// A leaf of a store form: its path from the root and whether it holds an array (§16.6).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LeafShape {
    pub path: Vec<String>,
    pub is_array: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CallDeclaration {
    pub callee: Ref,
    pub start: u32,
    pub end: u32,
    pub kind: DeclarationKind,
}

/// `const d = computed(() => expr)`: `expr` is plain expression syntax whose every identifier
/// names a top-level declaration of this module.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ComputedSummary {
    pub binding: u32,
    pub body: String,
    pub references: Vec<BodyReference>,
}

/// An identifier of a computed body, by offsets into the body, and the declaration it names.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BodyReference {
    pub start: u32,
    pub end: u32,
    pub binding: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DeclarationKind {
    /// `const [first, second] = f(…)`.
    Pair {
        first: u32,
        second: Option<u32>,
        /// O3 shape rules hold if `f` is `signal` (SPEC §8).
        is_foldable: bool,
        /// Text a literal initializer renders as (§7.10).
        literal: Option<String>,
        /// Leaves of the initializer when it is a store form (§15.6) and the declaration has
        /// the store shape.
        store_leaves: Option<Vec<LeafShape>>,
    },
    /// `const name = f(…)`.
    Single { binding: u32 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ComponentSummary {
    pub binding: u32,
    pub name: String,
    /// The whole function.
    pub start: u32,
    pub end: u32,
    /// First local violation of the static rules (§15.8), if any.
    pub violation: Option<Violation>,
    /// Facts the component needs from the program to be static, in source order.
    pub deps: Vec<Dep>,
    /// Runtime helpers the `hydrate` target emits for its JSX (§15.11).
    pub helpers: Vec<String>,
    /// Props used other than as JSX inserts, which it cannot take as island slots (§16.4).
    pub slot_uses: SlotUses,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConstantSummary {
    pub binding: u32,
    pub violation: Option<Violation>,
    pub deps: Vec<Dep>,
    pub is_array_literal: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RootSummary {
    /// Start of the `renderToString(…)` / `hydrate(…)` call.
    pub call: u32,
    pub end: u32,
    pub callee: Ref,
    pub argument_count: u32,
    /// The component `R` of `() => <R …/>`.
    pub component: Ref,
    /// Every attribute of `<R>` is a literal JSON form.
    pub has_json_attributes: bool,
}

pub fn summarize(
    source: &str,
    filename: &str,
    options: &SummaryOptions,
) -> Result<ModuleSummary, Vec<Diagnostic>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        let reports = parsed
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.labels.first().map_or(Span::empty(0), |label| {
                    let start = label.offset();
                    Span::new(start, start + label.len())
                });
                Report::new(Code::ParseError, span, d.message.to_string())
            })
            .collect();
        return Err(diagnostic::resolve(reports, source, filename));
    }
    let program = allocator.alloc(parsed.program);
    let (scoping, nodes) = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(program)
        .semantic
        .into_scoping_and_nodes();

    let mut builder = Builder {
        source,
        module_name: &options.module_name,
        scoping: &scoping,
        nodes: &nodes,
        specifier_index: HashMap::new(),
        summary: ModuleSummary {
            version: facts::VERSION.to_string(),
            source_hash: facts::source_hash(source),
            specifiers: Vec::new(),
            runtime_specifiers: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            dynamic: Dynamic::default(),
            uses: Vec::new(),
            reactive_reads: Vec::new(),
            computeds: Vec::new(),
            declarations: Vec::new(),
            components: Vec::new(),
            constants: Vec::new(),
            roots: Vec::new(),
        },
    };
    builder.visit_program(program);
    builder.top_level(program);
    let top_level = builder.top_level_bindings();
    let sites = builder.inert(program, &top_level);
    builder.roots(program, &top_level);
    builder.uses(&top_level, &sites);
    builder.helpers(&allocator, program, source_type);
    Ok(builder.summary)
}

struct Builder<'s, 'a> {
    source: &'a str,
    module_name: &'s str,
    scoping: &'s Scoping,
    nodes: &'s AstNodes<'a>,
    specifier_index: HashMap<String, u32>,
    summary: ModuleSummary,
}

/// Top-level bindings (locals and imports) by symbol, with the namespace imports.
pub(crate) struct TopLevel {
    pub starts: HashMap<SymbolId, u32>,
    pub namespaces: std::collections::HashSet<SymbolId>,
}

impl<'a> Builder<'_, 'a> {
    fn specifier(&mut self, specifier: &str) -> u32 {
        if let Some(&index) = self.specifier_index.get(specifier) {
            return index;
        }
        let index = self.summary.specifiers.len() as u32;
        self.summary.specifiers.push(specifier.to_string());
        self.specifier_index.insert(specifier.to_string(), index);
        if is_runtime_module(specifier, self.module_name) {
            self.summary.runtime_specifiers.push(index);
        }
        index
    }

    fn top_level_bindings(&self) -> TopLevel {
        let root = self.scoping.root_scope_id();
        let mut starts = HashMap::new();
        let mut namespaces = std::collections::HashSet::new();
        for (_, &symbol) in self.scoping.get_bindings(root).iter() {
            starts.insert(symbol, self.scoping.symbol_span(symbol).start);
        }
        for import in &self.summary.imports {
            if import.name == ImportName::Namespace
                && let Some((&symbol, _)) =
                    starts.iter().find(|(_, start)| **start == import.binding)
            {
                namespaces.insert(symbol);
            }
        }
        TopLevel { starts, namespaces }
    }

    fn reference_of(
        &self,
        id: &IdentifierReference<'_>,
        top_level: &TopLevel,
    ) -> Option<(SymbolId, u32)> {
        let symbol = self.scoping.get_reference(id.reference_id.get()?).symbol_id()?;
        top_level.starts.get(&symbol).map(|&start| (symbol, start))
    }

    /// `x` or `ns.x` as a reference to a binding of this module.
    fn callee_ref(&self, callee: &Expression<'_>, top_level: &TopLevel) -> Option<Ref> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => {
                let (symbol, binding) = self.reference_of(id, top_level)?;
                (!top_level.namespaces.contains(&symbol)).then_some(Ref { binding, member: None })
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let (symbol, binding) = self.reference_of(namespace, top_level)?;
                top_level
                    .namespaces
                    .contains(&symbol)
                    .then(|| Ref { binding, member: Some(member.property.name.to_string()) })
            }
            _ => None,
        }
    }

    fn top_level(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::ExportDeclaration(export) => {
                    for (name, binding) in declared_names(&export.declaration) {
                        self.summary.exports.push(Export::Local { name, binding });
                    }
                }
                Statement::ExportNamedDeclaration(export) => {
                    if export.export_kind.is_type() {
                        continue;
                    }
                    for specifier in &export.specifiers {
                        if specifier.export_kind.is_type() {
                            continue;
                        }
                        let ModuleExportName::IdentifierReference(local) = &specifier.local else {
                            continue;
                        };
                        let Some(symbol) = local
                            .reference_id
                            .get()
                            .and_then(|r| self.scoping.get_reference(r).symbol_id())
                        else {
                            continue;
                        };
                        self.summary.exports.push(Export::Local {
                            name: export_name(&specifier.exported),
                            binding: self.scoping.symbol_span(symbol).start,
                        });
                    }
                }
                Statement::ExportFromDeclaration(export) => {
                    if export.export_kind.is_type() {
                        continue;
                    }
                    let specifier = self.specifier_index[export.source.value.as_str()];
                    for item in &export.specifiers {
                        if item.export_kind.is_type() {
                            continue;
                        }
                        self.summary.exports.push(Export::Reexport {
                            name: export_name(&item.exported),
                            specifier,
                            imported: export_name(&item.local),
                        });
                    }
                }
                Statement::ExportAllDeclaration(export) => {
                    if export.export_kind.is_type() {
                        continue;
                    }
                    let specifier = self.specifier_index[export.source.value.as_str()];
                    self.summary.exports.push(match &export.exported {
                        Some(name) => Export::StarAs { name: export_name(name), specifier },
                        None => Export::Star { specifier },
                    });
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let binding = match &export.declaration {
                        ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                            f.id.as_ref().map(|id| id.span.start)
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                            c.id.as_ref().map(|id| id.span.start)
                        }
                        ExportDefaultDeclarationKind::Identifier(id) => id
                            .reference_id
                            .get()
                            .and_then(|r| self.scoping.get_reference(r).symbol_id())
                            .map(|symbol| self.scoping.symbol_span(symbol).start),
                        _ => None,
                    };
                    self.summary.exports.push(match binding {
                        Some(binding) => Export::Local { name: "default".to_string(), binding },
                        None => Export::DefaultExpression,
                    });
                }
                _ => {}
            }
        }
    }

    /// Components, constants and primitive-call declarations at the top level, with the inert
    /// checks of §15.8; returns the reference starts in inert positions with their owner.
    fn inert(&mut self, program: &Program<'a>, top_level: &TopLevel) -> HashMap<u32, u32> {
        let mut sites = HashMap::new();
        for statement in &program.body {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                Statement::VariableDeclaration(_) | Statement::FunctionDeclaration(_) => {
                    statement.as_declaration()
                }
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(f) =
                        &export.declaration
                        && let Some(id) = &f.id
                    {
                        self.function_component(id, f, top_level, &mut sites);
                    }
                    None
                }
                _ => None,
            };
            match declaration {
                Some(Declaration::FunctionDeclaration(f)) => {
                    if let Some(id) = &f.id {
                        self.function_component(id, f, top_level, &mut sites);
                    }
                }
                Some(Declaration::VariableDeclaration(variables)) => {
                    let is_single = variables.declarations.len() == 1;
                    if is_single
                        && variables.kind == VariableDeclarationKind::Const
                        && !variables.declare
                    {
                        self.portable_computed(&variables.declarations[0], top_level);
                    }
                    for declarator in &variables.declarations {
                        self.variable(declarator, variables.kind, is_single, top_level, &mut sites);
                    }
                }
                _ => {}
            }
        }
        sites
    }

    fn portable_computed(&mut self, declarator: &VariableDeclarator<'a>, top_level: &TopLevel) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else { return };
        let Some(expr) = computed::body(declarator) else { return };
        let Some(references) = computed::portable_references(expr, self.scoping) else { return };
        let body_start = expr.span().start;
        let mut body_references = Vec::with_capacity(references.len());
        for (span, symbol) in references {
            let Some(&binding) = top_level.starts.get(&symbol) else { return };
            if self.summary.imports.iter().any(|i| i.binding == binding) {
                return;
            }
            body_references.push(BodyReference {
                start: span.start - body_start,
                end: span.end - body_start,
                binding,
            });
        }
        body_references.sort_unstable_by_key(|r| r.start);
        self.summary.computeds.push(ComputedSummary {
            binding: id.span.start,
            body: expr.span().source_text(self.source).to_string(),
            references: body_references,
        });
    }

    fn function_component(
        &mut self,
        id: &BindingIdentifier<'a>,
        f: &Function<'a>,
        top_level: &TopLevel,
        sites: &mut HashMap<u32, u32>,
    ) {
        let Some(body) = &f.body else { return };
        if !has_jsx(|c| c.visit_function_body(body)) {
            return;
        }
        let checked = inert::Checker::new(self.source, self.scoping, top_level).function(
            &f.params,
            inert::Body::Block(body),
            f.r#async,
            f.generator,
        );
        let slot_uses = slots::slot_uses(
            self.source,
            self.scoping,
            self.nodes,
            f.node_id(),
            &f.params,
            Some(body),
        );
        self.component(id, f.span, checked, slot_uses, sites);
    }

    fn component(
        &mut self,
        id: &BindingIdentifier<'a>,
        span: Span,
        checked: inert::Checked,
        slot_uses: SlotUses,
        sites: &mut HashMap<u32, u32>,
    ) {
        for &site in &checked.sites {
            sites.insert(site, id.span.start);
        }
        self.summary.components.push(ComponentSummary {
            binding: id.span.start,
            name: id.name.to_string(),
            start: span.start,
            end: span.end,
            violation: checked.violation,
            deps: checked.deps,
            helpers: Vec::new(),
            slot_uses,
        });
    }

    fn variable(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        kind: VariableDeclarationKind,
        is_single: bool,
        top_level: &TopLevel,
        sites: &mut HashMap<u32, u32>,
    ) {
        let Some(init) = &declarator.init else { return };
        if let Some(Expression::CallExpression(call)) = Some(init.without_parentheses())
            && let Some(callee) = self.callee_ref(&call.callee, top_level)
        {
            self.declaration(declarator, call, callee, is_single);
        }
        if kind != VariableDeclarationKind::Const {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else { return };
        let checker = inert::Checker::new(self.source, self.scoping, top_level);
        match init.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow)
                if has_jsx(|c| c.visit_arrow_function_body(&arrow.body)) =>
            {
                let body = match &arrow.body {
                    ArrowFunctionBody::FunctionBody(block) => inert::Body::Arrow(block),
                    expression => inert::Body::Expression(
                        expression
                            .as_expression()
                            .expect("an arrow body is a block or an expression"),
                    ),
                };
                let checked = checker.function(&arrow.params, body, arrow.r#async, false);
                let slot_uses = slots::slot_uses(
                    self.source,
                    self.scoping,
                    self.nodes,
                    arrow.node_id(),
                    &arrow.params,
                    None,
                );
                self.component(id, arrow.span, checked, slot_uses, sites);
            }
            Expression::FunctionExpression(f)
                if f.body.as_ref().is_some_and(|b| has_jsx(|c| c.visit_function_body(b))) =>
            {
                let body = f.body.as_ref().expect("checked above");
                let checked =
                    checker.function(&f.params, inert::Body::Block(body), f.r#async, f.generator);
                let slot_uses = slots::slot_uses(
                    self.source,
                    self.scoping,
                    self.nodes,
                    f.node_id(),
                    &f.params,
                    Some(body),
                );
                self.component(id, f.span, checked, slot_uses, sites);
            }
            _ => {
                let checked = checker.constant(init);
                for &site in &checked.sites {
                    sites.insert(site, id.span.start);
                }
                self.summary.constants.push(ConstantSummary {
                    binding: id.span.start,
                    violation: checked.violation,
                    deps: checked.deps,
                    is_array_literal: matches!(
                        init.without_parentheses(),
                        Expression::ArrayExpression(_)
                    ),
                });
            }
        }
    }

    fn declaration(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        call: &CallExpression<'a>,
        callee: Ref,
        is_single: bool,
    ) {
        let kind = match &declarator.id {
            BindingPattern::BindingIdentifier(id) => {
                if !is_single || declarator.type_annotation.is_some() {
                    return;
                }
                DeclarationKind::Single { binding: id.span.start }
            }
            BindingPattern::ArrayPattern(pattern) => {
                let binding = |index: usize| match pattern.elements.get(index) {
                    Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.span.start),
                    _ => None,
                };
                let Some(first) = binding(0) else { return };
                let init = call.arguments.first().and_then(Argument::as_expression);
                let is_store_shaped = pattern.rest.is_none()
                    && (1..=2).contains(&pattern.elements.len())
                    && (pattern.elements.len() == 1 || binding(1).is_some())
                    && declarator.type_annotation.is_none()
                    && call.type_arguments.is_none()
                    && call.arguments.len() == 1;
                DeclarationKind::Pair {
                    first,
                    second: binding(1),
                    is_foldable: is_foldable_signal_shape(declarator, call),
                    literal: init.and_then(static_text_alone),
                    store_leaves: init
                        .filter(|_| is_store_shaped)
                        .and_then(lower::store::store_shape),
                }
            }
            _ => return,
        };
        self.summary.declarations.push(CallDeclaration {
            callee,
            start: declarator.span.start,
            end: declarator.span.end,
            kind,
        });
    }

    fn uses(&mut self, top_level: &TopLevel, sites: &HashMap<u32, u32>) {
        let mut uses = Vec::new();
        for (&symbol, &binding) in &top_level.starts {
            let is_namespace = top_level.namespaces.contains(&symbol);
            for &reference in self.scoping.get_resolved_reference_ids(symbol) {
                let Some(access) = usage::classify(reference, self.scoping, self.nodes) else {
                    continue;
                };
                let (member, keys) = if is_namespace {
                    match access.keys.split_first() {
                        Some((member, rest)) => (Some(member.to_string()), rest),
                        None => (None, &access.keys[..]),
                    }
                } else {
                    (None, &access.keys[..])
                };
                let class = if is_namespace && member.is_none() {
                    UseClass::Other
                } else {
                    self.class(&access, keys)
                };
                let start = access.span.start;
                if class == UseClass::Call0
                    && !is_namespace
                    && self.summary.imports.iter().any(|i| i.binding == binding)
                    && computed::is_reactive_read(self.nodes.parent_id(access.node), self.nodes)
                {
                    self.summary.reactive_reads.push(start);
                }
                uses.push(Use {
                    target: Ref { binding, member },
                    class,
                    start,
                    end: access.span.end,
                    site: sites.get(&start).copied(),
                    component: self
                        .summary
                        .components
                        .iter()
                        .find(|c| c.start <= start && start < c.end)
                        .map(|c| c.binding),
                });
            }
        }
        uses.sort_by_key(|u| (u.start, u.end));
        self.summary.reactive_reads.sort_unstable();
        self.summary.uses = uses;
    }

    fn class(&self, access: &usage::Access<'a>, keys: &[&str]) -> UseClass {
        let path: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        match access.context {
            Context::Call { argument_count: 0 } if keys.is_empty() && access.tail.is_empty() => {
                UseClass::Call0
            }
            Context::Tag if keys.is_empty() && access.tail.is_empty() => UseClass::Tag,
            Context::Call { argument_count: 1 } if keys.is_empty() && access.tail.is_empty() => {
                let AstKind::CallExpression(call) = self.nodes.parent_kind(access.node) else {
                    return UseClass::Other;
                };
                match lower::store::draft_accesses(call, self.scoping, self.nodes) {
                    Some(accesses) => UseClass::StoreSet(
                        accesses
                            .into_iter()
                            .map(|a| DraftUse {
                                path: a.path,
                                index: a.index.is_some(),
                                tail: a.index.map_or(Vec::new(), |index| index.tail),
                                method: a.method,
                                form: a.form,
                                array_site: a.array_site,
                                is_write: a.write.is_some(),
                            })
                            .collect(),
                    ),
                    None => UseClass::Other,
                }
            }
            Context::Call { .. } => {
                let Some(tail) = index_clean(&access.tail) else { return UseClass::Other };
                if !tail.is_empty() || path.is_empty() {
                    return UseClass::Other;
                }
                let call = self.nodes.parent_id(access.node);
                let statement =
                    matches!(self.nodes.parent_kind(call), AstKind::ExpressionStatement(_));
                UseClass::Array(ArrayUse { path, tail, site: ArraySite::Call { statement } })
            }
            Context::Read if !keys.is_empty() => {
                let Some(tail) = index_clean(&access.tail) else { return UseClass::Other };
                if tail.is_empty()
                    && path.last().is_some_and(|key| key == "length")
                    && path.len() >= 2
                {
                    return UseClass::Array(ArrayUse { path, tail, site: ArraySite::Read });
                }
                if !tail.is_empty()
                    && !lower::store::element_position_valid(access.node, self.nodes)
                {
                    return UseClass::Other;
                }
                match self.read_site(access.node) {
                    Some(site) => UseClass::Array(ArrayUse { path, tail, site }),
                    None if tail.is_empty() => UseClass::Path(path),
                    None => UseClass::Array(ArrayUse { path, tail, site: ArraySite::Read }),
                }
            }
            _ => UseClass::Other,
        }
    }

    /// The array position a read chain stands in: `each`, an array spread, or nowhere.
    fn read_site(&self, node: NodeId) -> Option<ArraySite> {
        let parent = self.nodes.parent_id(node);
        match self.nodes.kind(parent) {
            AstKind::JSXExpressionContainer(_) => {
                let attribute_node = self.nodes.parent_id(parent);
                let AstKind::JSXAttribute(attribute) = self.nodes.kind(attribute_node) else {
                    return None;
                };
                let JSXAttributeName::Identifier(id) = &attribute.name else { return None };
                if id.name != "each" {
                    return None;
                }
                let opening_node = self.nodes.parent_id(attribute_node);
                let AstKind::JSXOpeningElement(opening) = self.nodes.kind(opening_node) else {
                    return None;
                };
                Some(ArraySite::Each(jsx_tag_name(&opening.name)))
            }
            AstKind::SpreadElement(_) => {
                matches!(self.nodes.parent_kind(parent), AstKind::ArrayExpression(_))
                    .then_some(ArraySite::Spread)
            }
            _ => None,
        }
    }

    /// Runtime helpers each component's JSX compiles to for `hydrate`.
    fn helpers(
        &mut self,
        allocator: &'a Allocator,
        program: &Program<'a>,
        source_type: SourceType,
    ) {
        if self.summary.components.is_empty() {
            return;
        }
        let facts = analyze::analyze(
            program,
            self.scoping,
            self.nodes,
            self.module_name,
            false,
            None,
            &mut Vec::new(),
        );
        for component in &mut self.summary.components {
            let span = Span::new(component.start, component.end);
            let Some(node) = find_function(program, span) else { continue };
            let mut lowerer = lower::Lowerer::new(
                allocator,
                self.source,
                &facts,
                self.scoping,
                self.nodes,
                false,
                Namer::new(self.scoping),
                Vec::new(),
            );
            let embed = match node {
                FunctionNode::Statement(statement) => lowerer.stmt(statement),
                FunctionNode::Expression(expression) => lowerer.expr(expression),
            };
            let emitter = emit::Emitter::new(
                allocator,
                self.source,
                self.module_name,
                source_type.is_typescript(),
                Target::Hydrate,
                Namer::new(self.scoping),
            );
            component.helpers =
                emitter.runtime_exports(&embed).into_iter().map(str::to_string).collect();
        }
    }
}

enum FunctionNode<'b, 'a> {
    Statement(&'b Statement<'a>),
    Expression(&'b Expression<'a>),
}

/// The top-level statement or initializer that is the function at `span`.
fn find_function<'b, 'a>(program: &'b Program<'a>, span: Span) -> Option<FunctionNode<'b, 'a>> {
    for statement in &program.body {
        if !(statement.span().start <= span.start && span.end <= statement.span().end) {
            continue;
        }
        let declaration = match statement {
            Statement::ExportDeclaration(export) => &export.declaration,
            Statement::ExportDefaultDeclaration(_) | Statement::FunctionDeclaration(_) => {
                return Some(FunctionNode::Statement(statement));
            }
            _ => statement.as_declaration()?,
        };
        let Declaration::VariableDeclaration(variables) = declaration else {
            return Some(FunctionNode::Statement(statement));
        };
        return variables
            .declarations
            .iter()
            .filter_map(|d| d.init.as_ref())
            .find(|init| init.without_parentheses().span() == span)
            .map(FunctionNode::Expression);
    }
    None
}

fn declared_names(declaration: &Declaration<'_>) -> Vec<(String, u32)> {
    match declaration {
        Declaration::VariableDeclaration(variables) => variables
            .declarations
            .iter()
            .flat_map(|d| d.id.get_binding_identifiers())
            .map(|id| (id.name.to_string(), id.span.start))
            .collect(),
        Declaration::FunctionDeclaration(f) => {
            f.id.iter().map(|id| (id.name.to_string(), id.span.start)).collect()
        }
        Declaration::ClassDeclaration(c) => {
            c.id.iter().map(|id| (id.name.to_string(), id.span.start)).collect()
        }
        _ => Vec::new(),
    }
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

/// Collects specifiers, imports and dynamic imports in source order.
impl<'a> Visit<'a> for Builder<'_, 'a> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if it.import_kind.is_type() {
            return;
        }
        let specifier = self.specifier(it.source.value.as_str());
        for item in it.specifiers.iter().flatten() {
            let (local, name) = match item {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    if s.import_kind.is_type() {
                        continue;
                    }
                    (&s.local, ImportName::Named(export_name(&s.imported)))
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    (&s.local, ImportName::Default)
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    (&s.local, ImportName::Namespace)
                }
            };
            self.summary.imports.push(Import { binding: local.span.start, specifier, name });
        }
    }

    fn visit_export_from_declaration(&mut self, it: &ExportFromDeclaration<'a>) {
        if !it.export_kind.is_type() {
            self.specifier(it.source.value.as_str());
        }
    }

    fn visit_export_all_declaration(&mut self, it: &ExportAllDeclaration<'a>) {
        if !it.export_kind.is_type() {
            self.specifier(it.source.value.as_str());
        }
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        match literal_string(&it.source) {
            Some(specifier) => {
                let index = self.specifier(&specifier);
                if !self.summary.dynamic.imports.contains(&index) {
                    self.summary.dynamic.imports.push(index);
                }
            }
            None => self.summary.dynamic.opens_all = true,
        }
        walk::walk_import_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::StaticMemberExpression(member) = &it.callee
            && member.property.name == "glob"
            && matches!(&member.object, Expression::ImportMeta(_))
        {
            let patterns = match it.arguments.first().and_then(Argument::as_expression) {
                Some(Expression::ArrayExpression(array)) => array
                    .elements
                    .iter()
                    .map(|e| e.as_expression().and_then(literal_string))
                    .collect::<Option<Vec<_>>>(),
                Some(e) => literal_string(e).map(|s| vec![s]),
                None => None,
            };
            match patterns {
                Some(patterns) => self.summary.dynamic.globs.extend(patterns),
                None => self.summary.dynamic.opens_all = true,
            }
        }
        walk::walk_call_expression(self, it);
    }
}

impl<'a> Builder<'_, 'a> {
    /// `renderToString(() => <R …/>)` / `hydrate(() => <R …/>, el)` candidates, anywhere.
    fn roots(&mut self, program: &Program<'a>, top_level: &TopLevel) {
        let mut finder = RootFinder { builder: self, top_level, roots: Vec::new() };
        finder.visit_program(program);
        let roots = finder.roots;
        self.summary.roots = roots;
    }
}

struct RootFinder<'b, 's, 'a> {
    builder: &'b Builder<'s, 'a>,
    top_level: &'b TopLevel,
    roots: Vec<RootSummary>,
}

impl<'a> Visit<'a> for RootFinder<'_, '_, 'a> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(callee) = self.builder.callee_ref(&it.callee, self.top_level)
            && let Some(Expression::ArrowFunctionExpression(arrow)) =
                it.arguments.first().and_then(Argument::as_expression)
            && arrow.params.items.is_empty()
            && arrow.params.rest.is_none()
            && arrow.is_expression()
            && let Some(Expression::JSXElement(element)) =
                arrow.get_expression().map(Expression::without_parentheses)
            && let Some(component) = self.tag_ref(&element.opening_element.name)
        {
            self.roots.push(RootSummary {
                call: it.span.start,
                end: it.span.end,
                callee,
                argument_count: it.arguments.len() as u32,
                component,
                has_json_attributes: element.children.iter().all(is_blank_text)
                    && element.opening_element.attributes.iter().all(|a| match a {
                        JSXAttributeItem::Attribute(a) => inert::is_json_attribute(a),
                        JSXAttributeItem::SpreadAttribute(_) => false,
                    }),
            });
        }
        walk::walk_call_expression(self, it);
    }
}

impl RootFinder<'_, '_, '_> {
    fn tag_ref(&self, name: &JSXElementName<'_>) -> Option<Ref> {
        match name {
            JSXElementName::IdentifierReference(id) => {
                let (symbol, binding) = self.builder.reference_of(id, self.top_level)?;
                (!self.top_level.namespaces.contains(&symbol))
                    .then_some(Ref { binding, member: None })
            }
            JSXElementName::MemberExpression(member) => {
                let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object
                else {
                    return None;
                };
                let (symbol, binding) = self.builder.reference_of(namespace, self.top_level)?;
                self.top_level
                    .namespaces
                    .contains(&symbol)
                    .then(|| Ref { binding, member: Some(member.property.name.to_string()) })
            }
            _ => None,
        }
    }
}

pub(crate) fn is_blank_text(child: &JSXChild<'_>) -> bool {
    matches!(child, JSXChild::Text(t) if crate::html::clean_jsx_text(t.value.as_str()).is_empty())
}

fn literal_string(e: &Expression<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked.as_ref()).map(|c| c.to_string())
        }
        _ => None,
    }
}

/// The serializable form of an index tail; `None` when an index holds JSX.
fn index_clean(tail: &[usage::Tail<'_>]) -> Option<Vec<ArrayTail>> {
    tail.iter()
        .map(|step| match step {
            usage::Tail::Index { index } => {
                (!has_jsx(|check| check.visit_expression(index))).then_some(ArrayTail::Index)
            }
            usage::Tail::Key(key) => Some(ArrayTail::Key(key.to_string())),
        })
        .collect()
}

/// The tag of a JSX element as written, dotted for member tags.
fn jsx_tag_name(name: &JSXElementName<'_>) -> String {
    match name {
        JSXElementName::Identifier(id) => id.name.to_string(),
        JSXElementName::IdentifierReference(id) => id.name.to_string(),
        JSXElementName::NamespacedName(name) => {
            format!("{}:{}", name.namespace.name, name.name.name)
        }
        JSXElementName::MemberExpression(member) => jsx_member_name(member),
        JSXElementName::ThisExpression(_) => "this".to_string(),
    }
}

fn jsx_member_name(member: &JSXMemberExpression<'_>) -> String {
    let object = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
        JSXMemberExpressionObject::MemberExpression(nested) => jsx_member_name(nested),
        JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    };
    format!("{object}.{}", member.property.name)
}
