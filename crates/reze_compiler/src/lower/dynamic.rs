use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::Lowerer;
use crate::analyze::Primitive;
use crate::html::{is_mathml_root, is_svg_element};
use crate::ir::{Hole, HoleKind, Namespace, ScriptEdit};

impl<'a> Lowerer<'a, '_> {
    /// For `dynamic(source)` with a function `source`: an element component in place of each
    /// non-empty string literal the function can return, its namespace decided by the tag.
    pub(super) fn element_tags(&self, call: &CallExpression<'a>) -> std::vec::Vec<Hole<'a>> {
        let mut returns = Returns { holes: std::vec::Vec::new() };
        if self.analysis.primitive(&call.callee) != Some(Primitive::Dynamic) {
            return returns.holes;
        }
        let source = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .map(Expression::without_parentheses);
        match source {
            Some(Expression::ArrowFunctionExpression(arrow)) => match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => returns.visit_function_body(body),
                body => {
                    if let Some(value) = body.as_expression() {
                        push_tags(value, &mut returns.holes);
                    }
                }
            },
            Some(Expression::FunctionExpression(function)) => {
                if let Some(body) = &function.body {
                    returns.visit_function_body(body);
                }
            }
            _ => {}
        }
        returns.holes
    }
}

/// Tag holes of the `return` statements of one function body, nested functions and classes aside.
struct Returns<'a> {
    holes: std::vec::Vec<Hole<'a>>,
}

impl<'a> Visit<'a> for Returns<'a> {
    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if let Some(argument) = &it.argument {
            push_tags(argument, &mut self.holes);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &Class<'a>) {}
}

/// Tag holes of the values `value` can evaluate to; an empty string stays, as it renders nothing.
fn push_tags<'a>(value: &Expression<'a>, holes: &mut std::vec::Vec<Hole<'a>>) {
    match value.without_parentheses() {
        Expression::ConditionalExpression(e) => {
            push_tags(&e.consequent, holes);
            push_tags(&e.alternate, holes);
        }
        Expression::LogicalExpression(e) => {
            if e.operator != LogicalOperator::And {
                push_tags(&e.left, holes);
            }
            push_tags(&e.right, holes);
        }
        Expression::SequenceExpression(e) => {
            if let Some(last) = e.expressions.last() {
                push_tags(last, holes);
            }
        }
        Expression::TSAsExpression(e) => push_tags(&e.expression, holes),
        Expression::TSSatisfiesExpression(e) => push_tags(&e.expression, holes),
        Expression::TSNonNullExpression(e) => push_tags(&e.expression, holes),
        Expression::StringLiteral(s) if !s.value.is_empty() => {
            holes.push(tag_hole(s.span, s.value.as_str()));
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            if let Some(tag) =
                t.quasis.first().and_then(|q| q.value.cooked.as_ref()).filter(|tag| !tag.is_empty())
            {
                holes.push(tag_hole(t.span(), tag.as_str()));
            }
        }
        _ => {}
    }
}

fn tag_hole<'a>(span: Span, tag: &str) -> Hole<'a> {
    let namespace = if tag == "svg" || is_svg_element(tag) {
        Namespace::Svg
    } else if is_mathml_root(tag) {
        Namespace::MathMl
    } else {
        Namespace::Html
    };
    Hole { span, kind: HoleKind::Script(ScriptEdit::ElementTag { namespace }) }
}
