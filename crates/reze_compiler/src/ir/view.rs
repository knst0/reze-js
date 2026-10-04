use oxc_ast::ast::Expression;
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::{GetNodeId, NodeId};
use oxc_syntax::symbol::SymbolId;

use super::schedule::{Schedule, ValueMode};
use super::sites::SiteId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ExprRef {
    pub id: NodeId,
    pub span: Span,
}

impl ExprRef {
    pub fn of(e: &Expression<'_>) -> Self {
        Self { id: e.node_id(), span: e.span() }
    }

    pub fn at(node: &impl GetNodeId, span: Span) -> Self {
        Self { id: node.node_id(), span }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ViewId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaticKind {
    Unknown,
    Numeric,
    String,
    Boolean,
}

pub struct View {
    pub kind: ViewKind,
    pub site: Option<SiteId>,
    pub origin: Span,
}

#[derive(Clone, Copy)]
pub enum ViewReference {
    Expression(ExprRef),
    View(ViewId),
}

impl View {
    pub fn for_each_reference(&self, client_work: bool, visit: &mut impl FnMut(ViewReference)) {
        match &self.kind {
            ViewKind::Element(element) => {
                for prop in &element.props {
                    match prop {
                        ElementProp::Attr(attr) => attr_value_exprs(&attr.value, visit),
                        ElementProp::Event(handler) if client_work => {
                            visit(ViewReference::Expression(handler.handler));
                            if let Some(data) = handler.data {
                                visit(ViewReference::Expression(data));
                            }
                        }
                        ElementProp::Ref(target) if client_work => ref_target_exprs(&target.target, visit),
                        ElementProp::Event(_) | ElementProp::Ref(_) => {}
                        ElementProp::Spread(spread) => {
                            for part in &spread.parts {
                                match part {
                                    SpreadPart::Entries(entries) => {
                                        for entry in entries {
                                            attr_value_exprs(&entry.value, visit);
                                        }
                                    }
                                    SpreadPart::Generic { expr, .. } => visit(ViewReference::Expression(*expr)),
                                }
                            }
                        }
                    }
                }
                for insert in &element.inserts {
                    child_exprs(&insert.value, visit);
                }
                for late in &element.late_values {
                    attr_value_exprs(&late.value, visit);
                }
                for link in &element.links {
                    if let Some(href) = &link.href {
                        visit(ViewReference::Expression(href.expr));
                    }
                }
            }
            ViewKind::Component(component) => {
                if component.island.as_ref().is_none_or(|island| matches!(&island.loader, IslandLoader::Direct)) {
                    visit(ViewReference::Expression(component.callee));
                }
                for segment in &component.props {
                    match segment {
                        ComponentSegment::Object(entries) => {
                            for entry in entries {
                                match entry {
                                    ComponentProp::Value { value, .. } | ComponentProp::Getter { value, .. } => {
                                        component_value_exprs(value, visit);
                                    }
                                    ComponentProp::ForwardRef(target) => assign_target_exprs(target, visit),
                                }
                            }
                        }
                        ComponentSegment::Spread { expr, .. } => visit(ViewReference::Expression(*expr)),
                    }
                }
                if let Some(island) = &component.island
                    && let Some(fallback) = &island.fallback
                {
                    flow_render_exprs(fallback, visit);
                }
            }
            ViewKind::Fragment(children) => {
                for child in children {
                    child_exprs(child, visit);
                }
            }
            ViewKind::Flow(flow) => flow_exprs(flow, visit),
        }
    }
}

fn attr_value_exprs(value: &AttrValue, visit: &mut impl FnMut(ViewReference)) {
    match value {
        AttrValue::True | AttrValue::Str(_) => {}
        AttrValue::View(view) => visit(ViewReference::View(*view)),
        AttrValue::Expr(expr) | AttrValue::Truthy(expr) => visit(ViewReference::Expression(*expr)),
        AttrValue::Dynamic(dynamic) => visit(ViewReference::Expression(dynamic.expr)),
        AttrValue::ClassParts(parts) => {
            for part in parts {
                attr_value_exprs(part, visit);
            }
        }
        AttrValue::TextParts(parts) => text_part_exprs(parts, visit),
    }
}

fn text_part_exprs(parts: &[TextPart], visit: &mut impl FnMut(ViewReference)) {
    for part in parts {
        if let TextPart::Dynamic(dynamic) = part {
            visit(ViewReference::Expression(dynamic.expr));
        }
    }
}

fn ref_target_exprs(target: &RefOp, visit: &mut impl FnMut(ViewReference)) {
    match target {
        RefOp::Callback(expr) | RefOp::Expr(expr) => visit(ViewReference::Expression(*expr)),
        RefOp::Assign(target) => assign_target_exprs(target, visit),
    }
}

fn assign_target_exprs(target: &AssignTarget, visit: &mut impl FnMut(ViewReference)) {
    if let AssignTarget::Member { object, key } = target {
        visit(ViewReference::Expression(*object));
        if let MemberKey::Computed(expr) = key {
            visit(ViewReference::Expression(*expr));
        }
    }
}

fn child_exprs(child: &Child, visit: &mut impl FnMut(ViewReference)) {
    match child {
        Child::StaticText(_) => {}
        Child::View(view) => visit(ViewReference::View(*view)),
        Child::Dynamic(dynamic) => visit(ViewReference::Expression(dynamic.expr)),
        Child::Conditional(conditional) => {
            visit(ViewReference::Expression(conditional.test));
            child_exprs(&conditional.consequent, visit);
            if let Some(alternate) = &conditional.alternate {
                child_exprs(alternate, visit);
            }
        }
    }
}

fn component_value_exprs(value: &ComponentValue, visit: &mut impl FnMut(ViewReference)) {
    match value {
        ComponentValue::True | ComponentValue::Str(_) => {}
        ComponentValue::Nested(view) => visit(ViewReference::View(*view)),
        ComponentValue::Dynamic(dynamic) => visit(ViewReference::Expression(dynamic.expr)),
        ComponentValue::Children(children) => {
            for child in children {
                child_exprs(child, visit);
            }
        }
    }
}

fn flow_render_exprs(render: &FlowRender, visit: &mut impl FnMut(ViewReference)) {
    match render {
        FlowRender::Function(expr) => visit(ViewReference::Expression(*expr)),
        FlowRender::Child(child) => child_exprs(child, visit),
    }
}

fn flow_exprs(flow: &FlowView, visit: &mut impl FnMut(ViewReference)) {
    match flow {
        FlowView::Show(branch) => {
            visit(ViewReference::Expression(branch.when.expr));
            flow_render_exprs(&branch.child, visit);
            if let Some(fallback) = &branch.fallback {
                flow_render_exprs(fallback, visit);
            }
        }
        FlowView::Switch { whens, children, fallback } => {
            for when in whens {
                visit(ViewReference::Expression(when.expr));
            }
            for child in children {
                flow_render_exprs(child, visit);
            }
            if let Some(fallback) = fallback {
                flow_render_exprs(fallback, visit);
            }
        }
        FlowView::For { each, map, fallback, key } => {
            visit(ViewReference::Expression(each.expr));
            visit(ViewReference::Expression(*map));
            if let Some(fallback) = fallback {
                flow_render_exprs(fallback, visit);
            }
            match key {
                Some(FlowKeyValue::Expr(expr)) => visit(ViewReference::Expression(*expr)),
                Some(FlowKeyValue::View(view)) => visit(ViewReference::View(*view)),
                Some(FlowKeyValue::Str(_)) | None => {}
            }
        }
        FlowView::Repeat { count, map, fallback } => {
            visit(ViewReference::Expression(count.expr));
            visit(ViewReference::Expression(*map));
            if let Some(fallback) = fallback {
                flow_render_exprs(fallback, visit);
            }
        }
        FlowView::Rows { map, .. } => visit(ViewReference::Expression(*map)),
        FlowView::Loading { child, fallback } | FlowView::Errored { child, fallback } => {
            flow_render_exprs(child, visit);
            if let Some(fallback) = fallback {
                flow_render_exprs(fallback, visit);
            }
        }
        FlowView::Portal { child, mount } => {
            flow_render_exprs(child, visit);
            if let Some(mount) = mount {
                visit(ViewReference::Expression(mount.expr));
            }
        }
    }
}

pub enum ViewKind {
    Element(ElementView),
    Component(ComponentView),
    Fragment(Vec<Child>),
    Flow(FlowView),
}

pub struct StaticTree {
    pub nodes: Vec<StaticNode>,
}

pub struct StaticNode {
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub kind: StaticNodeKind,
    pub referenced: bool,
    pub ns: Namespace,
    pub tag: String,
    pub text: String,
    pub attrs: Vec<StaticAttr>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaticNodeKind {
    Element,
    Text,
    Marker,
}

pub struct StaticAttr {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    Only,
    Before(u32),
    End,
}

pub struct ElementView {
    pub namespace: Namespace,
    pub tag: String,
    pub statics: StaticTree,
    pub props: Vec<ElementProp>,
    pub inserts: Vec<InsertOp>,
    pub links: Vec<LinkProp>,
    pub late_values: Vec<LateProp>,
    pub schedule: Schedule,
}

pub enum ElementProp {
    Attr(Attr),
    Event(EventHandler),
    Ref(RefTarget),
    Spread(SpreadSegment),
}

pub struct Attr {
    pub node: u32,
    pub target: AttrTarget,
    pub value: AttrValue,
}

pub enum AttrTarget {
    Attr(String),
    AttrNs(&'static str, String),
    Bool(String),
    Prop(String),
    Class,
    ClassToggle(String),
    Text,
    Style,
}

pub enum AttrValue {
    True,
    Str(String),
    Expr(ExprRef),
    View(ViewId),
    Dynamic(Dynamic),
    ClassParts(Vec<AttrValue>),
    Truthy(ExprRef),
    TextParts(Vec<TextPart>),
}

pub enum TextPart {
    Static(String),
    Dynamic(Dynamic),
}

pub struct Dynamic {
    pub expr: ExprRef,
    pub mode: ValueMode,
    pub deps: Vec<SymbolId>,
    pub kind: StaticKind,
    pub getter: Option<GetterKind>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GetterKind {
    Call(ExprRef),
    Thunk { parenthesize: bool },
}

pub struct EventHandler {
    pub node: u32,
    pub name: String,
    pub kind: EventKind,
    pub handler: ExprRef,
    pub data: Option<ExprRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    DelegatedStatic,
    DelegatedDynamic,
    Direct,
}

pub struct RefTarget {
    pub node: u32,
    pub target: RefOp,
}

pub enum RefOp {
    Callback(ExprRef),
    Assign(AssignTarget),
    Expr(ExprRef),
}

pub enum AssignTarget {
    Identifier(String),
    Member { object: ExprRef, key: MemberKey },
}

pub enum MemberKey {
    Static(String),
    Computed(ExprRef),
}

pub struct SpreadSegment {
    pub node: u32,
    pub parts: Vec<SpreadPart>,
    pub closed: bool,
    pub is_svg: bool,
    pub has_children: bool,
}

pub enum SpreadPart {
    Entries(Vec<Attr>),
    Generic { expr: ExprRef, dynamic: bool },
}

pub struct InsertOp {
    pub slot: u32,
    pub parent: u32,
    pub value: Child,
    pub anchor: Anchor,
}

pub struct LateProp {
    pub node: u32,
    pub value: AttrValue,
}

pub struct LinkProp {
    pub node: u32,
    pub after_prop: usize,
    pub href: Option<Dynamic>,
}

pub enum Child {
    StaticText(String),
    Dynamic(Dynamic),
    View(ViewId),
    Conditional(ConditionalView),
}

pub struct ConditionalView {
    pub test: ExprRef,
    pub test_is_boolean: bool,
    pub consequent: Box<Child>,
    pub alternate: Option<Box<Child>>,
    pub origin: Span,
}

pub struct Branch {
    pub when: FlowSource,
    pub child: FlowRender,
    pub fallback: Option<FlowRender>,
}

pub struct FlowSource {
    pub expr: ExprRef,
    pub getter: Option<ExprRef>,
}

pub enum FlowRender {
    Function(ExprRef),
    Child(Box<Child>),
}

pub enum FlowView {
    Show(Branch),
    Switch { whens: Vec<FlowSource>, children: Vec<FlowRender>, fallback: Option<FlowRender> },
    For { each: FlowSource, map: ExprRef, fallback: Option<FlowRender>, key: Option<FlowKeyValue> },
    Repeat { count: FlowSource, map: ExprRef, fallback: Option<FlowRender> },
    Rows { times: u32, map: ExprRef },
    Loading { child: FlowRender, fallback: Option<FlowRender> },
    Errored { child: FlowRender, fallback: Option<FlowRender> },
    Portal { child: FlowRender, mount: Option<FlowSource> },
}

pub enum FlowKeyValue {
    Expr(ExprRef),
    Str(String),
    View(ViewId),
}

pub struct ComponentView {
    pub callee: ExprRef,
    pub props: Vec<ComponentSegment>,
    pub island: Option<IslandView>,
}

pub enum ComponentSegment {
    Object(Vec<ComponentProp>),
    Spread { expr: ExprRef, is_dynamic: bool },
}

pub enum ComponentProp {
    Value { key: String, value: ComponentValue },
    Getter { key: String, value: ComponentValue },
    ForwardRef(AssignTarget),
}

pub enum ComponentValue {
    True,
    Str(String),
    Dynamic(Dynamic),
    Nested(ViewId),
    Children(Vec<Child>),
}

pub struct IslandView {
    pub trigger: IslandTrigger,
    pub media: Option<String>,
    pub root_margin: Option<String>,
    pub loader: IslandLoader,
    pub fallback: Option<FlowRender>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IslandTrigger {
    Eager,
    Idle,
    Visible,
    Media,
    Interaction,
}

impl IslandTrigger {
    pub fn name(self) -> &'static str {
        match self {
            IslandTrigger::Eager => "eager",
            IslandTrigger::Idle => "idle",
            IslandTrigger::Visible => "visible",
            IslandTrigger::Media => "media",
            IslandTrigger::Interaction => "interaction",
        }
    }
}

pub enum IslandLoader {
    Direct,
    Split { specifier: String, path: Vec<String> },
}
