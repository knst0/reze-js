use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::Analysis;
use crate::kind::Kind;

/// Rendered text of static strings, templates, integers below 1e15 and folded signals.
pub fn static_text(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::NumericLiteral(n) => format_integer(n.value),
        Expression::UnaryExpression(_) => format_integer(numeric_value(e, analysis)?),
        Expression::CallExpression(call) => analysis.folded_read(call)?.1.text.clone(),
        inner => string_value(inner, analysis),
    }
}

/// A statically known string; numbers are excluded so `1 + 2` never folds to `"12"`.
fn string_value(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) => {
            let mut text = String::new();
            for (index, quasi) in t.quasis.iter().enumerate() {
                text.push_str(quasi.value.cooked.as_ref()?.as_str());
                if let Some(expression) = t.expressions.get(index) {
                    text.push_str(&primitive_text(expression, analysis)?);
                }
            }
            Some(text)
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            match (string_value(&b.left, analysis), string_value(&b.right, analysis)) {
                (Some(left), Some(right)) => Some(left + &right),
                (Some(left), None) => Some(left + &primitive_text(&b.right, analysis)?),
                (None, Some(right)) => Some(primitive_text(&b.left, analysis)? + &right),
                (None, None) => None,
            }
        }
        _ => None,
    }
}

fn primitive_text(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<String> {
    if let Some(number) = numeric_value(e, analysis) {
        return format_integer(number);
    }
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::NullLiteral(_) => Some("null".to_string()),
        Expression::Identifier(id) if is_global(id, "undefined", analysis) => {
            Some("undefined".to_string())
        }
        _ => static_text(e, analysis),
    }
}

fn is_global(id: &IdentifierReference<'_>, name: &str, analysis: &Analysis<'_>) -> bool {
    id.name == name
        && id.reference_id.get().is_some_and(|reference| {
            analysis.scoping.get_reference(reference).symbol_id().is_none()
        })
}

fn numeric_value(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<f64> {
    match e.without_parentheses() {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::Identifier(id) if is_global(id, "NaN", analysis) => Some(f64::NAN),
        Expression::Identifier(id) if is_global(id, "Infinity", analysis) => Some(f64::INFINITY),
        Expression::UnaryExpression(u) => {
            let value = numeric_value(&u.argument, analysis)?;
            match u.operator {
                UnaryOperator::UnaryNegation => Some(-value),
                UnaryOperator::UnaryPlus => Some(value),
                _ => None,
            }
        }
        Expression::BinaryExpression(b) => {
            let left = numeric_value(&b.left, analysis)?;
            let right = numeric_value(&b.right, analysis)?;
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
        Expression::Identifier(id) if is_global(id, "undefined", analysis) => {
            Some(Literal::Nullish)
        }
        _ => None,
    }
}

/// Truthiness of a literal; `None` when not statically known.
pub fn literal_truthy(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<bool> {
    if let Some(number) = numeric_value(e, analysis) {
        return Some(number != 0.0 && !number.is_nan());
    }
    if let Expression::CallExpression(call) = e.without_parentheses()
        && let Some((_, fold)) = analysis.folded_read(call)
        && fold.kind == Some(Kind::Numeric)
    {
        return fold
            .text
            .as_ref()?
            .parse::<f64>()
            .ok()
            .map(|number| number != 0.0 && !number.is_nan());
    }
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::NullLiteral(_) => Some(false),
        Expression::Identifier(id) if is_global(id, "undefined", analysis) => Some(false),
        _ => static_text(e, analysis).map(|text| !text.is_empty()),
    }
}

/// Whether the syntax proves `e` never evaluates to `undefined`: literals, templates, fresh
/// objects, functions and JSX, `new` and promises, and operators whose result excludes it.
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

/// The entries of an object literal whose spread dissolves: static keys but `__proto__`, and
/// values that never evaluate to `undefined`. `None` keeps the generic spread.
pub fn inline_entries<'x, 'a>(
    arg: &'x Expression<'a>,
) -> Option<std::vec::Vec<(&'a str, &'x Expression<'a>)>> {
    let Expression::ObjectExpression(object) = arg.without_parentheses() else { return None };
    object
        .properties
        .iter()
        .map(|property| {
            let (key, value) = static_property(property)?;
            (key != "__proto__" && is_defined(value)).then_some((key, value))
        })
        .collect()
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
/// of folded signals and of the key property of a keyed row are constants.
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

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if !self.analysis.keyed.is_key_read(it) {
            self.found = true;
        }
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
