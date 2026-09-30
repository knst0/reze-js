use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use crate::diagnostic::{Code, Report};
use crate::kind::{Kind, static_kind};
use crate::lower::async_component::AsyncFacts;
use crate::lower::constant::static_text;
use crate::lower::keyed::KeyedRows;
use crate::lower::props::PropsFacts;

pub(crate) const RUNTIME_MODULES: [&str; 3] = ["reze-js", "@rezejs/dom", "@rezejs/signals"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primitive {
    Signal,
    Computed,
    Intrinsic(Intrinsic),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intrinsic {
    Show,
    For,
    Repeat,
    Switch,
    Match,
    Loading,
    Errored,
}

impl Intrinsic {
    pub fn name(self) -> &'static str {
        match self {
            Intrinsic::Show => "Show",
            Intrinsic::For => "For",
            Intrinsic::Repeat => "Repeat",
            Intrinsic::Switch => "Switch",
            Intrinsic::Match => "Match",
            Intrinsic::Loading => "Loading",
            Intrinsic::Errored => "Errored",
        }
    }
}

impl Primitive {
    fn from_export(name: &str) -> Option<Primitive> {
        Some(match name {
            "signal" => Primitive::Signal,
            "computed" => Primitive::Computed,
            "Show" => Primitive::Intrinsic(Intrinsic::Show),
            "For" => Primitive::Intrinsic(Intrinsic::For),
            "Repeat" => Primitive::Intrinsic(Intrinsic::Repeat),
            "Switch" => Primitive::Intrinsic(Intrinsic::Switch),
            "Match" => Primitive::Intrinsic(Intrinsic::Match),
            "Loading" => Primitive::Intrinsic(Intrinsic::Loading),
            "Errored" => Primitive::Intrinsic(Intrinsic::Errored),
            _ => return None,
        })
    }
}

/// What a read of a folded signal (O3) stands for.
pub struct Fold {
    /// The rendered text of a literal initializer.
    pub text: Option<String>,
    pub kind: Option<Kind>,
}

/// Module facts every later phase consults.
pub struct Analysis<'s> {
    pub scoping: &'s Scoping,
    pub props: PropsFacts,
    pub asyncs: AsyncFacts,
    pub keyed: KeyedRows,
    named: HashMap<SymbolId, Primitive>,
    namespaces: HashSet<SymbolId>,
    getter_refs: HashSet<ReferenceId>,
    folded_refs: HashMap<ReferenceId, usize>,
    folds: Vec<Fold>,
    folded_bindings: HashSet<u32>,
}

impl<'s> Analysis<'s> {
    fn symbol(&self, id: &IdentifierReference<'_>) -> Option<SymbolId> {
        self.scoping.get_reference(id.reference_id.get()?).symbol_id()
    }

    /// The primitive `callee` names: an imported `signal`, or `R.signal` of a runtime namespace.
    pub fn primitive(&self, callee: &Expression<'_>) -> Option<Primitive> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => self.named.get(&self.symbol(id)?).copied(),
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                self.namespaces
                    .contains(&self.symbol(namespace)?)
                    .then(|| Primitive::from_export(member.property.name.as_str()))?
            }
            _ => None,
        }
    }

    pub fn intrinsic(&self, name: &JSXElementName<'_>) -> Option<Intrinsic> {
        let primitive = match name {
            JSXElementName::IdentifierReference(id) => self.named.get(&self.symbol(id)?).copied(),
            JSXElementName::MemberExpression(member) => {
                let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object
                else {
                    return None;
                };
                self.namespaces
                    .contains(&self.symbol(namespace)?)
                    .then(|| Primitive::from_export(member.property.name.as_str()))?
            }
            _ => None,
        };
        match primitive? {
            Primitive::Intrinsic(intrinsic) => Some(intrinsic),
            _ => None,
        }
    }

    pub fn is_getter(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.getter_refs.contains(&r))
    }

    /// Whether `id` reads a signal or computed getter whose binding is never reassigned.
    pub fn is_stable_getter(&self, id: &IdentifierReference<'_>) -> bool {
        self.is_getter(id) && self.symbol(id).is_some_and(|s| !self.scoping.symbol_is_mutated(s))
    }

    /// The getter binding of a folded `[get, set] = signal(init)` declarator.
    pub fn folded_getter(&self, declarator: &VariableDeclarator<'_>) -> Option<Span> {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
        let Some(Some(BindingPattern::BindingIdentifier(getter))) = pattern.elements.first() else {
            return None;
        };
        self.folded_bindings.contains(&getter.span.start).then_some(getter.span)
    }

    /// For `get()` reading a folded getter: the span of `get` and what it folds to.
    pub fn folded_read(&self, call: &CallExpression<'_>) -> Option<(Span, &Fold)> {
        if !call.arguments.is_empty() || call.optional {
            return None;
        }
        let Expression::Identifier(id) = &call.callee else { return None };
        let fold = self.folded_refs.get(&id.reference_id.get()?)?;
        Some((id.span, &self.folds[*fold]))
    }
}

pub fn analyze<'a, 's>(
    program: &Program<'a>,
    scoping: &'s Scoping,
    nodes: &AstNodes<'a>,
    reports: &mut Vec<Report>,
) -> Analysis<'s> {
    let mut analysis = Analysis {
        scoping,
        props: PropsFacts::collect(program, scoping),
        asyncs: AsyncFacts::collect(program, scoping, nodes),
        keyed: KeyedRows::default(),
        named: HashMap::new(),
        namespaces: HashSet::new(),
        getter_refs: HashSet::new(),
        folded_refs: HashMap::new(),
        folds: Vec::new(),
        folded_bindings: HashSet::new(),
    };
    collect_primitives(program, &mut analysis);
    if analysis.named.is_empty() && analysis.namespaces.is_empty() {
        return analysis;
    }
    report_intrinsic_values(&analysis, nodes, reports);
    analysis.keyed = KeyedRows::collect(program, &analysis, nodes);
    let mut collector = Collector {
        analysis: &analysis,
        called: HashSet::new(),
        signals: Vec::new(),
        computeds: Vec::new(),
    };
    collector.visit_program(program);
    let Collector { called, signals, computeds, .. } = collector;
    for getter in computeds {
        analysis.getter_refs.extend(scoping.get_resolved_reference_ids(getter));
    }
    let exported = exported_symbols(program, scoping);
    for signal in signals {
        let getter_refs = scoping.get_resolved_reference_ids(signal.getter);
        analysis.getter_refs.extend(getter_refs);
        let is_exported = exported.contains(&signal.getter)
            || signal.setter.is_some_and(|s| exported.contains(&s));
        let is_setter_unused =
            signal.setter.is_none_or(|s| scoping.get_resolved_reference_ids(s).is_empty());
        let is_only_called = getter_refs.iter().all(|r| called.contains(r));
        let is_awaited_value = getter_refs.iter().any(|&r| analysis.asyncs.is_awaited_value(r));
        if !signal.is_foldable_shape
            || is_exported
            || !is_setter_unused
            || !is_only_called
            || is_awaited_value
        {
            continue;
        }
        let fold = analysis.folds.len();
        analysis.folds.push(signal.fold);
        for &r in getter_refs {
            analysis.folded_refs.insert(r, fold);
        }
        analysis.folded_bindings.insert(scoping.symbol_span(signal.getter).start);
        let name = scoping.symbol_name(signal.getter);
        reports.push(Report::new(Code::SignalFolded, signal.span).arg("signal", name));
    }
    analysis
}

fn collect_primitives(program: &Program<'_>, analysis: &mut Analysis<'_>) {
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() || !RUNTIME_MODULES.contains(&import.source.value.as_str())
        {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    if specifier.import_kind.is_type() {
                        continue;
                    }
                    if let Some(primitive) =
                        Primitive::from_export(specifier.imported.name().as_str())
                    {
                        analysis.named.insert(specifier.local.symbol_id(), primitive);
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    analysis.namespaces.insert(specifier.local.symbol_id());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }
}

/// CONTROL_FLOW_AS_VALUE at every value reference of an intrinsic that is not a tag name.
fn report_intrinsic_values(
    analysis: &Analysis<'_>,
    nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) {
    let scoping = analysis.scoping;
    let is_value = |r: ReferenceId| !scoping.get_reference(r).flags().is_type_only();
    for (&symbol, &primitive) in &analysis.named {
        let Primitive::Intrinsic(intrinsic) = primitive else { continue };
        for &r in scoping.get_resolved_reference_ids(symbol) {
            let node = scoping.get_reference(r).node_id();
            if !is_value(r)
                || matches!(
                    nodes.parent_kind(node),
                    AstKind::JSXOpeningElement(_) | AstKind::JSXClosingElement(_)
                )
            {
                continue;
            }
            let span = nodes.kind(node).span();
            reports.push(Report::new(Code::ControlFlowAsValue, span).arg("name", intrinsic.name()));
        }
    }
    for &namespace in &analysis.namespaces {
        for &r in scoping.get_resolved_reference_ids(namespace) {
            if !is_value(r) {
                continue;
            }
            let AstKind::StaticMemberExpression(member) =
                nodes.parent_kind(scoping.get_reference(r).node_id())
            else {
                continue;
            };
            if let Some(Primitive::Intrinsic(intrinsic)) =
                Primitive::from_export(member.property.name.as_str())
            {
                reports.push(
                    Report::new(Code::ControlFlowAsValue, member.span)
                        .arg("name", intrinsic.name()),
                );
            }
        }
    }
}

/// Symbols exported by `export` declarations and local `export { … }` specifiers.
pub(crate) fn exported_symbols(program: &Program<'_>, scoping: &Scoping) -> HashSet<SymbolId> {
    let mut exported = HashSet::new();
    let resolved = |local: &IdentifierReference<'_>| {
        scoping.get_reference(local.reference_id.get()?).symbol_id()
    };
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
                    if let ModuleExportName::IdentifierReference(local) = &specifier.local
                        && let Some(symbol) = resolved(local)
                    {
                        exported.insert(symbol);
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::Identifier(local) = &export.declaration
                    && let Some(symbol) = resolved(local)
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
    fold: Fold,
}

struct Collector<'c, 's> {
    analysis: &'c Analysis<'s>,
    /// References that are the callee of a plain zero-argument call.
    called: HashSet<ReferenceId>,
    signals: Vec<SignalDecl>,
    computeds: Vec<SymbolId>,
}

impl Collector<'_, '_> {
    fn signal(&mut self, declarator: &VariableDeclarator<'_>, call: &CallExpression<'_>) {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return };
        let binding = |index: usize| match pattern.elements.get(index) {
            Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
            _ => None,
        };
        let Some(getter) = binding(0) else { return };
        let init = call.arguments.first().and_then(Argument::as_expression);
        let fold = Fold {
            text: init.and_then(|init| static_text(init, self.analysis)),
            kind: init.and_then(|init| static_kind(init, self.analysis)),
        };
        self.signals.push(SignalDecl {
            span: declarator.span,
            getter,
            setter: binding(1),
            is_foldable_shape: is_foldable_signal_shape(declarator, call),
            fold,
        });
    }
}

/// O3's shape: `[get]` or `[get, set]` without annotations, an initializer, and options that
/// are an object literal of literals and functions.
fn is_foldable_signal_shape(
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

impl<'a> Visit<'a> for Collector<'_, '_> {
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
            it.init.as_ref().map(Expression::without_parentheses)
        {
            match self.analysis.primitive(&call.callee) {
                Some(Primitive::Signal) => self.signal(it, call),
                Some(Primitive::Computed) => {
                    if let BindingPattern::BindingIdentifier(id) = &it.id {
                        self.computeds.push(id.symbol_id());
                    }
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}
