use oxc_allocator::Vec;
use oxc_ast::ast::*;
use oxc_span::Span;
use oxc_syntax::operator::BinaryOperator;
use oxc_syntax::scope::ScopeId;
use oxc_syntax::symbol::SymbolId;

use super::Lowerer;
use crate::diagnostic::{Code, Report};
use crate::ir::{Hole, HoleKind, ScriptEdit, Selector};

/// The row function of a `<For>` being lowered.
pub struct ForScope<'a> {
    scope: ScopeId,
    span: Span,
    params: std::vec::Vec<SymbolId>,
    selectors: std::vec::Vec<(SymbolId, Selector<'a>)>,
}

impl<'a> Lowerer<'a, '_> {
    /// Opens a row scope when `map` is an arrow or function expression.
    pub(super) fn enter_for(&mut self, map: &Expression<'a>) -> bool {
        let (scope, span, params) = match map.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                (arrow.scope_id.get(), arrow.span, &arrow.params)
            }
            Expression::FunctionExpression(function) => {
                (function.scope_id.get(), function.span, &function.params)
            }
            _ => return false,
        };
        let Some(scope) = scope else { return false };
        let params = params
            .items
            .iter()
            .filter_map(|param| match &param.pattern {
                BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                _ => None,
            })
            .collect();
        self.for_scopes.push(ForScope { scope, span, params, selectors: std::vec::Vec::new() });
        true
    }

    /// Closes the innermost row scope and returns the selectors its rows read.
    pub(super) fn leave_for(&mut self) -> Vec<'a, Selector<'a>> {
        let scope = self.for_scopes.pop().expect("a row scope is open");
        Vec::from_iter_in(scope.selectors.into_iter().map(|(_, selector)| selector), &self.alloc)
    }

    /// `S() === K` or `K === S()` (also `!==`) in a row, with `S` a getter declared outside
    /// the row and `K` built from the row's parameters.
    pub(super) fn selector_read(&mut self, it: &BinaryExpression<'a>) -> Option<Hole<'a>> {
        let is_negated = match it.operator {
            BinaryOperator::StrictEquality => false,
            BinaryOperator::StrictInequality => true,
            _ => return None,
        };
        let (source, key) = if let Some(source) = self.outer_getter_called_in_row(&it.left)
            && self.is_row_key(&it.right)
        {
            (source, &it.right)
        } else if let Some(source) = self.outer_getter_called_in_row(&it.right)
            && self.is_row_key(&it.left)
        {
            (source, &it.left)
        } else {
            return None;
        };
        let selector = self.selector_for(source)?;
        let key = self.expr(key);
        self.report(Report::new(Code::AutoSelector, it.span).arg("signal", source.name.as_str()));
        Some(Hole {
            span: it.span,
            kind: HoleKind::Script(ScriptEdit::SelectorRead { selector, key, is_negated }),
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
            || !self.analysis.is_getter(id)
            || self.analysis.folded_read(call).is_some()
        {
            return None;
        }
        let scoping = self.analysis.scoping;
        let reference = scoping.get_reference(id.reference_id.get()?);
        let declared = scoping.symbol_span(reference.symbol_id()?);
        if scope.span.contains_inclusive(declared) {
            return None;
        }
        let function_scope = scoping
            .scope_ancestors(reference.scope_id())
            .find(|&ancestor| scoping.scope_flags(ancestor).is_function())?;
        (function_scope == scope.scope).then_some(&**id)
    }

    fn is_row_key(&self, e: &Expression<'a>) -> bool {
        let Some(scope) = self.for_scopes.last() else { return false };
        let mut reads_param = false;
        self.is_row_path(e, &scope.params, &mut reads_param) && reads_param
    }

    /// Calls of row parameters, reads of them, static member reads of them, and literals. Reads
    /// cover both shapes: the raw value (`row`, `row.id`, `index`) and the accessor (`row()`,
    /// `row().id`, `index()`).
    fn is_row_path(&self, e: &Expression<'a>, params: &[SymbolId], reads_param: &mut bool) -> bool {
        match e.without_parentheses() {
            Expression::Identifier(id) => {
                let is_param = id
                    .reference_id
                    .get()
                    .and_then(|r| self.analysis.scoping.get_reference(r).symbol_id())
                    .is_some_and(|symbol| params.contains(&symbol));
                *reads_param |= is_param;
                is_param
            }
            Expression::CallExpression(call) => {
                let Expression::Identifier(id) = &call.callee else { return false };
                let is_param = id
                    .reference_id
                    .get()
                    .and_then(|r| self.analysis.scoping.get_reference(r).symbol_id())
                    .is_some_and(|symbol| params.contains(&symbol));
                let is_param_call = is_param
                    && call.arguments.is_empty()
                    && !call.optional
                    && call.type_arguments.is_none();
                *reads_param |= is_param_call;
                is_param_call
            }
            Expression::StaticMemberExpression(member) => {
                !member.optional && self.is_row_path(&member.object, params, reads_param)
            }
            Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_) => true,
            _ => false,
        }
    }

    fn selector_for(&mut self, source: &IdentifierReference<'a>) -> Option<&'a str> {
        let symbol = self.analysis.scoping.get_reference(source.reference_id.get()?).symbol_id()?;
        if let Some((_, existing)) =
            self.for_scopes.last()?.selectors.iter().find(|(s, _)| *s == symbol)
        {
            return Some(existing.name);
        }
        let name = self.fresh("_sel$");
        self.for_scopes
            .last_mut()?
            .selectors
            .push((symbol, Selector { name, source: source.span }));
        Some(name)
    }
}
