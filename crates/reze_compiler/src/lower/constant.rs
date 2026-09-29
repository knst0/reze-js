use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::Analysis;

/// The text a constant renders as: string literals, integers below 1e15, templates without
/// expressions, `+` chains with a string on one side of each `+`, and reads of folded signals
/// with such an initializer.
pub fn static_text(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::NumericLiteral(n) => format_integer(n.value),
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            match (string_value(&b.left), string_value(&b.right)) {
                (Some(left), Some(right)) => Some(left + &right),
                (Some(left), None) => Some(left + &static_text(&b.right, analysis)?),
                (None, Some(right)) => Some(static_text(&b.left, analysis)? + &right),
                (None, None) => None,
            }
        }
        Expression::CallExpression(call) => analysis.folded_read(call)?.1.text.clone(),
        inner => string_value(inner),
    }
}

/// A statically known string; numbers are excluded so `1 + 2` never folds to `"12"`.
fn string_value(e: &Expression<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first()?.value.cooked.as_ref().map(|cooked| cooked.to_string())
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            Some(string_value(&b.left)? + &string_value(&b.right)?)
        }
        _ => None,
    }
}

/// `String(n)` of an integer below 1e15; other numbers are left to the runtime.
pub fn format_integer(n: f64) -> Option<String> {
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15).then(|| format!("{}", n as i64))
}

pub enum Literal {
    Str(String),
    Bool(bool),
    Nullish,
}

pub fn literal(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<Literal> {
    if let Some(text) = static_text(e, analysis) {
        return Some(Literal::Str(text));
    }
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(Literal::Bool(b.value)),
        Expression::NullLiteral(_) => Some(Literal::Nullish),
        Expression::Identifier(id) if id.name == "undefined" => Some(Literal::Nullish),
        _ => None,
    }
}

/// Truthiness of a literal; `None` when not statically known.
pub fn literal_truthy(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<bool> {
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::NullLiteral(_) => Some(false),
        Expression::Identifier(id) if id.name == "undefined" => Some(false),
        Expression::NumericLiteral(n) => Some(n.value != 0.0 && !n.value.is_nan()),
        _ => static_text(e, analysis).map(|text| !text.is_empty()),
    }
}

/// `style={{…}}` with only literal values, as `a:b;c:d`.
pub fn static_style(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<String> {
    let Expression::ObjectExpression(object) = e.without_parentheses() else { return None };
    if object.properties.is_empty() {
        return None;
    }
    let mut out = String::new();
    for property in &object.properties {
        let (key, value) = static_property(property)?;
        let value = static_text(value, analysis)?;
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(key);
        out.push(':');
        out.push_str(&value);
    }
    Some(out)
}

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

/// A static class value flattened like the runtime's: keys in first-set order, a later set
/// overriding an earlier one.
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

    /// Adds `e`; `None` when any part of it is not static.
    pub fn add(&mut self, e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<()> {
        match e.without_parentheses() {
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    let (key, value) = static_property(property)?;
                    self.set(key, literal_truthy(value, analysis)?);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    let item = element.as_expression()?;
                    match item.without_parentheses() {
                        Expression::ObjectExpression(_) | Expression::ArrayExpression(_) => {
                            self.add(item, analysis)?
                        }
                        _ => match literal(item, analysis)? {
                            Literal::Str(text) if !text.is_empty() => self.set(&text, true),
                            Literal::Bool(true) => self.set("true", true),
                            Literal::Str(_) | Literal::Bool(false) | Literal::Nullish => {}
                        },
                    }
                }
            }
            _ => match literal(e, analysis)? {
                Literal::Str(text) => self.set(&text, true),
                Literal::Bool(_) | Literal::Nullish => {}
            },
        }
        Some(())
    }

    /// The `class` attribute: whitespace-split tokens of the enabled keys, deduplicated.
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

/// Whether evaluating `e` can read reactive state: a call, tagged template or member access
/// outside nested functions, a rewritten props read, or with `jsx_is_dynamic` any JSX. Reads
/// of folded signals are constants.
pub fn is_dynamic(e: &Expression<'_>, jsx_is_dynamic: bool, analysis: &Analysis<'_>) -> bool {
    let mut check = DynamicCheck { jsx_is_dynamic, analysis, found: false };
    check.visit_expression(e);
    check.found
}

struct DynamicCheck<'c, 's> {
    jsx_is_dynamic: bool,
    analysis: &'c Analysis<'s>,
    found: bool,
}

impl<'a> Visit<'a> for DynamicCheck<'_, '_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.analysis.folded_read(it).is_none() {
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
        self.found |= self.analysis.props.is_read(it) || self.analysis.asyncs.is_read(it);
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
