use oxc_ast::ast::*;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

use crate::analyze::Analysis;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A number or bigint, whose text is never empty.
    Numeric,
    String,
}

const STRING_METHODS: [&str; 7] =
    ["toString", "toFixed", "join", "toUpperCase", "toLowerCase", "trim", "padStart"];

/// `Some` only when the syntax proves every evaluation of `e` yields that kind.
pub fn static_kind(e: &Expression<'_>, analysis: &Analysis<'_>) -> Option<Kind> {
    match e.without_parentheses() {
        Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => Some(Kind::Numeric),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some(Kind::String),
        Expression::UnaryExpression(unary) => matches!(
            unary.operator,
            UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::BitwiseNot
        )
        .then_some(Kind::Numeric),
        Expression::BinaryExpression(binary) => binary_kind(binary, analysis),
        Expression::ConditionalExpression(conditional) => {
            let consequent = static_kind(&conditional.consequent, analysis)?;
            (static_kind(&conditional.alternate, analysis)? == consequent).then_some(consequent)
        }
        Expression::CallExpression(call) => call_kind(call, analysis),
        _ => None,
    }
}

/// Whether the syntax proves every evaluation of `e` yields a boolean.
pub fn is_boolean(e: &Expression<'_>, analysis: &Analysis<'_>) -> bool {
    match e.without_parentheses() {
        Expression::BooleanLiteral(_) => true,
        Expression::UnaryExpression(unary) => unary.operator == UnaryOperator::LogicalNot,
        Expression::BinaryExpression(binary) => matches!(
            binary.operator,
            BinaryOperator::Equality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqualThan
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqualThan
                | BinaryOperator::In
                | BinaryOperator::Instanceof
        ),
        Expression::LogicalExpression(logical) => {
            logical.operator != LogicalOperator::Coalesce
                && is_boolean(&logical.left, analysis)
                && is_boolean(&logical.right, analysis)
        }
        Expression::ConditionalExpression(conditional) => {
            is_boolean(&conditional.consequent, analysis)
                && is_boolean(&conditional.alternate, analysis)
        }
        Expression::CallExpression(call) => {
            !call.optional
                && matches!(&call.callee, Expression::Identifier(id) if id.name == "Boolean" && is_global(id, analysis))
        }
        _ => false,
    }
}

fn binary_kind(binary: &BinaryExpression<'_>, analysis: &Analysis<'_>) -> Option<Kind> {
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
            match (static_kind(&binary.left, analysis), static_kind(&binary.right, analysis)) {
                (Some(Kind::String), _) | (_, Some(Kind::String)) => Some(Kind::String),
                (Some(Kind::Numeric), Some(Kind::Numeric)) => Some(Kind::Numeric),
                _ => None,
            }
        }
        _ => None,
    }
}

fn call_kind(call: &CallExpression<'_>, analysis: &Analysis<'_>) -> Option<Kind> {
    if call.optional {
        return None;
    }
    if let Some((_, fold)) = analysis.folded_read(call) {
        return fold.kind;
    }
    match &call.callee {
        Expression::Identifier(id) if is_global(id, analysis) => match id.name.as_str() {
            "String" => Some(Kind::String),
            "Number" => Some(Kind::Numeric),
            _ => None,
        },
        Expression::StaticMemberExpression(member)
            if !member.optional && STRING_METHODS.contains(&member.property.name.as_str()) =>
        {
            Some(Kind::String)
        }
        _ => None,
    }
}

fn is_global(id: &IdentifierReference<'_>, analysis: &Analysis<'_>) -> bool {
    id.reference_id.get().is_some_and(|r| analysis.scoping.get_reference(r).symbol_id().is_none())
}
