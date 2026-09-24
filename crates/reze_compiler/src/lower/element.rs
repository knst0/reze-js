//! Native elements → `Template`: HTML, the node tree and its walks, and the runtime ops.

use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;

use super::children::Item;
use super::{Lowerer, attribute_name};
use crate::diagnostic::{Code, Report};
use crate::html::{is_svg_element, is_void};
use crate::ir::{Bind, From, Namespace, NodeId, Op, Template, Walk};

pub struct TemplateBuilder<'a> {
    pub html: String,
    nodes: std::vec::Vec<NodeInfo>,
    pub ops: Vec<'a, Op<'a>>,
    pub binds: Vec<'a, Bind<'a>>,
    pub memo_count: u32,
}

#[derive(Default)]
struct NodeInfo {
    children: std::vec::Vec<NodeId>,
    is_referenced: bool,
}

impl<'a> TemplateBuilder<'a> {
    fn new(alloc: &'a Allocator) -> Self {
        Self {
            html: String::new(),
            nodes: vec![NodeInfo::default()],
            ops: Vec::new_in(&alloc),
            binds: Vec::new_in(&alloc),
            memo_count: 0,
        }
    }

    /// A new static node (element, text run or `<!>` marker) appended to `parent`.
    pub fn node(&mut self, parent: NodeId) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(NodeInfo::default());
        self.nodes[parent.index()].children.push(id);
        id
    }

    pub fn children(&self, parent: NodeId) -> &[NodeId] {
        &self.nodes[parent.index()].children
    }

    /// Marks `node` as used by an op, so the template declares a variable for it.
    pub fn reference(&mut self, node: NodeId) {
        self.nodes[node.index()].is_referenced = true;
    }

    fn finish(self, namespace: Namespace, alloc: &'a Allocator) -> Template<'a> {
        let mut needs_walk = vec![false; self.nodes.len()];
        mark_needs_walk(&self.nodes, NodeId::ROOT, &mut needs_walk);
        let mut walks = Vec::new_in(&alloc);
        collect_walks(&self.nodes, &needs_walk, NodeId::ROOT, &mut walks);
        Template {
            html: alloc.alloc_str(&self.html),
            namespace,
            node_count: self.nodes.len() as u32,
            walks,
            ops: self.ops,
            binds: self.binds,
            memo_count: self.memo_count,
        }
    }
}

/// A node needs a walk when it or a descendant is referenced.
fn mark_needs_walk(nodes: &[NodeInfo], id: NodeId, needs_walk: &mut [bool]) -> bool {
    let mut needed = nodes[id.index()].is_referenced;
    for &child in &nodes[id.index()].children {
        needed |= mark_needs_walk(nodes, child, needs_walk);
    }
    needs_walk[id.index()] = needed;
    needed
}

/// Each needed node walks `nextSibling` from its nearest needed previous sibling, or
/// `firstChild` from its parent.
fn collect_walks(
    nodes: &[NodeInfo],
    needs_walk: &[bool],
    parent: NodeId,
    walks: &mut Vec<'_, Walk>,
) {
    let mut from = From::FirstChildOf(parent);
    let mut from_index = 0;
    for (index, &child) in nodes[parent.index()].children.iter().enumerate() {
        if !needs_walk[child.index()] {
            continue;
        }
        walks.push(Walk { node: child, from, next_siblings: (index - from_index) as u32 });
        from = From::Node(child);
        from_index = index;
        collect_walks(nodes, needs_walk, child, walks);
    }
}

impl<'a> Lowerer<'a, '_> {
    pub(super) fn template(&mut self, el: &JSXElement<'a>, tag: &'a str) -> Box<'a, Template<'a>> {
        let namespace = if is_svg_element(tag) {
            Namespace::Svg
        } else if tag == "math" {
            Namespace::MathMl
        } else {
            Namespace::Html
        };
        let mut builder = TemplateBuilder::new(self.alloc);
        self.native(&mut builder, el, tag, NodeId::ROOT, namespace == Namespace::Svg);
        let template = builder.finish(namespace, self.alloc);
        self.boxed(template)
    }

    /// Writes `el` into the template and queues its runtime work.
    pub(super) fn native(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        el: &JSXElement<'a>,
        tag: &'a str,
        node: NodeId,
        in_svg: bool,
    ) {
        self.path.push(tag.to_string());
        let is_svg = in_svg || tag == "svg";
        let attrs = &el.opening_element.attributes;
        let mut items = self.items(&el.children, true);
        self.children_attribute(attrs, &mut items);

        builder.html.push('<');
        builder.html.push_str(tag);
        let has_spread = attrs.iter().any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)));
        let deferred = if has_spread {
            self.spread(builder, node, attrs, is_svg, !items.is_empty());
            std::vec::Vec::new()
        } else {
            self.attributes(builder, node, tag, attrs, is_svg)
        };
        builder.html.push('>');
        if !is_void(tag) {
            let children_in_svg = is_svg && tag != "foreignObject";
            self.native_children(builder, node, items, children_in_svg);
            builder.html.push_str("</");
            builder.html.push_str(tag);
            builder.html.push('>');
        }
        builder.ops.extend(deferred);
        self.path.pop();
    }

    /// A `children={…}` attribute stands in for missing nested children (SPEC §7.2).
    fn children_attribute<'b>(
        &mut self,
        attrs: &'b [JSXAttributeItem<'a>],
        items: &mut std::vec::Vec<Item<'b, 'a>>,
    ) {
        for attr in attrs {
            let JSXAttributeItem::Attribute(a) = attr else { continue };
            if attribute_name(self, a) != "children" {
                continue;
            }
            let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value else { continue };
            let Some(e) = c.expression.as_expression() else { continue };
            if items.is_empty() {
                items.push(Item::Expr(e));
            } else {
                self.children_ignored(a.span);
            }
        }
    }

    pub(super) fn children_ignored(&mut self, span: oxc_span::Span) {
        let removal = self.removal(span);
        self.report(
            Report::new(
                Code::ChildrenPropIgnored,
                span,
                "`children` is set both as an attribute and as nested children; nested children win \
                 and the attribute is never rendered. Remove the attribute.",
            )
            .fix("remove the `children` attribute", vec![removal]),
        );
    }
}
