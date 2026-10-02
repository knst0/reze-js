use std::collections::HashSet;

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::GetSpan;
use oxc_syntax::operator::UnaryOperator;
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use super::control_flow::is_meaningful;
use crate::analyze::{Analysis, Intrinsic};

/// The reads `row().prop` in the row function of a `<For keyed={(p) => p.prop}>`: a keyed row is
/// kept only while its key stays equal, so the read is constant for the row's lifetime.
#[derive(Default)]
pub struct KeyedRows {
    calls: HashSet<ReferenceId>,
}

impl KeyedRows {
    pub fn collect<'a>(
        program: &Program<'a>,
        analysis: &Analysis<'_>,
        nodes: &AstNodes<'a>,
    ) -> Self {
        let mut collector = KeyedCollector { analysis, nodes, rows: KeyedRows::default() };
        collector.visit_program(program);
        collector.rows
    }

    /// Whether `member` is `row().prop` with `row` the row parameter and `prop` the key property.
    pub fn is_key_read(&self, member: &StaticMemberExpression<'_>) -> bool {
        let Expression::CallExpression(call) = member.object.without_parentheses() else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else { return false };
        callee.reference_id.get().is_some_and(|r| self.calls.contains(&r))
    }
}

struct KeyedCollector<'c, 'a, 's> {
    analysis: &'c Analysis<'s>,
    nodes: &'c AstNodes<'a>,
    rows: KeyedRows,
}

impl<'a> Visit<'a> for KeyedCollector<'_, 'a, '_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if self.analysis.intrinsic(&it.opening_element.name) == Some(Intrinsic::For)
            && let Some(property) = key_property(it)
            && let Some(row) = row_parameter(it)
        {
            self.collect_reads(row, property);
        }
        walk::walk_jsx_element(self, it);
    }
}

impl KeyedCollector<'_, '_, '_> {
    fn collect_reads(&mut self, row: SymbolId, property: &str) {
        let scoping = self.analysis.scoping;
        if scoping.symbol_is_mutated(row) {
            return;
        }
        for &reference in scoping.get_resolved_reference_ids(row) {
            if reads_key(self.nodes, scoping, reference, property) {
                self.rows.calls.insert(reference);
            }
        }
    }
}

/// `keyed={(p) => p.prop}` with plain `p`: the name of `prop`.
fn key_property<'b>(el: &'b JSXElement<'_>) -> Option<&'b str> {
    let attribute = attribute(el, "keyed")?;
    let JSXAttributeValue::ExpressionContainer(container) = attribute.value.as_ref()? else {
        return None;
    };
    let Expression::ArrowFunctionExpression(arrow) =
        container.expression.as_expression()?.without_parentheses()
    else {
        return None;
    };
    let param = plain_parameter(&arrow.params).filter(|_| !arrow.r#async)?;
    let Expression::StaticMemberExpression(member) =
        arrow.body.as_expression()?.without_parentheses()
    else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else { return None };
    (object.name == param.name && !member.optional).then(|| member.property.name.as_str())
}

/// The symbol of the first, plain parameter of the row function of a `For`.
fn row_parameter(el: &JSXElement<'_>) -> Option<SymbolId> {
    let mut children = el.children.iter().filter(|child| is_meaningful(child));
    let nested = match (children.next(), children.next()) {
        (None, _) => None,
        (Some(JSXChild::ExpressionContainer(c)), None) => Some(c.expression.as_expression()?),
        (Some(_), _) => return None,
    };
    let function = match nested {
        Some(function) => function,
        None => {
            let JSXAttributeValue::ExpressionContainer(c) =
                attribute(el, "children")?.value.as_ref()?
            else {
                return None;
            };
            c.expression.as_expression()?
        }
    };
    let params = match function.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) if !arrow.r#async => &arrow.params,
        Expression::FunctionExpression(f) if !f.r#async && !f.generator => &f.params,
        _ => return None,
    };
    let first = params.items.first()?;
    if first.initializer.is_some() {
        return None;
    }
    let BindingPattern::BindingIdentifier(id) = &first.pattern else { return None };
    id.symbol_id.get()
}

fn attribute<'b, 'a>(el: &'b JSXElement<'a>, name: &str) -> Option<&'b JSXAttribute<'a>> {
    el.opening_element.attributes.iter().rev().find_map(|item| match item {
        JSXAttributeItem::Attribute(a)
            if matches!(&a.name, JSXAttributeName::Identifier(id) if id.name == name) =>
        {
            Some(&**a)
        }
        _ => None,
    })
}

fn plain_parameter<'b, 'a>(params: &'b FormalParameters<'a>) -> Option<&'b BindingIdentifier<'a>> {
    if params.rest.is_some() || params.items.len() != 1 || params.items[0].initializer.is_some() {
        return None;
    }
    match &params.items[0].pattern {
        BindingPattern::BindingIdentifier(id) => Some(id),
        _ => None,
    }
}

/// `row().property` read (not written, updated or deleted) with `reference` naming `row`.
fn reads_key(
    nodes: &AstNodes<'_>,
    scoping: &Scoping,
    reference: ReferenceId,
    property: &str,
) -> bool {
    let reference = scoping.get_reference(reference);
    if !reference.flags().is_read() || reference.flags().is_write() {
        return false;
    }
    let node = reference.node_id();
    let AstKind::CallExpression(call) = nodes.parent_kind(node) else { return false };
    if !call.arguments.is_empty() || call.optional || call.callee.span() != nodes.kind(node).span()
    {
        return false;
    }
    let call_node = nodes.parent_id(node);
    let AstKind::StaticMemberExpression(member) = nodes.parent_kind(call_node) else {
        return false;
    };
    if member.optional || member.property.name != property || member.object.span() != call.span {
        return false;
    }
    !matches!(
        nodes.parent_kind(nodes.parent_id(call_node)),
        AstKind::AssignmentExpression(_)
            | AstKind::ArrayAssignmentTarget(_)
            | AstKind::ObjectAssignmentTarget(_)
            | AstKind::AssignmentTargetWithDefault(_)
            | AstKind::AssignmentTargetPropertyProperty(_)
            | AstKind::AssignmentTargetRest(_)
            | AstKind::UpdateExpression(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
    ) && !matches!(
        nodes.parent_kind(nodes.parent_id(call_node)),
        AstKind::UnaryExpression(unary) if unary.operator == UnaryOperator::Delete
    )
}
