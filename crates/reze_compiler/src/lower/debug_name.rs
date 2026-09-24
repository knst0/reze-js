//! Debug names (dev builds): `const [count, setCount] = signal(0)` passes `{ name: "count" }`,
//! `const doubled = computed(fn)` passes `{ name: "doubled" }`, for devtools.

use super::Lowerer;
use crate::facts::Primitive;
use crate::html::push_js_string;
use crate::ir::{Hole, HoleKind};
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

impl<'a> Lowerer<'a, '_> {
    /// The `{ name }` argument appended to a `signal`/`computed` call that has no options yet.
    pub(super) fn debug_name(&self, declarator: &VariableDeclarator<'a>) -> Option<Hole<'a>> {
        if !self.debug_names {
            return None;
        }
        let Expression::CallExpression(call) = declarator.init.as_ref()?.without_parentheses()
        else {
            return None;
        };
        let name = match (self.facts.primitives.of(&call.callee, self.scoping)?, &declarator.id) {
            (Primitive::Signal, BindingPattern::ArrayPattern(pattern)) => {
                match pattern.elements.first()?.as_ref()? {
                    BindingPattern::BindingIdentifier(id) => id.name.as_str(),
                    _ => return None,
                }
            }
            (Primitive::Computed, BindingPattern::BindingIdentifier(id)) => id.name.as_str(),
            _ => return None,
        };
        if call.arguments.len() > 1 || call.arguments.iter().any(Argument::is_spread) {
            return None;
        }
        let mut text = String::new();
        let at = match call.arguments.first() {
            Some(argument) => {
                text.push_str(", ");
                argument.span().end
            }
            None => {
                text.push_str("undefined, ");
                call.span.end - 1
            }
        };
        text.push_str("{ name: ");
        push_js_string(&mut text, name);
        text.push_str(" }");
        Some(Hole { span: Span::new(at, at), kind: HoleKind::Insert(self.alloc.alloc_str(&text)) })
    }
}
