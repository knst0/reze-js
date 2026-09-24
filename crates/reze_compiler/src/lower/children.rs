//! Child lists: JSX text rules, dead branches (O5), inserts, conditionals.

use oxc_allocator::Vec;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::constant::{is_dynamic, literal_truthy, static_text};
use super::element::TemplateBuilder;
use super::{Lowerer, is_function};
use crate::diagnostic::{Code, Report};
use crate::html::{clean_jsx_text, decode_entities};
use crate::ir::{Anchor, Child, Conditional, Embed, ExprChild, MemoId, NodeId, Op};

pub enum Item<'b, 'a> {
    Text(String),
    Element(&'b JSXElement<'a>),
    Fragment(&'b JSXFragment<'a>),
    Expr(&'b Expression<'a>),
}

/// A dynamic child waiting for its anchor: the `next_static`-th static sibling, if any.
struct PendingInsert<'a> {
    value: Child<'a>,
    hoisted_test: Option<(MemoId, Embed<'a>)>,
    next_static: usize,
}

impl<'a> Lowerer<'a, '_> {
    /// Meaningful children: cleaned non-empty text (adjacent text merged), elements, fragments
    /// and expressions. For native elements (`is_native`), fragments flatten and constants
    /// become text.
    pub(super) fn items<'b>(
        &mut self,
        children: &'b [JSXChild<'a>],
        is_native: bool,
    ) -> std::vec::Vec<Item<'b, 'a>> {
        let mut items = std::vec::Vec::new();
        self.collect_items(children, is_native, &mut items);
        items
    }

    fn collect_items<'b>(
        &mut self,
        children: &'b [JSXChild<'a>],
        is_native: bool,
        items: &mut std::vec::Vec<Item<'b, 'a>>,
    ) {
        for child in children {
            match child {
                JSXChild::Text(t) => {
                    push_text(items, clean_jsx_text(&decode_entities(t.value.as_str())));
                }
                JSXChild::Element(e) => items.push(Item::Element(e)),
                JSXChild::Fragment(f) if is_native => self.collect_items(&f.children, true, items),
                JSXChild::Fragment(f) => items.push(Item::Fragment(f)),
                JSXChild::ExpressionContainer(c) => {
                    if let Some(e) = c.expression.as_expression() {
                        self.expression_item(e, is_native, items);
                    }
                }
                JSXChild::Spread(s) => items.push(Item::Expr(&s.expression)),
            }
        }
    }

    fn expression_item<'b>(
        &mut self,
        e: &'b Expression<'a>,
        is_native: bool,
        items: &mut std::vec::Vec<Item<'b, 'a>>,
    ) {
        if self.optimize
            && let Some(live) = self.live_branch(e)
        {
            if let Some(live) = live {
                self.expression_item(live, is_native, items);
            }
            return;
        }
        match e.without_parentheses() {
            Expression::JSXElement(el) => items.push(Item::Element(el)),
            Expression::JSXFragment(f) if is_native => self.collect_items(&f.children, true, items),
            Expression::JSXFragment(f) => items.push(Item::Fragment(f)),
            inner => match static_text(inner, self.facts) {
                Some(text) if is_native => push_text(items, text),
                _ => items.push(Item::Expr(e)),
            },
        }
    }

    /// O5: for a child whose condition is a literal, `Some(live branch or nothing)`.
    fn live_branch<'b>(&mut self, e: &'b Expression<'a>) -> Option<Option<&'b Expression<'a>>> {
        let inner = e.without_parentheses();
        let (live, dropped): (Option<&'b Expression<'a>>, Span) = match inner {
            Expression::BooleanLiteral(_) | Expression::NullLiteral(_) => return Some(None),
            Expression::Identifier(id) if id.name.as_str() == "undefined" => return Some(None),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                let truthy = literal_truthy(&l.left, self.facts)?;
                if truthy { (Some(&l.right), l.left.span()) } else { (None, l.right.span()) }
            }
            Expression::ConditionalExpression(c) => {
                if literal_truthy(&c.test, self.facts)? {
                    (Some(&c.consequent), c.alternate.span())
                } else {
                    (Some(&c.alternate), c.consequent.span())
                }
            }
            _ => return None,
        };
        self.report(
            Report::new(
                Code::DeadBranchRemoved,
                inner.span(),
                "The condition is a literal, so this branch can never render; it was removed.",
            )
            .label(dropped, "never rendered"),
        );
        Some(live)
    }

    /// Children of a native element: static ones into the template, dynamic ones as inserts
    /// anchored before the next static node (SPEC §7.5).
    pub(super) fn native_children(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        parent: NodeId,
        items: std::vec::Vec<Item<'_, 'a>>,
        in_svg: bool,
    ) {
        let is_sole = items.len() == 1;
        let mut pending: std::vec::Vec<PendingInsert<'a>> = std::vec::Vec::new();
        let mut last_is_text = false;
        let mut after_dynamic = false;
        let mut static_count = 0;
        for item in items {
            match item {
                Item::Text(text) => {
                    if after_dynamic && last_is_text {
                        builder.html.push_str("<!>");
                        builder.node(parent);
                        static_count += 1;
                    }
                    crate::html::escape_text(&mut builder.html, &text);
                    builder.node(parent);
                    static_count += 1;
                    last_is_text = true;
                    after_dynamic = false;
                }
                Item::Element(el) => match self.tag_of(&el.opening_element.name) {
                    super::Tag::Native(tag) => {
                        let node = builder.node(parent);
                        self.native(builder, el, tag, node, in_svg);
                        static_count += 1;
                        last_is_text = false;
                        after_dynamic = false;
                    }
                    super::Tag::Component(callee) => {
                        let component = self.component(el, callee);
                        pending.push(PendingInsert {
                            value: Child::Jsx(crate::ir::Jsx::Component(component)),
                            hoisted_test: None,
                            next_static: static_count,
                        });
                        after_dynamic = true;
                    }
                },
                Item::Fragment(f) => {
                    let fragment = self.fragment(f);
                    pending.push(PendingInsert {
                        value: Child::Jsx(fragment),
                        hoisted_test: None,
                        next_static: static_count,
                    });
                    after_dynamic = true;
                }
                Item::Expr(e) => {
                    let (value, hoisted_test) = self.insert_value(e, builder);
                    pending.push(PendingInsert { value, hoisted_test, next_static: static_count });
                    after_dynamic = true;
                }
            }
        }

        let statics = builder.children(parent).to_vec();
        for PendingInsert { value, hoisted_test, next_static } in pending {
            let anchor = if is_sole {
                Anchor::Only
            } else if let Some(&node) = statics.get(next_static) {
                builder.reference(node);
                Anchor::Before(node)
            } else {
                Anchor::End
            };
            if let Some((id, test)) = hoisted_test {
                builder.ops.push(Op::Memo { id, test });
            }
            builder.reference(parent);
            builder.ops.push(Op::Insert { parent, value, anchor });
        }
    }

    /// The value handed to `insert`; a memoizable condition hoists its test into `Op::Memo`.
    fn insert_value(
        &mut self,
        e: &Expression<'a>,
        builder: &mut TemplateBuilder<'a>,
    ) -> (Child<'a>, Option<(MemoId, Embed<'a>)>) {
        if let Some((test, consequent, alternate)) = self.conditional_parts(e) {
            let test = self.expr(test);
            let consequent = self.expr(consequent);
            let alternate = alternate.map(|a| self.expr(a));
            let memo = MemoId(builder.memo_count);
            builder.memo_count += 1;
            let conditional = Conditional { memo, consequent, alternate };
            let child = Child::Expr(ExprChild::Conditional(self.boxed(conditional)));
            return (child, Some((memo, test)));
        }
        let child = if is_dynamic(e, false, self.facts) {
            ExprChild::Getter(self.getter(e))
        } else {
            ExprChild::Static(self.expr(e))
        };
        (Child::Expr(child), None)
    }

    /// `test ? a : b` / `test && a` worth memoizing: a dynamic test and a branch that builds
    /// JSX or reads reactive state.
    fn conditional_parts<'b>(
        &self,
        e: &'b Expression<'a>,
    ) -> Option<(&'b Expression<'a>, &'b Expression<'a>, Option<&'b Expression<'a>>)> {
        let (test, consequent, alternate) = match e.without_parentheses() {
            Expression::ConditionalExpression(c) => (&c.test, &c.consequent, Some(&c.alternate)),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                (&l.left, &l.right, None)
            }
            _ => return None,
        };
        let branch_is_dynamic = is_dynamic(consequent, true, self.facts)
            || alternate.is_some_and(|a| is_dynamic(a, true, self.facts));
        (is_dynamic(test, false, self.facts) && branch_is_dynamic)
            .then_some((test, consequent, alternate))
    }

    /// Entries of a children array: dynamic expressions become memos so they stay reactive.
    pub(super) fn list(&mut self, items: std::vec::Vec<Item<'_, 'a>>) -> Vec<'a, Child<'a>> {
        let mut list = self.vec();
        for item in items {
            let child = match item {
                Item::Text(text) => Child::Text(self.str(&text)),
                Item::Element(el) => Child::Jsx(self.element(el)),
                Item::Fragment(f) => Child::Jsx(self.fragment(f)),
                Item::Expr(e) if !is_function(e) && is_dynamic(e, false, self.facts) => {
                    Child::Expr(ExprChild::Memo(self.getter(e)))
                }
                Item::Expr(e) => Child::Expr(ExprChild::Static(self.expr(e))),
            };
            list.push(child);
        }
        list
    }
}

fn push_text(items: &mut std::vec::Vec<Item<'_, '_>>, text: String) {
    if text.is_empty() {
        return;
    }
    if let Some(Item::Text(previous)) = items.last_mut() {
        previous.push_str(&text);
    } else {
        items.push(Item::Text(text));
    }
}
