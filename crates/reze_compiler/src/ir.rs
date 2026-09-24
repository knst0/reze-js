//! Target-independent lowering of JSX and compiler rewrites (SPEC §6). Everything lives in the
//! arena; source text is referenced by span and copied only on emission.

use oxc_allocator::{Box, Vec};
use oxc_span::Span;

use crate::facts::IslandMode;

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
    /// `[state, setState] = store(init)` → `[s$a, set$a] = signal(<a>), [s$b] = signal(<b>)`.
    StoreDecl {
        leaves: Vec<'a, StoreLeaf<'a>>,
    },
    /// `export const [state, setState] = store(init);` → the declaration, then
    /// `export { s$a as state$a, … };` when `specifiers` is not empty.
    StoreExport {
        declaration: Embed<'a>,
        specifiers: Vec<'a, Specifier<'a>>,
    },
    /// `state.a.b` or draft `d.a.b` → `s$a$b()`.
    StoreRead {
        getter: &'a str,
    },
    /// `state.a[i].p`, `state.a.length` → `s$a()[i].p`, `s$a().length` (§16.6).
    ArrayRead {
        getter: &'a str,
        suffix: Vec<'a, ArraySuffix<'a>>,
    },
    /// `setState((d) => E)` → `void untrack(() => E')`; `body` is the expression or the block.
    StoreSet {
        body: Embed<'a>,
    },
    /// A draft write → `set$p(…)`.
    StoreWrite {
        setter: &'a str,
        write: StoreWriteKind<'a>,
    },
    /// An indexed or method draft write, or a literal write to a form path (§16.6).
    ArrayWrite {
        setter: &'a str,
        write: ArrayWriteKind<'a>,
    },
    /// The named specifiers of an import or export declaration, joined with `, `.
    Specifiers {
        specifiers: Vec<'a, Specifier<'a>>,
    },
    /// `d()` of an inlined computed → `(expr)` (O4).
    ComputedInline {
        body: Embed<'a>,
    },
    /// `d()` of a computed another module declares → `(body)`, the text of its expression with
    /// each reference renamed to a binding of this module (SPEC §16.5).
    ImportedComputed {
        body: &'a str,
        names: Vec<'a, BodyName<'a>>,
    },
    /// Source dropped without replacement: the declaration of an inlined computed (O4).
    Remove,
    /// A read of a folded prop → `(literal)`, `key: (literal)` for a shorthand property (§15.16).
    FoldedProp {
        source: &'a str,
        shorthand_key: Option<&'a str>,
    },
    /// `S() === key` in a `<For>` row → `selector(key)`, `!==` → `!selector(key)` (O6).
    /// The server keeps `original`.
    SelectorRead {
        selector: &'a str,
        key: Embed<'a>,
        original: Embed<'a>,
        is_negated: bool,
    },
    /// `renderToString(() => <R/>)` / `hydrate(() => <R/>, el)` of a static root (SPEC §15.9):
    /// server `renderToString(code, true)`, hydrate `hydrateIslands(el, { id: E, … })`, client
    /// as written.
    IslandRoot {
        kind: RootKind,
        callee: Span,
        code: Embed<'a>,
        element: Option<Embed<'a>>,
        islands: Vec<'a, IslandImport<'a>>,
    },
    /// A destructured props parameter → `name` (SPEC §15.7).
    PropsParam {
        name: &'a str,
    },
    /// A rewritten binding → `props.k₁…kₙ`, `(p === undefined ? fallback : p)` with a default,
    /// prefixed with `key: ` when `shorthand`.
    PropsRead {
        props: &'a str,
        path: Vec<'a, &'a str>,
        fallback: Option<PropsFallback<'a>>,
        shorthand: bool,
    },
    /// The statements rewritten props start the body with: the rest
    /// `const binding = splitProps(props, [keys])[1];`, then `const name = value;` per hoisted
    /// default, wrapping `body` as `{ …; return (body); }` for an expression-bodied arrow.
    PropsEntry {
        props: &'a str,
        rest: Option<PropsSplit<'a>>,
        defaults: Vec<'a, PropsTemporary<'a>>,
        body: Option<Embed<'a>>,
    },
}

pub enum PropsFallback<'a> {
    /// A literal default (SPEC §15.7), repeated at each read.
    Literal(Embed<'a>),
    /// The temporary holding a default evaluated once at the component's start (SPEC §16.7).
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

pub struct StoreLeaf<'a> {
    pub getter: &'a str,
    /// Declared only when the leaf is written.
    pub setter: Option<&'a str>,
    pub value: Embed<'a>,
}

pub enum StoreWriteKind<'a> {
    /// `() => value`, parenthesized when `value` starts with `{`.
    Assign { value: Embed<'a>, parenthesize: bool },
    /// `(v) => v op value`, with `value` parenthesized when it binds looser than `op`.
    Compound { parameter: &'a str, operator: &'static str, value: Embed<'a>, parenthesize: bool },
    /// `(v) => ++v` or `(v) => --v`.
    Update { parameter: &'a str, operator: &'static str },
}

pub enum ArraySuffix<'a> {
    /// `.p` or `["p"]`.
    Key(&'a str),
    /// `[e]`.
    Index(Embed<'a>),
}

pub enum ArrayWriteKind<'a> {
    /// `d.a[i] = e` → `set$a((v) => { const c = v.slice(); c[i] = e; return c; })`;
    /// with a tail, the element is replaced by a clone chain and the index hoisted:
    /// `d.a[i].p = e` → `set$a((v) => { const c = v.slice(); const t = i; c[t] = { ...c[t], p: e }; return c; })`.
    Index { index: Embed<'a>, tail: Vec<'a, &'a str>, op: IndexOp<'a>, temp: Option<&'a str> },
    /// `d.a.push(e)` → `set$a((v) => { const c = v.slice(); c.push(e); return c; })`.
    Method { name: &'a str, args: Vec<'a, Embed<'a>> },
    /// `d.a.b = {…}` → `{ const _t0 = v0; …; set$l0(() => _t0); … }`.
    Form { temps: Vec<'a, FormTemp<'a>>, sets: Vec<'a, FormSet<'a>> },
}

pub enum IndexOp<'a> {
    /// `c[i].p = e`.
    Assign { value: Embed<'a> },
    /// `c[i].p op= e`.
    Compound { operator: &'static str, value: Embed<'a> },
    /// `++c[i].p` or `--c[i].p`.
    Update { operator: &'static str },
}

pub struct FormTemp<'a> {
    pub name: &'a str,
    pub value: Embed<'a>,
}

pub struct FormSet<'a> {
    pub setter: &'a str,
    pub temp: &'a str,
}

pub enum Specifier<'a> {
    /// Kept as written.
    Source(Span),
    /// `name as alias`, or `name` when both are equal.
    Alias { name: &'a str, alias: &'a str },
}

/// `name` in place of `body[start..end]`.
pub struct BodyName<'a> {
    pub start: u32,
    pub end: u32,
    pub name: &'a str,
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
    /// Where each node sits in `html`, indexed by `NodeId`; the root is an element.
    pub nodes: Vec<'a, TemplateNode<'a>>,
    /// Declaration order: each walk starts at an already declared node.
    pub walks: Vec<'a, Walk>,
    /// Document order, except `<select value>`, which follows the select's inserts.
    pub ops: Vec<'a, Op<'a>>,
    pub binds: Vec<'a, Bind<'a>>,
    pub memo_count: u32,
}

impl<'a> Template<'a> {
    pub fn root_tag(&self) -> &'a str {
        match self.nodes[NodeId::ROOT.index()] {
            TemplateNode::Element { tag, .. } => tag,
            TemplateNode::Leaf { .. } => unreachable!("a template root is an element"),
        }
    }
}

/// Byte offsets into `Template::html`.
#[derive(Clone, Copy)]
pub enum TemplateNode<'a> {
    Element {
        tag: &'a str,
        start: u32,
        /// Just before the `>` of the opening tag: where attributes go.
        attributes_end: u32,
        /// Just before the closing tag (after `>` for void elements): where appended content goes.
        content_end: u32,
    },
    /// A text run or a `<!>` marker.
    Leaf { start: u32 },
}

impl TemplateNode<'_> {
    pub fn start(self) -> u32 {
        match self {
            TemplateNode::Element { start, .. } | TemplateNode::Leaf { start } => start,
        }
    }

    /// A leaf has no attributes: its start.
    pub fn attributes_end(self) -> u32 {
        match self {
            TemplateNode::Element { attributes_end, .. } => attributes_end,
            TemplateNode::Leaf { start } => start,
        }
    }

    /// A leaf has no content: its start.
    pub fn content_end(self) -> u32 {
        match self {
            TemplateNode::Element { content_end, .. } => content_end,
            TemplateNode::Leaf { start } => start,
        }
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
    Memo {
        id: MemoId,
        test: Embed<'a>,
    },
    /// The server's part of a class compiled to toggles (SPEC §7.3): `ssrClassTokens(toggles)`
    /// at `inside`, the end of the static `class` value in the template, or `ssrClass(toggles)`
    /// after the attributes when the template has no static `class`. The client ignores it.
    ServerClass {
        node: NodeId,
        toggles: Value<'a>,
        inside: Option<u32>,
    },
    /// `inserts_after`: later inserts of the same parent that share `anchor`.
    Insert {
        parent: NodeId,
        value: Child<'a>,
        anchor: Anchor,
        inserts_after: u32,
    },
}

#[derive(Clone, Copy)]
pub enum Anchor {
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
        html: PropHtml,
    },
    Class,
    /// `toggleClass(el, token, value, prev)` (SPEC §7.3); the server renders `Op::ServerClass`.
    ClassToggle(&'a str),
    /// `text.data = value` of a text run (SPEC §7.5). `placeholder` is where the template holds
    /// the one-space stand-in of a run without static text, which the server leaves out.
    Text {
        placeholder: Option<u32>,
    },
    Style,
}

impl BindTarget<'_> {
    /// Setters that diff against the previous value they returned.
    pub fn threads_prev(self) -> bool {
        matches!(self, BindTarget::Style | BindTarget::ClassToggle(_))
    }
}

/// How a property shows in server-rendered HTML.
#[derive(Clone, Copy)]
pub enum PropHtml {
    /// Client-only (`prop:x`, `<select value>`).
    None,
    /// `value`: the attribute holds the initial value.
    Attr,
    /// `checked`, `selected`.
    Bool,
    /// `textContent`, `<textarea value>`: escaped content.
    Text,
    /// `innerHTML`: raw content.
    Html,
}

pub enum Value<'a> {
    True,
    Str(&'a str),
    Expr(Embed<'a>),
    Jsx(Jsx<'a>),
    /// Several class sources merged into one array, in source order.
    ClassParts(Vec<'a, Value<'a>>),
    /// `!!(expr)`: the state of one toggled class token.
    Truthy(Embed<'a>),
    /// `{ "token": expr, … }`: the toggled class tokens the server renders.
    ClassToggles(Vec<'a, (&'a str, Embed<'a>)>),
    /// The parts of a text run, concatenated in order (SPEC §7.5).
    Text(Vec<'a, TextPart<'a>>),
}

pub enum TextPart<'a> {
    Static(&'a str),
    /// `at`: where the server renders the value in the template's HTML.
    Dynamic {
        value: Embed<'a>,
        at: u32,
    },
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
    pub callee: Embed<'a>,
    pub props: Props<'a>,
    /// Set on a boundary position (SPEC §15.9): the server renders `ssrIsland`.
    pub island: Option<Island<'a>>,
    /// Selectors the rows of a `<For>` read (O6): created once per `<For>`, before it.
    pub selectors: Vec<'a, SelectorSource<'a>>,
}

/// `const selector = selector(source)`, where `source` is the span of a getter identifier.
pub struct SelectorSource<'a> {
    pub selector: &'a str,
    pub source: Span,
}

pub struct Island<'a> {
    pub id: &'a str,
    pub mode: IslandMode,
    /// Keys of `props` the server passes as slots, not as serialized props (§16.4).
    pub slots: Vec<'a, &'a str>,
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
    /// The entry statements of rewritten props (`PropsEntry`), first in the body.
    pub props_entry: Option<Embed<'a>>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    RenderToString,
    Hydrate,
}

/// An island of a hydrate root: `import { export as <alias> } from "specifier"` when eager,
/// otherwise `lazyIsland(() => import("specifier"), mode, export)` (§16.3).
pub struct IslandImport<'a> {
    pub id: &'a str,
    pub specifier: &'a str,
    pub export: &'a str,
    pub mode: IslandMode,
}
