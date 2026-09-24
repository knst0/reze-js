//! Module-level facts the lowering consults: which references read signal getters, and which
//! signals fold to constants (O3, SPEC §8).

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::Span;
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use crate::diagnostic::{Code, Report};
use crate::lower::constant::static_text;

const RUNTIME_MODULES: [&str; 3] = ["reze-js", "@rezejs/signals", "@rezejs/dom"];

#[derive(Default)]
pub struct Facts {
    /// References to `signal`/`computed` getters.
    getter_refs: HashSet<ReferenceId>,
    /// References to folded getters, with the literal text of the initializer when it has one.
    folded_refs: HashMap<ReferenceId, Option<String>>,
    /// Folded getters by their binding span start.
    folded_bindings: HashSet<u32>,
}

impl Facts {
    pub fn is_getter(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.getter_refs.contains(&r))
    }

    pub fn is_folded_read(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.folded_refs.contains_key(&r))
    }

    pub fn folded_text(&self, id: &IdentifierReference<'_>) -> Option<&str> {
        id.reference_id.get().and_then(|r| self.folded_refs.get(&r)).and_then(|t| t.as_deref())
    }

    /// The folded getter's binding when `declarator` declares one.
    pub fn folded_getter(&self, declarator: &VariableDeclarator<'_>) -> Option<Span> {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
        let Some(Some(BindingPattern::BindingIdentifier(getter))) = pattern.elements.first() else {
            return None;
        };
        self.folded_bindings.contains(&getter.span.start).then_some(getter.span)
    }

    /// The getter of `e` when `e` is a zero-argument call of a folded getter.
    pub fn folded_call<'b, 'a>(
        &self,
        e: &'b Expression<'a>,
    ) -> Option<&'b IdentifierReference<'a>> {
        let Expression::CallExpression(call) = e.without_parentheses() else { return None };
        let Expression::Identifier(id) = &call.callee else { return None };
        (call.arguments.is_empty() && !call.optional && self.is_folded_read(id)).then_some(&**id)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Primitive {
    Signal,
    Computed,
}

pub fn analyze(
    program: &Program<'_>,
    scoping: &Scoping,
    module_name: &str,
    optimize: bool,
    reports: &mut Vec<Report>,
) -> Facts {
    let mut collector = Collector {
        scoping,
        primitives: runtime_imports(program, module_name),
        called: HashSet::new(),
        signals: Vec::new(),
        computed_getters: Vec::new(),
    };
    if collector.primitives.is_empty() {
        return Facts::default();
    }
    collector.visit_program(program);

    let mut facts = Facts::default();
    for &getter in &collector.computed_getters {
        facts.getter_refs.extend(scoping.get_resolved_reference_ids(getter));
    }
    for signal in &collector.signals {
        let getter_refs = scoping.get_resolved_reference_ids(signal.getter);
        facts.getter_refs.extend(getter_refs);
        let setter_unused =
            signal.setter.is_none_or(|s| scoping.get_resolved_reference_ids(s).is_empty());
        let only_called = getter_refs.iter().all(|r| collector.called.contains(r));
        if !(optimize && signal.is_foldable_shape && setter_unused && only_called) {
            continue;
        }
        for &r in getter_refs {
            facts.folded_refs.insert(r, signal.literal.clone());
        }
        facts.folded_bindings.insert(scoping.symbol_span(signal.getter).start);
        let name = scoping.symbol_name(signal.getter);
        reports.push(
            Report::new(
                Code::SignalFolded,
                signal.span,
                format!(
                    "`{name}` is never written: its setter is unused and every read is a call, \
                     so it compiled to a plain constant."
                ),
            )
            .data("signal", name),
        );
    }
    facts
}

fn runtime_imports(program: &Program<'_>, module_name: &str) -> HashMap<SymbolId, Primitive> {
    let mut primitives = HashMap::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let source = import.source.value.as_str();
        if import.import_kind.is_type()
            || !(source == module_name || RUNTIME_MODULES.contains(&source))
        {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                continue;
            };
            if specifier.import_kind.is_type() {
                continue;
            }
            let primitive = match specifier.imported.name().as_str() {
                "signal" => Primitive::Signal,
                "computed" => Primitive::Computed,
                _ => continue,
            };
            primitives.insert(specifier.local.symbol_id(), primitive);
        }
    }
    primitives
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
    primitives: HashMap<SymbolId, Primitive>,
    /// References that are the callee of a plain zero-argument call.
    called: HashSet<ReferenceId>,
    signals: Vec<SignalDecl>,
    computed_getters: Vec<SymbolId>,
}

impl Collector<'_> {
    fn primitive(&self, callee: &Expression<'_>) -> Option<Primitive> {
        let Expression::Identifier(id) = callee else { return None };
        let symbol = self.scoping.get_reference(id.reference_id.get()?).symbol_id()?;
        self.primitives.get(&symbol).copied()
    }

    fn signal(&mut self, declarator: &VariableDeclarator<'_>, call: &CallExpression<'_>) {
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return };
        let binding = |index: usize| match pattern.elements.get(index) {
            Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
            _ => None,
        };
        let Some(getter) = binding(0) else { return };
        let setter = binding(1);
        let is_foldable_shape = pattern.rest.is_none()
            && (1..=2).contains(&pattern.elements.len())
            && (pattern.elements.len() == 1 || setter.is_some())
            && declarator.type_annotation.is_none()
            && call.type_arguments.is_none()
            && (1..=2).contains(&call.arguments.len())
            && call.arguments.first().and_then(Argument::as_expression).is_some()
            && call.arguments.get(1).is_none_or(is_plain_options);
        let literal =
            call.arguments.first().and_then(Argument::as_expression).and_then(static_text_alone);
        self.signals.push(SignalDecl {
            span: declarator.span,
            getter,
            setter,
            is_foldable_shape,
            literal,
        });
    }
}

fn static_text_alone(e: &Expression<'_>) -> Option<String> {
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
            match self.primitive(&call.callee) {
                Some(Primitive::Signal) => self.signal(it, call),
                Some(Primitive::Computed) => {
                    if let BindingPattern::BindingIdentifier(id) = &it.id {
                        self.computed_getters.push(id.symbol_id());
                    }
                }
                None => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}
