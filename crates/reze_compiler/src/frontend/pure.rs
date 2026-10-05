use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_syntax::operator::{AssignmentOperator, LogicalOperator, UnaryOperator};

use crate::html::{clean_jsx_text, decode_entities};

/// A capitalized name denotes a component; anything else is a helper, a
/// native tag, or a flow intrinsic resolved through imports.
pub fn is_component_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

/// Whether `const C = function …` makes `function` the component `C`: a
/// function named as a component is one by its own name.
pub fn is_declared_component(function: &Function<'_>) -> bool {
    function.id.as_ref().is_none_or(|id| !is_component_name(id.name.as_str()))
}

/// Whether the nodes `visit` walks contain JSX.
pub fn has_jsx(visit: impl FnOnce(&mut JsxCheck)) -> bool {
    let mut check = JsxCheck { found: false };
    visit(&mut check);
    check.found
}

pub struct JsxCheck {
    found: bool,
}

impl<'a> Visit<'a> for JsxCheck {
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found = true;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found = true;
    }
}

/// Whitespace-only text and empty `{}` render nothing.
pub fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(c) => c.expression.as_expression().is_some(),
        _ => true,
    }
}

/// Whether the syntax proves `e` never evaluates to `undefined`: literals,
/// templates, fresh objects, functions and JSX, `new` and promises, and
/// operators whose result excludes it.
pub fn is_defined(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::ImportExpression(_)
        | Expression::ImportMeta(_)
        | Expression::NewExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::PrivateInExpression(_) => true,
        Expression::UnaryExpression(u) => u.operator != UnaryOperator::Void,
        Expression::BinaryExpression(_) => true,
        Expression::LogicalExpression(l) => match l.operator {
            LogicalOperator::Or | LogicalOperator::Coalesce => is_defined(&l.right),
            LogicalOperator::And => is_defined(&l.left) && is_defined(&l.right),
        },
        Expression::ConditionalExpression(c) => {
            is_defined(&c.consequent) && is_defined(&c.alternate)
        }
        Expression::SequenceExpression(s) => s.expressions.last().is_some_and(is_defined),
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::LogicalAnd => false,
            _ => is_defined(&a.right),
        },
        Expression::TSAsExpression(e) => is_defined(&e.expression),
        Expression::TSSatisfiesExpression(e) => is_defined(&e.expression),
        Expression::TSTypeAssertion(e) => is_defined(&e.expression),
        Expression::TSNonNullExpression(e) => is_defined(&e.expression),
        Expression::TSInstantiationExpression(e) => is_defined(&e.expression),
        _ => false,
    }
}

/// A statically-known property of an object literal: an init, non-method,
/// non-computed key with its value. `__proto__` filtering is the caller's
/// job; this only reports the static shape.
pub fn static_property<'b, 'a>(
    property: &'b ObjectPropertyKind<'a>,
) -> Option<(&'a str, &'b Expression<'a>)> {
    let ObjectPropertyKind::ObjectProperty(p) = property else { return None };
    if p.computed || p.kind != PropertyKind::Init || p.method {
        return None;
    }
    let key = match &p.key {
        PropertyKey::StaticIdentifier(id) => id.name.as_str(),
        PropertyKey::StringLiteral(s) => s.value.as_str(),
        _ => return None,
    };
    Some((key, &p.value))
}

/// `String(n)` of an integer below 1e15; other numbers are left to the runtime.
pub fn format_integer(n: f64) -> Option<String> {
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15).then(|| format!("{}", n as i64))
}

/// Whether `let [getter, setter?] = signal(init, options?)` has the shape a
/// constant fold requires: an array pattern of one or two plain identifiers,
/// no annotation, and one value argument plus optional plain options. The
/// remaining fold gates (export, setter use, call-only reads, awaiting) live
/// with the facts collectors, not here.
pub fn is_foldable_signal_shape(
    declarator: &VariableDeclarator<'_>,
    call: &CallExpression<'_>,
) -> bool {
    let has_foldable_binding = match &declarator.id {
        BindingPattern::BindingIdentifier(_) => true,
        BindingPattern::ArrayPattern(pattern) => {
            let is_identifier = |index: usize| {
                matches!(
                    pattern.elements.get(index),
                    Some(Some(BindingPattern::BindingIdentifier(_)))
                )
            };
            pattern.rest.is_none()
                && (1..=2).contains(&pattern.elements.len())
                && is_identifier(0)
                && (pattern.elements.len() == 1 || is_identifier(1))
        }
        _ => false,
    };
    has_foldable_binding
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

/// The entries of an object literal whose spread dissolves: static keys but
/// `__proto__`, values that never evaluate to `undefined`, and no duplicate
/// key. `None` keeps the generic spread. A duplicate key must stay generic:
/// dissolving keyed entries would keep only one value per key and silently
/// drop the shadowed value's observable evaluation (`{x: f(), x: g}`
/// evaluates both `f()` and `g()`).
pub fn inline_entries<'x, 'a>(
    arg: &'x Expression<'a>,
) -> Option<Vec<(&'a str, &'x Expression<'a>)>> {
    let Expression::ObjectExpression(object) = arg.without_parentheses() else { return None };
    let mut seen = std::collections::HashSet::new();
    object
        .properties
        .iter()
        .map(|property| {
            let (key, value) = static_property(property)?;
            if key == "__proto__" || !is_defined(value) || !seen.insert(key) {
                return None;
            }
            Some((key, value))
        })
        .collect()
}

/// An object-literal property dissolvable into entries: an init non-method
/// with a defined value under a static key. Unlike [`static_property`],
/// numeric keys qualify here; the caller still excludes `__proto__` shapes
/// through its own shape checks.
pub fn merge_property_is_static(property: &ObjectPropertyKind<'_>) -> bool {
    let ObjectPropertyKind::ObjectProperty(property) = property else { return false };
    if property.kind != PropertyKind::Init || property.method || !is_defined(&property.value) {
        return false;
    }
    match &property.key {
        PropertyKey::StaticIdentifier(key) => !property.computed && key.name != "__proto__",
        PropertyKey::StringLiteral(key) => key.value != "__proto__",
        PropertyKey::NumericLiteral(_) => true,
        _ => false,
    }
}
