//! How a reference uses its binding (SPEC §15.4): the static member chain it heads and what the
//! chain is used for.

use oxc_ast::AstKind;
use oxc_ast::ast::{Expression, UnaryOperator};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::reference::ReferenceId;

/// A reference as the root of `x.k₁…kₙ` (static keys only, `n ≥ 0`) and what uses the chain.
pub struct Access<'a> {
    pub keys: Vec<&'a str>,
    pub context: Context,
    /// The whole chain, `x` included.
    pub span: Span,
    /// The AST node of the whole chain.
    pub node: NodeId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    /// Callee of a call that is not optional and has no type arguments.
    Call { argument_count: usize },
    /// Read as a value: not a callee, assignment target, `delete` or update operand.
    Read,
    /// Assignment target, update operand or `delete` operand.
    Write,
    /// Name of a JSX element.
    Tag,
    /// Anything else: optional chains, computed keys, `new`, tags, type positions, spreads into
    /// destructuring targets.
    Other,
}

/// `None` when the reference is not a use: an export specifier or a closing tag.
pub fn classify<'a>(
    reference: ReferenceId,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
) -> Option<Access<'a>> {
    let reference = scoping.get_reference(reference);
    let start = reference.node_id();
    let start_span = nodes.kind(start).span();
    if reference.flags().is_type() && !reference.flags().is_value() {
        return Some(Access {
            keys: Vec::new(),
            context: Context::Other,
            span: start_span,
            node: start,
        });
    }
    match nodes.parent_kind(start) {
        AstKind::ExportSpecifier(_) | AstKind::JSXClosingElement(_) => return None,
        AstKind::JSXOpeningElement(_) => {
            return Some(Access {
                keys: Vec::new(),
                context: Context::Tag,
                span: start_span,
                node: start,
            });
        }
        AstKind::JSXMemberExpression(member) => {
            let tag_context = match nodes.parent_kind(nodes.parent_id(start)) {
                AstKind::JSXOpeningElement(_) => Context::Tag,
                AstKind::JSXClosingElement(_) => return None,
                _ => Context::Other,
            };
            return Some(Access {
                keys: vec![member.property.name.as_str()],
                context: tag_context,
                span: member.span,
                node: nodes.parent_id(start),
            });
        }
        _ => {}
    }

    let mut keys = Vec::new();
    let mut current = start;
    let mut current_span = start_span;
    loop {
        let parent = nodes.parent_id(current);
        match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(_) => {}
            AstKind::StaticMemberExpression(member) if member.object.span() == current_span => {
                if member.optional {
                    return Some(Access {
                        keys,
                        context: Context::Other,
                        span: member.span,
                        node: parent,
                    });
                }
                keys.push(member.property.name.as_str());
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == current_span => {
                let Expression::StringLiteral(key) = member.expression.without_parentheses() else {
                    return Some(Access {
                        keys,
                        context: Context::Other,
                        span: member.span,
                        node: parent,
                    });
                };
                if member.optional {
                    return Some(Access {
                        keys,
                        context: Context::Other,
                        span: member.span,
                        node: parent,
                    });
                }
                keys.push(key.value.as_str());
            }
            _ => break,
        }
        current = parent;
        current_span = nodes.kind(parent).span();
    }

    let context = match nodes.parent_kind(current) {
        AstKind::CallExpression(call) if call.callee.span() == current_span => {
            if call.optional || call.type_arguments.is_some() {
                Context::Other
            } else {
                Context::Call { argument_count: call.arguments.len() }
            }
        }
        AstKind::AssignmentExpression(assignment) if assignment.left.span() == current_span => {
            Context::Write
        }
        AstKind::UpdateExpression(_) => Context::Write,
        AstKind::UnaryExpression(unary) if unary.operator == UnaryOperator::Delete => {
            Context::Write
        }
        AstKind::ForInStatement(statement) if statement.left.span() == current_span => {
            Context::Write
        }
        AstKind::ForOfStatement(statement) if statement.left.span() == current_span => {
            Context::Write
        }
        AstKind::NewExpression(_)
        | AstKind::TaggedTemplateExpression(_)
        | AstKind::ChainExpression(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::ObjectAssignmentTarget(_)
        | AstKind::AssignmentTargetRest(_)
        | AstKind::AssignmentTargetWithDefault(_)
        | AstKind::AssignmentTargetPropertyIdentifier(_)
        | AstKind::AssignmentTargetPropertyProperty(_)
        | AstKind::TSAsExpression(_)
        | AstKind::TSSatisfiesExpression(_)
        | AstKind::TSNonNullExpression(_)
        | AstKind::TSTypeAssertion(_)
        | AstKind::TSInstantiationExpression(_)
        | AstKind::ExportDefaultDeclaration(_) => Context::Other,
        _ => Context::Read,
    };
    Some(Access { keys, context, span: current_span, node: current })
}
