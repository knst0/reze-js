//! What an expression evaluates to when that is provable from its syntax alone (SPEC §7.12).

use oxc_ast::ast::*;
use oxc_semantic::{AstNodes, Scoping};
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};

use crate::analyze::Facts;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaticKind {
    /// A number or a bigint: its text is never empty.
    Numeric,
    String,
}

const STRING_METHODS: [&str; 7] =
    ["toString", "toFixed", "join", "toUpperCase", "toLowerCase", "trim", "padStart"];

/// `Some` only when every evaluation of `e` yields a value of that kind; `None` when unsure.
pub fn static_kind<'a>(
    e: &Expression<'a>,
    facts: &Facts,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
) -> Option<StaticKind> {
    match e.without_parentheses() {
        Expression::NumericLiteral(_) => Some(StaticKind::Numeric),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some(StaticKind::String),
        Expression::UnaryExpression(unary) => match unary.operator {
            UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::BitwiseNot => {
                Some(StaticKind::Numeric)
            }
            _ => None,
        },
        Expression::BinaryExpression(binary) => binary_kind(binary, facts, scoping, nodes),
        Expression::ConditionalExpression(conditional) => {
            let consequent = static_kind(&conditional.consequent, facts, scoping, nodes)?;
            let alternate = static_kind(&conditional.alternate, facts, scoping, nodes)?;
            (consequent == alternate).then_some(consequent)
        }
        Expression::CallExpression(call) => call_kind(call, facts, scoping, nodes),
        _ => None,
    }
}

fn binary_kind<'a>(
    binary: &BinaryExpression<'a>,
    facts: &Facts,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
) -> Option<StaticKind> {
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
        | BinaryOperator::ShiftRightZeroFill => Some(StaticKind::Numeric),
        BinaryOperator::Addition => {
            let left = static_kind(&binary.left, facts, scoping, nodes);
            let right = static_kind(&binary.right, facts, scoping, nodes);
            match (left, right) {
                (Some(StaticKind::String), _) | (_, Some(StaticKind::String)) => {
                    Some(StaticKind::String)
                }
                (Some(StaticKind::Numeric), Some(StaticKind::Numeric)) => Some(StaticKind::Numeric),
                _ => None,
            }
        }
        _ => None,
    }
}

fn call_kind<'a>(
    call: &CallExpression<'a>,
    facts: &Facts,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
) -> Option<StaticKind> {
    if call.optional {
        return None;
    }
    if let Some(body) = facts.inlined_body(call, nodes) {
        return static_kind(body, facts, scoping, nodes);
    }
    if let Some((_, Some(text))) = facts.folded_callee(call) {
        return literal_text_kind(text);
    }
    match &call.callee {
        Expression::Identifier(id) if is_global(id, scoping) => match id.name.as_str() {
            "String" => Some(StaticKind::String),
            "Number" => Some(StaticKind::Numeric),
            _ => None,
        },
        Expression::StaticMemberExpression(member)
            if !member.optional && STRING_METHODS.contains(&member.property.name.as_str()) =>
        {
            Some(StaticKind::String)
        }
        _ => None,
    }
}

fn is_global(id: &IdentifierReference<'_>, scoping: &Scoping) -> bool {
    id.reference_id.get().is_some_and(|r| scoping.get_reference(r).symbol_id().is_none())
}

fn literal_text_kind(text: &str) -> Option<StaticKind> {
    match text.as_bytes().first()? {
        b'"' | b'\'' | b'`' => Some(StaticKind::String),
        b'0'..=b'9' | b'.' | b'-' => Some(StaticKind::Numeric),
        _ => None,
    }
}
