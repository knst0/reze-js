use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::Lowerer;
use crate::analyze::Primitive;
use crate::html::push_js_string;
use crate::ir::{Hole, HoleKind, ScriptEdit};

impl<'a> Lowerer<'a, '_> {
    /// `{ name }` after the first argument of a `signal`/`computed`/`action` call without options,
    /// named after the declared variable.
    pub(super) fn debug_name(&self, declarator: &VariableDeclarator<'a>) -> Option<Hole<'a>> {
        if !self.settings.debug_names || self.analysis.folded_getter(declarator).is_some() {
            return None;
        }
        let Expression::CallExpression(call) = declarator.init.as_ref()?.without_parentheses()
        else {
            return None;
        };
        let name = match (self.analysis.primitive(&call.callee)?, &declarator.id) {
            (Primitive::Signal, BindingPattern::ArrayPattern(pattern)) => {
                match pattern.elements.first()?.as_ref()? {
                    BindingPattern::BindingIdentifier(id) => id.name.as_str(),
                    _ => return None,
                }
            }
            (Primitive::Computed | Primitive::Action, BindingPattern::BindingIdentifier(id)) => {
                id.name.as_str()
            }
            _ => return None,
        };
        if call.arguments.len() > 1 || call.arguments.iter().any(Argument::is_spread) {
            return None;
        }
        let (at, mut text) = match call.arguments.first() {
            Some(argument) => (argument.span().end, String::from(", { name: ")),
            None => (call.span.end - 1, String::from("undefined, { name: ")),
        };
        push_js_string(&mut text, name);
        text.push_str(" }");
        let kind = HoleKind::Script(ScriptEdit::Insert(self.alloc.alloc_str(&text)));
        Some(Hole { span: Span::empty(at), kind })
    }
}
