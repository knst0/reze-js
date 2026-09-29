use oxc_allocator::Vec;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;

use super::constant::{is_dynamic, literal_truthy, static_text};
use super::element::TemplateBuilder;
use super::{Lowerer, Tag, attribute_name, has_jsx, is_function};
use crate::analyze::Intrinsic;
use crate::diagnostic::{Code, Report};
use crate::html::{clean_jsx_text, decode_entities, escape_text};
use crate::ir::{
    Anchor, Bind, BindTarget, Branch, Child, Conditional, ExprChild, Flow, Jsx, NodeId, Op,
    Placement, Render, TextPart, Value,
};
use crate::kind::{is_boolean, static_kind};

pub enum Item<'b, 'a> {
    Text(String),
    Element(&'b JSXElement<'a>),
    Fragment(&'b JSXFragment<'a>),
    Expr(&'b Expression<'a>),
}

enum Segment<'b, 'a> {
    Item(Item<'b, 'a>),
    /// Adjacent text and expressions of a known kind, rendered as one text node.
    TextRun(std::vec::Vec<Item<'b, 'a>>),
}

/// An insert op waiting for its anchor: the `next_static`-th static sibling, if any.
struct PendingInsert {
    op: usize,
    next_static: usize,
}

impl<'a> Lowerer<'a, '_> {
    /// Meaningful children: cleaned non-empty text (adjacent text merged), elements, fragments
    /// and expressions, with dead branches dropped. For native elements (`is_native`),
    /// fragments flatten and constants become text.
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
                    push_text(items, clean_jsx_text(&decode_entities(t.value.as_str())))
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
        if let Some(live) = self.live_branch(e) {
            if let Some(live) = live {
                self.expression_item(live, is_native, items);
            }
            return;
        }
        match e.without_parentheses() {
            Expression::JSXElement(el) => items.push(Item::Element(el)),
            Expression::JSXFragment(f) if is_native => self.collect_items(&f.children, true, items),
            Expression::JSXFragment(f) => items.push(Item::Fragment(f)),
            inner => match static_text(inner, self.analysis) {
                Some(text) if is_native => push_text(items, text),
                _ => items.push(Item::Expr(e)),
            },
        }
    }

    /// O5: for a child whose condition is a literal, `Some(live branch or nothing)`.
    fn live_branch<'b>(&mut self, e: &'b Expression<'a>) -> Option<Option<&'b Expression<'a>>> {
        let inner = e.without_parentheses();
        let (live, dropped) = match inner {
            Expression::BooleanLiteral(_) | Expression::NullLiteral(_) => return Some(None),
            Expression::Identifier(id) if id.name == "undefined" => return Some(None),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                if literal_truthy(&l.left, self.analysis)? {
                    (Some(&l.right), l.left.span())
                } else {
                    (None, l.right.span())
                }
            }
            Expression::ConditionalExpression(c) => {
                if literal_truthy(&c.test, self.analysis)? {
                    (Some(&c.consequent), c.alternate.span())
                } else {
                    (Some(&c.alternate), c.consequent.span())
                }
            }
            _ => return None,
        };
        self.report(
            Report::new(Code::DeadBranchRemoved, inner.span()).label(dropped, "never rendered"),
        );
        Some(live)
    }

    /// Children of a native element: static ones into the template, dynamic ones as inserts
    /// anchored before the next static node.
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
            let is_text = matches!(segment, Segment::TextRun(_) | Segment::Item(Item::Text(_)));
            if is_text && after_dynamic && last_is_text {
                builder.node(parent);
                builder.html.push_str("<!>");
                static_count += 1;
            }
            let value = match segment {
                Segment::TextRun(parts) => {
                    self.text_run(builder, parent, parts);
                    None
                }
                Segment::Item(Item::Text(text)) => {
                    builder.node(parent);
                    escape_text(&mut builder.html, &text);
                    None
                }
                Segment::Item(Item::Element(el)) => match self.tag_of(&el.opening_element.name) {
                    Tag::Native(tag) => {
                        let node = builder.node(parent);
                        self.native(builder, el, tag, node, in_svg);
                        None
                    }
                    Tag::Component(callee) => {
                        Some(Child::Jsx(Jsx::Component(self.component(el, callee))))
                    }
                    Tag::Intrinsic(intrinsic) => Some(self.native_flow(el, intrinsic)),
                },
                Segment::Item(Item::Fragment(f)) => Some(Child::Jsx(self.fragment(f))),
                Segment::Item(Item::Expr(e)) => Some(self.insert_value(e)),
            };
            match value {
                None => {
                    static_count += 1;
                    last_is_text = is_text;
                    after_dynamic = false;
                }
                Some(value) => {
                    pending
                        .push(PendingInsert { op: builder.ops.len(), next_static: static_count });
                    builder.ops.push(Op::Insert { parent, value, anchor: Anchor::End });
                    after_dynamic = true;
                }
            }
        }
        if pending.is_empty() {
            return;
        }
        builder.reference(parent);
        let statics = builder.children(parent).to_vec();
        for insert in pending {
            let resolved = if is_sole {
                Anchor::Only
            } else if let Some(&node) = statics.get(insert.next_static) {
                builder.reference(node);
                Anchor::Before(node)
            } else {
                Anchor::End
            };
            if let Op::Insert { anchor, .. } = &mut builder.ops[insert.op] {
                *anchor = resolved;
            }
        }
    }

    fn is_text_expression(&self, e: &Expression<'a>) -> bool {
        static_kind(e, self.analysis).is_some()
    }

    /// Groups adjacent text and known-kind expressions into text runs; a run needs an expression.
    fn segments<'b>(&self, items: std::vec::Vec<Item<'b, 'a>>) -> std::vec::Vec<Segment<'b, 'a>> {
        let mut segments = std::vec::Vec::new();
        let mut run: std::vec::Vec<Item<'b, 'a>> = std::vec::Vec::new();
        let flush = |run: &mut std::vec::Vec<Item<'b, 'a>>,
                     segments: &mut std::vec::Vec<Segment<'b, 'a>>| {
            if run.iter().any(|part| matches!(part, Item::Expr(_))) {
                segments.push(Segment::TextRun(std::mem::take(run)));
            } else {
                segments.extend(run.drain(..).map(Segment::Item));
            }
        };
        for item in items {
            match item {
                Item::Text(_) => run.push(item),
                Item::Expr(e) if self.is_text_expression(e) => run.push(item),
                item => {
                    flush(&mut run, &mut segments);
                    segments.push(Segment::Item(item));
                }
            }
        }
        flush(&mut run, &mut segments);
        segments
    }

    fn text_run(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        parent: NodeId,
        parts: std::vec::Vec<Item<'_, 'a>>,
    ) {
        let node = builder.node(parent);
        builder.html.push(' ');
        let mut is_reactive = false;
        let mut lowered = self.vec();
        for part in parts {
            match part {
                Item::Text(text) => lowered.push(TextPart::Static(self.str(&text))),
                Item::Expr(e) => {
                    is_reactive |= is_dynamic(e, false, self.analysis);
                    lowered.push(TextPart::Dynamic(self.expr(e)));
                }
                Item::Element(_) | Item::Fragment(_) => {}
            }
        }
        builder.reference(node);
        let target = BindTarget::Text;
        let value = Value::Text(lowered);
        if is_reactive {
            builder.binds.push(Bind { node, target, value });
        } else {
            builder.ops.push(Op::Set { node, target, value });
        }
    }

    /// A flow tag inserted into a native element; a `<Show>` without a function child or components
    /// becomes a conditional over one memo of `when`'s truthiness, while one holding a component
    /// or intrinsic element stays a `branch` so `loading` can hold its side until the new one is ready.
    fn native_flow(&mut self, el: &JSXElement<'a>, intrinsic: Intrinsic) -> Child<'a> {
        let jsx = self.control_flow(el, intrinsic);
        let Jsx::Flow(flow) = jsx else { return Child::Jsx(jsx) };
        match flow.unbox() {
            Flow::Show(Branch {
                when,
                child: Render::Child(consequent),
                fallback: fallback @ (None | Some(Render::Child(_))),
            }) if !self.contains_component(el) => {
                let alternate = match fallback {
                    Some(Render::Child(child)) => Some(inline(child)),
                    _ => None,
                };
                let conditional = Conditional {
                    test: when.expr,
                    test_is_boolean: self.is_boolean_when(el),
                    consequent: inline(consequent),
                    alternate,
                };
                Child::Expr(ExprChild::Conditional(self.boxed(conditional)))
            }
            flow => Child::Jsx(Jsx::Flow(self.boxed(flow))),
        }
    }

    /// Whether the last `when` of `el` always yields a boolean and can be an arrow's body as is.
    fn is_boolean_when(&self, el: &JSXElement<'a>) -> bool {
        let when = el.opening_element.attributes.iter().rev().find_map(|item| match item {
            JSXAttributeItem::Attribute(a) if attribute_name(self, a) == "when" => Some(a),
            _ => None,
        });
        let Some(JSXAttributeValue::ExpressionContainer(container)) =
            when.and_then(|a| a.value.as_ref())
        else {
            return false;
        };
        container.expression.as_expression().is_some_and(|e| {
            is_boolean(e, self.analysis) && self.source.as_bytes()[e.span().start as usize] != b'{'
        })
    }

    /// Whether `el` holds a component or intrinsic element among its children or its attribute
    /// values; such a `<Show>` stays a `branch` instead of becoming a conditional.
    fn contains_component(&self, el: &JSXElement<'a>) -> bool {
        let mut check = ComponentCheck { lowerer: self, found: false };
        for child in &el.children {
            check.visit_jsx_child(child);
            if check.found {
                return true;
            }
        }
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(a) = item else { continue };
            if let Some(value) = a.value.as_ref() {
                check.visit_jsx_attribute_value(value);
                if check.found {
                    return true;
                }
            }
        }
        check.found
    }

    /// The value an `insert` receives for a child expression.
    fn insert_value(&mut self, e: &Expression<'a>) -> Child<'a> {
        if let Some(branch) = self.conditional(e) {
            return Child::Jsx(Jsx::Flow(self.boxed(Flow::Show(branch))));
        }
        if is_dynamic(e, false, self.analysis) {
            Child::Expr(ExprChild::Getter(self.getter(e)))
        } else {
            Child::Expr(ExprChild::Static(self.expr(e)))
        }
    }

    /// `test ? a : b` or `test && a` with a non-literal test and JSX in a branch, as a branch.
    pub(super) fn conditional(&mut self, e: &Expression<'a>) -> Option<Branch<'a>> {
        let (test, consequent, alternate) = match e.without_parentheses() {
            Expression::ConditionalExpression(c) => (&c.test, &c.consequent, Some(&c.alternate)),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                (&l.left, &l.right, None)
            }
            _ => return None,
        };
        let has_jsx_branch = has_jsx(|check| check.visit_expression(consequent))
            || alternate.is_some_and(|a| has_jsx(|check| check.visit_expression(a)));
        if !has_jsx_branch || literal_truthy(test, self.analysis).is_some() {
            return None;
        }
        let when = self.source(test);
        let child = self.render(consequent);
        let fallback = alternate.map(|a| self.render(a));
        Some(Branch { when, child, fallback })
    }

    /// A branch of a flow, rendered by calling `() => child`.
    pub(super) fn render(&mut self, e: &Expression<'a>) -> Render<'a> {
        let child = match e.without_parentheses() {
            Expression::JSXElement(el) => Child::Jsx(self.element(el)),
            Expression::JSXFragment(f) => Child::Jsx(self.fragment(f)),
            _ => match self.conditional(e) {
                Some(branch) => Child::Jsx(Jsx::Flow(self.boxed(Flow::Show(branch)))),
                None if is_dynamic(e, false, self.analysis) => {
                    Child::Expr(ExprChild::Getter(self.getter(e)))
                }
                None if self.source.as_bytes()[e.span().start as usize] == b'{' => {
                    Child::Expr(ExprChild::Getter(self.thunk(e)))
                }
                None => Child::Expr(ExprChild::Static(self.expr(e))),
            },
        };
        render_child(child)
    }

    /// Entries of a children array: a dynamic entry is a getter so it stays reactive.
    pub(super) fn list(&mut self, items: std::vec::Vec<Item<'_, 'a>>) -> Vec<'a, Child<'a>> {
        let mut list = self.vec();
        for item in items {
            list.push(match item {
                Item::Text(text) => Child::Text(self.str(&text)),
                Item::Element(el) => Child::Jsx(self.element(el)),
                Item::Fragment(f) => Child::Jsx(self.fragment(f)),
                Item::Expr(e) => self.list_entry(e),
            });
        }
        list
    }

    fn list_entry(&mut self, e: &Expression<'a>) -> Child<'a> {
        if let Some(branch) = self.conditional(e) {
            return Child::Jsx(Jsx::Flow(self.boxed(Flow::Show(branch))));
        }
        if !is_function(e) && is_dynamic(e, false, self.analysis) {
            Child::Expr(ExprChild::Getter(self.getter(e)))
        } else {
            Child::Expr(ExprChild::Static(self.expr(e)))
        }
    }
}

/// Finds a component or intrinsic element for `contains_component`.
struct ComponentCheck<'l, 'a, 'f> {
    lowerer: &'l Lowerer<'a, 'f>,
    found: bool,
}

impl<'a, 'f> Visit<'a> for ComponentCheck<'_, 'a, 'f> {
    fn visit_jsx_element(&mut self, el: &JSXElement<'a>) {
        if self.found {
            return;
        }
        if !matches!(self.lowerer.tag_of(&el.opening_element.name), Tag::Native(_)) {
            self.found = true;
        } else {
            walk::walk_jsx_element(self, el);
        }
    }
}

/// `() => child`; a lone fragment entry unwraps, and a template with work is the arrow's body.
pub(super) fn render_child(child: Child<'_>) -> Render<'_> {
    let mut child = match child {
        Child::Jsx(Jsx::Fragment(mut list)) if list.len() == 1 => list.pop().expect("one entry"),
        child => child,
    };
    if let Child::Jsx(Jsx::Template(template)) = &mut child
        && template.has_work()
    {
        template.placement = Placement::Block;
    }
    Render::Child(child)
}

/// `child` as an expression inside a conditional.
fn inline(mut child: Child<'_>) -> Child<'_> {
    if let Child::Jsx(Jsx::Template(template)) = &mut child {
        template.placement = Placement::Expression;
    }
    child
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
