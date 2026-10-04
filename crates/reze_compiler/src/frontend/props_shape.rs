use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_syntax::identifier::is_identifier_name;
use oxc_syntax::scope::{ScopeFlags, ScopeId};
use oxc_syntax::symbol::SymbolId;

pub struct PropsPlan<'p, 'a> {
    pub bindings: Vec<PropsBinding<'p, 'a>>,
    pub rest: Option<PropsRestPlan>,
}

pub struct PropsBinding<'p, 'a> {
    pub symbol: SymbolId,
    pub path: Vec<String>,
    pub default: Option<&'p Expression<'a>>,
}

pub struct PropsRestPlan {
    pub symbol: SymbolId,
    pub keys: Vec<String>,
}

/// The rewrite of a component's parameters. `Ok(None)` when the first
/// parameter is not an object pattern; `Err` holds the `data.reason` of
/// PROPS_DESTRUCTURED. `function_body` is `Some` for non-arrow functions,
/// whose parameters and body must not read `arguments`; arrows pass `None`.
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

/// A literal (SPEC §7.10), a boolean or `null`: repeated at each read instead
/// of hoisted.
pub fn is_literal_default(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::UnaryExpression(unary) => {
            unary.operator == UnaryOperator::UnaryNegation
                && matches!(unary.argument, Expression::NumericLiteral(_))
        }
        other => is_string_constant(other),
    }
}

fn is_string_constant(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        Expression::BinaryExpression(binary) => {
            binary.operator == BinaryOperator::Addition
                && (is_string_constant(&binary.left) || is_string_constant(&binary.right))
                && is_constant_operand(&binary.left)
                && is_constant_operand(&binary.right)
        }
        _ => false,
    }
}

fn is_constant_operand(e: &Expression<'_>) -> bool {
    matches!(e.without_parentheses(), Expression::NumericLiteral(_)) || is_string_constant(e)
}

/// An expression whose evaluation calls no user code the compiler can see
/// (SPEC §16.7): literals, names, member reads, functions, untagged
/// templates, operators, and arrays and objects of such parts.
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

/// Whether every default, evaluated in source order at the start of the body,
/// reads what it read in the parameter list: outside nested functions it
/// reads only earlier bindings of the pattern, and no name resolves to a
/// declaration of the body.
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

/// Finds `arguments` of the function itself: nested non-arrow functions have
/// their own.
struct ArgumentsCheck {
    found: bool,
}

impl<'a> Visit<'a> for ArgumentsCheck {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.found |= it.name.as_str() == "arguments";
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
}

/// Whether any rewritten binding is read in type position under a path no
/// qualified type name can spell: only identifier segments survive there, so
/// a non-identifier segment would dangle once the binding is gone. Callers
/// refuse the whole component rewrite (and its facts) with the existing
/// `PropsDestructured` diagnostic instead of emitting broken code. The final
/// semantic rebuild is not relied on to diagnose this: it may not check TS
/// paths at all.
pub fn has_unnameable_type_read(scoping: &Scoping, bindings: &[PropsBinding<'_, '_>]) -> bool {
    bindings.iter().any(|binding| {
        binding.path.iter().any(|segment| !is_identifier_name(segment))
            && scoping
                .get_resolved_reference_ids(binding.symbol)
                .iter()
                .any(|reference| !scoping.get_reference(*reference).flags().is_value())
    })
}
