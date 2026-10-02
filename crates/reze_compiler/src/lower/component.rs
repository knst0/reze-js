use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::Span;

use super::children::Item;
use super::constant::{inline_entries, is_defined, is_dynamic};
use super::{Lowerer, attribute_name, is_function};
use crate::html::decode_entities;
use crate::ir::{Component, Embed, Flow, Jsx, Prop, PropValue, Props, PropsPart};

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

/// Whether each spread of `attrs` dissolves into entries (T1): the argument is an object literal
/// of static, defined values, and no later regular entry with the same key may be `undefined`.
/// `later` holds entries pushed after the attributes, like a component's `children`.
pub(super) fn spread_inline<'a>(
    lowerer: &Lowerer<'a, '_>,
    attrs: &[JSXAttributeItem<'a>],
    later: &[(&'a str, bool)],
) -> std::vec::Vec<bool> {
    let mut inline = std::vec::Vec::with_capacity(attrs.len());
    for (i, attr) in attrs.iter().enumerate() {
        let JSXAttributeItem::SpreadAttribute(s) = attr else {
            inline.push(false);
            continue;
        };
        let Some(entries) = inline_entries(&s.argument) else {
            inline.push(false);
            continue;
        };
        inline.push(entries.iter().all(|(key, _)| {
            !later.iter().any(|(late, defined)| *late == *key && !defined)
                && !attrs[i + 1..].iter().any(|after| match after {
                    JSXAttributeItem::Attribute(a) => {
                        attribute_name(lowerer, a) == *key && attr_defined(a) == Some(false)
                    }
                    JSXAttributeItem::SpreadAttribute(_) => false,
                })
        }));
    }
    inline
}

fn attr_defined(a: &JSXAttribute) -> Option<bool> {
    match &a.value {
        None => Some(true),
        Some(JSXAttributeValue::StringLiteral(_)) => Some(true),
        Some(JSXAttributeValue::Element(_)) | Some(JSXAttributeValue::Fragment(_)) => Some(true),
        Some(JSXAttributeValue::ExpressionContainer(c)) => {
            c.expression.as_expression().map(is_defined)
        }
    }
}

/// Defined-ness of the `children` entry pushed after the attributes, if any.
fn children_defined(items: &[Item<'_, '_>]) -> Option<bool> {
    if items.len() > 1 {
        return Some(true);
    }
    Some(match items.first()? {
        Item::Text(_) | Item::Element(_) | Item::Fragment(_) => true,
        Item::Expr(e) => is_defined(e),
    })
}

impl<'a> Lowerer<'a, '_> {
    pub(super) fn component(
        &mut self,
        el: &JSXElement<'a>,
        callee: Span,
    ) -> Box<'a, Component<'a>> {
        self.path.push(format!("<{}>", self.text(callee)));
        let items = self.items(&el.children, false);
        let children = children_defined(&items).map(|defined| vec![("children", defined)]);
        let inline =
            spread_inline(self, &el.opening_element.attributes, children.as_deref().unwrap_or(&[]));
        let mut props = PropsBuilder::new(self.alloc);
        for (attr, &dissolve) in el.opening_element.attributes.iter().zip(&inline) {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    self.spread_attr(&mut props, &s.argument, dissolve, true);
                }
                JSXAttributeItem::Attribute(a) => {
                    let key = attribute_name(self, a);
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
        let callee =
            self.embed(callee, |finder| finder.visit_jsx_element_name(&el.opening_element.name));
        self.boxed(Component { callee, props: props.finish() })
    }

    /// A spread attribute: entries spliced for a T1 `dissolve` argument, a generic part otherwise.
    pub(super) fn spread_attr(
        &mut self,
        props: &mut PropsBuilder<'a>,
        arg: &Expression<'a>,
        dissolve: bool,
        is_component: bool,
    ) {
        if dissolve && let Some(entries) = inline_entries(arg) {
            for (key, value) in entries {
                let entry = PropValue::Expr(self.expr(value));
                props.push(if is_dynamic(value, is_component, self.analysis) {
                    Prop::Getter { key, value: entry }
                } else {
                    Prop::Value { key, value: entry }
                });
            }
            return;
        }
        props.spread(self.expr(arg), is_dynamic(arg, false, self.analysis));
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
                if is_dynamic(e, is_component, self.analysis) {
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
            Item::Expr(e) if is_function(e) || !is_dynamic(e, true, self.analysis) => {
                Prop::Value { key, value: PropValue::Expr(self.expr(e)) }
            }
            Item::Expr(e) => match self.conditional(e) {
                Some(branch) => Prop::Getter {
                    key,
                    value: PropValue::Jsx(Jsx::Flow(self.boxed(Flow::Show(branch)))),
                },
                None => Prop::Getter { key, value: PropValue::Expr(self.expr(e)) },
            },
        })
    }
}
