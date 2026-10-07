use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::keyed;
use super::pure::{format_integer, has_jsx, is_component_name, is_declared_component};
use crate::diagnostic::{Code, Report};
use crate::kind::{Kind, STRING_METHODS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primitive {
    Signal,
    Computed,
    Action,
    Dynamic,
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
    Portal,
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
            Intrinsic::Portal => "Portal",
        }
    }
}

impl Primitive {
    pub(crate) fn from_export(name: &str) -> Option<Primitive> {
        Some(match name {
            "signal" => Primitive::Signal,
            "computed" => Primitive::Computed,
            "action" => Primitive::Action,
            "dynamic" | "dynamicElement" => Primitive::Dynamic,
            "Show" => Primitive::Intrinsic(Intrinsic::Show),
            "For" => Primitive::Intrinsic(Intrinsic::For),
            "Repeat" => Primitive::Intrinsic(Intrinsic::Repeat),
            "Switch" => Primitive::Intrinsic(Intrinsic::Switch),
            "Match" => Primitive::Intrinsic(Intrinsic::Match),
            "Loading" => Primitive::Intrinsic(Intrinsic::Loading),
            "Errored" => Primitive::Intrinsic(Intrinsic::Errored),
            "Portal" => Primitive::Intrinsic(Intrinsic::Portal),
            _ => return None,
        })
    }
}

pub struct Fold {
    pub text: Option<String>,
    pub kind: Option<Kind>,
}

pub struct SharedFacts {
    pub named: HashMap<SymbolId, Primitive>,
    pub namespaces: HashMap<SymbolId, &'static str>,
    pub getter_refs: HashSet<ReferenceId>,
    pub folded_refs: HashMap<ReferenceId, usize>,
    pub folds: Vec<Fold>,
    pub folded_bindings: HashSet<u32>,
    pub pruned_imports: HashSet<NodeId>,
    pub asyncs: AsyncFacts,
    pub keyed: keyed::KeyedFacts,
    pub runtime_calls: HashMap<NodeId, RuntimeCallKind>,
    pub dynamic_tags: HashMap<NodeId, super::dynamic::DynamicTag>,
    pub getter_kinds: HashMap<SymbolId, Kind>,
}

impl SharedFacts {
    pub fn prunes_import(&self, import: &ImportDeclaration<'_>) -> bool {
        self.pruned_imports.contains(&import.node_id())
    }

    fn symbol(scoping: &Scoping, id: &IdentifierReference<'_>) -> Option<SymbolId> {
        scoping.get_reference(id.reference_id.get()?).symbol_id()
    }

    pub fn primitive(&self, scoping: &Scoping, callee: &Expression<'_>) -> Option<Primitive> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => self.named.get(&Self::symbol(scoping, id)?).copied(),
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let source = self.namespaces.get(&Self::symbol(scoping, namespace)?)?;
                let name = member.property.name.as_str();
                super::imports::allows(source, name).then(|| Primitive::from_export(name))?
            }
            _ => None,
        }
    }

    pub fn intrinsic(&self, scoping: &Scoping, name: &JSXElementName<'_>) -> Option<Intrinsic> {
        jsx_intrinsic(&self.named, &self.namespaces, scoping, name)
    }
    pub fn is_getter(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.getter_refs.contains(&r))
    }

    pub fn is_stable_getter(&self, scoping: &Scoping, id: &IdentifierReference<'_>) -> bool {
        self.is_getter(id)
            && Self::symbol(scoping, id).is_some_and(|s| !scoping.symbol_is_mutated(s))
    }

    pub fn folded_getter(&self, declarator: &VariableDeclarator<'_>) -> Option<Span> {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
        let Some(Some(BindingPattern::BindingIdentifier(getter))) = pattern.elements.first() else {
            return None;
        };
        self.folded_bindings.contains(&getter.span.start).then_some(getter.span)
    }

    pub fn folded_read(&self, call: &CallExpression<'_>) -> Option<(Span, &Fold)> {
        if !call.arguments.is_empty() || call.optional {
            return None;
        }
        let Expression::Identifier(id) = &call.callee else { return None };
        let fold = self.folded_refs.get(&id.reference_id.get()?)?;
        Some((id.span, &self.folds[*fold]))
    }

    pub fn stable_getter_deps(
        &self,
        scoping: &Scoping,
        e: &Expression<'_>,
    ) -> std::vec::Vec<SymbolId> {
        let mut refs = StableGetterRefs { facts: self, scoping, symbols: std::vec::Vec::new() };
        refs.visit_expression(e);
        refs.symbols.sort();
        refs.symbols.dedup();
        refs.symbols
    }

    /// Whether every stable-getter read in `e` happens on every evaluation and nothing else in `e`
    /// can read reactive state, so the reads of a binding over `e` never change between runs.
    pub fn reads_unconditionally(&self, scoping: &Scoping, e: &Expression<'_>) -> bool {
        self.fixed_reads(scoping, e, true)
    }

    fn fixed_reads(&self, scoping: &Scoping, e: &Expression<'_>, allows_reads: bool) -> bool {
        match e.get_inner_expression() {
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::Identifier(_) => true,
            Expression::TemplateLiteral(template) => template
                .expressions
                .iter()
                .all(|part| self.fixed_reads(scoping, part, allows_reads)),
            Expression::UnaryExpression(unary) => {
                unary.operator != UnaryOperator::Delete
                    && self.fixed_reads(scoping, &unary.argument, allows_reads)
            }
            Expression::BinaryExpression(binary) => {
                !matches!(binary.operator, BinaryOperator::In | BinaryOperator::Instanceof)
                    && self.fixed_reads(scoping, &binary.left, allows_reads)
                    && self.fixed_reads(scoping, &binary.right, allows_reads)
            }
            Expression::ConditionalExpression(conditional) => {
                self.fixed_reads(scoping, &conditional.test, allows_reads)
                    && self.fixed_reads(scoping, &conditional.consequent, false)
                    && self.fixed_reads(scoping, &conditional.alternate, false)
            }
            Expression::LogicalExpression(logical) => {
                self.fixed_reads(scoping, &logical.left, allows_reads)
                    && self.fixed_reads(scoping, &logical.right, false)
            }
            Expression::CallExpression(call) => {
                if self.folded_read(call).is_some() {
                    return true;
                }
                match &call.callee {
                    Expression::Identifier(id) => {
                        call.arguments.is_empty()
                            && !call.optional
                            && call.type_arguments.is_none()
                            && self.is_stable_getter(scoping, id)
                            && allows_reads
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// The primitive kind every evaluation of `e` produces, when the source alone proves it.
    pub fn kind_of(&self, scoping: &Scoping, e: &Expression<'_>) -> Option<Kind> {
        match e.without_parentheses() {
            Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => Some(Kind::Numeric),
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some(Kind::String),
            Expression::UnaryExpression(unary) => matches!(
                unary.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::BitwiseNot
            )
            .then_some(Kind::Numeric),
            Expression::BinaryExpression(binary) => self.binary_kind(scoping, binary),
            Expression::ConditionalExpression(conditional) => {
                let consequent = self.kind_of(scoping, &conditional.consequent)?;
                (self.kind_of(scoping, &conditional.alternate)? == consequent).then_some(consequent)
            }
            Expression::CallExpression(call) => self.call_kind(scoping, call),
            _ => None,
        }
    }

    fn binary_kind(&self, scoping: &Scoping, binary: &BinaryExpression<'_>) -> Option<Kind> {
        match binary.operator {
            BinaryOperator::Subtraction
            | BinaryOperator::Multiplication
            | BinaryOperator::Division
            | BinaryOperator::Remainder
            | BinaryOperator::Exponential
            | BinaryOperator::BitwiseOR
            | BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseXOR
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightZeroFill => Some(Kind::Numeric),
            BinaryOperator::Addition => {
                match (self.kind_of(scoping, &binary.left), self.kind_of(scoping, &binary.right)) {
                    (Some(Kind::String), _) | (_, Some(Kind::String)) => Some(Kind::String),
                    (Some(Kind::Numeric), Some(Kind::Numeric)) => Some(Kind::Numeric),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn call_kind(&self, scoping: &Scoping, call: &CallExpression<'_>) -> Option<Kind> {
        if call.optional {
            return None;
        }
        if let Some((_, fold)) = self.folded_read(call) {
            return fold.kind;
        }
        match &call.callee {
            Expression::Identifier(id) => {
                if Self::symbol(scoping, id).is_none() && id.reference_id.get().is_some() {
                    return match id.name.as_str() {
                        "String" => Some(Kind::String),
                        "Number" => Some(Kind::Numeric),
                        _ => None,
                    };
                }
                if call.arguments.is_empty() && call.type_arguments.is_none() && self.is_stable_getter(scoping, id) {
                    return self.getter_kinds.get(&Self::symbol(scoping, id)?).copied();
                }
                None
            }
            Expression::StaticMemberExpression(member)
                if !member.optional && STRING_METHODS.contains(&member.property.name.as_str()) =>
            {
                Some(Kind::String)
            }
            _ => None,
        }
    }
}

struct StableGetterRefs<'a, 's> {
    facts: &'a SharedFacts,
    scoping: &'s Scoping,
    symbols: std::vec::Vec<SymbolId>,
}

impl<'a> Visit<'a> for StableGetterRefs<'_, '_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && it.arguments.is_empty()
            && !it.optional
            && it.type_arguments.is_none()
            && self.facts.is_stable_getter(self.scoping, id)
            && self.facts.folded_read(it).is_none()
            && let Some(symbol) = SharedFacts::symbol(self.scoping, id)
        {
            self.symbols.push(symbol);
        }
        walk::walk_call_expression(self, it);
    }
}

/// The value a setter call stores: the argument, or what a parameterless arrow returns, since the
/// setter calls a function argument with the latest value.
fn written_value<'e, 'a>(argument: &'e Argument<'a>) -> Option<&'e Expression<'a>> {
    match argument.as_expression()?.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow)
            if !arrow.r#async && arrow.params.items.is_empty() && arrow.params.rest.is_none() =>
        {
            arrow.get_expression()
        }
        value => Some(value),
    }
}

struct SetterWrites<'a> {
    facts: &'a SharedFacts,
    scoping: &'a Scoping,
    setters: HashMap<SymbolId, Kind>,
    proven: HashMap<SymbolId, usize>,
}

impl<'a> Visit<'a> for SetterWrites<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && let Some(symbol) = SharedFacts::symbol(self.scoping, id)
            && let Some(&kind) = self.setters.get(&symbol)
            && !it.optional
            && let [argument] = it.arguments.as_slice()
            && let Some(value) = written_value(argument)
            && self.facts.kind_of(self.scoping, value) == Some(kind)
        {
            *self.proven.entry(symbol).or_insert(0) += 1;
        }
        walk::walk_call_expression(self, it);
    }
}

/// Keeps the getters of written signals whose every write is a call of the setter with a value of the
/// seed's kind. A write's kind may depend on getters of the same set, so the set shrinks to a fixed point.
fn infer_getter_kinds(
    program: &Program<'_>,
    scoping: &Scoping,
    facts: &mut SharedFacts,
    written: &[(SymbolId, SymbolId, Kind)],
) {
    facts.getter_kinds = written
        .iter()
        .filter(|(getter, setter, _)| {
            !scoping.symbol_is_mutated(*getter) && !scoping.symbol_is_mutated(*setter)
        })
        .map(|&(getter, _, kind)| (getter, kind))
        .collect();
    loop {
        let mut writes = SetterWrites {
            facts,
            scoping,
            setters: written
                .iter()
                .filter(|(getter, ..)| facts.getter_kinds.contains_key(getter))
                .map(|&(_, setter, kind)| (setter, kind))
                .collect(),
            proven: HashMap::new(),
        };
        writes.visit_program(program);
        let proven = writes.proven;
        let before = facts.getter_kinds.len();
        facts.getter_kinds.retain(|getter, _| {
            written.iter().any(|&(g, setter, _)| {
                g == *getter
                    && proven.get(&setter).copied().unwrap_or(0)
                        == scoping.get_resolved_reference_ids(setter).len()
            })
        });
        if facts.getter_kinds.len() == before {
            return;
        }
    }
}

pub fn collect(
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) -> SharedFacts {
    let mut dynamic_tags = HashMap::new();
    let runtime_calls = collect_runtime_calls(program, scoping, &mut dynamic_tags);
    let mut facts = SharedFacts {
        named: HashMap::new(),
        namespaces: HashMap::new(),
        getter_refs: HashSet::new(),
        folded_refs: HashMap::new(),
        folds: Vec::new(),
        folded_bindings: HashSet::new(),
        pruned_imports: HashSet::new(),
        asyncs: AsyncFacts::collect(program, scoping, nodes),
        keyed: keyed::KeyedFacts::default(),
        runtime_calls,
        dynamic_tags,
        getter_kinds: HashMap::new(),
    };
    collect_primitives(program, &mut facts);
    if facts.named.is_empty() && facts.namespaces.is_empty() {
        return facts;
    }
    report_intrinsic_values(scoping, nodes, &facts, reports);
    facts.keyed = keyed::collect(program, scoping, nodes, &facts.named, &facts.namespaces);
    let mut collector = Collector {
        scoping,
        named: &facts.named,
        namespaces: &facts.namespaces,
        called: HashSet::new(),
        signals: Vec::new(),
        computeds: Vec::new(),
    };
    collector.visit_program(program);
    let Collector { called, signals, computeds, .. } = collector;
    for getter in computeds {
        facts.getter_refs.extend(scoping.get_resolved_reference_ids(getter));
    }
    let exported = exported_symbols(program, scoping);
    let mut folded_factories = HashSet::new();
    let mut written = Vec::new();
    for signal in signals {
        let getter_refs = scoping.get_resolved_reference_ids(signal.getter);
        facts.getter_refs.extend(getter_refs);
        let is_exported = exported.contains(&signal.getter)
            || signal.setter.is_some_and(|s| exported.contains(&s));
        let is_setter_unused =
            signal.setter.is_none_or(|s| scoping.get_resolved_reference_ids(s).is_empty());
        let is_only_called = getter_refs.iter().all(|r| called.contains(r));
        let is_awaited_value = getter_refs.iter().any(|&r| facts.asyncs.is_awaited_value(r));
        if let (Some(setter), Some(kind)) = (signal.setter, signal.fold.kind)
            && !exported.contains(&setter)
            && !is_setter_unused
            && !is_awaited_value
        {
            written.push((signal.getter, setter, kind));
        }
        if !signal.is_foldable_shape
            || is_exported
            || !is_setter_unused
            || !is_only_called
            || is_awaited_value
        {
            continue;
        }
        let fold = facts.folds.len();
        facts.folds.push(signal.fold);
        for &r in getter_refs {
            facts.folded_refs.insert(r, fold);
        }
        facts.folded_bindings.insert(scoping.symbol_span(signal.getter).start);
        folded_factories.extend(signal.factory_reference);
        let name = scoping.symbol_name(signal.getter);
        reports.push(Report::new(Code::SignalFolded, signal.span).arg("signal", name));
    }
    if scoping
        .scope_descendants_from_root()
        .any(|scope| scoping.scope_flags(scope).contains_direct_eval())
    {
        return facts;
    }
    infer_getter_kinds(program, scoping, &mut facts, &written);
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        if super::imports::home_of(import.source.value.as_str()).is_none() {
            continue;
        }
        let Some(specifiers) = &import.specifiers else { continue };
        let [ImportDeclarationSpecifier::ImportSpecifier(specifier)] = specifiers.as_slice() else {
            continue;
        };
        let symbol = specifier.local.symbol_id();
        let references = scoping.get_resolved_reference_ids(symbol);
        if !specifier.import_kind.is_type()
            && facts.named.get(&symbol) == Some(&Primitive::Signal)
            && !exported.contains(&symbol)
            && !references.is_empty()
            && references.iter().all(|r| folded_factories.contains(r))
        {
            facts.pruned_imports.insert(import.node_id());
        }
    }
    facts
}

fn collect_primitives(program: &Program<'_>, facts: &mut SharedFacts) {
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let Some(home) = super::imports::home_of(import.source.value.as_str()) else { continue };
        for specifier in import.specifiers.iter().flatten() {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    if specifier.import_kind.is_type() {
                        continue;
                    }
                    let name = specifier.imported.name();
                    if let Some(primitive) = Primitive::from_export(name.as_str())
                        && super::imports::allows(home, name.as_str())
                    {
                        facts.named.insert(specifier.local.symbol_id(), primitive);
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    facts.namespaces.insert(specifier.local.symbol_id(), home);
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }
}

fn report_intrinsic_values(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    facts: &SharedFacts,
    reports: &mut Vec<Report>,
) {
    let is_value = |r: ReferenceId| !scoping.get_reference(r).flags().is_type_only();
    for (&symbol, &primitive) in &facts.named {
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
    for (&namespace, source) in &facts.namespaces {
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
                && super::imports::allows(source, member.property.name.as_str())
            {
                reports.push(
                    Report::new(Code::ControlFlowAsValue, member.span)
                        .arg("name", intrinsic.name()),
                );
            }
        }
    }
}

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
    factory_reference: Option<ReferenceId>,
    getter: SymbolId,
    setter: Option<SymbolId>,
    is_foldable_shape: bool,
    fold: Fold,
}

struct Collector<'c, 's> {
    scoping: &'s Scoping,
    named: &'c HashMap<SymbolId, Primitive>,
    namespaces: &'c HashMap<SymbolId, &'static str>,
    called: HashSet<ReferenceId>,
    signals: Vec<SignalDecl>,
    computeds: Vec<SymbolId>,
}

impl Collector<'_, '_> {
    fn primitive(&self, callee: &Expression<'_>) -> Option<Primitive> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => self
                .named
                .get(&self.scoping.get_reference(id.reference_id.get()?).symbol_id()?)
                .copied(),
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let symbol =
                    self.scoping.get_reference(namespace.reference_id.get()?).symbol_id()?;
                let source = self.namespaces.get(&symbol)?;
                let name = member.property.name.as_str();
                super::imports::allows(source, name).then(|| Primitive::from_export(name))?
            }
            _ => None,
        }
    }

    fn signal(&mut self, declarator: &VariableDeclarator<'_>, call: &CallExpression<'_>) {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return };
        let binding = |index: usize| match pattern.elements.get(index) {
            Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
            _ => None,
        };
        let Some(getter) = binding(0) else { return };
        let init = call.arguments.first().and_then(Argument::as_expression);
        let fold = init
            .map(|init| fold_seed(init, self.scoping))
            .unwrap_or(Fold { text: None, kind: None });
        self.signals.push(SignalDecl {
            span: declarator.span,
            factory_reference: match call.callee.without_parentheses() {
                Expression::Identifier(id) => id.reference_id.get(),
                _ => None,
            },
            getter,
            setter: binding(1),
            is_foldable_shape: is_foldable_signal_shape(declarator, call),
            fold,
        });
    }
}

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
            match self.primitive(&call.callee) {
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

fn is_global_ref(id: &IdentifierReference<'_>, name: &str, scoping: &Scoping) -> bool {
    id.name.as_str() == name
        && id.reference_id.get().is_some_and(|r| scoping.get_reference(r).symbol_id().is_none())
}

fn seed_number(e: &Expression<'_>, scoping: &Scoping) -> Option<f64> {
    match e.without_parentheses() {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::Identifier(id) if is_global_ref(id, "NaN", scoping) => Some(f64::NAN),
        Expression::Identifier(id) if is_global_ref(id, "Infinity", scoping) => Some(f64::INFINITY),
        Expression::UnaryExpression(u) => {
            let value = seed_number(&u.argument, scoping)?;
            match u.operator {
                oxc_syntax::operator::UnaryOperator::UnaryNegation => Some(-value),
                oxc_syntax::operator::UnaryOperator::UnaryPlus => Some(value),
                _ => None,
            }
        }
        Expression::BinaryExpression(b) => {
            let left = seed_number(&b.left, scoping)?;
            let right = seed_number(&b.right, scoping)?;
            match b.operator {
                BinaryOperator::Addition => Some(left + right),
                BinaryOperator::Subtraction => Some(left - right),
                BinaryOperator::Multiplication => Some(left * right),
                BinaryOperator::Division => Some(left / right),
                _ => None,
            }
        }
        _ => None,
    }
}

fn seed_text(e: &Expression<'_>, scoping: &Scoping) -> Option<String> {
    match e.without_parentheses() {
        Expression::NumericLiteral(n) => format_integer(n.value),
        Expression::UnaryExpression(_) => format_integer(seed_number(e, scoping)?),
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) => {
            let mut text = String::new();
            for (index, quasi) in t.quasis.iter().enumerate() {
                text.push_str(quasi.value.cooked.as_ref()?.as_str());
                if let Some(expression) = t.expressions.get(index) {
                    text.push_str(&seed_primitive_text(expression, scoping)?);
                }
            }
            Some(text)
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            match (seed_text(&b.left, scoping), seed_text(&b.right, scoping)) {
                (Some(left), Some(right)) => Some(left + &right),
                (Some(left), None) => Some(left + &seed_primitive_text(&b.right, scoping)?),
                (None, Some(right)) => Some(seed_primitive_text(&b.left, scoping)? + &right),
                (None, None) => None,
            }
        }
        _ => None,
    }
}

fn seed_primitive_text(e: &Expression<'_>, scoping: &Scoping) -> Option<String> {
    if let Some(number) = seed_number(e, scoping) {
        return format_integer(number);
    }
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::NullLiteral(_) => Some("null".to_string()),
        Expression::Identifier(id) if is_global_ref(id, "undefined", scoping) => {
            Some("undefined".to_string())
        }
        _ => seed_text(e, scoping),
    }
}

fn seed_kind(e: &Expression<'_>, scoping: &Scoping) -> Option<Kind> {
    match e.without_parentheses() {
        Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => Some(Kind::Numeric),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some(Kind::String),
        Expression::UnaryExpression(unary) => {
            matches!(unary.operator, oxc_syntax::operator::UnaryOperator::UnaryNegation)
                .then(|| seed_number(&unary.argument, scoping))
                .flatten()
                .map(|_| Kind::Numeric)
        }
        Expression::BinaryExpression(binary) => match binary.operator {
            BinaryOperator::Subtraction
            | BinaryOperator::Multiplication
            | BinaryOperator::Division
            | BinaryOperator::Remainder
            | BinaryOperator::Exponential
            | BinaryOperator::BitwiseOR
            | BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseXOR
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightZeroFill => Some(Kind::Numeric),
            BinaryOperator::Addition => {
                match (seed_kind(&binary.left, scoping), seed_kind(&binary.right, scoping)) {
                    (Some(Kind::String), _) | (_, Some(Kind::String)) => Some(Kind::String),
                    (Some(Kind::Numeric), Some(Kind::Numeric)) => Some(Kind::Numeric),
                    _ => None,
                }
            }
            _ => None,
        },
        Expression::ConditionalExpression(conditional) => {
            let consequent = seed_kind(&conditional.consequent, scoping)?;
            let alternate = seed_kind(&conditional.alternate, scoping)?;
            (consequent == alternate).then_some(consequent)
        }
        Expression::CallExpression(call) if !call.optional => match &call.callee {
            Expression::Identifier(id)
                if id
                    .reference_id
                    .get()
                    .is_some_and(|r| scoping.get_reference(r).symbol_id().is_none()) =>
            {
                match id.name.as_str() {
                    "String" => Some(Kind::String),
                    "Number" => Some(Kind::Numeric),
                    _ => None,
                }
            }
            Expression::StaticMemberExpression(member)
                if !member.optional && STRING_METHODS.contains(&member.property.name.as_str()) =>
            {
                Some(Kind::String)
            }
            _ => None,
        },
        _ => None,
    }
}

fn fold_seed(init: &Expression<'_>, scoping: &Scoping) -> Fold {
    Fold { text: seed_text(init, scoping), kind: seed_kind(init, scoping) }
}

#[derive(Clone, Copy)]
pub(crate) struct Reject {
    pub(crate) reason: &'static str,
    pub(crate) span: Span,
}

#[derive(Default)]
struct AwaitCheck {
    found: bool,
}

impl<'a> Visit<'a> for AwaitCheck {
    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.found = true;
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if it.r#await {
            self.found = true;
        } else {
            walk::walk_for_of_statement(self, it);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

#[derive(Default)]
struct ReturnCheck {
    found: bool,
}

impl<'a> Visit<'a> for ReturnCheck {
    fn visit_return_statement(&mut self, _: &ReturnStatement<'a>) {
        self.found = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

struct References<'s> {
    scoping: &'s Scoping,
    symbols: HashSet<SymbolId>,
    ordered: std::vec::Vec<(ReferenceId, SymbolId)>,
}

impl<'a> Visit<'a> for References<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(reference) = it.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
        {
            self.symbols.insert(symbol);
            if self.scoping.get_reference(reference).flags().is_value() {
                self.ordered.push((reference, symbol));
            }
        }
    }
}

pub struct AsyncPlan {
    pub first: usize,
    pub last: usize,
    pub tracked: std::vec::Vec<Span>,
    pub values: std::vec::Vec<String>,
}

#[derive(Default)]
pub struct AsyncFacts {
    plans: HashMap<u32, Result<AsyncPlan, Reject>>,
    reads: HashMap<ReferenceId, (u32, usize)>,
}

impl AsyncFacts {
    pub fn collect(program: &Program<'_>, scoping: &Scoping, nodes: &AstNodes<'_>) -> Self {
        let mut collector = AsyncCollector { scoping, nodes, facts: AsyncFacts::default() };
        collector.visit_program(program);
        collector.facts
    }

    pub fn plan(&self, function_start: u32) -> Option<&Result<AsyncPlan, Reject>> {
        self.plans.get(&function_start)
    }

    pub fn is_awaited_value(&self, reference: ReferenceId) -> bool {
        self.reads.contains_key(&reference)
    }

    pub fn is_read(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.reads.contains_key(&r))
    }

    pub fn read(&self, id: &IdentifierReference<'_>) -> Option<(u32, usize)> {
        id.reference_id.get().and_then(|r| self.reads.get(&r)).copied()
    }
}

struct AsyncCollector<'c, 's> {
    scoping: &'s Scoping,
    nodes: &'c AstNodes<'c>,
    facts: AsyncFacts,
}

impl AsyncCollector<'_, '_> {
    fn component(&mut self, start: u32, statements: &[Statement<'_>]) {
        let Some(result) = plan(start, statements, self.scoping, self.nodes) else { return };
        match result {
            Ok((plan, reads)) => {
                self.facts.reads.extend(reads);
                self.facts.plans.insert(start, Ok(plan));
            }
            Err(reject) => {
                self.facts.plans.insert(start, Err(reject));
            }
        }
    }
}

impl<'a> Visit<'a> for AsyncCollector<'_, '_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.r#async
            && !it.generator
            && let Some(id) = &it.id
            && is_component_name(id.name.as_str())
            && let Some(body) = &it.body
        {
            self.component(it.span.start, &body.statements);
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && is_component_name(id.name.as_str())
            && let Some(init) = it.init.as_ref().map(Expression::without_parentheses)
        {
            match init {
                Expression::ArrowFunctionExpression(arrow) if arrow.r#async => {
                    if let ArrowFunctionBody::FunctionBody(body) = &arrow.body {
                        self.component(arrow.span.start, &body.statements);
                    }
                }
                Expression::FunctionExpression(function)
                    if function.r#async
                        && !function.generator
                        && is_declared_component(function) =>
                {
                    if let Some(body) = &function.body {
                        self.component(function.span.start, &body.statements);
                    }
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

type AsyncReads = std::vec::Vec<(ReferenceId, (u32, usize))>;

fn plan(
    function_start: u32,
    statements: &[Statement<'_>],
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<Result<(AsyncPlan, AsyncReads), Reject>> {
    let (first, last) = await_range(statements)?;
    let mut tracked = std::vec::Vec::new();
    for (i, statement) in statements[..=last].iter().enumerate() {
        let reject = |reason| Reject { reason, span: statement.span() };
        if has_jsx(|check| check.visit_statement(statement)) {
            return Some(Err(reject("jsx-before-await")));
        }
        if contains_return(statement) {
            return Some(Err(reject("return-before-await")));
        }
        if i < first {
            continue;
        }
        if contains_await(statement) {
            let operand = match await_operand(statement) {
                Ok(operand) => operand,
                Err(reject) => return Some(Err(reject)),
            };
            if i > first {
                tracked.push(operand.span());
            }
        } else if let Statement::VariableDeclaration(declaration) = statement {
            tracked.extend(
                declaration.declarations.iter().filter_map(|d| d.init.as_ref()).map(GetSpan::span),
            );
        } else {
            return Some(Err(reject("statement-between-awaits")));
        }
    }
    let mut declared = std::vec::Vec::new();
    statements[..=last].iter().for_each(|statement| declared_symbols(statement, &mut declared));
    let mut references =
        References { scoping, symbols: HashSet::new(), ordered: std::vec::Vec::new() };
    statements[last + 1..].iter().for_each(|statement| references.visit_statement(statement));
    let mut seen = HashSet::new();
    let kept: std::vec::Vec<(SymbolId, &str)> = declared
        .into_iter()
        .filter(|(symbol, _)| references.symbols.contains(symbol) && seen.insert(*symbol))
        .collect();
    for (symbol, _) in &kept {
        if let Some(&r) = scoping
            .get_resolved_reference_ids(*symbol)
            .iter()
            .find(|r| scoping.get_reference(**r).is_write())
        {
            let span = nodes.kind(scoping.get_reference(r).node_id()).span();
            return Some(Err(Reject { reason: "value-reassigned", span }));
        }
    }
    let mut positions = HashMap::new();
    for (index, (symbol, _)) in kept.iter().enumerate() {
        positions.insert(*symbol, index);
    }
    let values = kept.iter().map(|(_, name)| name.to_string()).collect();
    let reads = references
        .ordered
        .into_iter()
        .filter_map(|(r, symbol)| positions.get(&symbol).map(|&index| (r, (function_start, index))))
        .collect();
    Some(Ok((AsyncPlan { first, last, tracked, values }, reads)))
}

fn contains_await(statement: &Statement<'_>) -> bool {
    let mut check = AwaitCheck::default();
    check.visit_statement(statement);
    check.found
}

fn contains_return(statement: &Statement<'_>) -> bool {
    let mut check = ReturnCheck::default();
    check.visit_statement(statement);
    check.found
}

fn await_range(statements: &[Statement<'_>]) -> Option<(usize, usize)> {
    let mut awaiting = statements.iter().enumerate().filter(|(_, s)| contains_await(s));
    let first = awaiting.next()?.0;
    Some((first, awaiting.next_back().map_or(first, |(i, _)| i)))
}

fn await_operand<'s, 'a>(statement: &'s Statement<'a>) -> Result<&'s Expression<'a>, Reject> {
    let reject = |reason| Reject { reason, span: statement.span() };
    let (value, pattern) = match statement {
        Statement::VariableDeclaration(declaration)
            if !declaration.kind.is_using() && declaration.declarations.len() == 1 =>
        {
            let declarator = &declaration.declarations[0];
            (declarator.init.as_ref(), Some(&declarator.id))
        }
        Statement::ExpressionStatement(expression) => (Some(&expression.expression), None),
        _ => (None, None),
    };
    let Some(Expression::AwaitExpression(awaited)) = value else {
        return Err(reject("await-position"));
    };
    let mut check = AwaitCheck::default();
    if let Some(pattern) = pattern {
        check.visit_binding_pattern(pattern);
    }
    if check.found {
        return Err(reject("await-position"));
    }
    check.visit_expression(&awaited.argument);
    if check.found {
        return Err(reject("nested-await"));
    }
    Ok(&awaited.argument)
}

fn declared_symbols<'s, 'a>(statement: &'s Statement<'a>, out: &mut Vec<(SymbolId, &'s str)>) {
    let mut push = |id: &'s BindingIdentifier<'a>| out.push((id.symbol_id(), id.name.as_str()));
    match statement {
        Statement::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                declarator.id.get_binding_identifiers().into_iter().for_each(&mut push);
            }
        }
        Statement::FunctionDeclaration(function) => function.id.iter().for_each(&mut push),
        Statement::ClassDeclaration(class) => class.id.iter().for_each(&mut push),
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeCallKind {
    AsyncComputed,
    UniqueId,
    AsyncComponent,
    Dynamic,
    DynamicElement,
    Island,
}

impl RuntimeCallKind {
    fn from_export(name: &str) -> Option<Self> {
        Some(match name {
            "asyncComputed" => Self::AsyncComputed,
            "createUniqueId" => Self::UniqueId,
            "asyncComponent" => Self::AsyncComponent,
            "dynamic" => Self::Dynamic,
            "dynamicElement" => Self::DynamicElement,
            "island" => Self::Island,
            _ => return None,
        })
    }
}

pub(crate) fn jsx_intrinsic(
    named: &HashMap<SymbolId, Primitive>,
    namespaces: &HashMap<SymbolId, &'static str>,
    scoping: &Scoping,
    name: &JSXElementName<'_>,
) -> Option<Intrinsic> {
    let primitive = match name {
        JSXElementName::IdentifierReference(id) => {
            named.get(&SharedFacts::symbol(scoping, id)?).copied()
        }
        JSXElementName::MemberExpression(member) => {
            let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object else {
                return None;
            };
            let source = namespaces.get(&SharedFacts::symbol(scoping, namespace)?)?;
            let name = member.property.name.as_str();
            super::imports::allows(source, name).then(|| Primitive::from_export(name))?
        }
        _ => None,
    };
    match primitive? {
        Primitive::Intrinsic(intrinsic) => Some(intrinsic),
        _ => None,
    }
}

struct RuntimeCallCollector<'s, 't> {
    scoping: &'s Scoping,
    factories: HashMap<SymbolId, RuntimeCallKind>,
    namespaces: HashMap<SymbolId, &'static str>,
    calls: HashMap<NodeId, RuntimeCallKind>,
    dynamic_tags: &'t mut HashMap<NodeId, super::dynamic::DynamicTag>,
}

fn collect_runtime_calls(
    program: &Program<'_>,
    scoping: &Scoping,
    dynamic_tags: &mut HashMap<NodeId, super::dynamic::DynamicTag>,
) -> HashMap<NodeId, RuntimeCallKind> {
    let mut collector = RuntimeCallCollector {
        scoping,
        factories: HashMap::new(),
        namespaces: HashMap::new(),
        calls: HashMap::new(),
        dynamic_tags,
    };
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let Some(home) = super::imports::home_of(import.source.value.as_str()) else { continue };
        for specifier in import.specifiers.iter().flatten() {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) => {
                    if named.import_kind.is_type() {
                        continue;
                    }
                    let name = named.imported.name();
                    let Some(kind) = RuntimeCallKind::from_export(name.as_str()) else { continue };
                    if super::imports::allows(home, name.as_str()) {
                        collector.factories.insert(named.local.symbol_id(), kind);
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                    collector.namespaces.insert(namespace.local.symbol_id(), home);
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }
    collector.visit_program(program);
    collector.calls
}

impl<'a> Visit<'a> for RuntimeCallCollector<'_, '_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let mut found = None;
        match it.callee.without_parentheses() {
            Expression::Identifier(id) => {
                if let Some(reference) = id.reference_id.get()
                    && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
                    && let Some(&kind) = self.factories.get(&symbol)
                {
                    found = Some(kind);
                }
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                if let Expression::Identifier(object) = &member.object
                    && let Some(reference) = object.reference_id.get()
                    && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
                    && let Some(source) = self.namespaces.get(&symbol)
                {
                    let name = member.property.name.as_str();
                    let kind = RuntimeCallKind::from_export(name);
                    if let Some(kind) = kind
                        && super::imports::allows(source, name)
                    {
                        found = Some(kind);
                    }
                }
            }
            _ => {}
        }
        if let Some(kind) = found {
            self.calls.insert(it.node_id(), kind);
            if kind == RuntimeCallKind::Dynamic {
                super::dynamic::collect(it, self.dynamic_tags);
            }
        }
        walk::walk_call_expression(self, it);
    }
}
