//! Props destructured in a component's parameter, rewritten to lazy reads of one props object
//! (SPEC §7.8, §15.7, §16.7). The rewrite is JSX semantics, not an optimization: it runs in every
//! mode.

use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::{ScopeFlags, ScopeId};
use oxc_syntax::symbol::SymbolId;

use super::{Lowerer, has_jsx, is_component_name};
use crate::analyze::static_text_alone;
use crate::diagnostic::{Code, Report};
use crate::ir::{Embed, Hole, HoleKind, PropsFallback, PropsSplit, PropsTemporary};

pub struct PropsPlan<'p, 'a> {
    pub bindings: Vec<PropsBinding<'p, 'a>>,
    pub rest: Option<PropsRestPlan>,
}

pub struct PropsBinding<'p, 'a> {
    pub symbol: SymbolId,
    /// Keys from the props object to the value, outermost first.
    pub path: Vec<String>,
    /// Absent when the binding has no default or `undefined`.
    pub default: Option<&'p Expression<'a>>,
}

pub struct PropsRestPlan {
    pub symbol: SymbolId,
    /// Top-level keys the rest excludes, in source order.
    pub keys: Vec<String>,
}

/// The rewrite of a component's parameters. `Ok(None)` when the first parameter is not an
/// object pattern; `Err` holds the `data.reason` of PROPS_DESTRUCTURED. `function_body` is
/// `Some` for non-arrow functions, whose parameters and body must not read `arguments`; arrows
/// pass `None`.
pub fn props_plan<'p, 'a>(
    params: &'p FormalParameters<'a>,
    function_body: Option<&FunctionBody<'a>>,
    is_generator: bool,
    scoping: &Scoping,
) -> Result<Option<PropsPlan<'p, 'a>>, &'static str> {
    let Some(first) = params.items.first() else { return Ok(None) };
    let BindingPattern::ObjectPattern(pattern) = &first.pattern else { return Ok(None) };
    if is_generator {
        return Err("generator");
    }
    if params.items.len() > 1 || params.rest.is_some() {
        return Err("params");
    }
    let mut bindings = Vec::new();
    collect_object(pattern, &mut Vec::new(), scoping, &mut bindings)?;
    let rest = match &pattern.rest {
        Some(rest) => {
            let BindingPattern::BindingIdentifier(id) = &rest.argument else {
                return Err("nested-rest");
            };
            let keys: Result<Vec<String>, _> =
                pattern.properties.iter().map(|p| static_key(&p.key)).collect();
            Some(PropsRestPlan { symbol: id.symbol_id(), keys: keys? })
        }
        None => None,
    };
    let is_written = |symbol: SymbolId| {
        scoping.get_resolved_references(symbol).any(|r| r.is_write())
            || !scoping.symbol_redeclarations(symbol).is_empty()
    };
    if bindings.iter().map(|b| b.symbol).chain(rest.as_ref().map(|r| r.symbol)).any(is_written) {
        return Err("written");
    }
    if !defaults_hoist(&bindings, rest.as_ref().map(|r| r.symbol), scoping) {
        return Err("default");
    }
    if let Some(body) = function_body {
        let mut check = ArgumentsCheck { found: false };
        check.visit_formal_parameters(params);
        check.visit_function_body(body);
        if check.found {
            return Err("arguments");
        }
    }
    Ok(Some(PropsPlan { bindings, rest }))
}

fn collect_object<'p, 'a>(
    pattern: &'p ObjectPattern<'a>,
    path: &mut Vec<String>,
    scoping: &Scoping,
    bindings: &mut Vec<PropsBinding<'p, 'a>>,
) -> Result<(), &'static str> {
    if !path.is_empty() && pattern.rest.is_some() {
        return Err("nested-rest");
    }
    for property in &pattern.properties {
        path.push(static_key(&property.key)?);
        collect_value(&property.value, path, scoping, bindings)?;
        path.pop();
    }
    Ok(())
}

fn collect_value<'p, 'a>(
    value: &'p BindingPattern<'a>,
    path: &mut Vec<String>,
    scoping: &Scoping,
    bindings: &mut Vec<PropsBinding<'p, 'a>>,
) -> Result<(), &'static str> {
    let (id, default) = match value {
        BindingPattern::BindingIdentifier(id) => (id, None),
        BindingPattern::ObjectPattern(pattern) => {
            return collect_object(pattern, path, scoping, bindings);
        }
        BindingPattern::AssignmentPattern(assignment) => {
            let BindingPattern::BindingIdentifier(id) = &assignment.left else {
                return Err("nested-default");
            };
            if is_undefined(&assignment.right, scoping) {
                (id, None)
            } else if is_pure(&assignment.right) {
                (id, Some(&assignment.right))
            } else {
                return Err("default");
            }
        }
        BindingPattern::ArrayPattern(_) => return Err("array-pattern"),
    };
    bindings.push(PropsBinding { symbol: id.symbol_id(), path: path.clone(), default });
    Ok(())
}

fn static_key(key: &PropertyKey<'_>) -> Result<String, &'static str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Ok(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Ok(s.value.to_string()),
        PropertyKey::NumericLiteral(n) if n.value.fract() == 0.0 && n.value.abs() < 1e15 => {
            Ok(format!("{}", n.value as i64))
        }
        _ => Err("computed-key"),
    }
}

/// A literal (SPEC §7.10), a boolean or `null`: repeated at each read instead of hoisted.
fn is_literal_default(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::UnaryExpression(unary) => {
            unary.operator == UnaryOperator::UnaryNegation
                && matches!(unary.argument, Expression::NumericLiteral(_))
        }
        other => static_text_alone(other).is_some(),
    }
}

/// An expression whose evaluation calls no user code the compiler can see (SPEC §16.7):
/// literals, names, member reads, functions, untagged templates, operators, and arrays and
/// objects of such parts.
fn is_pure(e: &Expression<'_>) -> bool {
    match e {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Identifier(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.iter().all(is_pure),
        Expression::StaticMemberExpression(member) => is_pure(&member.object),
        Expression::ComputedMemberExpression(member) => {
            is_pure(&member.object) && is_pure(&member.expression)
        }
        Expression::PrivateFieldExpression(member) => is_pure(&member.object),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => is_pure(&member.object),
            ChainElement::ComputedMemberExpression(member) => {
                is_pure(&member.object) && is_pure(&member.expression)
            }
            ChainElement::PrivateFieldExpression(member) => is_pure(&member.object),
            ChainElement::TSNonNullExpression(non_null) => is_pure(&non_null.expression),
            ChainElement::CallExpression(_) => false,
        },
        Expression::UnaryExpression(unary) => {
            unary.operator != UnaryOperator::Delete && is_pure(&unary.argument)
        }
        Expression::BinaryExpression(binary) => is_pure(&binary.left) && is_pure(&binary.right),
        Expression::LogicalExpression(logical) => is_pure(&logical.left) && is_pure(&logical.right),
        Expression::ConditionalExpression(conditional) => {
            is_pure(&conditional.test)
                && is_pure(&conditional.consequent)
                && is_pure(&conditional.alternate)
        }
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| match element {
            ArrayExpressionElement::SpreadElement(_) => false,
            ArrayExpressionElement::Elision(_) => true,
            other => is_pure(other.to_expression()),
        }),
        Expression::ObjectExpression(object) => object.properties.iter().all(|property| {
            let ObjectPropertyKind::ObjectProperty(property) = property else { return false };
            property.key.as_expression().is_none_or(is_pure) && is_pure(&property.value)
        }),
        Expression::ParenthesizedExpression(parenthesized) => is_pure(&parenthesized.expression),
        Expression::TSAsExpression(e) => is_pure(&e.expression),
        Expression::TSSatisfiesExpression(e) => is_pure(&e.expression),
        Expression::TSNonNullExpression(e) => is_pure(&e.expression),
        Expression::TSTypeAssertion(e) => is_pure(&e.expression),
        Expression::TSInstantiationExpression(e) => is_pure(&e.expression),
        _ => false,
    }
}

/// The global `undefined`: a default that changes nothing.
fn is_undefined(e: &Expression<'_>, scoping: &Scoping) -> bool {
    matches!(e.without_parentheses(), Expression::Identifier(id)
        if id.name.as_str() == "undefined"
            && id.reference_id.get().is_none_or(|r| !scoping.has_binding(r)))
}

/// Whether every default, evaluated in source order at the start of the body, reads what it
/// read in the parameter list: outside nested functions it reads only earlier bindings of the
/// pattern, and no name resolves to a declaration of the body.
fn defaults_hoist(
    bindings: &[PropsBinding<'_, '_>],
    rest: Option<SymbolId>,
    scoping: &Scoping,
) -> bool {
    let Some(first) = bindings.first() else { return true };
    let order: HashMap<SymbolId, usize> =
        bindings.iter().enumerate().map(|(index, binding)| (binding.symbol, index)).collect();
    let mut check = DefaultScope {
        scoping,
        function_scope: scoping.symbol_scope_id(first.symbol),
        order: &order,
        rest,
        index: 0,
        function_depth: 0,
        hoists: true,
    };
    for (index, binding) in bindings.iter().enumerate() {
        if let Some(default) = binding.default {
            check.index = index;
            check.visit_expression(default);
        }
    }
    check.hoists
}

struct DefaultScope<'s> {
    scoping: &'s Scoping,
    /// The scope of the pattern's bindings, which the body's declarations share.
    function_scope: ScopeId,
    /// Pattern bindings by their position in source order.
    order: &'s HashMap<SymbolId, usize>,
    rest: Option<SymbolId>,
    /// Position of the binding whose default is visited.
    index: usize,
    /// Functions of the default around the visited node: their bodies run later.
    function_depth: u32,
    hoists: bool,
}

impl DefaultScope<'_> {
    fn keeps_meaning(&self, id: &IdentifierReference<'_>) -> bool {
        let symbol = id.reference_id.get().and_then(|r| self.scoping.get_reference(r).symbol_id());
        let Some(symbol) = symbol else {
            return self.scoping.get_binding(self.function_scope, id.name).is_none();
        };
        if let Some(&order) = self.order.get(&symbol) {
            return self.function_depth > 0 || order < self.index;
        }
        if Some(symbol) == self.rest {
            return self.function_depth > 0;
        }
        let scope = self.scoping.symbol_scope_id(symbol);
        if scope == self.function_scope {
            return self.scoping.symbol_redeclarations(symbol).is_empty();
        }
        self.scoping.scope_ancestors(scope).any(|s| s == self.function_scope)
            || self.scoping.get_binding(self.function_scope, id.name).is_none()
    }
}

impl<'a> Visit<'a> for DefaultScope<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.hoists &= self.keeps_meaning(it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        self.function_depth += 1;
        walk::walk_function(self, it, flags);
        self.function_depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.function_depth -= 1;
    }
}

/// Finds `arguments` of the function itself: nested non-arrow functions have their own.
struct ArgumentsCheck {
    found: bool,
}

impl<'a> Visit<'a> for ArgumentsCheck {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.found |= it.name.as_str() == "arguments";
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
}

/// Human wording of a PROPS_DESTRUCTURED reason.
fn describe(reason: &str) -> &'static str {
    match reason {
        "computed-key" => "a computed key",
        "default" => {
            "a default that cannot run once at the component's start (a call, `new`, a tagged \
             template, or a name declared later or in the body)"
        }
        "nested-default" => "a default on a nested pattern",
        "nested-rest" => "a rest element in a nested pattern",
        "written" => "a destructured name that is assigned",
        "arguments" => "a body that reads `arguments`",
        "generator" => "a generator function",
        "params" => "more than one parameter",
        _ => "an array pattern",
    }
}

/// The rest of a rewritten parameter: `const binding = splitProps(props, keys)[1];`.
pub struct PropsRest {
    pub binding: Span,
    pub keys: Vec<String>,
}

/// A default evaluated once at the start of the body into a temporary (SPEC §16.7).
#[derive(Clone)]
pub struct HoistedDefault {
    pub value: Span,
    /// Name of the destructured binding, the base of the temporary's name.
    pub binding: String,
}

/// What a rewritten parameter declares at the start of the body.
pub struct PropsEntry {
    pub rest: Option<PropsRest>,
    /// In source order.
    pub hoisted: Vec<HoistedDefault>,
}

#[derive(Clone)]
pub enum PropsDefault {
    /// Repeated at each read (SPEC §15.7).
    Literal(Span),
    Hoisted(HoistedDefault),
}

/// A reference to a rewritten binding.
pub struct PropsRead<'f> {
    /// Start of the destructured parameter the binding comes from.
    pub param: u32,
    pub path: &'f [String],
    /// `None` in type positions, where only the path is valid.
    pub default: Option<&'f PropsDefault>,
}

struct RewrittenBinding {
    param: u32,
    path: Vec<String>,
    default: Option<PropsDefault>,
}

/// Every component's destructured parameter in a module and the references it rewrites.
#[derive(Default)]
pub struct PropsFacts {
    /// By the start of the destructured parameter: its entry when rewritten, the reason otherwise.
    params: HashMap<u32, Result<PropsEntry, &'static str>>,
    bindings: Vec<RewrittenBinding>,
    /// Binding index and whether the reference is a type position.
    reads: HashMap<ReferenceId, (usize, bool)>,
}

impl PropsFacts {
    pub fn collect(program: &Program<'_>, scoping: &Scoping) -> Self {
        let mut collector = ComponentCollector { scoping, facts: PropsFacts::default() };
        collector.visit_program(program);
        collector.facts
    }

    pub fn param(
        &self,
        params: &FormalParameters<'_>,
    ) -> Option<&Result<PropsEntry, &'static str>> {
        self.params.get(&params.items.first()?.pattern.span().start)
    }

    pub fn rewrites(&self, reference: ReferenceId) -> bool {
        self.reads.get(&reference).is_some_and(|&(_, in_type)| !in_type)
    }

    pub fn is_read(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.reads.contains_key(&r))
    }

    pub fn read(&self, id: &IdentifierReference<'_>) -> Option<PropsRead<'_>> {
        let &(index, in_type) = self.reads.get(&id.reference_id.get()?)?;
        let binding = &self.bindings[index];
        Some(PropsRead {
            param: binding.param,
            path: &binding.path,
            default: binding.default.as_ref().filter(|_| !in_type),
        })
    }
}

/// Components as the lowering sees them: `function C` with a capitalized name, or
/// `const C = …` with an arrow or a function expression not named as a component, whose body
/// contains JSX.
struct ComponentCollector<'s> {
    scoping: &'s Scoping,
    facts: PropsFacts,
}

impl ComponentCollector<'_> {
    fn component(
        &mut self,
        params: &FormalParameters<'_>,
        function_body: Option<&FunctionBody<'_>>,
        is_generator: bool,
    ) {
        let Some(first) = params.items.first() else { return };
        let param = first.pattern.span().start;
        let decision = match props_plan(params, function_body, is_generator, self.scoping) {
            Ok(None) => return,
            Err(reason) => Err(reason),
            Ok(Some(plan)) => {
                let mut hoisted = Vec::new();
                for binding in plan.bindings {
                    let index = self.facts.bindings.len();
                    for &r in self.scoping.get_resolved_reference_ids(binding.symbol) {
                        let in_type = !self.scoping.get_reference(r).flags().is_value();
                        self.facts.reads.insert(r, (index, in_type));
                    }
                    let default = binding.default.map(|value| {
                        if is_literal_default(value) {
                            return PropsDefault::Literal(value.span());
                        }
                        let default = HoistedDefault {
                            value: value.span(),
                            binding: self.scoping.symbol_name(binding.symbol).to_string(),
                        };
                        hoisted.push(default.clone());
                        PropsDefault::Hoisted(default)
                    });
                    self.facts.bindings.push(RewrittenBinding {
                        param,
                        path: binding.path,
                        default,
                    });
                }
                let rest = plan.rest.map(|rest| PropsRest {
                    binding: self.scoping.symbol_span(rest.symbol),
                    keys: rest.keys,
                });
                Ok(PropsEntry { rest, hoisted })
            }
        };
        self.facts.params.insert(param, decision);
    }
}

impl<'a> Visit<'a> for ComponentCollector<'_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if let (Some(id), Some(body)) = (&it.id, &it.body)
            && is_component_name(id.name.as_str())
            && has_jsx(|c| c.visit_function_body(body))
        {
            self.component(&it.params, Some(body), it.generator);
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && is_component_name(id.name.as_str())
        {
            match it.init.as_ref().map(Expression::without_parentheses) {
                Some(Expression::ArrowFunctionExpression(arrow))
                    if has_jsx(|c| c.visit_arrow_function_body(&arrow.body)) =>
                {
                    self.component(&arrow.params, None, false);
                }
                Some(Expression::FunctionExpression(function))
                    if is_declared_component(function)
                        && let Some(body) = &function.body
                        && has_jsx(|c| c.visit_function_body(body)) =>
                {
                    self.component(&function.params, Some(body), function.generator);
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

/// Whether `const C = function …` makes `function` the component `C`: a function named as a
/// component is one by its own name.
pub fn is_declared_component(function: &Function<'_>) -> bool {
    function.id.as_ref().is_none_or(|id| !is_component_name(id.name.as_str()))
}

/// Where the entry declarations go in a block body: after the directives.
pub fn block_start(body: &FunctionBody<'_>) -> u32 {
    body.directives.last().map_or(body.span.start + 1, |d| d.span.end)
}

/// The defaults of `pattern` and its nested patterns, in source order.
fn pattern_defaults<'p, 'a>(pattern: &'p ObjectPattern<'a>, out: &mut Vec<&'p Expression<'a>>) {
    for property in &pattern.properties {
        match &property.value {
            BindingPattern::ObjectPattern(nested) => pattern_defaults(nested, out),
            BindingPattern::AssignmentPattern(assignment) => out.push(&assignment.right),
            _ => {}
        }
    }
}

impl<'a> Lowerer<'a, '_> {
    /// PROPS_REWRITTEN or PROPS_DESTRUCTURED for the component `name` taking `params`.
    pub(super) fn component_scope(&mut self, name: &str, params: &FormalParameters<'a>) {
        let Some(decision) = self.facts.props.param(params) else { return };
        let span = params.items[0].pattern.span();
        let report = match decision {
            Ok(_) => Report::new(
                Code::PropsRewritten,
                span,
                format!(
                    "`{name}` destructures its props: the pattern became one props object and \
                     every destructured name a read of it at its use, so each read stays reactive."
                ),
            ),
            Err(reason) => Report::new(
                Code::PropsDestructured,
                span,
                format!(
                    "`{name}` destructures its props with {}, which cannot be rewritten: each value \
                     is read once when the component runs and never updates. Take `props` and read \
                     `props.x` where it is used.",
                    describe(reason)
                ),
            )
            .data("reason", *reason),
        };
        self.report(report.data("component", name));
    }

    fn props_name(&mut self, param: u32) -> &'a str {
        if let Some(name) = self.props_names.get(&param) {
            return name;
        }
        let name = self.fresh("_props$");
        self.props_names.insert(param, name);
        name
    }

    fn props_temporary(&mut self, default: &HoistedDefault) -> &'a str {
        if let Some(name) = self.props_temporaries.get(&default.value.start) {
            return name;
        }
        let name = self.fresh(&format!("_{}$default", default.binding));
        self.props_temporaries.insert(default.value.start, name);
        name
    }

    /// `props` in place of a rewritten parameter's pattern.
    pub(super) fn props_param(&mut self, param: &FormalParameter<'a>) -> Option<Hole<'a>> {
        let span = param.pattern.span();
        let Some(Ok(_)) = self.facts.props.params.get(&span.start) else { return None };
        let name = self.props_name(span.start);
        Some(Hole { span, kind: HoleKind::PropsParam { name } })
    }

    /// The read replacing `id` at `span`, `{ a }` → `{ a: props.a }` when `shorthand`.
    pub(super) fn props_read(
        &mut self,
        id: &IdentifierReference<'a>,
        span: Span,
        shorthand: bool,
    ) -> Option<Hole<'a>> {
        let facts = self.facts;
        let read = facts.props.read(id)?;
        let path = oxc_allocator::Vec::from_iter_in(
            read.path.iter().map(|key| self.str(key)),
            &self.alloc,
        );
        let fallback = read.default.map(|default| match default {
            PropsDefault::Literal(span) => {
                PropsFallback::Literal(Embed { span: *span, holes: self.vec() })
            }
            PropsDefault::Hoisted(hoisted) => {
                PropsFallback::Temporary(self.props_temporary(hoisted))
            }
        });
        let props = self.props_name(read.param);
        Some(Hole { span, kind: HoleKind::PropsRead { props, path, fallback, shorthand } })
    }

    /// The entry declarations of a rewritten parameter at `span` (SPEC §15.7, §16.7): empty to
    /// insert them in a block body, or an expression body with `body`.
    pub(super) fn props_entry(
        &mut self,
        params: &FormalParameters<'a>,
        span: Span,
        body: Option<&Expression<'a>>,
    ) -> Option<Hole<'a>> {
        let facts = self.facts;
        let Some(Ok(entry)) = facts.props.param(params) else { return None };
        if entry.rest.is_none() && entry.hoisted.is_empty() {
            return None;
        }
        let rest = entry.rest.as_ref().map(|rest| PropsSplit {
            binding: rest.binding,
            keys: oxc_allocator::Vec::from_iter_in(
                rest.keys.iter().map(|key| self.str(key)),
                &self.alloc,
            ),
        });
        let mut values = Vec::new();
        if let BindingPattern::ObjectPattern(pattern) = &params.items[0].pattern {
            pattern_defaults(pattern, &mut values);
        }
        let mut defaults = self.vec();
        for hoisted in &entry.hoisted {
            let value = values.iter().find(|value| value.span() == hoisted.value)?;
            let name = self.props_temporary(hoisted);
            defaults.push(PropsTemporary { name, value: self.expr(value) });
        }
        let props = self.props_name(params.items[0].pattern.span().start);
        let body = body.map(|e| self.expr(e));
        Some(Hole { span, kind: HoleKind::PropsEntry { props, rest, defaults, body } })
    }
}
