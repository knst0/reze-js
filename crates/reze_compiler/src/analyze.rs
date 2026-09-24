//! Module-level facts the lowering consults: which references read signal getters, which
//! signals fold to constants (O3, SPEC §8) and which computeds inline into their read (O4). With
//! program facts, primitives re-exported through the program and exported signals the program
//! folds are recognized too (§15.4, §15.5).

pub mod computed;

use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::Span;
use oxc_syntax::node::NodeId;
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use crate::diagnostic::{Code, Related, Report};
use crate::facts::{IslandFact, ModuleFacts, Primitive, Reason, RootFact};
use crate::lower::constant::static_text;
use crate::lower::props::PropsFacts;
use crate::lower::store::{self, Stores};

pub const RUNTIME_MODULES: [&str; 3] = ["reze-js", "@rezejs/signals", "@rezejs/dom"];

pub fn is_runtime_module(specifier: &str, module_name: &str) -> bool {
    specifier == module_name || RUNTIME_MODULES.contains(&specifier)
}

#[derive(Default)]
pub struct Facts {
    pub primitives: Primitives,
    /// Destructured props parameters of components and the reads they rewrite (SPEC §15.7).
    pub props: PropsFacts,
    /// References to `signal`/`computed` getters.
    getter_refs: HashSet<ReferenceId>,
    /// References to folded getters, with the literal text of the initializer when it has one.
    folded_refs: HashMap<ReferenceId, Option<String>>,
    /// Namespace references heading `ns.member` reads of getters the program folded (§15.5).
    folded_member_refs: HashMap<ReferenceId, (String, Option<String>)>,
    /// Folded getters by their binding span start.
    folded_bindings: HashSet<u32>,
    /// Calls of inlined computeds, with the computed's declarator (O4).
    inlined_reads: HashMap<ReferenceId, NodeId>,
    /// Declarations of inlined computeds, and `export { … }` statements left without
    /// specifiers, by start, with the span removed (O4, §16.5).
    removed_declarations: HashMap<u32, Span>,
    /// Unproxied stores and the uses they rewrite (SPEC §15.6).
    pub stores: Stores,
    pub program: ProgramDecisions,
}

/// Decisions from program facts that lowering applies at a position (SPEC §15.5, §15.9).
#[derive(Default)]
pub struct ProgramDecisions {
    /// Export specifiers, by start, of setters whose signal the program folded and of computeds
    /// it inlined into another module.
    pub removed_specifiers: HashSet<u32>,
    /// Reads of computeds inlined from other modules, by the start of the callee (§16.5).
    pub computed_reads: HashMap<u32, computed::InlinedRead>,
    /// Import specifiers of those computeds, by local start, with the bindings that replace them.
    pub computed_imports: HashMap<u32, Vec<computed::ImportedName>>,
    /// Island boundaries by the start of their JSX element.
    pub islands: HashMap<u32, IslandFact>,
    /// Island roots by the start of their call.
    pub roots: HashMap<u32, RootFact>,
    /// With program facts, whether the build has islands; `None` in module mode, where
    /// `island:*` attributes compile as written without a warning (§16.3).
    pub islands_enabled: Option<bool>,
}

impl Facts {
    pub fn is_getter(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.getter_refs.contains(&r))
    }

    /// The folded getter's binding when `declarator` declares one.
    pub fn folded_getter(&self, declarator: &VariableDeclarator<'_>) -> Option<Span> {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
        let Some(Some(BindingPattern::BindingIdentifier(getter))) = pattern.elements.first() else {
            return None;
        };
        self.folded_bindings.contains(&getter.span.start).then_some(getter.span)
    }

    /// For `x()` / `ns.x()` reading a folded getter: the span of `x` / `ns.x`, and the text of
    /// a literal initializer.
    pub fn folded_callee(&self, call: &CallExpression<'_>) -> Option<(Span, Option<&str>)> {
        if !call.arguments.is_empty() || call.optional {
            return None;
        }
        match &call.callee {
            Expression::Identifier(id) => {
                let text = self.folded_refs.get(&id.reference_id.get()?)?;
                Some((id.span, text.as_deref()))
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let (name, text) = self.folded_member_refs.get(&namespace.reference_id.get()?)?;
                (name == member.property.name.as_str()).then(|| (member.span, text.as_deref()))
            }
            _ => None,
        }
    }

    /// The expression `call` is replaced with when it reads an inlined computed (O4).
    pub fn inlined_body<'a>(
        &self,
        call: &CallExpression<'_>,
        nodes: &AstNodes<'a>,
    ) -> Option<&'a Expression<'a>> {
        let Expression::Identifier(id) = &call.callee else { return None };
        let declarator = self.inlined_reads.get(&id.reference_id.get()?)?;
        let AstKind::VariableDeclarator(declarator) = nodes.kind(*declarator) else { return None };
        computed::body(declarator)
    }

    /// The span to remove for `declaration` when it declares an inlined computed (O4).
    pub fn removed_declaration(&self, declaration: &VariableDeclaration<'_>) -> Option<Span> {
        self.removed_declarations.get(&declaration.span.start).copied()
    }

    /// The span to remove for `export { … }` when the program removed each of its specifiers.
    pub fn removed_export(&self, export: &ExportNamedDeclaration<'_>) -> Option<Span> {
        self.removed_declarations.get(&export.span.start).copied()
    }
}

/// Runtime primitives in scope (SPEC §8.0, §15.4): named imports of a runtime module, members
/// of a runtime namespace import, and imports the program resolved to a primitive.
#[derive(Default)]
pub struct Primitives {
    named: HashMap<SymbolId, Primitive>,
    runtime_namespaces: HashSet<SymbolId>,
    program_namespaces: HashMap<SymbolId, HashMap<String, Primitive>>,
}

impl Primitives {
    pub fn collect(
        program: &Program<'_>,
        module_name: &str,
        module_facts: Option<&ModuleFacts>,
    ) -> Self {
        let mut primitives = Primitives::default();
        let from_facts = |binding: u32, member: Option<&str>| {
            module_facts?.primitives.iter().find_map(|p| {
                (p.import.binding == binding && p.import.member.as_deref() == member)
                    .then_some(p.primitive)
            })
        };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.import_kind.is_type() {
                continue;
            }
            let is_runtime = is_runtime_module(import.source.value.as_str(), module_name);
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                        if specifier.import_kind.is_type() {
                            continue;
                        }
                        let primitive = if is_runtime {
                            Primitive::from_export(specifier.imported.name().as_str())
                        } else {
                            from_facts(specifier.local.span.start, None)
                        };
                        if let Some(primitive) = primitive {
                            primitives.named.insert(specifier.local.symbol_id(), primitive);
                        }
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                        if let Some(primitive) = from_facts(specifier.local.span.start, None) {
                            primitives.named.insert(specifier.local.symbol_id(), primitive);
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                        let symbol = specifier.local.symbol_id();
                        if is_runtime {
                            primitives.runtime_namespaces.insert(symbol);
                            continue;
                        }
                        let Some(module_facts) = module_facts else { continue };
                        let members: HashMap<String, Primitive> = module_facts
                            .primitives
                            .iter()
                            .filter(|p| p.import.binding == specifier.local.span.start)
                            .filter_map(|p| Some((p.import.member.clone()?, p.primitive)))
                            .collect();
                        if !members.is_empty() {
                            primitives.program_namespaces.insert(symbol, members);
                        }
                    }
                }
            }
        }
        primitives
    }

    pub fn is_empty(&self) -> bool {
        self.named.is_empty()
            && self.runtime_namespaces.is_empty()
            && self.program_namespaces.is_empty()
    }

    /// The primitive `callee` names: `signal`, `s` re-exported as `signal`, or `R.signal`.
    pub fn of(&self, callee: &Expression<'_>, scoping: &Scoping) -> Option<Primitive> {
        let symbol_of = |id: &IdentifierReference<'_>| {
            scoping.get_reference(id.reference_id.get()?).symbol_id()
        };
        match callee.without_parentheses() {
            Expression::Identifier(id) => self.named.get(&symbol_of(id)?).copied(),
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let symbol = symbol_of(namespace)?;
                let name = member.property.name.as_str();
                if self.runtime_namespaces.contains(&symbol) {
                    return Primitive::from_export(name);
                }
                self.program_namespaces.get(&symbol)?.get(name).copied()
            }
            _ => None,
        }
    }
}

pub fn analyze<'a>(
    program: &Program<'a>,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
    module_name: &str,
    optimize: bool,
    module_facts: Option<&ModuleFacts>,
    reports: &mut Vec<Report>,
) -> Facts {
    let props = PropsFacts::collect(program, scoping);
    let primitives = Primitives::collect(program, module_name, module_facts);
    if primitives.is_empty() {
        let stores = if optimize {
            store::unproxy(program, &[], &HashSet::new(), scoping, nodes, module_facts, reports)
        } else {
            Stores::default()
        };
        let mut facts = Facts { props, stores, ..Facts::default() };
        if let Some(module_facts) = module_facts {
            apply_program_facts(&mut facts, program, scoping, nodes, module_facts, reports);
        }
        return facts;
    }
    let mut collector = Collector {
        scoping,
        primitives: &primitives,
        called: HashSet::new(),
        signals: Vec::new(),
        computed_getters: Vec::new(),
        computed_declarators: Vec::new(),
        stores: Vec::new(),
    };
    collector.visit_program(program);
    let exported = exported_symbols(program, scoping);
    let program_folded: HashSet<u32> = module_facts
        .map(|facts| facts.folded_signals.iter().map(|s| s.getter).collect())
        .unwrap_or_default();

    let Collector { signals, computed_getters, computed_declarators, called, stores, .. } =
        collector;
    let stores = if optimize {
        store::unproxy(program, &stores, &exported, scoping, nodes, module_facts, reports)
    } else {
        Stores::default()
    };
    let mut facts = Facts { primitives, props, stores, ..Facts::default() };
    for &getter in &computed_getters {
        facts.getter_refs.extend(scoping.get_resolved_reference_ids(getter));
    }
    for signal in &signals {
        let getter_refs = scoping.get_resolved_reference_ids(signal.getter);
        facts.getter_refs.extend(getter_refs);
        let getter_start = scoping.symbol_span(signal.getter).start;
        let is_exported = exported.contains(&signal.getter)
            || signal.setter.is_some_and(|s| exported.contains(&s));
        let is_program_fold = program_folded.contains(&getter_start);
        let setter_unused =
            signal.setter.is_none_or(|s| scoping.get_resolved_reference_ids(s).is_empty());
        let only_called = getter_refs.iter().all(|r| called.contains(r));
        let is_module_fold = !is_exported && setter_unused && only_called;
        if !(optimize && signal.is_foldable_shape && (is_module_fold || is_program_fold)) {
            continue;
        }
        for &r in getter_refs {
            facts.folded_refs.insert(r, signal.literal.clone());
        }
        if is_program_fold && let Some(setter) = signal.setter {
            for &r in scoping.get_resolved_reference_ids(setter) {
                if let AstKind::ExportSpecifier(specifier) =
                    nodes.parent_kind(scoping.get_reference(r).node_id())
                {
                    facts.program.removed_specifiers.insert(specifier.span.start);
                }
            }
        }
        facts.folded_bindings.insert(getter_start);
        let name = scoping.symbol_name(signal.getter);
        let mut report = Report::new(
            Code::SignalFolded,
            signal.span,
            format!(
                "`{name}` is never written: its setter is unused and every read is a call, \
                 so it compiled to a plain constant."
            ),
        )
        .data("signal", name)
        .data("scope", if is_program_fold { "program" } else { "module" });
        if let Some(folded) =
            module_facts.and_then(|f| f.folded_signals.iter().find(|s| s.getter == getter_start))
        {
            for related in &folded.related {
                report = report.related(related.clone());
            }
        }
        reports.push(report);
    }
    if optimize {
        computed::inline(
            &mut facts,
            &computed_declarators,
            &exported,
            program.source_text,
            scoping,
            nodes,
            reports,
        );
    }
    if let Some(module_facts) = module_facts {
        apply_program_facts(&mut facts, program, scoping, nodes, module_facts, reports);
    }
    facts
}

/// Imported folds and getters, component classification, islands and roots (SPEC §15.5,
/// §15.8–§15.10).
fn apply_program_facts(
    facts: &mut Facts,
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    module_facts: &ModuleFacts,
    reports: &mut Vec<Report>,
) {
    let import_symbols: HashMap<u32, SymbolId> = program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(import) => import.specifiers.as_ref(),
            _ => None,
        })
        .flatten()
        .map(|specifier| {
            let local = specifier.local();
            (local.span.start, local.symbol_id())
        })
        .collect();
    let member_refs = |symbol: SymbolId, member: &str| -> Vec<ReferenceId> {
        scoping
            .get_resolved_reference_ids(symbol)
            .iter()
            .copied()
            .filter(|&r| {
                matches!(nodes.parent_kind(scoping.get_reference(r).node_id()),
                    AstKind::StaticMemberExpression(m) if m.property.name == member)
            })
            .collect()
    };
    for folded in &module_facts.folded_imports {
        let Some(&symbol) = import_symbols.get(&folded.import.binding) else { continue };
        match &folded.import.member {
            None => {
                for &r in scoping.get_resolved_reference_ids(symbol) {
                    facts.folded_refs.insert(r, folded.literal.clone());
                }
            }
            Some(member) => {
                for r in member_refs(symbol, member) {
                    facts.folded_member_refs.insert(r, (member.clone(), folded.literal.clone()));
                }
            }
        }
    }
    for getter in &module_facts.getter_imports {
        if getter.member.is_none()
            && let Some(&symbol) = import_symbols.get(&getter.binding)
        {
            facts.getter_refs.extend(scoping.get_resolved_reference_ids(symbol));
        }
    }
    computed::apply_program(facts, program, scoping, nodes, module_facts, reports);
    facts.program.islands =
        module_facts.islands.iter().map(|island| (island.element, island.clone())).collect();
    facts.program.roots = module_facts.roots.iter().map(|root| (root.call, root.clone())).collect();
    facts.program.islands_enabled = Some(module_facts.islands_enabled);

    let root = scoping.root_scope_id();
    for component in &module_facts.components {
        let Some((_, &symbol)) = scoping
            .get_bindings(root)
            .iter()
            .find(|(_, symbol)| scoping.symbol_span(**symbol).start == component.binding)
        else {
            continue;
        };
        let name = scoping.symbol_name(symbol);
        let span = scoping.symbol_span(symbol);
        let report = match &component.client {
            None => Report::new(
                Code::StaticComponent,
                span,
                format!(
                    "`{name}` is static: it renders HTML and nothing else, so under an islands root \
                     it runs on the server only."
                ),
            ),
            Some(reason) => {
                let mut report = Report::new(
                    Code::ClientComponent,
                    span,
                    format!("`{name}` runs on the client: {}.", reason.message),
                )
                .data("reason", reason.message.as_str())
                .label(reason.span, "first client-only part");
                let mut cause = reason.cause.as_deref();
                while let Some(link) = cause {
                    report = report.related(related_of(link));
                    cause = link.cause.as_deref();
                }
                report
            }
        };
        reports.push(report.data("component", name));
    }
}

fn related_of(reason: &Reason) -> Related {
    Related {
        file: reason.module.clone(),
        start: reason.span.start,
        end: reason.span.end,
        message: reason.message.clone(),
    }
}

/// Local symbols exported by `export const/let/var/function/class` or `export { x }`.
fn exported_symbols(program: &Program<'_>, scoping: &Scoping) -> HashSet<SymbolId> {
    let mut exported = HashSet::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => {
                if let Declaration::VariableDeclaration(variables) = &export.declaration {
                    for declarator in &variables.declarations {
                        exported.extend(
                            declarator.id.get_binding_identifiers().iter().map(|id| id.symbol_id()),
                        );
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    let ModuleExportName::IdentifierReference(local) = &specifier.local else {
                        continue;
                    };
                    if let Some(symbol) =
                        local.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id())
                    {
                        exported.insert(symbol);
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::Identifier(local) = &export.declaration
                    && let Some(symbol) =
                        local.reference_id.get().and_then(|r| scoping.get_reference(r).symbol_id())
                {
                    exported.insert(symbol);
                }
            }
            _ => {}
        }
    }
    exported
}

struct SignalDecl {
    span: Span,
    getter: SymbolId,
    setter: Option<SymbolId>,
    is_foldable_shape: bool,
    literal: Option<String>,
}

struct Collector<'s> {
    scoping: &'s Scoping,
    primitives: &'s Primitives,
    /// References that are the callee of a plain zero-argument call.
    called: HashSet<ReferenceId>,
    signals: Vec<SignalDecl>,
    computed_getters: Vec<SymbolId>,
    computed_declarators: Vec<NodeId>,
    stores: Vec<store::Candidate>,
}

impl Collector<'_> {
    fn signal(&mut self, declarator: &VariableDeclarator<'_>, call: &CallExpression<'_>) {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return };
        let binding = |index: usize| match pattern.elements.get(index) {
            Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
            _ => None,
        };
        let Some(getter) = binding(0) else { return };
        let setter = binding(1);
        let literal =
            call.arguments.first().and_then(Argument::as_expression).and_then(static_text_alone);
        self.signals.push(SignalDecl {
            span: declarator.span,
            getter,
            setter,
            is_foldable_shape: is_foldable_signal_shape(declarator, call),
            literal,
        });
    }
}

/// O3's shape rules: `[get]` or `[get, set]` without annotations, an initializer, and plain
/// options (SPEC §8, O3).
pub fn is_foldable_signal_shape(
    declarator: &VariableDeclarator<'_>,
    call: &CallExpression<'_>,
) -> bool {
    let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return false };
    let is_identifier = |index: usize| {
        matches!(pattern.elements.get(index), Some(Some(BindingPattern::BindingIdentifier(_))))
    };
    pattern.rest.is_none()
        && (1..=2).contains(&pattern.elements.len())
        && is_identifier(0)
        && (pattern.elements.len() == 1 || is_identifier(1))
        && declarator.type_annotation.is_none()
        && call.type_arguments.is_none()
        && (1..=2).contains(&call.arguments.len())
        && call.arguments.first().and_then(Argument::as_expression).is_some()
        && call.arguments.get(1).is_none_or(is_plain_options)
}

pub fn static_text_alone(e: &Expression<'_>) -> Option<String> {
    static_text(e, &Facts::default())
}

/// Options that cannot observe the fold: an object literal of literals and functions.
fn is_plain_options(argument: &Argument<'_>) -> bool {
    let Some(Expression::ObjectExpression(object)) = argument.as_expression() else { return false };
    object.properties.iter().all(|property| match property {
        ObjectPropertyKind::ObjectProperty(p) => {
            !p.computed
                && matches!(
                    p.value.without_parentheses(),
                    Expression::BooleanLiteral(_)
                        | Expression::NumericLiteral(_)
                        | Expression::StringLiteral(_)
                        | Expression::NullLiteral(_)
                        | Expression::ArrowFunctionExpression(_)
                        | Expression::FunctionExpression(_)
                )
        }
        ObjectPropertyKind::SpreadProperty(_) => false,
    })
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && it.arguments.is_empty()
            && !it.optional
            && it.type_arguments.is_none()
            && let Some(reference) = id.reference_id.get()
        {
            self.called.insert(reference);
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(Expression::CallExpression(call)) =
            it.init.as_ref().map(|e| e.without_parentheses())
        {
            match self.primitives.of(&call.callee, self.scoping) {
                Some(Primitive::Signal) => self.signal(it, call),
                Some(Primitive::Computed) => {
                    if let BindingPattern::BindingIdentifier(id) = &it.id {
                        self.computed_getters.push(id.symbol_id());
                        self.computed_declarators.push(it.node_id.get());
                    }
                }
                Some(Primitive::Store) => self.stores.extend(store::candidate(it, call)),
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}
