//! Target-independent lowering of JSX and compiler rewrites (SPEC §6). Everything lives in the
//! arena; source text is referenced by span and copied only on emission.

use oxc_allocator::{Box, Vec};
use oxc_span::Span;

/// A slice of the source with sub-spans replaced by compiled holes.
pub struct Embed<'a> {
    pub span: Span,
    /// Sorted by start, disjoint.
    pub holes: Vec<'a, Hole<'a>>,
}

pub struct Hole<'a> {
    pub span: Span,
    pub kind: HoleKind<'a>,
}

pub enum HoleKind<'a> {
    Jsx(Jsx<'a>),
    AsyncComponent(Box<'a, AsyncComponent<'a>>),
    /// `[get, set] = signal(init)` → `get = init`.
    ConstSignalDecl {
        getter: Span,
        init: Embed<'a>,
    },
    /// `get()` → `get`.
    ConstSignalRead {
        getter: Span,
    },
}

pub enum Jsx<'a> {
    Template(Box<'a, Template<'a>>),
    Component(Box<'a, Component<'a>>),
    Fragment(Vec<'a, Child<'a>>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemoId(pub u32);

pub struct Template<'a> {
    pub html: &'a str,
    pub namespace: Namespace,
    /// Number of node ids handed out, the root included.
    pub node_count: u32,
    /// Declaration order: each walk starts at an already declared node.
    pub walks: Vec<'a, Walk>,
    pub ops: Vec<'a, Op<'a>>,
    pub binds: Vec<'a, Bind<'a>>,
    pub memo_count: u32,
}

pub struct Walk {
    pub node: NodeId,
    pub from: From,
    pub next_siblings: u32,
}

#[derive(Clone, Copy)]
pub enum From {
    FirstChildOf(NodeId),
    Node(NodeId),
}

pub enum Op<'a> {
    Set { node: NodeId, target: Target<'a>, value: Value<'a> },
    Event { node: NodeId, event: &'a str, handler: Handler<'a> },
    Ref { node: NodeId, target: RefTarget<'a> },
    Spread { node: NodeId, props: Props<'a>, is_svg: bool, has_children: bool },
    Memo { id: MemoId, test: Embed<'a> },
    Insert { parent: NodeId, value: Child<'a>, anchor: Anchor },
}

#[derive(Clone, Copy)]
pub enum Anchor {
    Only,
    Before(NodeId),
    End,
}

pub struct Bind<'a> {
    pub node: NodeId,
    pub target: Target<'a>,
    pub value: Value<'a>,
}

#[derive(Clone, Copy)]
pub enum Target<'a> {
    Attr(&'a str),
    AttrNs(&'static str, &'a str),
    Bool(&'a str),
    Prop(&'a str),
    Class,
    Style,
}

impl Target<'_> {
    /// Setters that diff against the previous value they returned.
    pub fn threads_prev(self) -> bool {
        matches!(self, Target::Style)
    }
}

pub enum Value<'a> {
    True,
    Str(&'a str),
    Expr(Embed<'a>),
    Jsx(Jsx<'a>),
    /// Several class sources merged into one array, in source order.
    ClassParts(Vec<'a, Value<'a>>),
}

pub enum Handler<'a> {
    /// `el.$$click = handler`, plus `el.$$clickData = data` for `[handler, data]`.
    Delegated { handler: Embed<'a>, data: Option<Embed<'a>> },
    /// `addEventListener(el, name, handler, true)`: delegated, handler shape unknown.
    DelegatedDynamic(Embed<'a>),
    /// `addEventListener(el, name, handler)`.
    Direct(Embed<'a>),
}

pub enum Child<'a> {
    Text(&'a str),
    Jsx(Jsx<'a>),
    Expr(ExprChild<'a>),
}

pub enum ExprChild<'a> {
    Static(Embed<'a>),
    Getter(Getter<'a>),
    /// `memo(getter)`: a dynamic entry of a children array.
    Memo(Getter<'a>),
    Conditional(Box<'a, Conditional<'a>>),
}

pub enum Getter<'a> {
    /// A bare `f()`: passes `f`.
    Call(Span),
    /// `() => body`; `parenthesize` when the body would parse as a block.
    Thunk { body: Embed<'a>, parenthesize: bool },
}

/// `() => _c$() ? consequent : alternate`, `_c$` declared by the template's `Op::Memo`.
pub struct Conditional<'a> {
    pub memo: MemoId,
    pub consequent: Embed<'a>,
    pub alternate: Option<Embed<'a>>,
}

pub struct Component<'a> {
    pub callee: Span,
    pub props: Props<'a>,
}

pub struct Props<'a> {
    pub parts: Vec<'a, PropsPart<'a>>,
}

pub enum PropsPart<'a> {
    Object(Vec<'a, Prop<'a>>),
    Spread { value: Embed<'a>, is_dynamic: bool },
}

pub enum Prop<'a> {
    Value {
        key: &'a str,
        value: PropValue<'a>,
    },
    Getter {
        key: &'a str,
        value: PropValue<'a>,
    },
    /// `ref(r$) { … }`: assigns the element the component renders, or calls it if a function.
    ForwardRef(AssignTarget<'a>),
}

pub enum PropValue<'a> {
    True,
    Str(&'a str),
    Expr(Embed<'a>),
    Jsx(Jsx<'a>),
    Children(Vec<'a, Child<'a>>),
}

pub enum RefTarget<'a> {
    /// `ref={(el) => …}`.
    Callback(Embed<'a>),
    /// Called when it holds a function, assigned otherwise.
    Assign(AssignTarget<'a>),
    /// Called when it evaluates to a function.
    Expr(Embed<'a>),
}

pub enum AssignTarget<'a> {
    Identifier(Span),
    /// Object and key are evaluated once.
    Member {
        object: Embed<'a>,
        key: MemberKey<'a>,
    },
}

pub enum MemberKey<'a> {
    /// `.name` or `.#name`, as written.
    Static(&'a str),
    Computed(Embed<'a>),
}

pub struct AsyncComponent<'a> {
    pub head: AsyncHead,
    pub params: Embed<'a>,
    pub return_type: ReturnType,
    pub steps: Vec<'a, AsyncStep<'a>>,
    /// Statements between the last `await` and the `return`.
    pub tail: Vec<'a, Embed<'a>>,
    pub result: Embed<'a>,
}

pub enum AsyncHead {
    Function { id: Option<Span>, type_parameters: Option<Span> },
    Arrow { type_parameters: Option<Span> },
}

pub enum ReturnType {
    None,
    /// `Promise<T>` unwrapped: the span of `T`.
    Unwrapped(Span),
    /// The whole annotation, `: …` included.
    Verbatim(Span),
}

pub struct AsyncStep<'a> {
    /// Statements between the previous `await` and this one.
    pub before: Vec<'a, Embed<'a>>,
    pub kind: &'static str,
    pub pattern: Span,
    pub annotation: Option<Span>,
    pub argument: Embed<'a>,
}
