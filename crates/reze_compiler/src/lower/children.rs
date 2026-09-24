//! Child lists: JSX text rules, dead branches (O5), inserts, conditionals.

use oxc_allocator::Vec;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};

use super::constant::{is_dynamic, literal_truthy, static_text};
use super::element::TemplateBuilder;
use super::types::{StaticKind, static_kind};
use super::{Lowerer, is_function};
use crate::diagnostic::{Code, Report};
use crate::html::{clean_jsx_text, decode_entities};
use crate::ir::{
    Anchor, Bind, BindTarget, Child, Conditional, Embed, ExprChild, MemoId, NodeId, Op, TextPart,
    Value,
};

pub enum Item<'b, 'a> {
    Text(String),
    Element(&'b JSXElement<'a>),
    Fragment(&'b JSXFragment<'a>),
    Expr(&'b Expression<'a>),
}

enum RunPart<'b, 'a> {
    Text(String),
    Expr(&'b Expression<'a>),
}

enum Segment<'b, 'a> {
    Item(Item<'b, 'a>),
    /// Adjacent text and expressions of a known kind, rendered as one text node (SPEC §7.5).
    TextRun(std::vec::Vec<RunPart<'b, 'a>>),
}

/// An insert op, pushed in document order, waiting for its anchor: the `next_static`-th static
/// sibling, if any.
struct PendingInsert {
    op: usize,
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
        let segments = self.segments(items);
        let is_sole = segments.len() == 1;
        let mut pending: std::vec::Vec<PendingInsert> = std::vec::Vec::new();
        let mut last_is_text = false;
        let mut after_dynamic = false;
        let mut static_count = 0;
        for segment in segments {
            let item = match segment {
                Segment::Item(item) => item,
                Segment::TextRun(parts) => {
                    if after_dynamic && last_is_text {
                        builder.node(parent);
                        builder.html.push_str("<!>");
                        static_count += 1;
                    }
                    self.text_run(builder, parent, parts);
                    static_count += 1;
                    last_is_text = true;
                    after_dynamic = false;
                    continue;
                }
            };
            let value = match item {
                Item::Text(text) => {
                    if after_dynamic && last_is_text {
                        builder.node(parent);
                        builder.html.push_str("<!>");
                        static_count += 1;
                    }
                    builder.node(parent);
                    crate::html::escape_text(&mut builder.html, &text);
                    static_count += 1;
                    last_is_text = true;
                    after_dynamic = false;
                    continue;
                }
                Item::Element(el) => match self.tag_of(&el.opening_element.name) {
                    super::Tag::Native(tag) => {
                        let node = builder.node(parent);
                        self.native(builder, el, tag, node, in_svg);
                        static_count += 1;
                        last_is_text = false;
                        after_dynamic = false;
                        continue;
                    }
                    super::Tag::Component(callee) => match self.show_conditional(el, builder) {
                        Some((child, id, test)) => {
                            builder.ops.push(Op::Memo { id, test });
                            child
                        }
                        None => Child::Jsx(crate::ir::Jsx::Component(self.component(el, callee))),
                    },
                },
                Item::Fragment(f) => Child::Jsx(self.fragment(f)),
                Item::Expr(e) => {
                    let (value, hoisted_test) = self.insert_value(e, builder);
                    if let Some((id, test)) = hoisted_test {
                        builder.ops.push(Op::Memo { id, test });
                    }
                    value
                }
            };
            pending.push(PendingInsert { op: builder.ops.len(), next_static: static_count });
            builder.ops.push(Op::Insert { parent, value, anchor: Anchor::End, inserts_after: 0 });
            after_dynamic = true;
        }

        let statics = builder.children(parent).to_vec();
        if !pending.is_empty() {
            builder.reference(parent);
        }
        for (i, insert) in pending.iter().enumerate() {
            let shared = pending[i + 1..].iter().filter(|p| p.next_static == insert.next_static);
            let resolved = if is_sole {
                Anchor::Only
            } else if let Some(&node) = statics.get(insert.next_static) {
                builder.reference(node);
                Anchor::Before(node)
            } else {
                Anchor::End
            };
            if let Op::Insert { anchor, inserts_after, .. } = &mut builder.ops[insert.op] {
                *anchor = resolved;
                *inserts_after = shared.count() as u32;
            }
        }
    }

    fn text_kind(&self, e: &Expression<'a>) -> Option<StaticKind> {
        if self.conditional_parts(e).is_some() {
            return None;
        }
        static_kind(e, self.facts, self.scoping, self.nodes)
    }

    /// Groups adjacent text and expressions of a known kind into text runs; a run needs an
    /// expression and text that is never empty: static text, or only numeric expressions.
    fn segments<'b>(&self, items: std::vec::Vec<Item<'b, 'a>>) -> std::vec::Vec<Segment<'b, 'a>> {
        let mut segments = std::vec::Vec::new();
        let mut run: std::vec::Vec<RunPart<'b, 'a>> = std::vec::Vec::new();
        let flush = |run: &mut std::vec::Vec<RunPart<'b, 'a>>,
                     segments: &mut std::vec::Vec<Segment<'b, 'a>>,
                     lowerer: &Self| {
            let has_expression = run.iter().any(|part| matches!(part, RunPart::Expr(_)));
            let has_text = run.iter().any(|part| matches!(part, RunPart::Text(t) if !t.is_empty()));
            let is_numeric = run.iter().all(|part| match part {
                RunPart::Text(_) => true,
                RunPart::Expr(e) => lowerer.text_kind(e) == Some(StaticKind::Numeric),
            });
            if has_expression && (has_text || is_numeric) {
                segments.push(Segment::TextRun(std::mem::take(run)));
                return;
            }
            for part in run.drain(..) {
                segments.push(Segment::Item(match part {
                    RunPart::Text(text) => Item::Text(text),
                    RunPart::Expr(e) => Item::Expr(e),
                }));
            }
        };
        for item in items {
            match item {
                Item::Text(text) => run.push(RunPart::Text(text)),
                Item::Expr(e) if self.text_kind(e).is_some() => run.push(RunPart::Expr(e)),
                item => {
                    flush(&mut run, &mut segments, self);
                    segments.push(Segment::Item(item));
                }
            }
        }
        flush(&mut run, &mut segments, self);
        segments
    }

    fn text_run(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        parent: NodeId,
        parts: std::vec::Vec<RunPart<'_, 'a>>,
    ) {
        let node = builder.node(parent);
        let start = builder.html.len() as u32;
        let mut is_reactive = false;
        let mut lowered = self.vec();
        for part in parts {
            match part {
                RunPart::Text(text) => {
                    crate::html::escape_text(&mut builder.html, &text);
                    lowered.push(TextPart::Static(self.str(&text)));
                }
                RunPart::Expr(e) => {
                    is_reactive |= is_dynamic(e, false, self.facts);
                    let at = builder.html.len() as u32;
                    lowered.push(TextPart::Dynamic { value: self.expr(e), at });
                }
            }
        }
        let placeholder = (builder.html.len() as u32 == start).then(|| {
            builder.html.push(' ');
            start
        });
        builder.reference(node);
        let target = BindTarget::Text { placeholder };
        let value = Value::Text(lowered);
        if is_reactive {
            builder.binds.push(Bind { node, target, value });
        } else {
            builder.ops.push(Op::Set { node, target, value });
        }
    }

    /// O7: a runtime `<Show when fallback?>` with one non-function child, as the conditional
    /// `when ? child : fallback`.
    fn show_conditional(
        &mut self,
        el: &JSXElement<'a>,
        builder: &mut TemplateBuilder<'a>,
    ) -> Option<(Child<'a>, MemoId, Embed<'a>)> {
        if !self.optimize {
            return None;
        }
        let JSXElementName::IdentifierReference(tag) = &el.opening_element.name else {
            return None;
        };
        if self.facts.primitives.of_reference(tag, self.scoping)
            != Some(crate::facts::Primitive::Show)
        {
            return None;
        }
        let mut when = None;
        let mut fallback = None;
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(attribute) = item else { return None };
            let JSXAttributeName::Identifier(name) = &attribute.name else { return None };
            match (name.name.as_str(), attribute.value.as_ref()?) {
                ("when", JSXAttributeValue::ExpressionContainer(c)) => {
                    when = Some(c.expression.as_expression()?);
                }
                ("fallback", value) => fallback = Some(value),
                _ => return None,
            }
        }
        let when = when?;
        let mut children = el.children.iter().filter(|child| match child {
            JSXChild::Text(text) => {
                !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty()
            }
            _ => true,
        });
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        let consequent = match child {
            JSXChild::Element(child) => self.embed(child.span, |f| f.visit_jsx_element(child)),
            JSXChild::Fragment(child) => self.embed(child.span, |f| f.visit_jsx_fragment(child)),
            JSXChild::ExpressionContainer(c) => {
                self.untracked_branch(c.expression.as_expression()?)?
            }
            _ => return None,
        };
        let alternate = match fallback {
            None => None,
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                Some(self.untracked_branch(c.expression.as_expression()?)?)
            }
            Some(JSXAttributeValue::Element(e)) => {
                Some(self.embed(e.span, |f| f.visit_jsx_element(e)))
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                Some(self.embed(f.span, |finder| finder.visit_jsx_fragment(f)))
            }
            Some(JSXAttributeValue::StringLiteral(_)) => return None,
        };
        let test = self.expr(when);
        let memo = MemoId(builder.memo_count);
        builder.memo_count += 1;
        self.report(Report::new(
            Code::ShowInlined,
            el.opening_element.span,
            "This `<Show>` compiled to a conditional: one memo of `when`'s truthiness and an \
             insert, instead of a component with its own computeds.",
        ));
        let conditional = Conditional { memo, consequent, alternate };
        Some((Child::Expr(ExprChild::Conditional(self.boxed(conditional))), memo, test))
    }

    /// A `<Show>` branch that reads nothing reactive while it is built, as `<Show>` builds it
    /// untracked: JSX, or an expression without reactive reads.
    fn untracked_branch(&mut self, e: &Expression<'a>) -> Option<Embed<'a>> {
        let is_jsx = matches!(
            e.without_parentheses(),
            Expression::JSXElement(_) | Expression::JSXFragment(_)
        );
        (is_jsx || !(is_function(e) || is_dynamic(e, true, self.facts))).then(|| self.expr(e))
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
