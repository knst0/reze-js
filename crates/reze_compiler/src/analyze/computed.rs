//! O4 (SPEC §8): `const d = computed(() => expr)` read once, as `d()` inside a reactive JSX
//! expression of the function that declares it, is inlined into that read.

use std::collections::HashSet;

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::Span;
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeId;
use oxc_syntax::symbol::SymbolId;

use super::Facts;
use crate::diagnostic::{Code, Report};
use crate::lower::constant::literal_truthy;
use crate::lower::is_native_name;
use crate::usage::{Context, classify};

/// `expr` of `computed(() => expr)` without annotations, type arguments or options.
pub fn body<'b, 'a>(declarator: &'b VariableDeclarator<'a>) -> Option<&'b Expression<'a>> {
    if declarator.type_annotation.is_some() {
        return None;
    }
    let Some(Expression::CallExpression(call)) =
        declarator.init.as_ref().map(Expression::without_parentheses)
    else {
        return None;
    };
    if call.optional || call.type_arguments.is_some() || call.arguments.len() != 1 {
        return None;
    }
    let Some(Expression::ArrowFunctionExpression(arrow)) =
        call.arguments[0].as_expression().map(Expression::without_parentheses)
    else {
        return None;
    };
    let is_plain = !arrow.r#async
        && arrow.type_parameters.is_none()
        && arrow.return_type.is_none()
        && arrow.params.items.is_empty()
        && arrow.params.rest.is_none();
    if !is_plain {
        return None;
    }
    arrow.get_expression().map(Expression::without_parentheses)
}

pub fn inline(
    facts: &mut Facts,
    declarators: &[NodeId],
    exported: &HashSet<SymbolId>,
    source: &str,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) {
    for &declarator_node in declarators {
        let AstKind::VariableDeclarator(declarator) = nodes.kind(declarator_node) else { continue };
        let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
        let computed = id.symbol_id();
        if exported.contains(&computed) {
            continue;
        }
        let Some(expr) = body(declarator) else { continue };
        let declaration_node = nodes.parent_id(declarator_node);
        let AstKind::VariableDeclaration(declaration) = nodes.kind(declaration_node) else {
            continue;
        };
        if declaration.kind != VariableDeclarationKind::Const
            || declaration.declare
            || declaration.declarations.len() != 1
        {
            continue;
        }
        let is_top_level = match nodes.parent_kind(declaration_node) {
            AstKind::Program(_) => true,
            AstKind::FunctionBody(_) | AstKind::BlockStatement(_) => false,
            _ => continue,
        };
        let &[reference] = scoping.get_resolved_reference_ids(computed) else { continue };
        let Some(access) = classify(reference, scoping, nodes) else { continue };
        if access.context != (Context::Call { argument_count: 0 }) || !access.keys.is_empty() {
            continue;
        }
        let call_node = nodes.parent_id(access.node);
        let AstKind::CallExpression(call) = nodes.kind(call_node) else { continue };
        if !matches!(call.callee, Expression::Identifier(_))
            || call.span.start < declaration.span.end
        {
            continue;
        }
        let Some(read_boundary) = reactive_jsx_boundary(call_node, nodes, facts) else { continue };
        if read_boundary != boundary(declaration_node, nodes)
            || !is_plain_boundary(nodes.kind(read_boundary))
        {
            continue;
        }
        let read_scope = nodes.get_node(call_node).scope_id();
        if !resolves_alike(expr, declarator.span, read_scope, scoping) {
            continue;
        }

        facts.inlined_reads.insert(reference, declarator_node);
        facts
            .removed_declarations
            .insert(declaration.span.start, removal(source, declaration.span, is_top_level));
        let name = scoping.symbol_name(computed);
        reports.push(
            Report::new(
                Code::ComputedInlined,
                declarator.span,
                format!(
                    "`{name}` is read only once, by `{name}()` in a reactive JSX expression of the \
                     same function, so the declaration was removed and its expression inlined there."
                ),
            )
            .label(call.span, "inlined here")
            .data("computed", name)
            .data("scope", "module"),
        );
    }
}

/// The node whose body runs `node`: the nearest function, class member or the program.
fn boundary(node: NodeId, nodes: &AstNodes<'_>) -> NodeId {
    nodes.ancestor_ids(node).find(|&id| is_boundary(nodes.kind(id))).unwrap_or(NodeId::ROOT)
}

fn is_boundary(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::Program(_)
            | AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::PropertyDefinition(_)
            | AstKind::AccessorProperty(_)
            | AstKind::StaticBlock(_)
            | AstKind::TSModuleBlock(_)
    )
}

/// Async and generator bodies are restructured by lowering, so their statements are not kept
/// in place.
fn is_plain_boundary(kind: AstKind<'_>) -> bool {
    match kind {
        AstKind::Function(function) => !function.r#async && !function.generator,
        AstKind::ArrowFunctionExpression(arrow) => !arrow.r#async,
        _ => true,
    }
}

/// The boundary of a call inside a JSX expression that lowering compiles as reactive (a bind,
/// an insert or a getter prop) and keeps: no spread, event, `ref`, `children`, overridden
/// attribute or dead branch on the way up.
fn reactive_jsx_boundary(call: NodeId, nodes: &AstNodes<'_>, facts: &Facts) -> Option<NodeId> {
    let mut in_jsx = false;
    for id in nodes.ancestor_ids(call) {
        match nodes.kind(id) {
            kind if is_boundary(kind) => return in_jsx.then_some(id),
            AstKind::JSXExpressionContainer(container) => {
                match nodes.parent_kind(id) {
                    AstKind::JSXElement(_) | AstKind::JSXFragment(_) => {
                        if has_literal_condition(container.expression.as_expression()?, facts) {
                            return None;
                        }
                    }
                    AstKind::JSXAttribute(_) => {}
                    _ => return None,
                }
                in_jsx = true;
            }
            AstKind::JSXAttribute(attribute) => {
                let opening = nodes.ancestor_ids(id).find_map(|a| match nodes.kind(a) {
                    AstKind::JSXOpeningElement(opening) => Some(opening),
                    _ => None,
                })?;
                if !is_reactive_attribute(attribute, opening) {
                    return None;
                }
            }
            AstKind::JSXSpreadAttribute(_) | AstKind::JSXSpreadChild(_) => return None,
            _ => {}
        }
    }
    None
}

/// A child `a && b` or `a ? b : c` whose condition O5 decides statically.
fn has_literal_condition(e: &Expression<'_>, facts: &Facts) -> bool {
    match e.without_parentheses() {
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            literal_truthy(&logical.left, facts).is_some()
        }
        Expression::ConditionalExpression(conditional) => {
            literal_truthy(&conditional.test, facts).is_some()
        }
        _ => false,
    }
}

fn is_reactive_attribute(attribute: &JSXAttribute<'_>, opening: &JSXOpeningElement<'_>) -> bool {
    let is_native = match &opening.name {
        JSXElementName::Identifier(id) => is_native_name(id.name.as_str()),
        JSXElementName::IdentifierReference(id) => is_native_name(id.name.as_str()),
        JSXElementName::NamespacedName(_) => true,
        JSXElementName::MemberExpression(_) | JSXElementName::ThisExpression(_) => false,
    };
    let is_plain_name = match &attribute.name {
        JSXAttributeName::Identifier(id) => {
            let name = id.name.as_str();
            name != "ref" && name != "children" && !(is_native && name.starts_with("on"))
        }
        JSXAttributeName::NamespacedName(name) => {
            !is_native || matches!(name.namespace.name.as_str(), "prop" | "attr" | "bool")
        }
    };
    let has_spread =
        opening.attributes.iter().any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)));
    let same_name = opening
        .attributes
        .iter()
        .filter(|a| match a {
            JSXAttributeItem::Attribute(other) => same_attribute_name(&other.name, &attribute.name),
            JSXAttributeItem::SpreadAttribute(_) => false,
        })
        .count();
    is_plain_name && !(is_native && has_spread) && same_name == 1
}

fn same_attribute_name(a: &JSXAttributeName<'_>, b: &JSXAttributeName<'_>) -> bool {
    match (a, b) {
        (JSXAttributeName::Identifier(a), JSXAttributeName::Identifier(b)) => a.name == b.name,
        (JSXAttributeName::NamespacedName(a), JSXAttributeName::NamespacedName(b)) => {
            a.namespace.name == b.namespace.name && a.name.name == b.name.name
        }
        _ => false,
    }
}

/// Whether every free identifier of `expr` (declared outside `declarator`) names the same symbol
/// from `scope`.
fn resolves_alike(
    expr: &Expression<'_>,
    declarator: Span,
    scope: ScopeId,
    scoping: &Scoping,
) -> bool {
    let mut check = SameResolution { declarator, scope, scoping, is_alike: true };
    check.visit_expression(expr);
    check.is_alike
}

struct SameResolution<'s> {
    declarator: Span,
    scope: ScopeId,
    scoping: &'s Scoping,
    is_alike: bool,
}

impl<'a> Visit<'a> for SameResolution<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let symbol = it.reference_id.get().and_then(|r| self.scoping.get_reference(r).symbol_id());
        if symbol.is_some_and(|s| self.declarator.contains_inclusive(self.scoping.symbol_span(s))) {
            return;
        }
        self.is_alike &= self.scoping.find_binding(self.scope, it.name) == symbol;
    }
}

/// The declaration together with its line when it stands alone on it.
fn removal(source: &str, span: Span, is_top_level: bool) -> Span {
    let bytes = source.as_bytes();
    let is_blank = |b: u8| b == b' ' || b == b'\t';
    let mut start = span.start as usize;
    while start > 0 && is_blank(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = span.end as usize;
    while end < bytes.len() && is_blank(bytes[end]) {
        end += 1;
    }
    let starts_line = start == 0 || bytes[start - 1] == b'\n';
    let rest = &source[end..];
    let line_break = if rest.starts_with("\r\n") {
        2
    } else if rest.starts_with('\n') {
        1
    } else {
        0
    };
    if starts_line && line_break > 0 {
        let start = if is_top_level { span.start as usize } else { start };
        return Span::new(start as u32, (end + line_break) as u32);
    }
    Span::new(span.start, end as u32)
}
