use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::Span;
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeFlags;

use crate::ir::view::Namespace;

#[derive(Clone, Copy)]
pub struct DynamicTag {
    pub source: Span,
    pub namespace: Namespace,
}

pub fn collect(call: &CallExpression<'_>, tags: &mut HashMap<NodeId, DynamicTag>) {
    let Some(source) = call.arguments.first().and_then(Argument::as_expression) else { return };
    let mut collector = ReturnedTags { source: call.span, tags };
    match source.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) if !arrow.r#async => {
            if let Some(value) = arrow.get_expression() {
                collector.value(value);
            } else if let ArrowFunctionBody::FunctionBody(body) = &arrow.body {
                collector.visit_function_body(body);
            }
        }
        Expression::FunctionExpression(function) if !function.r#async && !function.generator => {
            if let Some(body) = &function.body {
                collector.visit_function_body(body);
            }
        }
        _ => {}
    }
}

struct ReturnedTags<'t> {
    source: Span,
    tags: &'t mut HashMap<NodeId, DynamicTag>,
}

impl ReturnedTags<'_> {
    fn value(&mut self, expression: &Expression<'_>) {
        match expression.without_parentheses() {
            Expression::StringLiteral(literal) if !literal.value.is_empty() => {
                let tag = literal.value.as_str();
                let namespace = if tag == "svg" || crate::html::is_svg_element(tag) {
                    Namespace::Svg
                } else if crate::html::is_mathml_root(tag) {
                    Namespace::MathMl
                } else {
                    Namespace::Html
                };
                self.tags.insert(literal.node_id(), DynamicTag { source: self.source, namespace });
            }
            Expression::ConditionalExpression(conditional) => {
                self.value(&conditional.consequent);
                self.value(&conditional.alternate);
            }
            Expression::LogicalExpression(logical) => {
                self.value(&logical.left);
                self.value(&logical.right);
            }
            Expression::SequenceExpression(sequence) => {
                if let Some(last) = sequence.expressions.last() {
                    self.value(last);
                }
            }
            Expression::TSAsExpression(value) => self.value(&value.expression),
            Expression::TSSatisfiesExpression(value) => self.value(&value.expression),
            Expression::TSNonNullExpression(value) => self.value(&value.expression),
            Expression::TSTypeAssertion(value) => self.value(&value.expression),
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for ReturnedTags<'_> {
    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        if let Some(value) = &statement.argument {
            self.value(value);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}
