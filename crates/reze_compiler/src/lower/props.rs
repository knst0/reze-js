//! Props destructured in a component's parameter, rewritten to lazy reads of one props object
//! (SPEC §7.8, §15.7). The rewrite is JSX semantics, not an optimization: it runs in every mode.

use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::{Lowerer, has_jsx, is_component_name};
use crate::analyze::static_text_alone;
use crate::diagnostic::{Code, Report};
use crate::ir::{Embed, Hole, HoleKind};

pub struct PropsPlan {
    pub bindings: Vec<PropsBinding>,
    pub rest: Option<PropsRestPlan>,
}

pub struct PropsBinding {
    pub symbol: SymbolId,
    /// Keys from the props object to the value, outermost first.
    pub path: Vec<String>,
}

pub struct PropsRestPlan {
    pub symbol: SymbolId,
    /// Top-level keys the rest excludes, in source order.
    pub keys: Vec<String>,
}

/// The rewrite of a component's parameters. `Ok(None)` when the first parameter is not an
/// object pattern; `Err` holds the `data.reason` of PROPS_DESTRUCTURED. `function_body` is
/// `Some` for non-arrow functions, whose body must not read `arguments`; arrows pass `None`.
pub fn props_plan(
    params: &FormalParameters<'_>,
    function_body: Option<&FunctionBody<'_>>,
    is_generator: bool,
    scoping: &Scoping,
) -> Result<Option<PropsPlan>, &'static str> {
    Ok(plan(params, function_body, is_generator, scoping)?.map(|(plan, _)| plan))
}

/// The plan, with the default of each binding (by index) when it has one.
fn plan(
    params: &FormalParameters<'_>,
    function_body: Option<&FunctionBody<'_>>,
    is_generator: bool,
    scoping: &Scoping,
) -> Result<Option<(PropsPlan, Vec<Option<Span>>)>, &'static str> {
    let Some(first) = params.items.first() else { return Ok(None) };
    let BindingPattern::ObjectPattern(pattern) = &first.pattern else { return Ok(None) };
    if is_generator {
        return Err("generator");
    }
    if params.items.len() > 1 || params.rest.is_some() {
        return Err("params");
    }
    let mut collected = Collected { scoping, bindings: Vec::new(), defaults: Vec::new() };
    collected.object(pattern, &mut Vec::new())?;
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
    let Collected { bindings, defaults, .. } = collected;
    let is_written = |symbol: SymbolId| {
        scoping.get_resolved_references(symbol).any(|r| r.is_write())
            || !scoping.symbol_redeclarations(symbol).is_empty()
    };
    if bindings.iter().map(|b| b.symbol).chain(rest.as_ref().map(|r| r.symbol)).any(is_written) {
        return Err("written");
    }
    if let Some(body) = function_body {
        let mut check = ArgumentsCheck { found: false };
        check.visit_function_body(body);
        if check.found {
            return Err("arguments");
        }
    }
    Ok(Some((PropsPlan { bindings, rest }, defaults)))
}

struct Collected<'s> {
    scoping: &'s Scoping,
    bindings: Vec<PropsBinding>,
    defaults: Vec<Option<Span>>,
}

impl Collected<'_> {
    fn object(
        &mut self,
        pattern: &ObjectPattern<'_>,
        path: &mut Vec<String>,
    ) -> Result<(), &'static str> {
        if !path.is_empty() && pattern.rest.is_some() {
            return Err("nested-rest");
        }
        for property in &pattern.properties {
            path.push(static_key(&property.key)?);
            self.value(&property.value, path)?;
            path.pop();
        }
        Ok(())
    }

    fn value(
        &mut self,
        value: &BindingPattern<'_>,
        path: &mut Vec<String>,
    ) -> Result<(), &'static str> {
        match value {
            BindingPattern::BindingIdentifier(id) => self.bind(id, path, None),
            BindingPattern::ObjectPattern(pattern) => self.object(pattern, path)?,
            BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
                BindingPattern::BindingIdentifier(id)
                    if is_undefined(&assignment.right, self.scoping) =>
                {
                    self.bind(id, path, None);
                }
                BindingPattern::BindingIdentifier(id) if is_literal_default(&assignment.right) => {
                    self.bind(id, path, Some(assignment.right.span()));
                }
                BindingPattern::BindingIdentifier(_) => return Err("default"),
                _ => return Err("nested-default"),
            },
            BindingPattern::ArrayPattern(_) => return Err("array-pattern"),
        }
        Ok(())
    }

    fn bind(&mut self, id: &BindingIdentifier<'_>, path: &[String], default: Option<Span>) {
        self.bindings.push(PropsBinding { symbol: id.symbol_id(), path: path.to_vec() });
        self.defaults.push(default);
    }
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

/// A literal (SPEC §7.10), a boolean or `null`.
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

/// The global `undefined`: a default that changes nothing.
fn is_undefined(e: &Expression<'_>, scoping: &Scoping) -> bool {
    matches!(e.without_parentheses(), Expression::Identifier(id)
        if id.name.as_str() == "undefined"
            && id.reference_id.get().is_none_or(|r| !scoping.has_binding(r)))
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
        "default" => "a default that is not a literal",
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

/// A reference to a rewritten binding.
pub struct PropsRead<'f> {
    /// Start of the destructured parameter the binding comes from.
    pub param: u32,
    pub path: &'f [String],
    /// `None` in type positions, where only the path is valid.
    pub default: Option<Span>,
}

struct RewrittenBinding {
    param: u32,
    path: Vec<String>,
    default: Option<Span>,
}

/// Every component's destructured parameter in a module and the references it rewrites.
#[derive(Default)]
pub struct PropsFacts {
    /// By the start of the destructured parameter: its rest when rewritten, the reason otherwise.
    params: HashMap<u32, Result<Option<PropsRest>, &'static str>>,
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
    ) -> Option<&Result<Option<PropsRest>, &'static str>> {
        self.params.get(&params.items.first()?.pattern.span().start)
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
            default: binding.default.filter(|_| !in_type),
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
        let decision = match plan(params, function_body, is_generator, self.scoping) {
            Ok(None) => return,
            Err(reason) => Err(reason),
            Ok(Some((plan, defaults))) => {
                for (binding, default) in plan.bindings.into_iter().zip(defaults) {
                    let index = self.facts.bindings.len();
                    for &r in self.scoping.get_resolved_reference_ids(binding.symbol) {
                        let in_type = !self.scoping.get_reference(r).flags().is_value();
                        self.facts.reads.insert(r, (index, in_type));
                    }
                    self.facts.bindings.push(RewrittenBinding {
                        param,
                        path: binding.path,
                        default,
                    });
                }
                Ok(plan.rest.map(|rest| PropsRest {
                    binding: self.scoping.symbol_span(rest.symbol),
                    keys: rest.keys,
                }))
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

/// Where the rest declaration goes in a block body: after the directives.
pub fn block_start(body: &FunctionBody<'_>) -> u32 {
    body.directives.last().map_or(body.span.start + 1, |d| d.span.end)
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
        let read = self.facts.props.read(id)?;
        let path = oxc_allocator::Vec::from_iter_in(
            read.path.iter().map(|key| self.str(key)),
            &self.alloc,
        );
        let default = read.default.map(|span| Embed { span, holes: self.vec() });
        let props = self.props_name(read.param);
        Some(Hole { span, kind: HoleKind::PropsRead { props, path, default, shorthand } })
    }

    /// The rest declaration of a rewritten parameter at `span`: empty to insert it in a block
    /// body, or an expression body with `body`.
    pub(super) fn props_rest(
        &mut self,
        params: &FormalParameters<'a>,
        span: Span,
        body: Option<&Expression<'a>>,
    ) -> Option<Hole<'a>> {
        let Some(Ok(Some(rest))) = self.facts.props.param(params) else { return None };
        let binding = rest.binding;
        let keys = oxc_allocator::Vec::from_iter_in(
            rest.keys.iter().map(|key| self.str(key)),
            &self.alloc,
        );
        let props = self.props_name(params.items[0].pattern.span().start);
        let body = body.map(|e| self.expr(e));
        Some(Hole { span, kind: HoleKind::PropsRest { props, binding, keys, body } })
    }
}
