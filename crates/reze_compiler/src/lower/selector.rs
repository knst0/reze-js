//! O6: `S() === key` in a `<For>` row reads a selector created once per `<For>`.

use oxc_allocator::Vec;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::BinaryOperator;
use oxc_syntax::scope::ScopeId;
use oxc_syntax::symbol::SymbolId;

use super::{Lowerer, is_function};
use crate::diagnostic::{Code, Report};
use crate::facts::Primitive;
use crate::ir::{Hole, HoleKind, SelectorSource};

/// The row callback of a `<For>` whose element is being lowered.
pub(super) struct ForScope<'a> {
    callback_scope: ScopeId,
    callback_span: Span,
    params: std::vec::Vec<SymbolId>,
    selectors: std::vec::Vec<(SymbolId, SelectorSource<'a>)>,
}

struct RowCallback<'e, 'a> {
    scope: ScopeId,
    span: Span,
    params: &'e FormalParameters<'a>,
}

fn row_callback<'e, 'a>(e: &'e Expression<'a>) -> Option<RowCallback<'e, 'a>> {
    match e.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => Some(RowCallback {
            scope: arrow.scope_id.get()?,
            span: arrow.span,
            params: &arrow.params,
        }),
        Expression::FunctionExpression(function) => Some(RowCallback {
            scope: function.scope_id.get()?,
            span: function.span,
            params: &function.params,
        }),
        _ => None,
    }
}

fn children_callback<'e, 'a>(el: &'e JSXElement<'a>) -> Option<RowCallback<'e, 'a>> {
    let child = el.children.iter().find_map(|child| match child {
        JSXChild::ExpressionContainer(c) => c.expression.as_expression().filter(|e| is_function(e)),
        _ => None,
    });
    let attribute = || {
        el.opening_element.attributes.iter().find_map(|item| match item {
            JSXAttributeItem::Attribute(a)
                if matches!(&a.name, JSXAttributeName::Identifier(id) if id.name == "children") =>
            {
                match &a.value {
                    Some(JSXAttributeValue::ExpressionContainer(c)) => c.expression.as_expression(),
                    _ => None,
                }
            }
            _ => None,
        })
    };
    row_callback(child.or_else(attribute)?)
}

impl<'a> Lowerer<'a, '_> {
    /// Opens a row scope when `el` is the runtime `<For>` with a function child, under `optimize`.
    pub(super) fn enter_for(&mut self, el: &JSXElement<'a>) -> bool {
        if !self.optimize {
            return false;
        }
        let JSXElementName::IdentifierReference(tag) = &el.opening_element.name else {
            return false;
        };
        if self.facts.primitives.of_reference(tag, self.scoping) != Some(Primitive::For) {
            return false;
        }
        let Some(callback) = children_callback(el) else { return false };
        let params = callback
            .params
            .items
            .iter()
            .filter_map(|param| match &param.pattern {
                BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                _ => None,
            })
            .collect();
        self.for_scopes.push(ForScope {
            callback_scope: callback.scope,
            callback_span: callback.span,
            params,
            selectors: std::vec::Vec::new(),
        });
        true
    }

    /// Closes the innermost row scope and returns the selectors its rows read.
    pub(super) fn leave_for(&mut self) -> Vec<'a, SelectorSource<'a>> {
        let scope = self.for_scopes.pop().expect("an open row scope");
        Vec::from_iter_in(scope.selectors.into_iter().map(|(_, source)| source), &self.alloc)
    }

    pub(super) fn selector_read(&mut self, it: &BinaryExpression<'a>) -> Option<Hole<'a>> {
        let is_negated = match it.operator {
            BinaryOperator::StrictEquality => false,
            BinaryOperator::StrictInequality => true,
            _ => return None,
        };
        let (source, key) = if let Some(source) = self.outer_getter_called_in_row(&it.left)
            && self.is_built_from_row_params(&it.right)
        {
            (source, &it.right)
        } else if let Some(source) = self.outer_getter_called_in_row(&it.right)
            && self.is_built_from_row_params(&it.left)
        {
            (source, &it.left)
        } else {
            return None;
        };
        let selector = self.selector_for(source);
        let key = self.expr(key);
        let original = self.embed(it.span, |finder| {
            finder.visit_expression(&it.left);
            finder.visit_expression(&it.right);
        });
        let getter = &self.source[source.span.start as usize..source.span.end as usize];
        self.report(
            Report::new(
                Code::AutoSelector,
                it.span,
                format!(
                    "Each row compares `{getter}()` with its own key, so a change of `{getter}` \
                     would re-run every row. In the browser the comparison reads a selector \
                     created once for this `<For>` instead: only the rows whose result flips \
                     re-run. The server keeps the comparison."
                ),
            )
            .data("source", getter),
        );
        Some(Hole {
            span: it.span,
            kind: HoleKind::SelectorRead { selector, key, original, is_negated },
        })
    }

    fn outer_getter_called_in_row<'e>(
        &self,
        e: &'e Expression<'a>,
    ) -> Option<&'e IdentifierReference<'a>> {
        let scope = self.for_scopes.last()?;
        let Expression::CallExpression(call) = e.without_parentheses() else { return None };
        let Expression::Identifier(id) = &call.callee else { return None };
        if !call.arguments.is_empty()
            || call.optional
            || call.type_arguments.is_some()
            || !self.facts.is_getter(id)
            || self.facts.folded_callee(call).is_some()
            || self.facts.inlined_body(call, self.nodes).is_some()
            || self.facts.program.computed_reads.contains_key(&id.span.start)
        {
            return None;
        }
        let reference = self.scoping.get_reference(id.reference_id.get()?);
        let symbol = reference.symbol_id()?;
        let declared = self.scoping.symbol_span(symbol);
        if scope.callback_span.contains_inclusive(declared) {
            return None;
        }
        let function_scope = self
            .scoping
            .scope_ancestors(reference.scope_id())
            .find(|&ancestor| self.scoping.scope_flags(ancestor).is_function())?;
        (function_scope == scope.callback_scope).then_some(&**id)
    }

    fn is_built_from_row_params(&self, e: &Expression<'a>) -> bool {
        let Some(scope) = self.for_scopes.last() else { return false };
        let mut reads_param = false;
        let is_key = self.is_row_param_path(e, &scope.params, &mut reads_param);
        is_key && reads_param
    }

    fn is_row_param_path(
        &self,
        e: &Expression<'a>,
        params: &[SymbolId],
        reads_param: &mut bool,
    ) -> bool {
        match e.without_parentheses() {
            Expression::CallExpression(call) => {
                let Expression::Identifier(id) = &call.callee else { return false };
                let is_param = id
                    .reference_id
                    .get()
                    .and_then(|r| self.scoping.get_reference(r).symbol_id())
                    .is_some_and(|symbol| params.contains(&symbol));
                let is_plain_call =
                    call.arguments.is_empty() && !call.optional && call.type_arguments.is_none();
                *reads_param |= is_param && is_plain_call;
                is_param && is_plain_call
            }
            Expression::StaticMemberExpression(member) => {
                !member.optional && self.is_row_param_path(&member.object, params, reads_param)
            }
            Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_) => true,
            _ => false,
        }
    }

    fn selector_for(&mut self, source: &IdentifierReference<'a>) -> &'a str {
        let symbol = source
            .reference_id
            .get()
            .and_then(|r| self.scoping.get_reference(r).symbol_id())
            .expect("a resolved getter");
        let scope = self.for_scopes.last().expect("an open row scope");
        if let Some((_, existing)) = scope.selectors.iter().find(|(s, _)| *s == symbol) {
            return existing.selector;
        }
        let selector = self.fresh("_sel$");
        let scope = self.for_scopes.last_mut().expect("an open row scope");
        scope.selectors.push((symbol, SelectorSource { selector, source: source.span() }));
        selector
    }
}
