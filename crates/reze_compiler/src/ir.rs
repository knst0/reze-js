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
    Script(ScriptEdit<'a>),
}

/// Rewrites of non-JSX code.
pub enum ScriptEdit<'a> {
    /// O3: `[get, set] = signal(init)` → `get = init`.
    ConstSignalDecl {
        getter: Span,
        init: Embed<'a>,
    },
    /// O3: `get()` → `get`.
    ConstSignalRead {
        getter: Span,
    },
    /// A destructured props parameter → `name`.
    PropsParam {
        name: &'a str,
    },
    /// A rewritten binding → `props.k₁…kₙ`, or `(p === undefined ? fallback : p)` with a
    /// default; `key: ` first when `shorthand`.
    PropsRead {
        props: &'a str,
        path: Vec<'a, &'a str>,
        fallback: Option<PropsFallback<'a>>,
        shorthand: bool,
    },
    /// The declarations rewritten props start the body with: the rest's `splitProps`, then each
    /// hoisted default; an expression body becomes `{ …; return (body); }`.
    PropsEntry {
        props: &'a str,
        rest: Option<PropsSplit<'a>>,
        defaults: Vec<'a, PropsTemporary<'a>>,
        body: Option<Embed<'a>>,
    },
    /// `S() === key` in a `<For>` row → `selector(key)`; `!==` → `!selector(key)`.
    SelectorRead {
        selector: &'a str,
        key: Embed<'a>,
        is_negated: bool,
    },
    /// Text at an empty span.
    Insert(&'a str),
    Hot(HotEdit<'a>),
}

pub enum PropsFallback<'a> {
    /// A literal default, repeated at each read.
    Literal(Embed<'a>),
    /// The temporary holding a default evaluated once at the component's start.
    Temporary(&'a str),
}

pub struct PropsSplit<'a> {
    pub binding: Span,
    pub keys: Vec<'a, &'a str>,
}

pub struct PropsTemporary<'a> {
    pub name: &'a str,
    pub value: Embed<'a>,
}

/// Component hot-swap registration.
pub enum HotEdit<'a> {
    /// `name = hotComponent(import.meta.hot, "<file>#name", name);` after a declaration.
    AfterDeclaration { name: &'a str },
    /// `hotComponent(import.meta.hot, "<file>#name", init)`.
    WrapInit { name: &'a str, init: Embed<'a> },
    /// `if (import.meta.hot) import.meta.hot.accept();` at the end of the module.
    Accept,
}

pub enum Jsx<'a> {
    Template(Box<'a, Template<'a>>),
    Component(Box<'a, Component<'a>>),
    Fragment(Vec<'a, Child<'a>>),
    Flow(Box<'a, Flow<'a>>),
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

/// Where a template with work is written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// An expression: `(() => { …; return el; })()`.
    Expression,
    /// Replacing a `return` statement or an arrow's expression body: `{ …; return el; }`.
    Block,
}

pub struct Template<'a> {
    pub html: &'a str,
    pub namespace: Namespace,
    pub node_count: u32,
    /// Declaration order: each walk starts at an already declared node.
    pub walks: Vec<'a, Walk>,
    /// Document order, except `<select value>`, which follows the select's inserts.
    pub ops: Vec<'a, Op<'a>>,
    /// Merged into one render effect.
    pub binds: Vec<'a, Bind<'a>>,
    pub placement: Placement,
}

impl Template<'_> {
    pub fn has_work(&self) -> bool {
        !self.walks.is_empty() || !self.ops.is_empty() || !self.binds.is_empty()
    }
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
    Set {
        node: NodeId,
        target: BindTarget<'a>,
        value: Value<'a>,
    },
    Event {
        node: NodeId,
        event: &'a str,
        handler: Handler<'a>,
    },
    Ref {
        node: NodeId,
        target: RefTarget<'a>,
    },
    Spread {
        node: NodeId,
        props: Props<'a>,
        is_svg: bool,
        has_children: bool,
    },
    /// A claimed `<a>`: `link(el)`, or `link(el, href)` when the href is reactive.
    Link {
        node: NodeId,
        href: Option<Getter<'a>>,
    },
    Insert {
        parent: NodeId,
        value: Child<'a>,
        anchor: Anchor,
    },
}

#[derive(Clone, Copy)]
pub enum Anchor {
    /// The parent's only child: no marker.
    Only,
    Before(NodeId),
    End,
}

pub struct Bind<'a> {
    pub node: NodeId,
    pub target: BindTarget<'a>,
    pub value: Value<'a>,
}

#[derive(Clone, Copy)]
pub enum BindTarget<'a> {
    Attr(&'a str),
    AttrNs(&'static str, &'a str),
    Bool(&'a str),
    Prop {
        name: &'a str,
    },
    Class,
    /// `toggleClass(el, token, value, prev)`.
    ClassToggle(&'a str),
    /// `text.data = value` of a text run.
    Text,
    Style,
}

impl BindTarget<'_> {
    /// Setters that return the state their next call compares against.
    pub fn threads_prev(self) -> bool {
        matches!(self, BindTarget::Style | BindTarget::ClassToggle(_))
    }

    /// Setters that write only when the value differs from the previous one they are given.
    pub fn compares_itself(self) -> bool {
        matches!(self, BindTarget::ClassToggle(_))
    }
}

pub enum Value<'a> {
    True,
    Str(&'a str),
    Expr(Embed<'a>),
    Jsx(Jsx<'a>),
    /// Several class sources merged into one array, in source order.
    ClassParts(Vec<'a, Value<'a>>),
    /// `!!(expr)`: the state of a toggled class token.
    Truthy(Embed<'a>),
    /// The parts of a text run, concatenated.
    Text(Vec<'a, TextPart<'a>>),
}

pub enum TextPart<'a> {
    Static(&'a str),
    Dynamic(Embed<'a>),
}

pub enum Handler<'a> {
    /// `el.$$click = handler`, plus `el.$$clickData = data` for `[handler, data]`.
    Delegated { handler: Embed<'a>, data: Option<Embed<'a>> },
    /// `addEventListener(el, name, handler, true)`.
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
    /// A `<Show>` inserted into a native element: `var c = computed(() => !!(test))`, then
    /// `() => c() ? consequent : alternate` (`c() && consequent` without an alternate).
    Conditional(Box<'a, Conditional<'a>>),
}

pub struct Conditional<'a> {
    pub test: Embed<'a>,
    /// `test` always yields a boolean and is written without `!!`.
    pub test_is_boolean: bool,
    pub consequent: Child<'a>,
    pub alternate: Option<Child<'a>>,
}

pub enum Getter<'a> {
    /// A bare `f()`: passes `f`.
    Call(Span),
    /// `() => body`; `parenthesize` when the body would parse as a block.
    Thunk { body: Embed<'a>, parenthesize: bool },
}

/// What a flow renders for a case.
pub enum Render<'a> {
    /// A function written by the user, passed as is.
    Function(Embed<'a>),
    /// `() => child`.
    Child(Child<'a>),
}

pub struct Branch<'a> {
    pub when: Embed<'a>,
    pub child: Render<'a>,
    pub fallback: Option<Render<'a>>,
}

pub enum Flow<'a> {
    Show(Branch<'a>),
    Switch {
        whens: Vec<'a, Embed<'a>>,
        children: Vec<'a, Render<'a>>,
        fallback: Option<Render<'a>>,
    },
    For {
        each: Embed<'a>,
        map: Embed<'a>,
        fallback: Option<Render<'a>>,
        key: Option<Embed<'a>>,
        selectors: Vec<'a, Selector<'a>>,
    },
}

/// A selector created once per `<For>` over the getter at `source`.
pub struct Selector<'a> {
    pub name: &'a str,
    pub source: Span,
}

pub struct Component<'a> {
    pub callee: Embed<'a>,
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
    /// `ref(r$) { … }`: calls or assigns the element the component renders.
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
    /// `.name` or `.#name`.
    Static(&'a str),
    Computed(Embed<'a>),
}
