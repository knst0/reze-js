//! Static evaluation of expressions (SPEC §7.10, §7.11).

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::Facts;

/// The text a literal child renders as.
pub fn static_text(e: &Expression<'_>, facts: &Facts) -> Option<String> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => format_integer(n.value),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked.as_ref()).map(|c| c.to_string())
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            match (string_value(&b.left), string_value(&b.right)) {
                (Some(l), Some(r)) => Some(l + &r),
                (Some(l), None) => static_text(&b.right, facts).map(|r| l + &r),
                (None, Some(r)) => static_text(&b.left, facts).map(|l| l + &r),
                (None, None) => None,
            }
        }
        Expression::CallExpression(call) => {
            facts.folded_callee(call).and_then(|(_, text)| text).map(str::to_string)
        }
        _ => None,
    }
}

/// A statically known string. Numbers are excluded: `1 + 2` must not fold to `"12"`.
fn string_value(e: &Expression<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked.as_ref()).map(|c| c.to_string())
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            Some(string_value(&b.left)? + &string_value(&b.right)?)
        }
        _ => None,
    }
}

/// `String(n)` for integers; other numbers are left to the runtime.
fn format_integer(n: f64) -> Option<String> {
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15).then(|| format!("{}", n as i64))
}

pub enum Literal {
    Str(String),
    Bool(bool),
    Nullish,
}

pub fn literal(e: &Expression<'_>, facts: &Facts) -> Option<Literal> {
    if let Some(s) = static_text(e, facts) {
        return Some(Literal::Str(s));
    }
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(Literal::Bool(b.value)),
        Expression::NullLiteral(_) => Some(Literal::Nullish),
        Expression::Identifier(id) if id.name.as_str() == "undefined" => Some(Literal::Nullish),
        _ => None,
    }
}

/// JS truthiness of a literal; `None` when not statically known.
pub fn literal_truthy(e: &Expression<'_>, facts: &Facts) -> Option<bool> {
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::NullLiteral(_) => Some(false),
        Expression::Identifier(id) if id.name.as_str() == "undefined" => Some(false),
        Expression::NumericLiteral(n) => Some(n.value != 0.0 && !n.value.is_nan()),
        _ => static_text(e, facts).map(|s| !s.is_empty()),
    }
}

/// `style={{…}}` with all-literal values as `a:b;c:d`; `None` when anything is dynamic or empty.
pub fn static_style(e: &Expression<'_>, facts: &Facts) -> Option<String> {
    let Expression::ObjectExpression(object) = e.without_parentheses() else { return None };
    if object.properties.is_empty() {
        return None;
    }
    let mut out = String::new();
    for property in &object.properties {
        let (key, value) = static_property(property)?;
        let value = static_text(value, facts)?;
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(key);
        out.push(':');
        out.push_str(&value);
    }
    Some(out)
}

fn static_property<'b, 'a>(
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

/// A static `ClassValue` flattened like the runtime's `flattenClassValue`: raw keys in first-set
/// order, a later set overriding an earlier one (`["a", { a: false }]` drops `a`).
#[derive(Default)]
pub struct ClassKeys {
    keys: Vec<(String, bool)>,
}

impl ClassKeys {
    pub fn set(&mut self, key: &str, is_on: bool) {
        match self.keys.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = is_on,
            None => self.keys.push((key.to_string(), is_on)),
        }
    }

    /// Adds `e`; `None` when anything in it is not static (SPEC §7.3).
    pub fn add(&mut self, e: &Expression<'_>, facts: &Facts) -> Option<()> {
        match e.without_parentheses() {
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    let (key, value) = static_property(property)?;
                    self.set(key, literal_truthy(value, facts)?);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    let item = element.as_expression()?;
                    match item.without_parentheses() {
                        Expression::ObjectExpression(_) | Expression::ArrayExpression(_) => {
                            self.add(item, facts)?;
                        }
                        _ => match literal(item, facts)? {
                            Literal::Str(text) if !text.is_empty() => self.set(&text, true),
                            Literal::Bool(true) => self.set("true", true),
                            Literal::Str(_) | Literal::Bool(false) | Literal::Nullish => {}
                        },
                    }
                }
            }
            _ => match literal(e, facts)? {
                Literal::Str(text) => self.set(&text, true),
                Literal::Bool(_) | Literal::Nullish => {}
            },
        }
        Some(())
    }

    /// The `class` attribute value: whitespace-split tokens of the enabled keys, deduplicated.
    pub fn attribute(&self) -> String {
        let mut tokens: Vec<&str> = Vec::new();
        for (key, _) in self.keys.iter().filter(|(_, is_on)| *is_on) {
            for token in key.split_whitespace() {
                if !tokens.contains(&token) {
                    tokens.push(token);
                }
            }
        }
        tokens.join(" ")
    }
}

/// Whether evaluating `e` may read reactive state: a call, a tagged template or a member access
/// outside nested functions (and, with `jsx_is_dynamic`, any JSX). Calls of folded signals are
/// constant reads; rewritten props bindings are member accesses (SPEC §15.7).
pub fn is_dynamic(e: &Expression<'_>, jsx_is_dynamic: bool, facts: &Facts) -> bool {
    let mut check = DynamicCheck { jsx_is_dynamic, facts, found: false };
    check.visit_expression(e);
    check.found
}

struct DynamicCheck<'f> {
    jsx_is_dynamic: bool,
    facts: &'f Facts,
    found: bool,
}

impl<'a> Visit<'a> for DynamicCheck<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.facts.folded_callee(it).is_none() {
            self.found = true;
        }
    }

    fn visit_tagged_template_expression(&mut self, _: &TaggedTemplateExpression<'a>) {
        self.found = true;
    }

    fn visit_static_member_expression(&mut self, _: &StaticMemberExpression<'a>) {
        self.found = true;
    }

    fn visit_computed_member_expression(&mut self, _: &ComputedMemberExpression<'a>) {
        self.found = true;
    }

    fn visit_private_field_expression(&mut self, _: &PrivateFieldExpression<'a>) {
        self.found = true;
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.found |= self.facts.props.is_read(it);
    }

    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found |= self.jsx_is_dynamic;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found |= self.jsx_is_dynamic;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}
