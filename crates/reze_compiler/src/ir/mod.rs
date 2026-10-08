pub mod layout;
pub mod schedule;
pub mod sites;
pub mod view;

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::symbol::SymbolId;

use crate::diagnostic::{Code, Report};
use crate::frontend::analysis::{Intrinsic, SharedFacts};
use crate::frontend::pure::{has_jsx, inline_entries, is_defined, static_property};
use crate::html::*;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

use schedule::{EffectGroup, MemberKind, SchedMember, Schedule, ValueMode};
use sites::{SiteId, SiteRegistry};
use view::*;

pub struct ModuleIr {
    pub views: Vec<View>,
    pub by_span: HashMap<(u32, u32), ViewId>,
    pub module_id: Option<String>,
    pub islands: IslandPlan,
    pub sites: Option<SiteRegistry>,
    pub callback_sites: HashMap<(u32, u32), SiteId>,
    pub reports: Vec<Report>,
    pub has_views: bool,
}

impl ModuleIr {
    pub fn view(&self, id: ViewId) -> &View {
        &self.views[id.0 as usize]
    }
}

pub fn build_module_ir<'x>(
    program: &Program<'_>,
    scoping: &'x Scoping,
    facts: &'x SharedFacts,
    source: &'x str,
    module_id: Option<&str>,
    links: bool,
    pre: SourceSites,
) -> ModuleIr {
    let islands = IslandPlan::scan(program, scoping);
    let mut builder = Builder {
        source,
        scoping,
        facts,
        module_id: module_id.map(str::to_string),
        links,
        islands,
        source_sites: pre.assigned,
        views: Vec::new(),
        by_span: HashMap::new(),
        reports: Vec::new(),
        path: Vec::new(),
        has_views: false,
    };
    builder.visit_program(program);
    let Builder { views, by_span, module_id, islands, source_sites, reports, has_views, .. } =
        builder;
    ModuleIr {
        views,
        by_span,
        module_id,
        islands,
        sites: pre.sites,
        callback_sites: source_sites,
        reports,
        has_views,
    }
}

struct Builder<'x> {
    source: &'x str,
    scoping: &'x Scoping,
    facts: &'x SharedFacts,
    module_id: Option<String>,
    links: bool,
    islands: IslandPlan,
    source_sites: HashMap<(u32, u32), SiteId>,
    views: Vec<View>,
    by_span: HashMap<(u32, u32), ViewId>,
    reports: Vec<Report>,
    path: Vec<String>,
    has_views: bool,
}
impl Builder<'_> {
    fn report(&mut self, mut report: Report) {
        report.path = self.path.clone();
        self.reports.push(report);
    }

    fn text(&self, span: Span) -> &str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn push_view(&mut self, kind: ViewKind, origin: Span) -> ViewId {
        let id = ViewId(self.views.len() as u32);
        let site = self.source_sites.get(&(origin.start, origin.end)).copied();
        self.by_span.insert((origin.start, origin.end), id);
        self.views.push(View { kind, site, origin });
        self.has_views = true;
        id
    }

    fn dep_list(&self, exprs: &[&Expression<'_>]) -> Vec<SymbolId> {
        let mut deps = Vec::new();
        for e in exprs {
            deps.extend(self.facts.stable_getter_deps(self.scoping, e));
        }
        deps.sort();
        deps.dedup();
        deps
    }

    fn dynamic_of(&self, expr: ExprRef, mode: ValueMode) -> Dynamic {
        Dynamic {
            expr,
            mode,
            deps: Vec::new(),
            kind: StaticKind::Unknown,
            getter: None,
            fixed: false,
        }
    }

    fn tracked(&self, e: &Expression<'_>) -> Dynamic {
        let mut dynamic = Dynamic {
            expr: ExprRef::of(e),
            mode: ValueMode::Tracked,
            deps: self.dep_list(&[e]),
            kind: self.fold_kind(e).unwrap_or(StaticKind::Unknown),
            getter: None,
            fixed: self.facts.reads_unconditionally(self.scoping, e),
        };
        if self.fold_boolean(e) {
            dynamic.kind = StaticKind::Boolean;
        }
        dynamic
    }

    fn getter_of(&self, e: &Expression<'_>) -> GetterKind {
        if let Expression::CallExpression(call) = e.without_parentheses()
            && let Expression::Identifier(id) = &call.callee
            && call.arguments.is_empty()
            && !call.optional
            && call.type_arguments.is_none()
            && !self.facts.asyncs.is_read(id)
        {
            return GetterKind::Call(ExprRef::at(id.as_ref(), id.span));
        }
        GetterKind::Thunk { parenthesize: self.source.as_bytes()[e.span().start as usize] == b'{' }
    }

    fn stable_callee(&self, e: &Expression<'_>) -> Option<ExprRef> {
        let Expression::CallExpression(call) = e.without_parentheses() else { return None };
        let Expression::Identifier(id) = &call.callee else { return None };
        let plain = call.arguments.is_empty() && !call.optional && call.type_arguments.is_none();
        (plain
            && self.facts.is_stable_getter(self.scoping, id)
            && self.facts.folded_read(call).is_none())
        .then_some(ExprRef::at(id.as_ref(), id.span))
    }
}

enum Tag {
    Native(String),
    Component(ExprRef),
    Intrinsic(Intrinsic),
}

impl Builder<'_> {
    fn tag_of(&self, name: &JSXElementName<'_>) -> Tag {
        if let Some(intrinsic) = self.facts.intrinsic(self.scoping, name) {
            return Tag::Intrinsic(intrinsic);
        }
        match name {
            JSXElementName::Identifier(id) => {
                if is_native_name(id.name.as_str()) {
                    Tag::Native(id.name.as_str().to_string())
                } else {
                    Tag::Component(ExprRef::at(id.as_ref(), id.span))
                }
            }
            JSXElementName::IdentifierReference(id) => {
                if is_native_name(id.name.as_str()) {
                    Tag::Native(id.name.as_str().to_string())
                } else {
                    Tag::Component(ExprRef::at(id.as_ref(), id.span))
                }
            }
            JSXElementName::NamespacedName(n) => {
                Tag::Native(format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str()))
            }
            JSXElementName::MemberExpression(m) => Tag::Component(ExprRef::at(m.as_ref(), m.span)),
            JSXElementName::ThisExpression(t) => Tag::Component(ExprRef::at(t.as_ref(), t.span)),
        }
    }

    fn is_native_tag(&self, name: &JSXElementName<'_>) -> bool {
        matches!(self.tag_of(name), Tag::Native(_))
    }

    fn build_root(&mut self, el: &JSXElement<'_>) -> ViewId {
        let kind = match self.tag_of(&el.opening_element.name) {
            Tag::Native(tag) => ViewKind::Element(self.build_element(el, &tag)),
            Tag::Component(callee) => ViewKind::Component(self.build_component(el, callee)),
            Tag::Intrinsic(intrinsic) => match self.build_flow(el, intrinsic) {
                Some(flow) => ViewKind::Flow(flow),
                None => ViewKind::Fragment(Vec::new()),
            },
        };
        self.push_view(kind, el.span)
    }

    fn build_fragment_root(&mut self, fragment: &JSXFragment<'_>) -> ViewId {
        let items = self.items(&fragment.children, false);
        let children = self.component_list(items);
        self.push_view(ViewKind::Fragment(children), fragment.span)
    }
}

fn is_native_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-')
}

fn is_function(e: &Expression<'_>) -> bool {
    matches!(
        e.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(c) => c.expression.as_expression().is_some(),
        _ => true,
    }
}

impl<'a> Visit<'a> for Builder<'_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if !self.by_span.contains_key(&(it.span.start, it.span.end)) {
            self.build_root(it);
        }
        NestedExpressions { builder: self }.visit_jsx_element(it);
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        if !self.by_span.contains_key(&(it.span.start, it.span.end)) {
            self.build_fragment_root(it);
        }
        NestedExpressions { builder: self }.visit_jsx_fragment(it);
    }
}

struct NestedExpressions<'b, 'x> {
    builder: &'b mut Builder<'x>,
}

impl<'a> Visit<'a> for NestedExpressions<'_, '_> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        self.builder.visit_expression(expression);
    }
}

enum Item<'b, 'a> {
    Text(String),
    Element(&'b JSXElement<'a>),
    Fragment(&'b JSXFragment<'a>),
    Expr(&'b Expression<'a>),
}

enum Segment<'b, 'a> {
    Item(Item<'b, 'a>),
    TextRun(Vec<Item<'b, 'a>>),
}

impl Builder<'_> {
    fn collect_items<'b, 'x>(
        &mut self,
        children: &'b [JSXChild<'x>],
        is_native: bool,
        items: &mut Vec<Item<'b, 'x>>,
    ) {
        for child in children {
            match child {
                JSXChild::Text(t) => {
                    push_item_text(items, clean_jsx_text(&decode_entities(t.value.as_str())))
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

    fn items<'b, 'x>(
        &mut self,
        children: &'b [JSXChild<'x>],
        is_native: bool,
    ) -> Vec<Item<'b, 'x>> {
        let mut items = Vec::new();
        self.collect_items(children, is_native, &mut items);
        items
    }

    fn expression_item<'b, 'x>(
        &mut self,
        e: &'b Expression<'x>,
        is_native: bool,
        items: &mut Vec<Item<'b, 'x>>,
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
            inner => match self.fold_text(inner) {
                Some(text) if is_native => push_item_text(items, text),
                _ => items.push(Item::Expr(e)),
            },
        }
    }

    fn live_branch<'b, 'x>(&mut self, e: &'b Expression<'x>) -> Option<Option<&'b Expression<'x>>> {
        let inner = e.without_parentheses();
        let (live, dropped) = match inner {
            Expression::BooleanLiteral(_) | Expression::NullLiteral(_) => return Some(None),
            Expression::Identifier(_)
                if matches!(self.fold_literal(inner), Some(Literal::Nullish)) =>
            {
                return Some(None);
            }
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                if self.fold_truthy(&l.left)? {
                    (Some(&l.right), l.left.span())
                } else {
                    (Some(&l.left), l.right.span())
                }
            }
            Expression::ConditionalExpression(c) => {
                if self.fold_truthy(&c.test)? {
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

    fn is_text_expression(&self, e: &Expression<'_>) -> bool {
        self.fold_kind(e).is_some()
    }

    fn segments<'b, 'x>(&self, items: Vec<Item<'b, 'x>>) -> Vec<Segment<'b, 'x>> {
        let mut segments = Vec::new();
        let mut run: Vec<Item<'b, 'x>> = Vec::new();
        let flush = |run: &mut Vec<Item<'b, 'x>>, segments: &mut Vec<Segment<'b, 'x>>| {
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
}

fn push_item_text<'b, 'x>(items: &mut Vec<Item<'b, 'x>>, text: String) {
    if text.is_empty() {
        return;
    }
    if let Some(Item::Text(previous)) = items.last_mut() {
        previous.push_str(&text);
    } else {
        items.push(Item::Text(text));
    }
}

fn attr_name(a: &JSXAttribute<'_>) -> String {
    match &a.name {
        JSXAttributeName::Identifier(id) => id.name.as_str().to_string(),
        JSXAttributeName::NamespacedName(n) => {
            format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
        }
    }
}

struct ElementState {
    view: ElementView,
    binds: Vec<(usize, Vec<SymbolId>, bool)>,
}

impl Builder<'_> {
    fn new_element(&self, namespace: Namespace, tag: &str) -> ElementState {
        ElementState {
            view: ElementView {
                namespace,
                tag: tag.to_string(),
                statics: StaticTree {
                    nodes: vec![StaticNode {
                        parent: None,
                        children: Vec::new(),
                        kind: StaticNodeKind::Element,
                        referenced: false,
                        ns: namespace,
                        tag: tag.to_string(),
                        text: String::new(),
                        attrs: Vec::new(),
                    }],
                },
                props: Vec::new(),
                inserts: Vec::new(),
                links: Vec::new(),
                late_values: Vec::new(),
                schedule: Schedule::empty(),
            },
            binds: Vec::new(),
        }
    }

    fn static_node(
        &mut self,
        state: &mut ElementState,
        parent: u32,
        kind: StaticNodeKind,
        ns: Namespace,
    ) -> u32 {
        let id = state.view.statics.nodes.len() as u32;
        state.view.statics.nodes.push(StaticNode {
            parent: Some(parent),
            children: Vec::new(),
            kind,
            referenced: false,
            ns,
            tag: String::new(),
            text: String::new(),
            attrs: Vec::new(),
        });
        state.view.statics.nodes[parent as usize].children.push(id);
        id
    }

    fn parent_ns(&self, state: &ElementState, parent: u32) -> Namespace {
        state.view.statics.nodes[parent as usize].ns
    }

    fn child_ns(parent: &StaticNode, tag: &str) -> Namespace {
        let html_context = match parent.ns {
            Namespace::Html => true,
            Namespace::Svg => matches!(parent.tag.as_str(), "foreignObject" | "desc" | "title"),
            Namespace::MathMl => {
                (matches!(parent.tag.as_str(), "mi" | "mo" | "mn" | "ms" | "mtext")
                    && !matches!(tag, "mglyph" | "malignmark"))
                    || (parent.tag == "annotation-xml"
                        && (tag == "svg"
                            || parent.attrs.iter().any(|attr| {
                                attr.name.eq_ignore_ascii_case("encoding")
                                    && attr.value.as_deref().is_some_and(|value| {
                                        value.eq_ignore_ascii_case("text/html")
                                            || value.eq_ignore_ascii_case("application/xhtml+xml")
                                    })
                            })))
            }
        };
        if html_context {
            return match tag {
                "svg" => Namespace::Svg,
                "math" => Namespace::MathMl,
                _ => Namespace::Html,
            };
        }
        let breaks_out = matches!(
            tag,
            "b" | "big"
                | "blockquote"
                | "body"
                | "br"
                | "center"
                | "code"
                | "dd"
                | "div"
                | "dl"
                | "dt"
                | "em"
                | "embed"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "head"
                | "hr"
                | "i"
                | "img"
                | "li"
                | "listing"
                | "menu"
                | "meta"
                | "nobr"
                | "ol"
                | "p"
                | "pre"
                | "ruby"
                | "s"
                | "small"
                | "span"
                | "strong"
                | "strike"
                | "sub"
                | "sup"
                | "table"
                | "tt"
                | "u"
                | "ul"
                | "var"
        );
        if breaks_out { Namespace::Html } else { parent.ns }
    }

    fn reference(&mut self, state: &mut ElementState, node: u32) {
        state.view.statics.nodes[node as usize].referenced = true;
    }

    fn removal(&self, span: Span) -> crate::diagnostic::Edit {
        let start = self.source[..span.start as usize].trim_end().len() as u32;
        crate::diagnostic::Edit { start, end: span.end, text: String::new() }
    }

    fn build_element(&mut self, el: &JSXElement<'_>, tag: &str) -> ElementView {
        for item in &el.opening_element.attributes {
            if let JSXAttributeItem::Attribute(a) = item
                && let JSXAttributeName::Identifier(id) = &a.name
                && is_island_attr(id.name.as_str())
            {
                self.report(Report::new(Code::IslandOnElement, a.span));
            }
        }
        let namespace = if tag == "svg" || is_svg_element(tag) {
            Namespace::Svg
        } else if is_mathml_root(tag) {
            Namespace::MathMl
        } else {
            Namespace::Html
        };
        self.path.push(tag.to_string());
        let mut state = self.new_element(namespace, tag);
        self.build_native(&mut state, 0, el, tag);
        self.path.pop();
        self.finish_schedule(&mut state);
        state.view
    }

    fn build_native(
        &mut self,
        state: &mut ElementState,
        node: u32,
        el: &JSXElement<'_>,
        tag: &str,
    ) {
        let is_svg = state.view.statics.nodes[node as usize].ns != Namespace::Html;
        let mut items = self.items(&el.children, true);
        self.children_attribute(&el.opening_element.attributes, &mut items);
        let has_spread = el
            .opening_element
            .attributes
            .iter()
            .any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)));
        if has_spread {
            self.build_spread(state, node, el, is_svg, !items.is_empty());
        } else {
            self.build_attributes(state, node, el, tag);
        }
        if is_svg || !is_void(tag) {
            self.native_children(state, node, items);
        }
    }

    fn children_attribute<'b, 'x>(
        &mut self,
        attrs: &'b [JSXAttributeItem<'x>],
        items: &mut Vec<Item<'b, 'x>>,
    ) {
        for attr in attrs {
            let JSXAttributeItem::Attribute(a) = attr else { continue };
            if attr_name(a) != "children" {
                continue;
            }
            let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value else { continue };
            let Some(e) = c.expression.as_expression() else { continue };
            if items.is_empty() {
                items.push(Item::Expr(e));
            } else {
                self.report(
                    Report::new(Code::ChildrenPropIgnored, a.span).fix(vec![self.removal(a.span)]),
                );
            }
        }
    }

    fn native_children(&mut self, state: &mut ElementState, parent: u32, items: Vec<Item<'_, '_>>) {
        let base = state.view.inserts.len();
        let mut pending = Vec::new();
        let segments = self.segments(items);
        let sole = segments.len() == 1;
        let mut last_is_text = false;
        let mut after_dynamic = false;
        let mut static_count = 0u32;
        for segment in segments {
            let is_text = matches!(segment, Segment::TextRun(_) | Segment::Item(Item::Text(_)));
            if is_text && after_dynamic && last_is_text {
                let ns = self.parent_ns(state, parent);
                self.static_node(state, parent, StaticNodeKind::Marker, ns);
                static_count += 1;
            }
            let value = match segment {
                Segment::TextRun(parts) => {
                    self.text_run(state, parent, parts);
                    None
                }
                Segment::Item(Item::Text(text)) => {
                    let ns = self.parent_ns(state, parent);
                    let node = self.static_node(state, parent, StaticNodeKind::Text, ns);
                    state.view.statics.nodes[node as usize].text = text;
                    None
                }
                Segment::Item(Item::Element(el)) => match self.tag_of(&el.opening_element.name) {
                    Tag::Native(tag) => {
                        let ns = Self::child_ns(&state.view.statics.nodes[parent as usize], &tag);
                        let node = self.static_node(state, parent, StaticNodeKind::Element, ns);
                        state.view.statics.nodes[node as usize].tag = tag.clone();
                        self.build_native(state, node, el, &tag);
                        None
                    }
                    Tag::Component(callee) => {
                        let child = self.build_component(el, callee);
                        let id = self.push_view(ViewKind::Component(child), el.span);
                        Some(Child::View(id))
                    }
                    Tag::Intrinsic(intrinsic) => Some(self.native_flow(el, intrinsic)),
                },
                Segment::Item(Item::Fragment(_)) => None,
                Segment::Item(Item::Expr(e)) => Some(self.insert_value(e)),
            };
            match value {
                None => {
                    static_count += 1;
                    last_is_text = is_text;
                    after_dynamic = false;
                }
                Some(value) => {
                    pending.push((state.view.inserts.len(), static_count));
                    state.view.inserts.push(InsertOp {
                        slot: state.view.inserts.len() as u32,
                        parent,
                        value,
                        anchor: Anchor::End,
                    });
                    let index = state.view.inserts.len() - 1;
                    state
                        .view
                        .schedule
                        .children
                        .push(SchedMember { kind: MemberKind::Insert, index });
                    after_dynamic = true;
                }
            }
        }
        let statics = state.view.statics.nodes[parent as usize].children.clone();
        for (op, next_static) in pending {
            let resolved = if sole {
                Anchor::Only
            } else if let Some(&node) = statics.get(next_static as usize) {
                self.reference(state, node);
                Anchor::Before(node)
            } else {
                Anchor::End
            };
            state.view.inserts[op].anchor = resolved;
        }
        if state.view.inserts.len() > base {
            self.reference(state, parent);
        }
    }

    fn text_run(&mut self, state: &mut ElementState, parent: u32, parts: Vec<Item<'_, '_>>) {
        let ns = self.parent_ns(state, parent);
        let node = self.static_node(state, parent, StaticNodeKind::Text, ns);
        state.view.statics.nodes[node as usize].text = " ".to_string();
        let mut lowered = Vec::new();
        for part in parts {
            match part {
                Item::Text(text) => lowered.push(TextPart::Static(text)),
                Item::Expr(e) => {
                    lowered.push(TextPart::Dynamic(self.tracked(e)));
                }
                Item::Element(_) | Item::Fragment(_) => {}
            }
        }
        self.reference(state, node);
        state.view.props.push(ElementProp::Attr(Attr {
            node,
            target: AttrTarget::Text,
            value: AttrValue::TextParts(lowered),
        }));
    }

    fn insert_value(&mut self, e: &Expression<'_>) -> Child {
        if let Some(branch) = self.conditional(e) {
            let id = self.push_view(ViewKind::Flow(FlowView::Show(branch)), e.span());
            return Child::View(id);
        }
        if self.fold_dynamic(e, false) {
            let mut dynamic = self.tracked(e);
            dynamic.getter = Some(self.getter_of(e));
            Child::Dynamic(dynamic)
        } else {
            let mut dynamic = self.dynamic_of(ExprRef::of(e), ValueMode::Once);
            dynamic.kind = self.fold_kind(e).unwrap_or(StaticKind::Unknown);
            Child::Dynamic(dynamic)
        }
    }

    fn conditional(&mut self, e: &Expression<'_>) -> Option<Branch> {
        let (test, consequent, alternate) = match e.without_parentheses() {
            Expression::ConditionalExpression(c) => (&c.test, &c.consequent, Some(&c.alternate)),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                (&l.left, &l.right, None)
            }
            _ => return None,
        };
        let has_jsx_branch = has_jsx(|check| check.visit_expression(consequent))
            || alternate.is_some_and(|a| has_jsx(|check| check.visit_expression(a)));
        if !has_jsx_branch || self.fold_truthy(test).is_some() {
            return None;
        }
        let when = FlowSource { expr: ExprRef::of(test), getter: self.stable_callee(test) };
        let child = self.render_branch(consequent);
        let fallback = alternate.map(|a| self.render_branch(a));
        Some(Branch { when, child, fallback })
    }

    fn render_branch(&mut self, e: &Expression<'_>) -> FlowRender {
        let child = match e.without_parentheses() {
            Expression::JSXElement(el) => {
                let id = self.build_root(el);
                Child::View(id)
            }
            Expression::JSXFragment(f) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                Child::View(id)
            }
            _ => match self.conditional(e) {
                Some(branch) => {
                    let id = self.push_view(ViewKind::Flow(FlowView::Show(branch)), e.span());
                    Child::View(id)
                }
                None if self.fold_dynamic(e, false) => {
                    let mut dynamic = self.tracked(e);
                    dynamic.getter = Some(self.getter_of(e));
                    Child::Dynamic(dynamic)
                }
                None if self.source.as_bytes()[e.span().start as usize] == b'{' => {
                    let mut dynamic = self.tracked(e);
                    dynamic.getter = Some(self.getter_of(e));
                    Child::Dynamic(dynamic)
                }
                None => {
                    let mut dynamic = self.dynamic_of(ExprRef::of(e), ValueMode::Once);
                    dynamic.kind = StaticKind::Unknown;
                    Child::Dynamic(dynamic)
                }
            },
        };
        self.wrap_render(child)
    }

    fn wrap_render(&mut self, child: Child) -> FlowRender {
        FlowRender::Child(Box::new(child))
    }

    fn fragment_children(&mut self, children: &[JSXChild<'_>]) -> Vec<Child> {
        let items = self.items(children, false);
        self.component_list(items)
    }

    fn component_list(&mut self, items: Vec<Item<'_, '_>>) -> Vec<Child> {
        let mut list = Vec::new();
        for item in items {
            list.push(match item {
                Item::Text(text) => Child::StaticText(text),
                Item::Element(el) => {
                    let id = self.build_root(el);
                    Child::View(id)
                }
                Item::Fragment(f) => {
                    let children = self.fragment_children(&f.children);
                    let id = self.push_view(ViewKind::Fragment(children), f.span);
                    Child::View(id)
                }
                Item::Expr(e) => self.list_entry(e),
            });
        }
        list
    }

    fn list_entry(&mut self, e: &Expression<'_>) -> Child {
        if let Some(branch) = self.conditional(e) {
            let id = self.push_view(ViewKind::Flow(FlowView::Show(branch)), e.span());
            return Child::View(id);
        }
        if !is_function(e) && self.fold_dynamic(e, false) {
            let mut dynamic = self.tracked(e);
            dynamic.getter = Some(self.getter_of(e));
            Child::Dynamic(dynamic)
        } else {
            let mut dynamic = self.dynamic_of(ExprRef::of(e), ValueMode::Once);
            dynamic.kind = StaticKind::Unknown;
            Child::Dynamic(dynamic)
        }
    }
}

enum AttrKind {
    Attr(String),
    AttrNs(&'static str, String),
    Bool(String),
    Prop(String),
    InlineProp(String),
    LateProp(String),
    Style,
}

enum NativeValue<'b, 'x> {
    Bare,
    Str(String),
    Expr(&'b Expression<'x>),
    View(ViewId),
}

impl Builder<'_> {
    fn build_attributes(
        &mut self,
        state: &mut ElementState,
        node: u32,
        el: &JSXElement<'_>,
        tag: &str,
    ) {
        let attrs: Vec<&JSXAttribute<'_>> = el
            .opening_element
            .attributes
            .iter()
            .filter_map(|item| match item {
                JSXAttributeItem::Attribute(a) => Some(&**a),
                JSXAttributeItem::SpreadAttribute(_) => None,
            })
            .collect();
        let names: Vec<String> = attrs.iter().map(|a| attr_name(a)).collect();
        let is_overridden = self.duplicates(&attrs, &names);
        let class_sources: Vec<&JSXAttribute<'_>> = attrs
            .iter()
            .zip(&names)
            .enumerate()
            .filter(|(i, (_, name))| !is_overridden[*i] && *name == "class")
            .map(|(_, (a, _))| *a)
            .collect();
        let link = self.claimed_href(tag, &attrs, &names, &is_overridden);
        let mut link_href = None;
        let mut is_class_done = false;
        for (i, (a, name)) in attrs.iter().zip(&names).enumerate() {
            if is_overridden[i] {
                continue;
            }
            if *name == "class" {
                if !is_class_done {
                    self.build_class(state, node, &class_sources);
                    is_class_done = true;
                }
                continue;
            }
            if link == Some(i)
                && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                && let Some(e) = c.expression.as_expression()
                && self.fold_dynamic(e, false)
            {
                let mut dynamic = self.tracked(e);
                dynamic.getter = Some(self.getter_of(e));
                link_href = Some(dynamic);
                continue;
            }
            self.build_attribute(state, node, tag, a, name);
        }
        if link.is_some() {
            self.reference(state, node);
            state.view.links.push(LinkProp {
                node,
                after_prop: state.view.props.len(),
                href: link_href,
            });
        }
    }

    fn duplicates(&mut self, attrs: &[&JSXAttribute<'_>], names: &[String]) -> Vec<bool> {
        let mut is_overridden = vec![false; attrs.len()];
        let mut last: HashMap<&str, usize> = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            if let Some(previous) = last.insert(name.as_str(), i) {
                is_overridden[previous] = true;
                self.report(
                    Report::new(Code::DuplicateAttribute, attrs[i].span)
                        .arg("attribute", name.clone())
                        .label(attrs[previous].span, "overridden here")
                        .fix(vec![self.removal(attrs[previous].span)]),
                );
            }
        }
        is_overridden
    }

    fn claimed_href(
        &self,
        tag: &str,
        attrs: &[&JSXAttribute<'_>],
        names: &[String],
        is_overridden: &[bool],
    ) -> Option<usize> {
        if !self.links || tag != "a" {
            return None;
        }
        if names.iter().any(|name| {
            matches!(name.as_str(), "aria-current" | "attr:aria-current" | "prop:ariaCurrent")
        }) {
            return None;
        }
        let i = (0..attrs.len()).find(|&i| names[i] == "href" && !is_overridden[i])?;
        let is_routable = match &attrs[i].value {
            Some(JSXAttributeValue::StringLiteral(s)) => {
                is_routable_href(&decode_entities(s.value.as_str()))
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                match self.fold_literal(c.expression.as_expression()?) {
                    Some(Literal::Str(s)) => is_routable_href(&s),
                    Some(Literal::Bool(_) | Literal::Nullish) => false,
                    None => true,
                }
            }
            None | Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => false,
        };
        is_routable.then_some(i)
    }

    fn build_class(&mut self, state: &mut ElementState, node: u32, sources: &[&JSXAttribute<'_>]) {
        let mut values = Vec::new();
        for a in sources {
            match self.native_value(a) {
                Some(NativeValue::Expr(e)) => {
                    values.push(NativeValue::Expr(e));
                }
                Some(value @ NativeValue::Str(_)) => values.push(value),
                Some(NativeValue::Bare | NativeValue::View(_)) | None => {}
            }
        }
        if values.is_empty() {
            return;
        }
        let mut keys = ClassKeys::default();
        let is_static = values.iter().all(|value| match value {
            NativeValue::Str(s) => {
                keys.set(s, !s.is_empty());
                true
            }
            NativeValue::Expr(e) => self.class_add(&mut keys, e).is_some(),
            NativeValue::Bare | NativeValue::View(_) => true,
        });
        if is_static {
            let class = keys.attribute();
            if !class.is_empty() {
                state.view.statics.nodes[node as usize]
                    .attrs
                    .push(StaticAttr { name: "class".to_string(), value: Some(class.clone()) });
                state.view.props.push(ElementProp::Attr(Attr {
                    node,
                    target: AttrTarget::Attr("class".to_string()),
                    value: AttrValue::Str(class),
                }));
            }
            return;
        }
        if let Some(toggles) = self.class_toggles(&values) {
            if !toggles.0.is_empty() {
                let class = toggles.0.join(" ");
                state.view.statics.nodes[node as usize]
                    .attrs
                    .push(StaticAttr { name: "class".to_string(), value: Some(class.clone()) });
            }
            self.reference(state, node);
            for (token, value) in toggles.1 {
                let toggle = if self.fold_boolean(value) {
                    AttrValue::Expr(ExprRef::of(value))
                } else {
                    AttrValue::Truthy(ExprRef::of(value))
                };
                if self.fold_dynamic(value, false) {
                    let deps = self.dep_list(&[value]);
                    let fixed = self.facts.reads_unconditionally(self.scoping, value);
                    state.view.props.push(ElementProp::Attr(Attr {
                        node,
                        target: AttrTarget::ClassToggle(token),
                        value: toggle,
                    }));
                    let index = state.view.props.len() - 1;
                    state.binds.push((index, deps, fixed));
                } else {
                    state.view.props.push(ElementProp::Attr(Attr {
                        node,
                        target: AttrTarget::ClassToggle(token),
                        value: toggle,
                    }));
                }
            }
            return;
        }
        let is_string = matches!(
            values.as_slice(),
            [NativeValue::Expr(only)]
                if self.fold_kind(only) == Some(StaticKind::String)
        );
        let mut parts = Vec::new();
        for value in values {
            match value {
                NativeValue::Str(s) => parts.push(AttrValue::Str(s)),
                NativeValue::Expr(e) => {
                    if self.fold_dynamic(e, false) {
                        parts.push(AttrValue::Dynamic(self.tracked(e)));
                    } else {
                        parts.push(AttrValue::Expr(ExprRef::of(e)));
                    }
                }
                NativeValue::Bare | NativeValue::View(_) => {}
            }
        }
        let target =
            if is_string { AttrTarget::Attr("class".to_string()) } else { AttrTarget::Class };
        let value = if parts.len() == 1 {
            parts.pop().expect("one part")
        } else {
            AttrValue::ClassParts(parts)
        };
        self.reference(state, node);
        state.view.props.push(ElementProp::Attr(Attr { node, target, value }));
    }
}

impl Builder<'_> {
    fn native_value<'b, 'x>(&mut self, a: &'b JSXAttribute<'x>) -> Option<NativeValue<'b, 'x>> {
        Some(match &a.value {
            None => NativeValue::Bare,
            Some(JSXAttributeValue::StringLiteral(s)) => {
                NativeValue::Str(decode_entities(s.value.as_str()).into_owned())
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                NativeValue::Expr(c.expression.as_expression()?)
            }
            Some(JSXAttributeValue::Element(e)) => {
                let id = self.build_root(e);
                NativeValue::View(id)
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                NativeValue::View(id)
            }
        })
    }

    fn class_toggles<'b, 'x>(
        &self,
        values: &[NativeValue<'b, 'x>],
    ) -> Option<(Vec<String>, Vec<(String, &'b Expression<'x>)>)> {
        let mut static_tokens: Vec<String> = Vec::new();
        let mut off_tokens: Vec<String> = Vec::new();
        let mut toggles: Vec<(String, &'b Expression<'x>)> = Vec::new();
        for value in values {
            match value {
                NativeValue::Str(s) => {
                    for token in s.split_whitespace() {
                        if !static_tokens.iter().any(|t| t == token) {
                            static_tokens.push(token.to_string());
                        }
                    }
                }
                NativeValue::Expr(e) => {
                    self.class_pieces(e, &mut static_tokens, &mut off_tokens, &mut toggles)?;
                }
                NativeValue::Bare | NativeValue::View(_) => {}
            }
        }
        if toggles.is_empty() {
            return None;
        }
        let is_toggled = |token: &str| toggles.iter().any(|(t, _)| t == token);
        let has_conflict =
            static_tokens.iter().any(|t| off_tokens.iter().any(|o| o == t) || is_toggled(t))
                || off_tokens.iter().any(|t| is_toggled(t));
        if has_conflict {
            return None;
        }
        Some((static_tokens, toggles))
    }

    fn class_pieces<'b, 'x>(
        &self,
        e: &'b Expression<'x>,
        static_tokens: &mut Vec<String>,
        off_tokens: &mut Vec<String>,
        toggles: &mut Vec<(String, &'b Expression<'x>)>,
    ) -> Option<()> {
        match e.without_parentheses() {
            Expression::StringLiteral(s) => {
                for token in s.value.as_str().split_whitespace() {
                    if !static_tokens.iter().any(|t| t == token) {
                        static_tokens.push(token.to_string());
                    }
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    let (key, value) = static_property(property)?;
                    match self.fold_truthy(value) {
                        Some(true) => {
                            for token in key.split_whitespace() {
                                if !static_tokens.iter().any(|t| t == token) {
                                    static_tokens.push(token.to_string());
                                }
                            }
                        }
                        Some(false) => {
                            off_tokens.extend(key.split_whitespace().map(str::to_string));
                        }
                        None if key.split_whitespace().count() == 1 && key.trim() == key => {
                            if toggles.iter().any(|(t, _)| t == key) {
                                return None;
                            }
                            toggles.push((key.to_string(), value));
                        }
                        None => return None,
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    self.class_pieces(
                        element.as_expression()?,
                        static_tokens,
                        off_tokens,
                        toggles,
                    )?;
                }
            }
            _ => return None,
        }
        Some(())
    }
}

impl Builder<'_> {
    fn attr_kind(
        &mut self,
        a: &JSXAttribute<'_>,
        name: &str,
        tag: &str,
        namespace: Namespace,
    ) -> AttrKind {
        if name == "style" {
            return AttrKind::Style;
        }
        if let Some(n) = name.strip_prefix("prop:") {
            return AttrKind::Prop(n.to_string());
        }
        if let Some(n) = name.strip_prefix("attr:") {
            return AttrKind::Attr(n.to_string());
        }
        if let Some(n) = name.strip_prefix("bool:") {
            return AttrKind::Bool(n.to_string());
        }
        if let Some((prefix, _)) = name.split_once(':')
            && let Some(ns) = crate::html::attribute_namespace(prefix)
        {
            return AttrKind::AttrNs(ns, name.to_string());
        }
        if namespace == Namespace::Html && crate::html::is_property(name) {
            return match name {
                "value" if tag == "textarea" || tag == "select" => {
                    AttrKind::LateProp(name.to_string())
                }
                "value" | "checked" | "selected" => AttrKind::InlineProp(name.to_string()),
                _ => AttrKind::Prop(name.to_string()),
            };
        }
        if name == "key" {
            self.report(Report::new(Code::KeyOnElement, a.span).fix(vec![self.removal(a.span)]));
        } else if namespace != Namespace::MathMl
            && !name.contains(['-', ':'])
            && let Some(suggestion) = crate::html::suggest_attribute(name)
        {
            let name_span = a.name.span();
            self.report(
                Report::new(Code::UnknownAttribute, name_span)
                    .arg("attribute", name.to_string())
                    .arg("suggestion", suggestion.to_string())
                    .fix(vec![crate::diagnostic::Edit {
                        start: name_span.start,
                        end: name_span.end,
                        text: suggestion.to_string(),
                    }]),
            );
        }
        AttrKind::Attr(name.to_string())
    }

    fn build_attribute(
        &mut self,
        state: &mut ElementState,
        node: u32,
        tag: &str,
        a: &JSXAttribute<'_>,
        name: &str,
    ) {
        if name == "children" {
            return;
        }
        let Some(value) = self.native_value(a) else { return };
        if name == "ref" {
            if let NativeValue::Expr(e) = value {
                let target = self.ref_target(e);
                self.reference(state, node);
                state.view.props.push(ElementProp::Ref(RefTarget { node, target }));
            }
            return;
        }
        if let Some(event) = name.strip_prefix("on:") {
            self.build_event(state, node, event.to_string(), false, value);
            return;
        }
        if name.len() > 2 && name.starts_with("on") && !name.contains(':') {
            let third = name.as_bytes()[2];
            if third.is_ascii_uppercase() {
                let event = crate::html::event_name(&name[2..]);
                self.build_event(state, node, event, true, value);
                return;
            }
            if third.is_ascii_lowercase() && matches!(value, NativeValue::Expr(_)) {
                self.event_lowercase(a, name);
                let event = crate::html::event_name(&name[2..]);
                self.build_event(state, node, event, true, value);
                return;
            }
        }
        let kind = self.attr_kind(a, name, tag, state.view.statics.nodes[node as usize].ns);
        if let (AttrKind::Style, NativeValue::Expr(e)) = (&kind, &value)
            && let Some(style) = self.fold_style(e)
        {
            state.view.statics.nodes[node as usize]
                .attrs
                .push(StaticAttr { name: "style".to_string(), value: Some(style.clone()) });
            state.view.props.push(ElementProp::Attr(Attr {
                node,
                target: AttrTarget::Style,
                value: AttrValue::Str(style),
            }));
            return;
        }
        if let Some(statik) = self.inline_static(&kind, &value) {
            match statik {
                (html_name, literal) => {
                    match literal {
                        InlineLiteral::Absent => {}
                        InlineLiteral::Bare => {
                            state.view.statics.nodes[node as usize]
                                .attrs
                                .push(StaticAttr { name: html_name.clone(), value: None });
                            state.view.props.push(ElementProp::Attr(Attr {
                                node,
                                target: AttrTarget::Attr(html_name),
                                value: AttrValue::True,
                            }));
                        }
                        InlineLiteral::Str(s) => {
                            state.view.statics.nodes[node as usize].attrs.push(StaticAttr {
                                name: html_name.clone(),
                                value: Some(s.clone()),
                            });
                            state.view.props.push(ElementProp::Attr(Attr {
                                node,
                                target: AttrTarget::Attr(html_name),
                                value: AttrValue::Str(s),
                            }));
                        }
                    }
                    return;
                }
            }
        }
        let target = match &kind {
            AttrKind::Attr(n) => AttrTarget::Attr(n.clone()),
            AttrKind::AttrNs(ns, n) => AttrTarget::AttrNs(ns, n.clone()),
            AttrKind::Bool(n) => AttrTarget::Bool(n.clone()),
            AttrKind::Prop(name) | AttrKind::InlineProp(name) => AttrTarget::Prop(name.clone()),
            AttrKind::LateProp(name) => AttrTarget::Prop(name.clone()),
            AttrKind::Style => AttrTarget::Style,
        };
        self.reference(state, node);
        let value = match value {
            NativeValue::Expr(e) if self.fold_dynamic(e, false) => {
                AttrValue::Dynamic(self.tracked(e))
            }
            NativeValue::Bare => AttrValue::True,
            NativeValue::Str(value) => AttrValue::Str(value),
            NativeValue::Expr(expr) => AttrValue::Expr(ExprRef::of(expr)),
            NativeValue::View(view) => AttrValue::View(view),
        };
        if matches!(kind, AttrKind::LateProp(_)) {
            state.view.late_values.push(LateProp { node, value });
        } else {
            state.view.props.push(ElementProp::Attr(Attr { node, target, value }));
        }
    }

    fn inline_static(
        &self,
        kind: &AttrKind,
        value: &NativeValue<'_, '_>,
    ) -> Option<(String, InlineLiteral)> {
        let html_name = match kind {
            AttrKind::Attr(n)
            | AttrKind::AttrNs(_, n)
            | AttrKind::InlineProp(n)
            | AttrKind::Bool(n) => n.clone(),
            AttrKind::Style => "style".to_string(),
            AttrKind::Prop(_) | AttrKind::LateProp(_) => return None,
        };
        let literal = match value {
            NativeValue::Bare => Literal::Bool(true),
            NativeValue::Str(s) => Literal::Str(s.clone()),
            NativeValue::Expr(e) if matches!(kind, AttrKind::Bool(_) | AttrKind::InlineProp(_)) => {
                match self.fold_truthy(e) {
                    Some(truthy) => Literal::Bool(truthy),
                    None => return None,
                }
            }
            NativeValue::Expr(e) => match self.fold_literal(e) {
                Some(literal) => literal,
                None => return None,
            },
            NativeValue::View(_) => return None,
        };
        let is_bool = matches!(kind, AttrKind::Bool(_));
        let is_bare_value = matches!(value, NativeValue::Bare);
        match literal {
            Literal::Bool(true)
                if is_bare_value || is_bool || matches!(kind, AttrKind::InlineProp(_)) =>
            {
                Some((html_name, InlineLiteral::Bare))
            }
            Literal::Bool(true) => Some((html_name, InlineLiteral::Str("true".to_string()))),
            Literal::Str(s) if is_bool => {
                if s.is_empty() {
                    Some((html_name, InlineLiteral::Absent))
                } else {
                    Some((html_name, InlineLiteral::Bare))
                }
            }
            Literal::Str(s) => Some((html_name, InlineLiteral::Str(s))),
            Literal::Bool(false) | Literal::Nullish => Some((html_name, InlineLiteral::Absent)),
        }
    }
}

enum InlineLiteral {
    Absent,
    Bare,
    Str(String),
}

fn is_routable_href(href: &str) -> bool {
    let Some(first) = href.bytes().next() else { return false };
    if first == b'?' || (first == b'#' && !href.starts_with("#/")) || href.starts_with("//") {
        return false;
    }
    let scheme_end =
        href.bytes().position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-')));
    !(first.is_ascii_alphabetic() && scheme_end.is_some_and(|end| href.as_bytes()[end] == b':'))
}

impl Builder<'_> {
    fn event_lowercase(&mut self, a: &JSXAttribute<'_>, name: &str) {
        let name_span = a.name.span();
        let mut camel = String::from("on");
        let rest = &name[2..];
        camel.push(rest.as_bytes()[0].to_ascii_uppercase() as char);
        camel.push_str(&rest[1..]);
        self.report(
            Report::new(Code::EventNameLowercase, name_span)
                .arg("attribute", name.to_string())
                .arg("suggestion", camel.clone())
                .fix(vec![crate::diagnostic::Edit {
                    start: name_span.start,
                    end: name_span.end,
                    text: camel,
                }]),
        );
    }

    fn build_event(
        &mut self,
        state: &mut ElementState,
        node: u32,
        event: String,
        is_delegatable: bool,
        value: NativeValue<'_, '_>,
    ) {
        let handler = match value {
            NativeValue::Expr(e) => e,
            NativeValue::Str(s) => {
                state.view.statics.nodes[node as usize]
                    .attrs
                    .push(StaticAttr { name: format!("on{event}"), value: Some(s) });
                return;
            }
            NativeValue::Bare | NativeValue::View(_) => return,
        };
        let kind = if !(is_delegatable && crate::html::is_delegated_event(&event)) {
            EventKind::Direct
        } else if is_function(handler) {
            EventKind::DelegatedStatic
        } else if let Expression::ArrayExpression(array) = handler.without_parentheses()
            && array.elements.len() == 2
            && let (Some(_), Some(_)) =
                (array.elements[0].as_expression(), array.elements[1].as_expression())
        {
            EventKind::DelegatedStatic
        } else {
            EventKind::DelegatedDynamic
        };
        let data = match handler.without_parentheses() {
            Expression::ArrayExpression(array)
                if kind == EventKind::DelegatedStatic && array.elements.len() == 2 =>
            {
                array.elements[1].as_expression().map(ExprRef::of)
            }
            _ => None,
        };
        let handler = match kind {
            EventKind::DelegatedStatic
                if data.is_some()
                    && let Expression::ArrayExpression(array) = handler.without_parentheses()
                    && let Some(f) = array.elements[0].as_expression() =>
            {
                ExprRef::of(f)
            }
            _ => ExprRef::of(handler),
        };
        self.reference(state, node);
        state.view.props.push(ElementProp::Event(EventHandler {
            node,
            name: event,
            kind,
            handler,
            data,
        }));
    }

    fn ref_target(&self, e: &Expression<'_>) -> RefOp {
        if is_function(e) {
            return RefOp::Callback(ExprRef::of(e));
        }
        match self.assign_target(e) {
            Some(target) => RefOp::Assign(target),
            None => RefOp::Expr(ExprRef::of(e)),
        }
    }

    fn assign_target(&self, e: &Expression<'_>) -> Option<AssignTarget> {
        Some(match e.without_parentheses() {
            Expression::Identifier(id) if !self.facts.asyncs.is_read(id) => {
                AssignTarget::Identifier(id.name.as_str().to_string())
            }
            Expression::StaticMemberExpression(m) if !m.optional => AssignTarget::Member {
                object: ExprRef::of(&m.object),
                key: MemberKey::Static(m.property.name.as_str().to_string()),
            },
            Expression::ComputedMemberExpression(m) if !m.optional => AssignTarget::Member {
                object: ExprRef::of(&m.object),
                key: MemberKey::Computed(ExprRef::of(&m.expression)),
            },
            Expression::PrivateFieldExpression(m) if !m.optional => AssignTarget::Member {
                object: ExprRef::of(&m.object),
                key: MemberKey::Static(format!("#{}", m.field.name.as_str())),
            },
            _ => return None,
        })
    }
}

fn close_spread(parts: &mut [SpreadPart], is_svg: bool) -> bool {
    let [SpreadPart::Entries(entries)] = parts else { return false };
    let mut keys = std::collections::HashSet::with_capacity(entries.len());
    let safe = entries.iter().all(|attr| {
        let AttrTarget::Attr(key) = &attr.target else { return false };
        keys.insert(key.as_str())
            && !key.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && !key.starts_with("on")
            && !matches!(
                key.as_str(),
                "children"
                    | "ref"
                    | "__proto__"
                    | "constructor"
                    | "toString"
                    | "toLocaleString"
                    | "valueOf"
                    | "hasOwnProperty"
                    | "isPrototypeOf"
                    | "propertyIsEnumerable"
                    | "__defineGetter__"
                    | "__defineSetter__"
                    | "__lookupGetter__"
                    | "__lookupSetter__"
            )
    });
    drop(keys);
    if !safe {
        return false;
    }
    for attr in entries {
        let AttrTarget::Attr(mut key) = std::mem::replace(&mut attr.target, AttrTarget::Text)
        else {
            unreachable!("closed spread keys were checked");
        };
        attr.target = if key == "style" {
            AttrTarget::Style
        } else if key == "class" {
            AttrTarget::Class
        } else if key.starts_with("prop:") {
            key.drain(..5);
            AttrTarget::Prop(key)
        } else if key.starts_with("attr:") {
            key.drain(..5);
            AttrTarget::Attr(key)
        } else if key.starts_with("bool:") {
            key.drain(..5);
            AttrTarget::Bool(key)
        } else if !is_svg && crate::html::is_property(&key) {
            AttrTarget::Prop(key)
        } else {
            AttrTarget::Attr(key)
        };
    }
    true
}

impl Builder<'_> {
    fn build_spread(
        &mut self,
        state: &mut ElementState,
        node: u32,
        el: &JSXElement<'_>,
        is_svg: bool,
        has_children: bool,
    ) {
        let attrs = &el.opening_element.attributes;
        let inline = self.spread_inline(attrs, &[]);
        let props_before = state.view.props.len();
        let mut parts = Vec::new();
        let mut current = Vec::new();
        let flush = |parts: &mut Vec<SpreadPart>, current: &mut Vec<Attr>| {
            if !current.is_empty() {
                parts.push(SpreadPart::Entries(std::mem::take(current)));
            }
        };
        for (attr, &dissolve) in attrs.iter().zip(&inline) {
            match attr {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    if dissolve && let Some(entries) = inline_entries(&spread.argument) {
                        for (key, expression) in entries {
                            let value = if self.fold_dynamic(expression, false) {
                                AttrValue::Dynamic(self.tracked(expression))
                            } else {
                                AttrValue::Expr(ExprRef::of(expression))
                            };
                            current.push(Attr {
                                node,
                                target: AttrTarget::Attr(key.to_string()),
                                value,
                            });
                        }
                    } else {
                        flush(&mut parts, &mut current);
                        parts.push(SpreadPart::Generic {
                            expr: ExprRef::of(&spread.argument),
                            dynamic: self.fold_dynamic(&spread.argument, false),
                        });
                    }
                }
                JSXAttributeItem::Attribute(attr) => {
                    let name = attr_name(attr);
                    if name == "ref" {
                        if let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value
                            && let Some(expression) = container.expression.as_expression()
                        {
                            let target = self.ref_target(expression);
                            state.view.props.push(ElementProp::Ref(RefTarget { node, target }));
                        }
                    } else if !(name == "children" && has_children)
                        && let Some(property) = self.spread_prop(node, &name, attr)
                    {
                        current.push(property);
                    }
                }
            }
        }
        flush(&mut parts, &mut current);
        if parts.is_empty() {
            if state.view.props.len() != props_before {
                self.reference(state, node);
            }
            return;
        }
        let closed = close_spread(&mut parts, is_svg);
        self.reference(state, node);
        state.view.props.push(ElementProp::Spread(SpreadSegment {
            node,
            parts,
            closed,
            is_svg,
            has_children,
        }));
    }

    fn spread_prop(&mut self, node: u32, key: &str, a: &JSXAttribute<'_>) -> Option<Attr> {
        Some(match &a.value {
            None => {
                Attr { node, target: AttrTarget::Attr(key.to_string()), value: AttrValue::True }
            }
            Some(JSXAttributeValue::StringLiteral(s)) => Attr {
                node,
                target: AttrTarget::Attr(key.to_string()),
                value: AttrValue::Str(decode_entities(s.value.as_str()).into_owned()),
            },
            Some(JSXAttributeValue::Element(e)) => {
                let id = self.build_root(e);
                Attr { node, target: AttrTarget::Attr(key.to_string()), value: AttrValue::View(id) }
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                Attr { node, target: AttrTarget::Attr(key.to_string()), value: AttrValue::View(id) }
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                let e = c.expression.as_expression()?;
                let value = if self.fold_dynamic(e, false) {
                    AttrValue::Dynamic(self.tracked(e))
                } else {
                    AttrValue::Expr(ExprRef::of(e))
                };
                Attr { node, target: AttrTarget::Attr(key.to_string()), value }
            }
        })
    }

    fn spread_inline(&self, attrs: &[JSXAttributeItem<'_>], later: &[(&str, bool)]) -> Vec<bool> {
        let mut inline = Vec::with_capacity(attrs.len());
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
                !later.iter().any(|(late, defined)| *late == *key && !*defined)
                    && !attrs[i + 1..].iter().any(|after| match after {
                        JSXAttributeItem::Attribute(a) => {
                            attr_name(a) == *key && attr_defined(a) == Some(false)
                        }
                        JSXAttributeItem::SpreadAttribute(_) => false,
                    })
            }));
        }
        inline
    }
}

fn attr_defined(a: &JSXAttribute<'_>) -> Option<bool> {
    match &a.value {
        None => Some(true),
        Some(JSXAttributeValue::StringLiteral(_)) => Some(true),
        Some(JSXAttributeValue::Element(_)) | Some(JSXAttributeValue::Fragment(_)) => Some(true),
        Some(JSXAttributeValue::ExpressionContainer(c)) => {
            c.expression.as_expression().map(is_defined)
        }
    }
}

impl Builder<'_> {
    fn native_flow(&mut self, el: &JSXElement<'_>, intrinsic: Intrinsic) -> Child {
        let Some(flow) = self.build_flow(el, intrinsic) else {
            let id = self.push_view(ViewKind::Fragment(Vec::new()), el.span);
            return Child::View(id);
        };
        match flow {
            FlowView::Show(branch)
                if matches!(branch.child, FlowRender::Child(_))
                    && branch
                        .fallback
                        .as_ref()
                        .is_none_or(|f| matches!(f, FlowRender::Child(_)))
                    && !self.contains_component(el) =>
            {
                let origin = el.span;
                let test_is_boolean = self.is_boolean_when(el);
                let consequent = match branch.child {
                    FlowRender::Child(child) => *child,
                    child => {
                        return self.flow_child(
                            FlowView::Show(Branch {
                                when: branch.when,
                                child,
                                fallback: branch.fallback,
                            }),
                            origin,
                        );
                    }
                };
                let alternate = match branch.fallback {
                    Some(FlowRender::Child(child)) => Some(child),
                    Some(child) => {
                        return self.flow_child(
                            FlowView::Show(Branch {
                                when: branch.when,
                                child: FlowRender::Child(Box::new(consequent)),
                                fallback: Some(child),
                            }),
                            origin,
                        );
                    }
                    None => None,
                };
                Child::Conditional(ConditionalView {
                    test: branch.when.expr,
                    test_is_boolean,
                    consequent: Box::new(consequent),
                    alternate,
                    origin,
                })
            }
            flow => self.flow_child(flow, el.span),
        }
    }

    fn flow_child(&mut self, flow: FlowView, origin: Span) -> Child {
        let id = self.push_view(ViewKind::Flow(flow), origin);
        Child::View(id)
    }

    fn is_boolean_when(&self, el: &JSXElement<'_>) -> bool {
        let when = el.opening_element.attributes.iter().rev().find_map(|item| match item {
            JSXAttributeItem::Attribute(a) if attr_name(a) == "when" => Some(a),
            _ => None,
        });
        let Some(JSXAttributeValue::ExpressionContainer(container)) =
            when.and_then(|a| a.value.as_ref())
        else {
            return false;
        };
        container.expression.as_expression().is_some_and(|e| {
            self.fold_boolean(e) && self.source.as_bytes()[e.span().start as usize] != b'{'
        })
    }

    fn contains_component(&self, el: &JSXElement<'_>) -> bool {
        let mut check = ComponentCheck { builder: self, found: false };
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
}

struct ComponentCheck<'b, 'x> {
    builder: &'b Builder<'x>,
    found: bool,
}

impl<'a> Visit<'a> for ComponentCheck<'_, '_> {
    fn visit_jsx_element(&mut self, el: &JSXElement<'a>) {
        if self.found {
            return;
        }
        if !self.builder.is_native_tag(&el.opening_element.name) {
            self.found = true;
        } else {
            walk::walk_jsx_element(self, el);
        }
    }
}

impl Builder<'_> {
    fn build_component(&mut self, el: &JSXElement<'_>, callee: ExprRef) -> ComponentView {
        let tag = self.text(callee.span).to_string();
        self.path.push(format!("<{tag}>"));
        let found = self.island_attributes(el);
        if !found.present {
            self.island_orphans(&found);
        }
        let items = self.items(&el.children, false);
        let children = children_defined(&items).map(|defined| vec![("children", defined)]);
        let inline =
            self.spread_inline(&el.opening_element.attributes, children.as_deref().unwrap_or(&[]));
        let mut props: Vec<ComponentSegment> = Vec::new();
        let mut object: Vec<ComponentProp> = Vec::new();
        for (attr, &dissolve) in el.opening_element.attributes.iter().zip(&inline) {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    if !object.is_empty() {
                        props.push(ComponentSegment::Object(std::mem::take(&mut object)));
                    }
                    if dissolve && let Some(entries) = inline_entries(&s.argument) {
                        for (key, value) in entries {
                            let entry = ComponentValue::Dynamic(self.tracked(value));
                            object.push(if self.fold_dynamic(value, true) {
                                ComponentProp::Getter { key: key.to_string(), value: entry }
                            } else {
                                ComponentProp::Value { key: key.to_string(), value: entry }
                            });
                        }
                    } else {
                        props.push(ComponentSegment::Spread {
                            expr: ExprRef::of(&s.argument),
                            is_dynamic: self.fold_dynamic(&s.argument, false),
                        });
                    }
                }
                JSXAttributeItem::Attribute(a) => {
                    let key = attr_name(a);
                    if found.present && is_island_attr(&key) {
                        continue;
                    }
                    if key == "children" && !items.is_empty() {
                        self.report(
                            Report::new(Code::ChildrenPropIgnored, a.span)
                                .fix(vec![self.removal(a.span)]),
                        );
                        continue;
                    }
                    if let Some(prop) = self.component_prop(&key, a) {
                        object.push(prop);
                    }
                }
            }
        }
        if let Some(children) = self.component_children(items) {
            object.push(children);
        }
        if !object.is_empty() {
            props.push(ComponentSegment::Object(object));
        }
        self.path.pop();
        let island = if found.present {
            defer_component_props(&mut props);
            self.build_island(el, &found, callee, &tag)
        } else {
            None
        };
        ComponentView { callee, props, island }
    }

    fn component_prop(&mut self, key: &str, a: &JSXAttribute<'_>) -> Option<ComponentProp> {
        Some(match &a.value {
            None => ComponentProp::Value { key: key.to_string(), value: ComponentValue::True },
            Some(JSXAttributeValue::StringLiteral(s)) => ComponentProp::Value {
                key: key.to_string(),
                value: ComponentValue::Str(decode_entities(s.value.as_str()).into_owned()),
            },
            Some(JSXAttributeValue::Element(e)) => {
                let id = self.build_root(e);
                ComponentProp::Getter { key: key.to_string(), value: ComponentValue::Nested(id) }
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                ComponentProp::Getter { key: key.to_string(), value: ComponentValue::Nested(id) }
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                let e = c.expression.as_expression()?;
                if key == "ref"
                    && let Some(target) = self.assign_target(e)
                {
                    return Some(ComponentProp::ForwardRef(target));
                }
                let value = ComponentValue::Dynamic(self.tracked(e));
                if self.fold_dynamic(e, true) {
                    ComponentProp::Getter { key: key.to_string(), value }
                } else {
                    ComponentProp::Value { key: key.to_string(), value }
                }
            }
        })
    }

    fn component_children(&mut self, items: Vec<Item<'_, '_>>) -> Option<ComponentProp> {
        let key = "children".to_string();
        if items.len() > 1 {
            return Some(ComponentProp::Getter {
                key,
                value: ComponentValue::Children(self.component_list(items)),
            });
        }
        Some(match items.into_iter().next()? {
            Item::Text(text) => ComponentProp::Value { key, value: ComponentValue::Str(text) },
            Item::Element(el) => {
                let id = self.build_root(el);
                ComponentProp::Getter { key, value: ComponentValue::Nested(id) }
            }
            Item::Fragment(f) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                ComponentProp::Getter { key, value: ComponentValue::Nested(id) }
            }
            Item::Expr(e) if is_function(e) || !self.fold_dynamic(e, true) => {
                ComponentProp::Value { key, value: ComponentValue::Dynamic(self.tracked(e)) }
            }
            Item::Expr(e) => match self.conditional(e) {
                Some(branch) => {
                    let id = self.push_view(ViewKind::Flow(FlowView::Show(branch)), e.span());
                    ComponentProp::Getter { key, value: ComponentValue::Nested(id) }
                }
                None => {
                    ComponentProp::Getter { key, value: ComponentValue::Dynamic(self.tracked(e)) }
                }
            },
        })
    }
}

fn children_defined(items: &[Item<'_, '_>]) -> Option<bool> {
    if items.len() > 1 {
        return Some(true);
    }
    Some(match items.first()? {
        Item::Text(_) | Item::Element(_) | Item::Fragment(_) => true,
        Item::Expr(e) => is_defined(e),
    })
}

fn defer_component_props(props: &mut [ComponentSegment]) {
    for part in props.iter_mut() {
        match part {
            ComponentSegment::Object(entries) => {
                for entry in entries.iter_mut() {
                    if let ComponentProp::Value { key, value } = entry {
                        let deferred = matches!(
                            value,
                            ComponentValue::Dynamic(_) | ComponentValue::Children(_)
                        );
                        if deferred {
                            let key = std::mem::take(key);
                            let value = std::mem::replace(value, ComponentValue::True);
                            *entry = ComponentProp::Getter { key, value };
                        }
                    }
                }
            }
            ComponentSegment::Spread { is_dynamic, .. } => *is_dynamic = true,
        }
    }
}

pub struct IslandPlan {
    imports: HashMap<SymbolId, ImportUse>,
    uses: HashMap<SymbolId, u32>,
    pruned: HashSet<NodeId>,
}

pub struct ImportUse {
    pub specifier: String,
    pub base: ImportBase,
    pub decl: NodeId,
    pub single: bool,
    pub exported: bool,
    pub total_refs: usize,
}

#[derive(Clone)]
pub enum ImportBase {
    Named(String),
    Default,
    Namespace,
}

impl IslandPlan {
    pub fn scan(program: &Program<'_>, scoping: &Scoping) -> Self {
        let mut plan =
            Self { imports: HashMap::new(), uses: HashMap::new(), pruned: HashSet::new() };
        {
            let mut imports = IslandImports { plan: &mut plan, scoping };
            imports.visit_program(program);
        }
        let exported = exported_symbols(program, scoping);
        for (symbol, entry) in plan.imports.iter_mut() {
            entry.exported = exported.contains(symbol);
            entry.total_refs = scoping.get_resolved_references(*symbol).count();
            let uses = plan.uses.get(symbol).copied().unwrap_or(0) as usize;
            if !entry.exported && entry.single && uses == entry.total_refs && uses > 0 {
                plan.pruned.insert(entry.decl);
            }
        }
        plan
    }

    pub fn prunes_import(&self, import: &ImportDeclaration<'_>) -> bool {
        self.pruned.contains(&import.node_id())
    }

    pub fn decide(&self, symbol: SymbolId) -> IslandDecision<'_> {
        let Some(entry) = self.imports.get(&symbol) else { return IslandDecision::Direct };
        if entry.exported {
            return IslandDecision::Warn("exported");
        }
        let uses = self.uses.get(&symbol).copied().unwrap_or(0) as usize;
        if uses < entry.total_refs {
            return IslandDecision::Warn("used outside islands");
        }
        if !entry.single {
            return IslandDecision::Warn("sharing its import with other names");
        }
        IslandDecision::Split(entry)
    }
}

pub enum IslandDecision<'p> {
    Split(&'p ImportUse),
    Warn(&'static str),
    Direct,
}

struct IslandImports<'p> {
    plan: &'p mut IslandPlan,
    scoping: &'p Scoping,
}

impl<'a> Visit<'a> for IslandImports<'_> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if matches!(it.import_kind, ImportOrExportKind::Type) {
            return;
        }
        let Some(specifiers) = &it.specifiers else { return };
        let single = specifiers.len() == 1;
        for specifier in specifiers {
            let (local, base) = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                    let name = match &spec.imported {
                        ModuleExportName::IdentifierName(name) => name.name.as_str(),
                        ModuleExportName::IdentifierReference(name) => name.name.as_str(),
                        ModuleExportName::StringLiteral(_) => return,
                    };
                    (&spec.local, ImportBase::Named(name.to_string()))
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                    (&spec.local, ImportBase::Default)
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                    (&spec.local, ImportBase::Namespace)
                }
            };
            let symbol = local.symbol_id();
            if self.plan.imports.contains_key(&symbol) {
                continue;
            }
            self.plan.imports.insert(
                symbol,
                ImportUse {
                    specifier: it.source.value.to_string(),
                    base,
                    decl: it.node_id(),
                    single,
                    exported: false,
                    total_refs: 0,
                },
            );
        }
        walk::walk_import_declaration(self, it);
    }

    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if has_island_attr(&it.opening_element.attributes)
            && let Some(symbol) = self.callee_symbol(&it.opening_element.name)
        {
            *self.plan.uses.entry(symbol).or_insert(0) += 1;
        }
        walk::walk_jsx_element(self, it);
    }
}

impl IslandImports<'_> {
    fn callee_symbol(&self, name: &JSXElementName<'_>) -> Option<SymbolId> {
        let root = match name {
            JSXElementName::IdentifierReference(id) => id.as_ref(),
            JSXElementName::MemberExpression(member) => root_object(&member.object)?,
            _ => return None,
        };
        let reference = root.reference_id.get()?;
        self.scoping.get_reference(reference).symbol_id()
    }
}

fn has_island_attr(attributes: &[JSXAttributeItem<'_>]) -> bool {
    attributes.iter().any(|item| match item {
        JSXAttributeItem::Attribute(a) => match &a.name {
            JSXAttributeName::Identifier(id) => id.name.as_str() == "island",
            JSXAttributeName::NamespacedName(_) => false,
        },
        JSXAttributeItem::SpreadAttribute(_) => false,
    })
}

fn root_object<'a, 'b>(
    object: &'b JSXMemberExpressionObject<'a>,
) -> Option<&'b IdentifierReference<'a>> {
    match object {
        JSXMemberExpressionObject::IdentifierReference(id) => Some(id),
        JSXMemberExpressionObject::MemberExpression(member) => root_object(&member.object),
        JSXMemberExpressionObject::ThisExpression(_) => None,
    }
}

fn member_path(name: &JSXElementName<'_>) -> Option<Vec<String>> {
    let mut reversed = Vec::new();
    let mut object = match name {
        JSXElementName::IdentifierReference(_) => return Some(Vec::new()),
        JSXElementName::MemberExpression(member) => {
            reversed.push(member.property.name.as_str().to_string());
            &member.object
        }
        _ => return None,
    };
    loop {
        match object {
            JSXMemberExpressionObject::IdentifierReference(_) => {
                reversed.reverse();
                return Some(reversed);
            }
            JSXMemberExpressionObject::MemberExpression(member) => {
                reversed.push(member.property.name.as_str().to_string());
                object = &member.object;
            }
            JSXMemberExpressionObject::ThisExpression(_) => return None,
        }
    }
}

impl Builder<'_> {
    fn fold_kind(&self, e: &Expression<'_>) -> Option<StaticKind> {
        self.facts.kind_of(self.scoping, e).map(|kind| match kind {
            crate::kind::Kind::Numeric => StaticKind::Numeric,
            crate::kind::Kind::String => StaticKind::String,
        })
    }

    fn is_global(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.scoping.get_reference(r).symbol_id().is_none())
    }

    fn is_global_named(&self, id: &IdentifierReference<'_>, name: &str) -> bool {
        id.name == name
            && id.reference_id.get().is_some_and(|reference| {
                self.scoping.get_reference(reference).symbol_id().is_none()
            })
    }

    fn fold_boolean(&self, e: &Expression<'_>) -> bool {
        match e.without_parentheses() {
            Expression::BooleanLiteral(_) => true,
            Expression::UnaryExpression(unary) => unary.operator == UnaryOperator::LogicalNot,
            Expression::BinaryExpression(binary) => matches!(
                binary.operator,
                BinaryOperator::Equality
                    | BinaryOperator::Inequality
                    | BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
                    | BinaryOperator::LessThan
                    | BinaryOperator::LessEqualThan
                    | BinaryOperator::GreaterThan
                    | BinaryOperator::GreaterEqualThan
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof
            ),
            Expression::LogicalExpression(logical) => {
                logical.operator != LogicalOperator::Coalesce
                    && self.fold_boolean(&logical.left)
                    && self.fold_boolean(&logical.right)
            }
            Expression::ConditionalExpression(conditional) => {
                self.fold_boolean(&conditional.consequent)
                    && self.fold_boolean(&conditional.alternate)
            }
            Expression::CallExpression(call) => {
                !call.optional
                    && matches!(&call.callee, Expression::Identifier(id) if id.name == "Boolean" && self.is_global(id))
            }
            _ => false,
        }
    }

    fn fold_text(&self, e: &Expression<'_>) -> Option<String> {
        match e.without_parentheses() {
            Expression::NumericLiteral(n) => format_integer(n.value),
            Expression::UnaryExpression(_) => format_integer(self.numeric_value(e)?),
            Expression::CallExpression(call) => self.facts.folded_read(call)?.1.text.clone(),
            inner => self.string_value(inner),
        }
    }

    fn string_value(&self, e: &Expression<'_>) -> Option<String> {
        match e.without_parentheses() {
            Expression::StringLiteral(s) => Some(s.value.to_string()),
            Expression::TemplateLiteral(t) => {
                let mut text = String::new();
                for (index, quasi) in t.quasis.iter().enumerate() {
                    text.push_str(quasi.value.cooked.as_ref()?.as_str());
                    if let Some(expression) = t.expressions.get(index) {
                        text.push_str(&self.primitive_text(expression)?);
                    }
                }
                Some(text)
            }
            Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
                match (self.string_value(&b.left), self.string_value(&b.right)) {
                    (Some(left), Some(right)) => Some(left + &right),
                    (Some(left), None) => Some(left + &self.primitive_text(&b.right)?),
                    (None, Some(right)) => Some(self.primitive_text(&b.left)? + &right),
                    (None, None) => None,
                }
            }
            _ => None,
        }
    }

    fn primitive_text(&self, e: &Expression<'_>) -> Option<String> {
        if let Some(number) = self.numeric_value(e) {
            return format_integer(number);
        }
        match e.without_parentheses() {
            Expression::BooleanLiteral(b) => Some(b.value.to_string()),
            Expression::NullLiteral(_) => Some("null".to_string()),
            Expression::Identifier(id) if self.is_global_named(id, "undefined") => {
                Some("undefined".to_string())
            }
            _ => self.fold_text(e),
        }
    }

    fn numeric_value(&self, e: &Expression<'_>) -> Option<f64> {
        match e.without_parentheses() {
            Expression::NumericLiteral(n) => Some(n.value),
            Expression::Identifier(id) if self.is_global_named(id, "NaN") => Some(f64::NAN),
            Expression::Identifier(id) if self.is_global_named(id, "Infinity") => {
                Some(f64::INFINITY)
            }
            Expression::UnaryExpression(u) => {
                let value = self.numeric_value(&u.argument)?;
                match u.operator {
                    UnaryOperator::UnaryNegation => Some(-value),
                    UnaryOperator::UnaryPlus => Some(value),
                    _ => None,
                }
            }
            Expression::BinaryExpression(b) => {
                let left = self.numeric_value(&b.left)?;
                let right = self.numeric_value(&b.right)?;
                match b.operator {
                    BinaryOperator::Addition => Some(left + right),
                    BinaryOperator::Subtraction => Some(left - right),
                    BinaryOperator::Multiplication => Some(left * right),
                    BinaryOperator::Division => Some(left / right),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn fold_literal(&self, e: &Expression<'_>) -> Option<Literal> {
        if let Some(text) = self.fold_text(e) {
            return Some(Literal::Str(text));
        }
        match e.without_parentheses() {
            Expression::BooleanLiteral(b) => Some(Literal::Bool(b.value)),
            Expression::NullLiteral(_) => Some(Literal::Nullish),
            Expression::Identifier(id) if self.is_global_named(id, "undefined") => {
                Some(Literal::Nullish)
            }
            _ => None,
        }
    }

    fn fold_truthy(&self, e: &Expression<'_>) -> Option<bool> {
        if let Some(number) = self.numeric_value(e) {
            return Some(number != 0.0 && !number.is_nan());
        }
        if let Expression::CallExpression(call) = e.without_parentheses()
            && let Some((_, fold)) = self.facts.folded_read(call)
            && fold.kind == Some(crate::kind::Kind::Numeric)
        {
            return fold
                .text
                .as_ref()?
                .parse::<f64>()
                .ok()
                .map(|number| number != 0.0 && !number.is_nan());
        }
        match e.without_parentheses() {
            Expression::BooleanLiteral(b) => Some(b.value),
            Expression::NullLiteral(_) => Some(false),
            Expression::Identifier(id) if self.is_global_named(id, "undefined") => Some(false),
            _ => self.fold_text(e).map(|text| !text.is_empty()),
        }
    }

    fn fold_style(&self, e: &Expression<'_>) -> Option<String> {
        let Expression::ObjectExpression(object) = e.without_parentheses() else { return None };
        if object.properties.is_empty() {
            return None;
        }
        let mut out = String::new();
        for property in &object.properties {
            let (key, value) = static_property(property)?;
            let value = self.fold_text(value)?;
            if !out.is_empty() {
                out.push(';');
            }
            out.push_str(key);
            out.push(':');
            out.push_str(&value);
        }
        Some(out)
    }

    fn class_add(&self, keys: &mut ClassKeys, e: &Expression<'_>) -> Option<()> {
        match e.without_parentheses() {
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    let (key, value) = static_property(property)?;
                    keys.set(key, self.fold_truthy(value)?);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    let item = element.as_expression()?;
                    match item.without_parentheses() {
                        Expression::ObjectExpression(_) | Expression::ArrayExpression(_) => {
                            self.class_add(keys, item)?
                        }
                        _ => match self.fold_literal(item)? {
                            Literal::Str(text) if !text.is_empty() => keys.set(&text, true),
                            Literal::Bool(true) => keys.set("true", true),
                            Literal::Str(_) | Literal::Bool(false) | Literal::Nullish => {}
                        },
                    }
                }
            }
            _ => match self.fold_literal(e)? {
                Literal::Str(text) => keys.set(&text, true),
                Literal::Bool(_) | Literal::Nullish => {}
            },
        }
        Some(())
    }

    fn fold_dynamic(&self, e: &Expression<'_>, jsx_is_dynamic: bool) -> bool {
        let mut check = DynamicCheck { jsx_is_dynamic, facts: self.facts, found: false };
        check.visit_expression(e);
        check.found
    }
}

fn format_integer(n: f64) -> Option<String> {
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15).then(|| format!("{}", n as i64))
}

struct DynamicCheck<'f> {
    jsx_is_dynamic: bool,
    facts: &'f SharedFacts,
    found: bool,
}

impl<'a> Visit<'a> for DynamicCheck<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.facts.folded_read(it).is_none() {
            self.found = true;
        }
    }

    fn visit_tagged_template_expression(&mut self, _: &TaggedTemplateExpression<'a>) {
        self.found = true;
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if !self.facts.keyed.is_key_read(it) {
            self.found = true;
        }
    }

    fn visit_computed_member_expression(&mut self, _: &ComputedMemberExpression<'a>) {
        self.found = true;
    }

    fn visit_private_field_expression(&mut self, _: &PrivateFieldExpression<'a>) {
        self.found = true;
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.found |= self.facts.asyncs.is_read(it);
    }

    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found |= self.jsx_is_dynamic;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found |= self.jsx_is_dynamic;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

fn exported_symbols(
    program: &Program<'_>,
    scoping: &Scoping,
) -> std::collections::HashSet<SymbolId> {
    let mut exported = std::collections::HashSet::new();
    let resolved = |local: &IdentifierReference<'_>| {
        scoping.get_reference(local.reference_id.get()?).symbol_id()
    };
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => {
                if let Declaration::VariableDeclaration(variables) = &export.declaration {
                    for declarator in &variables.declarations {
                        exported.extend(
                            declarator.id.get_binding_identifiers().iter().map(|id| id.symbol_id()),
                        );
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    if let ModuleExportName::IdentifierReference(local) = &specifier.local
                        && let Some(symbol) = resolved(local)
                    {
                        exported.insert(symbol);
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::Identifier(local) = &export.declaration
                    && let Some(symbol) = resolved(local)
                {
                    exported.insert(symbol);
                }
            }
            _ => {}
        }
    }
    exported
}

pub struct SourceSites {
    sites: Option<SiteRegistry>,
    assigned: HashMap<(u32, u32), SiteId>,
}

pub fn collect_sites(program: &Program<'_>, module_id: Option<&str>, source: &str) -> SourceSites {
    let mut sites = SourceSites {
        sites: module_id.map(|id| SiteRegistry::new(id, source)),
        assigned: HashMap::new(),
    };
    if sites.sites.is_some() {
        sites.visit_program(program);
    }
    sites
}

impl SourceSites {
    fn claim(&mut self, span: Span) {
        let key = (span.start, span.end);
        if !self.assigned.contains_key(&key)
            && let Some(sites) = self.sites.as_mut()
        {
            self.assigned.insert(key, sites.assign());
        }
    }
}

impl<'a> Visit<'a> for SourceSites {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.claim(it.span);
        walk::walk_call_expression(self, it);
    }

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        self.claim(it.span);
        walk::walk_await_expression(self, it);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.claim(it.span);
        walk::walk_arrow_function_expression(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.claim(it.span);
        walk::walk_function(self, it, flags);
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        self.claim(it.span);
        walk::walk_conditional_expression(self, it);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        self.claim(it.span);
        walk::walk_logical_expression(self, it);
    }

    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        self.claim(it.span);
        walk::walk_jsx_element(self, it);
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        self.claim(it.span);
        walk::walk_jsx_fragment(self, it);
    }
}

impl Builder<'_> {
    fn finish_schedule(&mut self, state: &mut ElementState) {
        let mut explicit = std::mem::take(&mut state.binds).into_iter().peekable();
        let mut known_groups: HashMap<Vec<SymbolId>, usize> = HashMap::new();
        let mut groups = Vec::new();
        let mut links = state.view.links.iter().enumerate().peekable();
        for (index, prop) in state.view.props.iter_mut().enumerate() {
            while links.peek().is_some_and(|(_, link)| link.after_prop == index) {
                let (index, _) = links.next().expect("link at current property boundary");
                state.view.schedule.immediate.push(SchedMember { kind: MemberKind::Link, index });
            }
            let member = SchedMember { kind: MemberKind::Prop, index };
            let mut deps = Vec::new();
            let mut fixed = true;
            let tracked = if explicit.peek().is_some_and(|(at, ..)| *at == index) {
                let (_, explicit_deps, explicit_fixed) =
                    explicit.next().expect("matching dependency registration");
                deps = explicit_deps;
                fixed = explicit_fixed;
                true
            } else {
                match prop {
                    ElementProp::Attr(attr) => {
                        take_attr_tracking(&mut attr.value, &mut deps, &mut fixed)
                    }
                    ElementProp::Spread(_) | ElementProp::Event(_) | ElementProp::Ref(_) => false,
                }
            };
            if tracked {
                add_effect_member(&mut groups, &mut known_groups, member, deps, fixed);
            } else {
                state.view.schedule.immediate.push(member);
            }
        }
        for (index, _) in links {
            state.view.schedule.immediate.push(SchedMember { kind: MemberKind::Link, index });
        }
        for (index, late) in state.view.late_values.iter_mut().enumerate() {
            let member = SchedMember { kind: MemberKind::Late, index };
            let mut deps = Vec::new();
            let mut fixed = true;
            if take_attr_tracking(&mut late.value, &mut deps, &mut fixed) {
                add_effect_member(&mut groups, &mut known_groups, member, deps, fixed);
            } else {
                state.view.schedule.post_children.push(member);
            }
        }
        for (deps, index) in known_groups {
            groups[index].deps = deps;
        }
        state.view.schedule.effects = groups;
    }
}

fn add_effect_member(
    groups: &mut Vec<EffectGroup>,
    known: &mut HashMap<Vec<SymbolId>, usize>,
    member: SchedMember,
    mut deps: Vec<SymbolId>,
    fixed: bool,
) {
    deps.sort_unstable();
    deps.dedup();
    let at = if deps.is_empty() {
        let at = groups.len();
        groups.push(EffectGroup { deps: Vec::new(), fixed: true, members: Vec::new() });
        at
    } else {
        *known.entry(deps).or_insert_with(|| {
            let at = groups.len();
            groups.push(EffectGroup { deps: Vec::new(), fixed: true, members: Vec::new() });
            at
        })
    };
    groups[at].fixed &= fixed;
    groups[at].members.push(member);
}

fn take_attr_tracking(value: &mut AttrValue, deps: &mut Vec<SymbolId>, fixed: &mut bool) -> bool {
    match value {
        AttrValue::Dynamic(dynamic) => {
            deps.append(&mut dynamic.deps);
            *fixed &= dynamic.fixed;
            dynamic.mode == ValueMode::Tracked
        }
        AttrValue::ClassParts(parts) => {
            let mut tracked = false;
            for part in parts {
                tracked |= take_attr_tracking(part, deps, fixed);
            }
            tracked
        }
        AttrValue::TextParts(parts) => {
            let mut tracked = false;
            for part in parts {
                if let TextPart::Dynamic(dynamic) = part {
                    deps.append(&mut dynamic.deps);
                    *fixed &= dynamic.fixed;
                    tracked |= dynamic.mode == ValueMode::Tracked;
                }
            }
            tracked
        }
        AttrValue::True
        | AttrValue::Str(_)
        | AttrValue::Expr(_)
        | AttrValue::View(_)
        | AttrValue::Truthy(_) => false,
    }
}

impl Builder<'_> {
    fn build_flow(&mut self, el: &JSXElement<'_>, intrinsic: Intrinsic) -> Option<FlowView> {
        self.path.push(format!("<{}>", intrinsic.name()));
        let flow = match intrinsic {
            Intrinsic::Show => self.flow_show(el, intrinsic).map(FlowView::Show),
            Intrinsic::For => self.flow_for(el),
            Intrinsic::Repeat => self.flow_repeat(el),
            Intrinsic::Switch => self.flow_switch(el),
            Intrinsic::Portal => self.flow_portal(el),
            Intrinsic::Match => {
                self.report(Report::new(Code::MatchOutsideSwitch, el.opening_element.span));
                None
            }
        };
        self.path.pop();
        flow
    }

    fn flow_attributes<'b, 'x>(
        &mut self,
        el: &'b JSXElement<'x>,
        intrinsic: Intrinsic,
        accepted: &[&str],
    ) -> Vec<(String, &'b JSXAttribute<'x>)> {
        let mut attributes = Vec::new();
        for item in &el.opening_element.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    let span = spread.span;
                    self.report(
                        Report::new(Code::ControlFlowAttribute, span)
                            .arg("tag", intrinsic.name().to_string())
                            .arg("attribute", "{...}")
                            .fix(vec![self.removal(span)]),
                    );
                }
                JSXAttributeItem::Attribute(a) => {
                    let name = attr_name(a);
                    if accepted.contains(&name.as_str()) {
                        attributes.push((name, &**a));
                        continue;
                    }
                    let span = a.span;
                    self.report(
                        Report::new(Code::ControlFlowAttribute, span)
                            .arg("tag", intrinsic.name().to_string())
                            .arg("attribute", name)
                            .fix(vec![self.removal(span)]),
                    );
                }
            }
        }
        attributes
    }
}

impl Builder<'_> {
    fn required_source<'b, 'x>(
        &mut self,
        el: &JSXElement<'x>,
        intrinsic: Intrinsic,
        attributes: &[(String, &'b JSXAttribute<'x>)],
        name: &str,
    ) -> Option<FlowSource> {
        let source = self.flow_attr_source(attributes, name);
        if source.is_none() {
            self.report(
                Report::new(Code::ControlFlowMissing, el.opening_element.span)
                    .arg("tag", intrinsic.name().to_string())
                    .arg("attribute", name.to_string()),
            );
        }
        source
    }

    fn flow_attr_source<'b, 'x>(
        &mut self,
        attributes: &[(String, &'b JSXAttribute<'x>)],
        name: &str,
    ) -> Option<FlowSource> {
        let (_, a) = attributes.iter().rev().find(|(n, _)| n == name)?;
        Some(match a.value.as_ref()? {
            JSXAttributeValue::ExpressionContainer(c) => {
                let e = c.expression.as_expression()?;
                FlowSource { expr: ExprRef::of(e), getter: self.stable_callee(e) }
            }
            JSXAttributeValue::StringLiteral(s) => {
                FlowSource { expr: ExprRef::at(s.as_ref(), s.span), getter: None }
            }
            JSXAttributeValue::Element(e) => {
                FlowSource { expr: ExprRef::at(e.as_ref(), e.span), getter: None }
            }
            JSXAttributeValue::Fragment(f) => {
                FlowSource { expr: ExprRef::at(f.as_ref(), f.span), getter: None }
            }
        })
    }

    fn flow_fallback<'b, 'x>(
        &mut self,
        attributes: &[(String, &'b JSXAttribute<'x>)],
    ) -> Option<FlowRender> {
        let (_, a) = attributes.iter().rev().find(|(name, _)| *name == "fallback")?;
        Some(match a.value.as_ref()? {
            JSXAttributeValue::ExpressionContainer(c) => {
                self.render_branch(c.expression.as_expression()?)
            }
            JSXAttributeValue::StringLiteral(s) => FlowRender::Child(Box::new(Child::StaticText(
                decode_entities(s.value.as_str()).into_owned(),
            ))),
            JSXAttributeValue::Element(e) => {
                let id = self.build_root(e);
                FlowRender::Child(Box::new(Child::View(id)))
            }
            JSXAttributeValue::Fragment(f) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                FlowRender::Child(Box::new(Child::View(id)))
            }
        })
    }

    fn case_children(&mut self, el: &JSXElement<'_>, intrinsic: Intrinsic) -> Option<FlowRender> {
        let mut children = el.children.iter().filter(|child| is_meaningful(child));
        let Some(first) = children.next() else {
            self.report(
                Report::new(Code::ControlFlowChildren, el.opening_element.span)
                    .arg("tag", intrinsic.name().to_string())
                    .arg("expected", "at least one child"),
            );
            return None;
        };
        if children.next().is_none()
            && let JSXChild::ExpressionContainer(c) = first
            && let Some(e) = c.expression.as_expression().filter(|e| is_function(e))
        {
            return Some(FlowRender::Function(ExprRef::of(e)));
        }
        let items = self.items(&el.children, false);
        let mut list = self.component_list(items);
        if list.len() == 1 {
            return Some(FlowRender::Child(Box::new(list.pop().expect("one entry"))));
        }
        let id = self.push_view(ViewKind::Fragment(list), el.span);
        Some(FlowRender::Child(Box::new(Child::View(id))))
    }

    fn flow_show(&mut self, el: &JSXElement<'_>, intrinsic: Intrinsic) -> Option<Branch> {
        let attributes = self.flow_attributes(el, intrinsic, &["when", "fallback"]);
        let when = self.required_source(el, intrinsic, &attributes, "when")?;
        let child = self.case_children(el, intrinsic)?;
        let fallback = self.flow_fallback(&attributes);
        Some(Branch { when, child, fallback })
    }

    fn flow_portal(&mut self, el: &JSXElement<'_>) -> Option<FlowView> {
        let intrinsic = Intrinsic::Portal;
        let attributes = self.flow_attributes(el, intrinsic, &["mount"]);
        let child = self.case_children(el, intrinsic)?;
        let mount = self.flow_attr_source(&attributes, "mount");
        Some(FlowView::Portal { child, mount })
    }
}

impl Builder<'_> {
    fn flow_for(&mut self, el: &JSXElement<'_>) -> Option<FlowView> {
        let intrinsic = Intrinsic::For;
        let attributes =
            self.flow_attributes(el, intrinsic, &["each", "fallback", "keyed", "children"]);
        let keyed = attributes.iter().rev().find(|(name, _)| *name == "keyed").map(|(_, a)| *a);
        if !keyed.is_some_and(is_indexed)
            && let Some((_, a)) = attributes.iter().find(|(name, _)| *name == "each")
            && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
            && let Some(Expression::ArrayExpression(array)) =
                c.expression.as_expression().map(Expression::without_parentheses)
        {
            self.report(Report::new(Code::InlineEach, array.span));
        }
        let each = self.required_source(el, intrinsic, &attributes, "each")?;
        let map =
            self.row_function(el, &attributes, intrinsic, "one function `(item, index) => …`")?;
        let fallback = self.flow_fallback(&attributes);
        let key = match keyed {
            Some(a) => Some(self.flow_key(a)),
            None => None,
        };
        Some(FlowView::For { each, map: ExprRef::of(map), fallback, key })
    }

    fn flow_key(&mut self, a: &JSXAttribute<'_>) -> FlowKeyValue {
        match a.value.as_ref() {
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(e) => FlowKeyValue::Expr(ExprRef::of(e)),
                None => FlowKeyValue::Expr(ExprRef::at(c.as_ref(), c.span)),
            },
            Some(JSXAttributeValue::StringLiteral(s)) => {
                FlowKeyValue::Str(decode_entities(s.value.as_str()).into_owned())
            }
            Some(JSXAttributeValue::Element(e)) => {
                let id = self.build_root(e);
                FlowKeyValue::View(id)
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                let children = self.fragment_children(&f.children);
                let id = self.push_view(ViewKind::Fragment(children), f.span);
                FlowKeyValue::View(id)
            }
            None => FlowKeyValue::Expr(ExprRef::at(a, a.span)),
        }
    }

    fn row_function<'b, 'x>(
        &mut self,
        el: &'b JSXElement<'x>,
        attributes: &[(String, &'b JSXAttribute<'x>)],
        intrinsic: Intrinsic,
        expected: &'static str,
    ) -> Option<&'b Expression<'x>> {
        let mut children = el.children.iter().filter(|child| is_meaningful(child));
        let nested = match (children.next(), children.next()) {
            (None, _) => None,
            (Some(JSXChild::ExpressionContainer(c)), None) => c.expression.as_expression().map(Ok),
            (Some(child), _) => Some(Err(child.span())),
        };
        let attribute = attributes.iter().find(|(name, _)| *name == "children").map(|(_, a)| a);
        let map = match (nested, attribute) {
            (Some(Ok(e)), attribute) => {
                if let Some(a) = attribute {
                    self.report(
                        Report::new(Code::ChildrenPropIgnored, a.span)
                            .fix(vec![self.removal(a.span)]),
                    );
                }
                Ok(e)
            }
            (Some(Err(span)), _) => Err(span),
            (None, Some(a)) => match &a.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    c.expression.as_expression().ok_or(a.span)
                }
                _ => Err(a.span),
            },
            (None, None) => Err(el.opening_element.span),
        };
        match map {
            Ok(map) => Some(map),
            Err(span) => {
                self.report(
                    Report::new(Code::ControlFlowChildren, span)
                        .arg("tag", intrinsic.name().to_string())
                        .arg("expected", expected),
                );
                None
            }
        }
    }

    fn flow_repeat(&mut self, el: &JSXElement<'_>) -> Option<FlowView> {
        let intrinsic = Intrinsic::Repeat;
        let attributes = self.flow_attributes(el, intrinsic, &["count", "fallback", "children"]);
        let count = self.required_source(el, intrinsic, &attributes, "count")?;
        let row = self.row_function(el, &attributes, intrinsic, "one function `(index) => …`")?;
        let fallback = self.flow_fallback(&attributes);
        let map = ExprRef::of(row);
        match self.static_count(&attributes).filter(|_| self.is_unrollable_row(row)) {
            Some(times) => Some(FlowView::Rows { times, map }),
            None => Some(FlowView::Repeat { count, map, fallback }),
        }
    }

    fn static_count<'b, 'x>(&self, attributes: &[(String, &'b JSXAttribute<'x>)]) -> Option<u32> {
        let (_, a) = attributes.iter().rev().find(|(name, _)| *name == "count")?;
        let JSXAttributeValue::ExpressionContainer(c) = a.value.as_ref()? else { return None };
        let count = c.expression.as_expression()?;
        if self.fold_kind(count) != Some(StaticKind::Numeric) {
            return None;
        }
        self.fold_text(count)?.parse().ok().filter(|times| *times > 0)
    }

    fn is_unrollable_row(&self, row: &Expression<'_>) -> bool {
        let Expression::ArrowFunctionExpression(arrow) = row.without_parentheses() else {
            return false;
        };
        let params = &arrow.params;
        let has_plain_params = params.rest.is_none()
            && params.items.len() <= 1
            && params.items.iter().all(|param| {
                matches!(param.pattern, BindingPattern::BindingIdentifier(_))
                    && param.initializer.is_none()
            });
        let Some(body) = arrow.body.as_expression().map(Expression::without_parentheses) else {
            return false;
        };
        let mut eager = EagerCheck { found: false };
        eager.visit_expression(body);
        has_plain_params
            && matches!(body, Expression::JSXElement(_) | Expression::JSXFragment(_))
            && !eager.found
            && !self.scoping.symbol_names().any(|name| name == "Array")
    }

    fn flow_switch(&mut self, el: &JSXElement<'_>) -> Option<FlowView> {
        let intrinsic = Intrinsic::Switch;
        let attributes = self.flow_attributes(el, intrinsic, &["fallback"]);
        let mut whens = Vec::new();
        let mut children = Vec::new();
        let mut is_valid = true;
        for child in el.children.iter().filter(|child| is_meaningful(child)) {
            let case = match child {
                JSXChild::Element(case)
                    if matches!(
                        self.tag_of(&case.opening_element.name),
                        Tag::Intrinsic(Intrinsic::Match)
                    ) =>
                {
                    case
                }
                other => {
                    if is_valid {
                        self.report(
                            Report::new(Code::ControlFlowChildren, other.span())
                                .arg("tag", intrinsic.name().to_string())
                                .arg("expected", "only `<Match>` elements"),
                        );
                    }
                    is_valid = false;
                    continue;
                }
            };
            self.path.push(String::from("<Match>"));
            let case_attributes = self.flow_attributes(case, Intrinsic::Match, &["when"]);
            let when = self.required_source(case, Intrinsic::Match, &case_attributes, "when");
            let render = self.case_children(case, Intrinsic::Match);
            self.path.pop();
            match (when, render) {
                (Some(when), Some(render)) => {
                    whens.push(when);
                    children.push(render);
                }
                _ => is_valid = false,
            }
        }
        let fallback = self.flow_fallback(&attributes);
        is_valid.then_some(FlowView::Switch { whens, children, fallback })
    }
}

fn is_indexed(a: &JSXAttribute<'_>) -> bool {
    matches!(
        &a.value,
        Some(JSXAttributeValue::ExpressionContainer(c))
            if matches!(
                c.expression.as_expression().map(Expression::without_parentheses),
                Some(Expression::BooleanLiteral(literal)) if !literal.value
            )
    )
}

struct EagerCheck {
    found: bool,
}

impl<'a> Visit<'a> for EagerCheck {
    fn visit_jsx_spread_attribute(&mut self, _: &JSXSpreadAttribute<'a>) {
        self.found = true;
    }

    fn visit_jsx_spread_child(&mut self, _: &JSXSpreadChild<'a>) {
        self.found = true;
    }

    fn visit_jsx_attribute(&mut self, it: &JSXAttribute<'a>) {
        let JSXAttributeName::Identifier(name) = &it.name else {
            self.found = true;
            return;
        };
        let is_handler = name.name.starts_with("on")
            && matches!(&it.value, Some(JSXAttributeValue::ExpressionContainer(c))
                if c.expression.as_expression().is_none_or(|e| !is_function(e)));
        if name.name == "ref" || is_handler {
            self.found = true;
        } else {
            walk::walk_jsx_attribute(self, it);
        }
    }
}

struct IslandAttributes<'b, 'x> {
    present: bool,
    island: Option<&'b JSXAttribute<'x>>,
    media: Option<&'b JSXAttribute<'x>>,
    root_margin: Option<&'b JSXAttribute<'x>>,
    fallback: Option<&'b JSXAttribute<'x>>,
}

impl Builder<'_> {
    fn island_attributes<'b, 'x>(&self, el: &'b JSXElement<'x>) -> IslandAttributes<'b, 'x> {
        let mut found = IslandAttributes {
            present: false,
            island: None,
            media: None,
            root_margin: None,
            fallback: None,
        };
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(a) = item else { continue };
            match attr_name(a).as_str() {
                "island" => {
                    found.present = true;
                    found.island = Some(a);
                }
                "islandMedia" => found.media = Some(a),
                "islandRootMargin" => found.root_margin = Some(a),
                "islandFallback" => found.fallback = Some(a),
                _ => {}
            }
        }
        found
    }

    fn island_orphans(&mut self, found: &IslandAttributes<'_, '_>) {
        for (name, attr) in [
            ("islandMedia", found.media),
            ("islandRootMargin", found.root_margin),
            ("islandFallback", found.fallback),
        ] {
            if let Some(a) = attr {
                self.report(
                    Report::new(Code::IslandOrphan, a.span).arg("attribute", name.to_string()),
                );
            }
        }
    }

    fn build_island(
        &mut self,
        el: &JSXElement<'_>,
        found: &IslandAttributes<'_, '_>,
        callee: ExprRef,
        tag: &str,
    ) -> Option<IslandView> {
        let trigger = self.island_trigger(found)?;
        let media = found.media.and_then(|a| self.island_string(a));
        let root_margin = found.root_margin.and_then(|a| self.island_string(a));
        if trigger == IslandTrigger::Media && media.is_none() {
            let span = found.island.map_or(el.opening_element.span, |a| a.span);
            self.report(Report::new(Code::IslandMediaMissing, span));
            return None;
        }
        let fallback = found.fallback.and_then(|a| self.island_fallback(a));
        let loader = self.island_loader(&el.opening_element.name, callee, tag);
        Some(IslandView { trigger, media, root_margin, loader, fallback })
    }

    fn island_trigger(&mut self, found: &IslandAttributes<'_, '_>) -> Option<IslandTrigger> {
        let a = found.island?;
        match &a.value {
            None => Some(IslandTrigger::Eager),
            Some(JSXAttributeValue::StringLiteral(s)) => {
                self.island_trigger_name(&decode_entities(s.value.as_str()), a.span)
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(Expression::StringLiteral(s)) => {
                    self.island_trigger_name(s.value.as_str(), a.span)
                }
                Some(Expression::BooleanLiteral(b)) if b.value => Some(IslandTrigger::Eager),
                _ => {
                    self.report(
                        Report::new(Code::IslandTrigger, a.span)
                            .arg("value", self.text(a.span).to_string()),
                    );
                    None
                }
            },
            Some(_) => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span)
                        .arg("value", self.text(a.span).to_string()),
                );
                None
            }
        }
    }

    fn island_trigger_name(&mut self, name: &str, span: Span) -> Option<IslandTrigger> {
        match name {
            "eager" => Some(IslandTrigger::Eager),
            "idle" => Some(IslandTrigger::Idle),
            "visible" => Some(IslandTrigger::Visible),
            "media" => Some(IslandTrigger::Media),
            "interaction" => Some(IslandTrigger::Interaction),
            _ => {
                self.report(Report::new(Code::IslandTrigger, span).arg("value", name.to_string()));
                None
            }
        }
    }

    fn island_string(&mut self, a: &JSXAttribute<'_>) -> Option<String> {
        match &a.value {
            None => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span)
                        .arg("value", self.text(a.span).to_string()),
                );
                None
            }
            Some(JSXAttributeValue::StringLiteral(s)) => {
                Some(decode_entities(s.value.as_str()).into_owned())
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(Expression::StringLiteral(s)) => Some(s.value.to_string()),
                _ => {
                    self.report(
                        Report::new(Code::IslandTrigger, a.span)
                            .arg("value", self.text(a.span).to_string()),
                    );
                    None
                }
            },
            Some(_) => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span)
                        .arg("value", self.text(a.span).to_string()),
                );
                None
            }
        }
    }

    fn island_fallback(&mut self, a: &JSXAttribute<'_>) -> Option<FlowRender> {
        self.flow_fallback(&[("fallback".to_string(), a)])
    }

    fn island_loader(
        &mut self,
        name: &JSXElementName<'_>,
        callee: ExprRef,
        tag: &str,
    ) -> IslandLoader {
        let root = match name {
            JSXElementName::IdentifierReference(id) => id.as_ref(),
            JSXElementName::MemberExpression(member) => match root_object(&member.object) {
                Some(root) => root,
                None => return IslandLoader::Direct,
            },
            _ => return IslandLoader::Direct,
        };
        let Some(reference) = root.reference_id.get() else { return IslandLoader::Direct };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else {
            return IslandLoader::Direct;
        };
        let (specifier, base) = match self.islands.decide(symbol) {
            IslandDecision::Split(entry) => (entry.specifier.clone(), entry.base.clone()),
            IslandDecision::Warn(reason) => {
                self.report(
                    Report::new(Code::IslandNotSplit, callee.span)
                        .arg("component", tag.to_string())
                        .arg("reason", reason.to_string()),
                );
                return IslandLoader::Direct;
            }
            IslandDecision::Direct => return IslandLoader::Direct,
        };
        let mut path = Vec::new();
        match &base {
            ImportBase::Named(name) => path.push(name.clone()),
            ImportBase::Default => path.push("default".to_string()),
            ImportBase::Namespace => {}
        }
        if let Some(members) = member_path(name) {
            path.extend(members);
        }
        IslandLoader::Split { specifier, path }
    }
}

enum Literal {
    Str(String),
    Bool(bool),
    Nullish,
}

#[derive(Default)]
struct ClassKeys {
    keys: Vec<(String, bool)>,
}

impl ClassKeys {
    fn set(&mut self, key: &str, is_on: bool) {
        match self.keys.iter_mut().find(|(candidate, _)| candidate == key) {
            Some(entry) => entry.1 = is_on,
            None => self.keys.push((key.to_string(), is_on)),
        }
    }

    fn attribute(&self) -> String {
        let mut tokens = Vec::new();
        for (key, _) in self.keys.iter().filter(|(_, is_on)| *is_on) {
            for token in key.split_whitespace() {
                if !tokens.contains(&token) {
                    tokens.push(token);
                }
            }
        }
        tokens.join(" ")
    }
}

fn is_island_attr(name: &str) -> bool {
    matches!(name, "island" | "islandMedia" | "islandRootMargin" | "islandFallback")
}
