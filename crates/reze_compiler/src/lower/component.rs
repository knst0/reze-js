//! Components, props objects and component children (SPEC §7.8).

use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::Span;

use super::children::Item;
use super::constant::is_dynamic;
use super::island::DirectiveSite;
use super::{Lowerer, attribute_name, is_function};
use crate::diagnostic::{Code, Report};
use crate::html::decode_entities;
use crate::ir::{Component, Embed, Prop, PropValue, Props, PropsPart};

pub struct PropsBuilder<'a> {
    parts: Vec<'a, PropsPart<'a>>,
    object: Vec<'a, Prop<'a>>,
    alloc: &'a Allocator,
}

impl<'a> PropsBuilder<'a> {
    pub fn new(alloc: &'a Allocator) -> Self {
        Self { parts: Vec::new_in(&alloc), object: Vec::new_in(&alloc), alloc }
    }

    pub fn push(&mut self, prop: Prop<'a>) {
        self.object.push(prop);
    }

    pub fn spread(&mut self, value: Embed<'a>, is_dynamic: bool) {
        self.flush();
        self.parts.push(PropsPart::Spread { value, is_dynamic });
    }

    fn flush(&mut self) {
        if !self.object.is_empty() {
            let object = std::mem::replace(&mut self.object, Vec::new_in(&self.alloc));
            self.parts.push(PropsPart::Object(object));
        }
    }

    pub fn finish(mut self) -> Props<'a> {
        self.flush();
        Props { parts: self.parts }
    }
}

impl<'a> Lowerer<'a, '_> {
    pub(super) fn component(
        &mut self,
        el: &JSXElement<'a>,
        callee: Span,
    ) -> Box<'a, Component<'a>> {
        let name = &self.source[callee.start as usize..callee.end as usize];
        self.path.push(format!("<{name}>"));
        if name == "For" {
            self.check_inline_each(el);
        }
        let items = self.items(&el.children, false);
        let is_boundary = self.facts.program.islands.contains_key(&el.span.start);
        let mut props = PropsBuilder::new(self.alloc);
        for attr in &el.opening_element.attributes {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    let is_reactive = is_dynamic(&s.argument, false, self.facts);
                    let value = self.expr(&s.argument);
                    props.spread(value, is_reactive);
                }
                JSXAttributeItem::Attribute(a) => {
                    let key = attribute_name(self, a);
                    let site = DirectiveSite::Component { name, is_boundary };
                    if key.starts_with("island:") && self.island_directive(a, key, site) {
                        continue;
                    }
                    if key == "children" && !items.is_empty() {
                        self.children_ignored(a.span);
                        continue;
                    }
                    if let Some(prop) = self.prop(key, a, true) {
                        props.push(prop);
                    }
                }
            }
        }
        if let Some(children) = self.component_children(items) {
            props.push(children);
        }
        self.path.pop();
        let island = self.island(el);
        let callee =
            self.embed(callee, |finder| finder.visit_jsx_element_name(&el.opening_element.name));
        self.boxed(Component { callee, props: props.finish(), island })
    }

    fn check_inline_each(&mut self, el: &JSXElement<'a>) {
        for attr in &el.opening_element.attributes {
            if let JSXAttributeItem::Attribute(a) = attr
                && attribute_name(self, a) == "each"
                && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                && let Some(Expression::ArrayExpression(array)) = c.expression.as_expression()
            {
                self.report(Report::new(
                    Code::InlineEach,
                    array.span,
                    "`<For each={[…]}>` builds a new array on every evaluation, so every row is \
                     rebuilt each time. Hoist the array to a constant or keep it in a signal.",
                ));
            }
        }
    }

    /// One entry of a props object; `None` for an empty `{}` value.
    pub(super) fn prop(
        &mut self,
        key: &'a str,
        a: &JSXAttribute<'a>,
        is_component: bool,
    ) -> Option<Prop<'a>> {
        Some(match &a.value {
            None => Prop::Value { key, value: PropValue::True },
            Some(JSXAttributeValue::StringLiteral(s)) => Prop::Value {
                key,
                value: PropValue::Str(self.str(&decode_entities(s.value.as_str()))),
            },
            Some(JSXAttributeValue::Element(e)) => {
                Prop::Getter { key, value: PropValue::Jsx(self.element(e)) }
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                Prop::Getter { key, value: PropValue::Jsx(self.fragment(f)) }
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                let e = c.expression.as_expression()?;
                if is_component
                    && key == "ref"
                    && let Some(target) = self.assign_target(e)
                {
                    return Some(Prop::ForwardRef(target));
                }
                let value = PropValue::Expr(self.expr(e));
                if is_dynamic(e, is_component, self.facts) {
                    Prop::Getter { key, value }
                } else {
                    Prop::Value { key, value }
                }
            }
        })
    }

    fn component_children(&mut self, items: std::vec::Vec<Item<'_, 'a>>) -> Option<Prop<'a>> {
        let key = "children";
        if items.len() > 1 {
            return Some(Prop::Getter { key, value: PropValue::Children(self.list(items)) });
        }
        Some(match items.into_iter().next()? {
            Item::Text(text) => Prop::Value { key, value: PropValue::Str(self.str(&text)) },
            Item::Element(el) => Prop::Getter { key, value: PropValue::Jsx(self.element(el)) },
            Item::Fragment(f) => Prop::Getter { key, value: PropValue::Jsx(self.fragment(f)) },
            Item::Expr(e) if !is_function(e) && is_dynamic(e, true, self.facts) => {
                Prop::Getter { key, value: PropValue::Expr(self.expr(e)) }
            }
            Item::Expr(e) => Prop::Value { key, value: PropValue::Expr(self.expr(e)) },
        })
    }
}
