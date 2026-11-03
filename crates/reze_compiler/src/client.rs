//! Client code: handler sites, the bindings their writes reach, and the scopes that carry captured
//! values from the server render to the browser.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use oxc_ast::ast::{
    Argument, ArrowFunctionBody, ArrowFunctionExpression, AssignmentTarget, BinaryOperator,
    BindingPattern, CallExpression, ChainElement, Class, Declaration, Expression, Function,
    IdentifierReference, ImportDeclarationSpecifier, LogicalOperator, ObjectProperty,
    SimpleAssignmentTarget, Statement, UnaryOperator, UpdateExpression, VariableDeclarationKind,
    VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::{Analysis, Api, EndpointKind, Export, ImportSource, field_type, restore};
use crate::diagnostic::{Code, Edit, Report};
use crate::emit::template_escape;
use crate::html;
use crate::project::Module;
use crate::routes::RouteTable;
use crate::view::{
    AttributeItem, Branch, CallTarget, Callback, ComponentRef, Content, Element, Node, PropValue,
    Value,
};

/// The module compiled client code imports its helpers from.
pub const CLIENT_RUNTIME: &str = "reze-js/runtime/client";

type Sym = (usize, SymbolId);

/// A `data-rz` attribute: a handler site, or an anchor that bindings are located from.
pub struct Mark {
    pub scope: usize,
    pub index: usize,
    /// `data-rz-prevent` tokens.
    pub prevent: Option<String>,
}

pub struct ComponentScope {
    pub id: usize,
    pub events: String,
    /// Server expressions whose values the state entry carries, in entry order.
    pub captures: Vec<String>,
    pub eager: bool,
}

/// The scope of one callback call, such as a list row, whose values client code captures.
pub struct RowScope {
    pub scope: usize,
    pub parent: usize,
    pub captures: Vec<String>,
}

/// What the server render of one module adds for client code.
#[derive(Default)]
pub struct ModulePlan {
    /// By element span start.
    pub marks: HashMap<u32, Mark>,
    /// Text holes that are bindings, by span start.
    pub bindings: HashSet<u32>,
    /// A comment before the text run at `(container span start, node index)`.
    pub anchors: HashMap<(u32, usize), (usize, usize)>,
    /// By template span start.
    pub components: HashMap<u32, ComponentScope>,
    /// By callback content span start.
    pub rows: HashMap<u32, RowScope>,
    /// Ranges that client code updates, by [`range_key`]: `<!--[scope.mark-->` … `<!--]-->`.
    pub ranges: HashMap<u32, (usize, usize)>,
    /// Callback content span starts of rows that open with `<!--,-->`.
    pub separated: HashSet<u32>,
    pub client: Option<String>,
}

pub struct Plan {
    pub modules: Vec<ModulePlan>,
    /// Client chunk of each route node, by node index.
    pub chunks: BTreeMap<usize, String>,
    pub reports: Vec<Vec<Report>>,
}

pub fn scope_var(scope: usize) -> String {
    if scope == 0 { "$i".to_string() } else { format!("$j{scope}") }
}

pub fn base36(mut n: usize) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[n % 36]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn is_slot(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::Identifier(id) => id.name == "children",
        Expression::StaticMemberExpression(member) => member.property.name == "children",
        _ => false,
    }
}

/// The key of a control-flow node in [`ModulePlan::ranges`].
pub fn range_key(node: &Node<'_>) -> Option<u32> {
    match node {
        Node::Show(show) => Some(show.when.span().start),
        Node::Switch(switch) => match (switch.matches.first(), &switch.fallback) {
            (Some(first), _) => Some(first.when.span().start),
            (None, Some(Branch::Content(content))) => Some(content.span.start),
            (None, Some(Branch::Callback(callback))) => Some(callback.function.span.start),
            (None, None) => None,
        },
        Node::For(each) | Node::Repeat(each) => Some(each.source.span().start),
        _ => None,
    }
}

/// How a node takes part in a run of text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Static(usize),
    Server,
    Binding,
    /// Renders nothing in place: `<head>`, `<Portal>`.
    Nothing,
    Break,
}

pub fn piece(node: &Node<'_>, bindings: &HashSet<u32>) -> Piece {
    match node {
        Node::Text(text) => Piece::Static(text.encode_utf16().count()),
        Node::Expr(expression) if is_slot(expression) => Piece::Break,
        Node::Expr(expression) if bindings.contains(&expression.span().start) => Piece::Binding,
        Node::Expr(_) => Piece::Server,
        Node::Head(_) | Node::Portal(_) => Piece::Nothing,
        _ => Piece::Break,
    }
}

/// Whether a `<!>` precedes each piece: between two adjacent text holes, and where a binding and
/// a server hole meet in one run of text, so that every binding's text is measurable.
pub fn separators(pieces: &[Piece]) -> Vec<bool> {
    let mut out = Vec::with_capacity(pieces.len());
    let mut adjacent = false;
    let mut last: Option<Piece> = None;
    for &piece in pieces {
        let separate = match piece {
            Piece::Break => {
                adjacent = false;
                last = None;
                false
            }
            Piece::Static(_) => {
                adjacent = false;
                false
            }
            Piece::Nothing => false,
            Piece::Server | Piece::Binding => {
                let separate = adjacent || last.is_some_and(|kind| kind != piece);
                adjacent = true;
                last = Some(piece);
                separate
            }
        };
        out.push(separate);
    }
    out
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Signal(Option<&'a Expression<'a>>),
    Store(Option<&'a Expression<'a>>),
    Computed(Option<&'a Expression<'a>>),
    Action(Option<&'a Expression<'a>>),
    Declarator(&'a VariableDeclarator<'a>, VariableDeclarationKind),
    Function(&'a Function<'a>),
    Class(&'a Class<'a>),
}

#[derive(Clone, Copy)]
struct Decl<'a> {
    kind: Kind<'a>,
    exported: bool,
    span: Span,
}

impl Decl<'_> {
    fn is_state(&self) -> bool {
        matches!(self.kind, Kind::Signal(_) | Kind::Store(_) | Kind::Computed(_))
    }
}

fn first_argument<'a>(call: &'a CallExpression<'a>) -> Option<&'a Expression<'a>> {
    call.arguments.first().and_then(Argument::as_expression)
}

fn is_constant(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::UnaryExpression(u) => {
            matches!(u.operator, UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus)
                && matches!(u.argument.without_parentheses(), Expression::NumericLiteral(_))
        }
        Expression::ArrayExpression(array) => {
            array.elements.iter().all(|element| element.as_expression().is_some_and(is_constant))
        }
        Expression::ObjectExpression(object) => object.properties.iter().all(|property| {
            matches!(property, oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p)
                if !p.computed && is_constant(&p.value))
        }),
        Expression::TSAsExpression(e) => is_constant(&e.expression),
        Expression::TSSatisfiesExpression(e) => is_constant(&e.expression),
        _ => false,
    }
}

fn is_function(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

struct Index<'m, 'a> {
    modules: &'m [Module<'a>],
    analysis: &'m Analysis<'a>,
    /// Module-level declarations.
    decls: Vec<HashMap<SymbolId, Decl<'a>>>,
    /// Component-level declarations that become code in the factory.
    locals: HashMap<ComponentRef, HashMap<SymbolId, Decl<'a>>>,
    reactive: HashSet<Sym>,
}

impl<'a> Index<'_, 'a> {
    fn symbol(&self, module: usize, reference: &IdentifierReference<'a>) -> Option<SymbolId> {
        let id = reference.reference_id.get()?;
        self.modules[module].semantic.scoping().get_reference(id).symbol_id()
    }

    /// The declaration a binding names, following imports of project modules.
    fn origin(&self, module: usize, symbol: SymbolId) -> Sym {
        let mut current = (module, symbol);
        for _ in 0..32 {
            let Some(import) = self.analysis.facts[current.0].imports.get(&current.1) else {
                break;
            };
            let ImportSource::Module(target) = import.source else { break };
            match self.exported(target, &import.name) {
                Some(next) => current = next,
                None => break,
            }
        }
        current
    }

    fn exported(&self, module: usize, name: &str) -> Option<Sym> {
        let mut module = module;
        let mut name = name.to_string();
        for _ in 0..32 {
            match self.analysis.facts[module].exports.get(&name)? {
                Export::Local(symbol) => return Some(self.origin(module, *symbol)),
                Export::Component(_) => return None,
                Export::Reexport { module: target, name: next } => {
                    module = *target;
                    name = next.clone();
                }
            }
        }
        None
    }

    fn decl(&self, sym: Sym, component: Option<ComponentRef>) -> Option<&Decl<'a>> {
        if let Some(component) = component
            && component.module == sym.0
            && let Some(decl) = self.locals.get(&component).and_then(|l| l.get(&sym.1))
        {
            return Some(decl);
        }
        self.decls[sym.0].get(&sym.1)
    }

    fn api(&self, module: usize, expression: &Expression<'a>) -> Option<Api> {
        let Expression::Identifier(reference) = expression.without_parentheses() else {
            return None;
        };
        self.analysis.facts[module].apis.get(&self.symbol(module, reference)?).copied()
    }

    fn component(&self, component: ComponentRef) -> &crate::analyze::Component<'a> {
        &self.analysis.facts[component.module].components[component.index]
    }
}

fn declare<'a>(
    index: &Index<'_, 'a>,
    module: usize,
    declaration: &'a Declaration<'a>,
    exported: bool,
    out: &mut HashMap<SymbolId, Decl<'a>>,
    local: bool,
) {
    match declaration {
        Declaration::VariableDeclaration(variables) => {
            for declarator in &variables.declarations {
                let init = declarator.init.as_ref();
                let call = match init.map(Expression::without_parentheses) {
                    Some(Expression::CallExpression(call)) => Some(&**call),
                    _ => None,
                };
                let api = call.and_then(|call| index.api(module, &call.callee));
                let kind = match (api, call) {
                    (Some(Api::Signal), Some(call)) => Kind::Signal(first_argument(call)),
                    (Some(Api::Store), Some(call)) => Kind::Store(first_argument(call)),
                    (Some(Api::Computed), Some(call)) => Kind::Computed(first_argument(call)),
                    (Some(Api::Action), Some(call)) => Kind::Action(first_argument(call)),
                    (Some(Api::UseLocation | Api::UseSearchParams | Api::UseLinkState), _) => {
                        Kind::Declarator(declarator, variables.kind)
                    }
                    _ if !local || init.is_some_and(|e| is_function(e) || is_constant(e)) => {
                        Kind::Declarator(declarator, variables.kind)
                    }
                    _ => continue,
                };
                let decl = Decl { kind, exported, span: declarator.span };
                let symbols: Vec<SymbolId> = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => {
                        id.symbol_id.get().into_iter().collect()
                    }
                    pattern if !local => pattern
                        .get_binding_identifiers()
                        .iter()
                        .filter_map(|id| id.symbol_id.get())
                        .collect(),
                    _ => Vec::new(),
                };
                for symbol in symbols {
                    out.insert(symbol, decl);
                }
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(symbol) = function.id.as_ref().and_then(|id| id.symbol_id.get()) {
                out.insert(
                    symbol,
                    Decl { kind: Kind::Function(function), exported, span: function.span },
                );
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(symbol) = class.id.as_ref().and_then(|id| id.symbol_id.get()) {
                out.insert(symbol, Decl { kind: Kind::Class(class), exported, span: class.span });
            }
        }
        _ => {}
    }
}

/// Every resolved reference under a node, as declarations.
struct Refs<'i, 'm, 'a> {
    index: &'i Index<'m, 'a>,
    module: usize,
    out: Vec<Sym>,
}

impl<'a> Visit<'a> for Refs<'_, '_, 'a> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(symbol) = self.index.symbol(self.module, it) {
            self.out.push(self.index.origin(self.module, symbol));
        }
    }
}

fn refs_of<'a>(
    index: &Index<'_, 'a>,
    module: usize,
    visit: impl FnOnce(&mut Refs<'_, '_, 'a>),
) -> Vec<Sym> {
    let mut refs = Refs { index, module, out: Vec::new() };
    visit(&mut refs);
    refs.out
}

/// Targets of assignments and updates under a node.
struct Writes<'i, 'm, 'a> {
    index: &'i Index<'m, 'a>,
    module: usize,
    out: Vec<(Sym, Span)>,
    /// Roots of member assignments and method calls, which only write stores.
    members: Vec<Sym>,
}

impl<'a> Writes<'_, '_, 'a> {
    fn target(&mut self, reference: &IdentifierReference<'a>) {
        if let Some(symbol) = self.index.symbol(self.module, reference) {
            self.out.push((self.index.origin(self.module, symbol), reference.span));
        }
    }
}

impl<'a> Writes<'_, '_, 'a> {
    /// The root of `a.b[c]`, a mutation of a store.
    fn root(&mut self, mut expression: &Expression<'a>) {
        loop {
            match expression.without_parentheses() {
                Expression::Identifier(reference) => {
                    if let Some(symbol) = self.index.symbol(self.module, reference) {
                        self.members.push(self.index.origin(self.module, symbol));
                    }
                    return;
                }
                Expression::StaticMemberExpression(m) => expression = &m.object,
                Expression::ComputedMemberExpression(m) => expression = &m.object,
                _ => return,
            }
        }
    }
}

impl<'a> Visit<'a> for Writes<'_, '_, 'a> {
    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        match &it.left {
            AssignmentTarget::AssignmentTargetIdentifier(reference) => self.target(reference),
            AssignmentTarget::StaticMemberExpression(m) => self.root(&m.object),
            AssignmentTarget::ComputedMemberExpression(m) => self.root(&m.object),
            _ => {}
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::StaticMemberExpression(m) = it.callee.without_parentheses() {
            self.root(&m.object);
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(reference) = &it.argument {
            self.target(reference);
        }
        walk::walk_update_expression(self, it);
    }
}

/// Signals and stores that code under a node writes.
fn writes_of<'a>(
    index: &Index<'_, 'a>,
    module: usize,
    visit: impl FnOnce(&mut Writes<'_, '_, 'a>),
) -> Vec<Sym> {
    let mut writes = Writes { index, module, out: Vec::new(), members: Vec::new() };
    visit(&mut writes);
    let store = |sym: &Sym| {
        let is_store = |d: &Decl<'_>| matches!(d.kind, Kind::Store(_));
        index.decls[sym.0].get(&sym.1).is_some_and(is_store)
            || index
                .locals
                .iter()
                .any(|(c, l)| c.module == sym.0 && l.get(&sym.1).is_some_and(is_store))
    };
    let members = writes.members.into_iter().filter(store);
    writes.out.into_iter().map(|(sym, _)| sym).chain(members).collect()
}

fn visit_decl<'a>(visitor: &mut impl Visit<'a>, decl: &Decl<'a>) {
    match decl.kind {
        Kind::Signal(e) | Kind::Store(e) | Kind::Computed(e) | Kind::Action(e) => {
            if let Some(e) = e {
                visitor.visit_expression(e);
            }
        }
        Kind::Declarator(declarator, _) => {
            if let Some(init) = &declarator.init {
                visitor.visit_expression(init);
            }
        }
        Kind::Function(function) => visitor.visit_function(function, ScopeFlags::Function),
        Kind::Class(class) => visitor.visit_class(class),
    }
}

// ---------------------------------------------------------------------------------------------
// Template layout

#[derive(Clone)]
enum LKind<'a> {
    Element { tag: &'a str, span: u32 },
    Fixed,
    Variable,
}

struct LNode<'a> {
    kind: LKind<'a>,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Region<'e, 'a> {
    parent: Option<usize>,
    callback: Option<usize>,
    roots: Vec<usize>,
    /// The range this region is a branch of.
    range: Option<usize>,
    nodes: &'e [Node<'a>],
    /// The span start that keys `nodes`.
    key: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeKind {
    Show,
    For,
    Repeat,
}

/// A `<Show>`, `<Switch>`, `<For>`, or `<Repeat>`.
struct RangeInfo<'a> {
    region: usize,
    key: u32,
    kind: RangeKind,
    /// The conditions of each branch, or the source of a list.
    sources: Vec<&'a Expression<'a>>,
    /// One region per condition, or the row.
    branches: Vec<usize>,
    fallback: Option<usize>,
    svg: bool,
}

struct CallbackInfo<'a> {
    function: &'a ArrowFunctionExpression<'a>,
    content: u32,
    region: usize,
}

struct Container<'e, 'a> {
    key: u32,
    region: usize,
    /// The element whose children these are; `None` at the root of the region.
    element: Option<usize>,
    nodes: &'e [Node<'a>],
    ids: Vec<usize>,
}

struct Site<'e, 'a> {
    node: usize,
    region: usize,
    element: &'e Element<'a>,
}

struct Attr<'a> {
    node: usize,
    region: usize,
    name: &'a str,
    tag: &'a str,
    expression: &'a Expression<'a>,
}

#[derive(Default)]
struct Layout<'e, 'a> {
    nodes: Vec<LNode<'a>>,
    regions: Vec<Region<'e, 'a>>,
    callbacks: Vec<CallbackInfo<'a>>,
    containers: Vec<Container<'e, 'a>>,
    sites: Vec<Site<'e, 'a>>,
    attrs: Vec<Attr<'a>>,
    /// `ref` callbacks, by element node.
    refs: Vec<(usize, usize, &'a Expression<'a>)>,
    ranges: Vec<RangeInfo<'a>>,
    /// Anchors whose `aria-current` follows their `href`, by element node.
    actives: Vec<(usize, usize, &'e Value<'a>)>,
    /// Nodes client code cannot create, by region.
    unsupported: Vec<(usize, Span, &'static str)>,
    svg: bool,
}

impl<'e, 'a> Layout<'e, 'a> {
    fn push(&mut self, kind: LKind<'a>, parent: Option<usize>) -> usize {
        self.nodes.push(LNode { kind, parent, children: Vec::new() });
        self.nodes.len() - 1
    }

    fn region(
        &mut self,
        parent: Option<usize>,
        callback: Option<usize>,
        nodes: &'e [Node<'a>],
        key: u32,
    ) -> usize {
        self.regions.push(Region { parent, callback, roots: Vec::new(), range: None, nodes, key });
        self.regions.len() - 1
    }

    fn content(&mut self, content: &'e Content<'a>, parent: Option<usize>) -> usize {
        let region = self.region(parent, None, &content.nodes, content.span.start);
        let roots = self.nodes_in(&content.nodes, None, region, content.span.start);
        self.regions[region].roots = roots;
        region
    }

    fn callback(&mut self, callback: &'e Callback<'a>, parent: usize) -> usize {
        let index = self.callbacks.len();
        let content = &callback.content;
        let region = self.region(Some(parent), Some(index), &content.nodes, content.span.start);
        self.callbacks.push(CallbackInfo {
            function: callback.function,
            content: callback.content.span.start,
            region,
        });
        let roots = self.nodes_in(&content.nodes, None, region, content.span.start);
        self.regions[region].roots = roots;
        region
    }

    fn branch(&mut self, branch: &'e Branch<'a>, parent: usize) -> usize {
        match branch {
            Branch::Content(content) => self.content(content, Some(parent)),
            Branch::Callback(callback) => self.callback(callback, parent),
        }
    }

    /// Records a range over `branches`, the regions of its conditions or its row, and `fallback`.
    fn range(
        &mut self,
        node: &Node<'a>,
        region: usize,
        kind: RangeKind,
        sources: Vec<&'a Expression<'a>>,
        branches: Vec<usize>,
        fallback: Option<usize>,
    ) {
        let index = self.ranges.len();
        for &branch in branches.iter().chain(&fallback) {
            self.regions[branch].range = Some(index);
        }
        let key = range_key(node).unwrap_or_default();
        let svg = self.svg;
        self.ranges.push(RangeInfo { region, key, kind, sources, branches, fallback, svg });
    }

    fn nodes_in(
        &mut self,
        nodes: &'e [Node<'a>],
        element: Option<usize>,
        region: usize,
        key: u32,
    ) -> Vec<usize> {
        let mut ids = Vec::with_capacity(nodes.len());
        for node in nodes {
            let id = match node {
                Node::Element(child) => {
                    let id = self
                        .push(LKind::Element { tag: child.tag, span: child.span.start }, element);
                    for item in &child.attributes {
                        match item {
                            AttributeItem::Attribute(attribute) => {
                                let Value::Expr(expression) = &attribute.value else { continue };
                                if child.tag == "form" && attribute.name == "action" {
                                    self.unsupported.push((
                                        region,
                                        expression.span(),
                                        "a computed form `action`",
                                    ));
                                }
                                self.attrs.push(Attr {
                                    node: id,
                                    region,
                                    name: attribute.name,
                                    tag: child.tag,
                                    expression,
                                });
                            }
                            AttributeItem::Spread(expression) => {
                                self.unsupported.push((
                                    region,
                                    expression.span(),
                                    "a spread attribute",
                                ));
                            }
                        }
                    }
                    if let Some(href) = child.active_href() {
                        self.actives.push((id, region, href));
                    }
                    if !child.handlers.is_empty() {
                        self.sites.push(Site { node: id, region, element: child });
                    }
                    if let Some(reference) = child.reference {
                        self.refs.push((id, region, reference));
                    }
                    let svg = self.svg;
                    self.svg = match child.tag {
                        "svg" => true,
                        "foreignObject" => false,
                        _ => svg,
                    };
                    let children =
                        self.nodes_in(&child.children, Some(id), region, child.span.start);
                    self.svg = svg;
                    self.nodes[id].children = children;
                    id
                }
                Node::Text(_) => self.push(LKind::Fixed, element),
                Node::Head(elements) => {
                    if let Some(first) = elements.first() {
                        self.unsupported.push((region, first.span, "`<head>`"));
                    }
                    self.push(LKind::Fixed, element)
                }
                Node::Portal(boundary) => {
                    self.unsupported.push((region, boundary.children.span, "`<Portal>`"));
                    self.push(LKind::Fixed, element)
                }
                Node::Expr(expression) if is_slot(expression) => {
                    self.unsupported.push((region, expression.span(), "`children`"));
                    self.push(LKind::Variable, element)
                }
                Node::Expr(_) => self.push(LKind::Fixed, element),
                Node::Component(call) => {
                    self.unsupported.push((region, call.span, "a component"));
                    let values = call.props.iter().map(|p| &p.value).chain(call.children.iter());
                    for value in values {
                        if let PropValue::Content(content) = value {
                            self.content(content, Some(region));
                        }
                    }
                    self.push(LKind::Variable, element)
                }
                Node::Show(show) => {
                    let branches = vec![self.branch(&show.children, region)];
                    let fallback = show.fallback.as_ref().map(|f| self.branch(f, region));
                    self.range(node, region, RangeKind::Show, vec![show.when], branches, fallback);
                    self.push(LKind::Variable, element)
                }
                Node::Switch(switch) => {
                    let branches =
                        switch.matches.iter().map(|m| self.branch(&m.children, region)).collect();
                    let fallback = switch.fallback.as_ref().map(|f| self.branch(f, region));
                    let sources = switch.matches.iter().map(|m| m.when).collect();
                    self.range(node, region, RangeKind::Show, sources, branches, fallback);
                    self.push(LKind::Variable, element)
                }
                Node::For(each) | Node::Repeat(each) => {
                    let row = self.callback(&each.children, region);
                    let fallback = each.fallback.as_ref().map(|f| self.branch(f, region));
                    let kind = if matches!(node, Node::For(_)) {
                        RangeKind::For
                    } else {
                        RangeKind::Repeat
                    };
                    self.range(node, region, kind, vec![each.source], vec![row], fallback);
                    self.push(LKind::Variable, element)
                }
                Node::Loading(boundary) | Node::Errored(boundary) => {
                    let what = if matches!(node, Node::Loading(_)) {
                        "`<Loading>`"
                    } else {
                        "`<Errored>`"
                    };
                    self.unsupported.push((region, boundary.children.span, what));
                    self.content(&boundary.children, Some(region));
                    if let Some(fallback) = &boundary.fallback {
                        self.branch(fallback, region);
                    }
                    self.push(LKind::Variable, element)
                }
            };
            ids.push(id);
        }
        self.containers.push(Container { key, region, element, nodes, ids: ids.clone() });
        ids
    }

    fn is_element(&self, id: usize) -> bool {
        matches!(self.nodes[id].kind, LKind::Element { .. })
    }

    fn is_variable(&self, id: usize) -> bool {
        matches!(self.nodes[id].kind, LKind::Variable)
    }

    fn siblings(&self, id: usize, region: usize) -> &[usize] {
        match self.nodes[id].parent {
            Some(parent) => &self.nodes[parent].children,
            None => &self.regions[region].roots,
        }
    }

    fn chain(&self, id: usize) -> Vec<usize> {
        let mut chain = vec![id];
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            chain.push(parent);
            current = parent;
        }
        chain.reverse();
        chain
    }

    /// Steps from `parent` (or the region root) down to its child `child`.
    fn down(&self, child: usize, region: usize, out: &mut String) -> Option<()> {
        let siblings = self.siblings(child, region);
        let at = siblings.iter().position(|&s| s == child)?;
        if let Some(parent) = self.nodes[child].parent
            && let (LKind::Element { tag: "table", .. }, LKind::Element { tag: "tr", .. }) =
                (&self.nodes[parent].kind, &self.nodes[child].kind)
        {
            return None;
        }
        let before = &siblings[..at];
        let after = &siblings[at + 1..];
        if !before.iter().any(|&s| self.is_variable(s)) {
            let k = before.iter().filter(|&&s| self.is_element(s)).count();
            write!(out, ".children[{k}]").unwrap();
            return Some(());
        }
        if !after.iter().any(|&s| self.is_variable(s)) {
            out.push_str(".lastElementChild");
            for _ in after.iter().filter(|&&s| self.is_element(s)) {
                out.push_str(".previousElementSibling");
            }
            return Some(());
        }
        None
    }

    /// Sibling steps from `x` to `y`, which share a parent or are both roots of `region`.
    fn across(&self, x: usize, y: usize, region: usize) -> Option<String> {
        let siblings = self.siblings(x, region);
        let i = siblings.iter().position(|&s| s == x)?;
        let j = siblings.iter().position(|&s| s == y)?;
        let between = &siblings[i.min(j) + 1..i.max(j)];
        if between.iter().any(|&s| self.is_variable(s)) {
            return None;
        }
        let steps = between.iter().filter(|&&s| self.is_element(s)).count() + 1;
        let step = if i < j { ".nextElementSibling" } else { ".previousElementSibling" };
        Some(step.repeat(steps))
    }

    /// A property chain from element `from` to element `to` in the same region, when every step
    /// has a fixed position.
    fn path(&self, from: usize, to: usize, region: usize) -> Option<String> {
        let a = self.chain(from);
        let b = self.chain(to);
        let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let down = |mut out: String, steps: &[usize]| {
            steps.iter().try_for_each(|&step| self.down(step, region, &mut out)).map(|()| out)
        };
        let sideways = (common < a.len() && common < b.len())
            .then(|| self.across(a[common], b[common], region))
            .flatten()
            .and_then(|steps| {
                down(".parentNode".repeat(a.len() - common - 1) + &steps, &b[common + 1..])
            });
        let through_parent = (common > 0)
            .then(|| down(".parentNode".repeat(a.len() - common), &b[common..]))
            .flatten();
        match (sideways, through_parent) {
            (Some(x), Some(y)) => Some(if y.len() < x.len() { y } else { x }),
            (x, y) => x.or(y),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Event decisions

fn event_condition<'e>(expression: &'e Expression<'e>, param: &str) -> Option<Vec<String>> {
    let field = |e: &'e Expression<'e>| -> Option<&'e str> {
        match e.without_parentheses() {
            Expression::StaticMemberExpression(member) => match member.object.without_parentheses()
            {
                Expression::Identifier(id) if id.name == param => {
                    Some(member.property.name.as_str())
                }
                _ => None,
            },
            _ => None,
        }
    };
    let modifier = |name: &str, pressed: bool| {
        matches!(name, "ctrlKey" | "altKey" | "metaKey" | "shiftKey")
            .then(|| vec![format!("{name}={pressed}")])
    };
    match expression.without_parentheses() {
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            let mut left = event_condition(&logical.left, param)?;
            left.extend(event_condition(&logical.right, param)?);
            Some(left)
        }
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            modifier(field(&unary.argument)?, false)
        }
        Expression::BinaryExpression(binary)
            if matches!(
                binary.operator,
                BinaryOperator::StrictEquality | BinaryOperator::Equality
            ) =>
        {
            let (name, value) = match (field(&binary.left), field(&binary.right)) {
                (Some(name), None) => (name, &binary.right),
                (None, Some(name)) => (name, &binary.left),
                _ => return None,
            };
            let value = match value.without_parentheses() {
                Expression::StringLiteral(s) => s.value.to_string(),
                Expression::NumericLiteral(n) => n.value.to_string(),
                _ => return None,
            };
            if value.contains([' ', ':', '+', '=']) {
                return None;
            }
            match name {
                "key" => Some(vec![value]),
                "code" => Some(vec![format!("code={value}")]),
                "button" => Some(vec![format!("button={value}")]),
                _ => None,
            }
        }
        e => modifier(field(e)?, true),
    }
}

const DECISIONS: [&str; 3] = ["preventDefault", "stopPropagation", "stopImmediatePropagation"];

/// Calls of event decisions on `param`, which the dispatcher cannot evaluate.
struct Deferred<'p> {
    param: &'p str,
    condition: Option<Span>,
    out: Vec<Deferral>,
}

/// A decision call the dispatcher cannot evaluate, with the test it depends on when it has one.
struct Deferral {
    span: Span,
    method: &'static str,
    condition: Option<Span>,
}

impl<'a> Visit<'a> for Deferred<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(method) = decision(it, self.param) {
            self.out.push(Deferral { span: it.span, method, condition: self.condition });
        }
        walk::walk_call_expression(self, it);
    }
}

fn decision(call: &CallExpression<'_>, param: &str) -> Option<&'static str> {
    let Expression::StaticMemberExpression(member) = call.callee.without_parentheses() else {
        return None;
    };
    let Expression::Identifier(object) = member.object.without_parentheses() else { return None };
    if object.name != param {
        return None;
    }
    DECISIONS.iter().find(|&&m| m == member.property.name.as_str()).copied()
}

struct Decisions<'p> {
    event: &'p str,
    param: &'p str,
    prevent: Vec<String>,
    deferred: Vec<Deferral>,
}

impl<'a> Decisions<'_> {
    fn expression(&mut self, expression: &Expression<'a>, condition: &[String]) {
        match expression.without_parentheses() {
            Expression::CallExpression(call) if decision(call, self.param).is_some() => {
                if decision(call, self.param) == Some("preventDefault") {
                    let mut token = self.event.to_string();
                    if !condition.is_empty() {
                        token.push(':');
                        token.push_str(&condition.join("+"));
                    }
                    self.prevent.push(token);
                }
                for argument in &call.arguments {
                    self.deferred_in(None, |d| d.visit_argument(argument));
                }
            }
            Expression::SequenceExpression(sequence) => {
                for e in &sequence.expressions {
                    self.expression(e, condition);
                }
            }
            e => self.deferred_in(None, |d| d.visit_expression(e)),
        }
    }

    fn statements(&mut self, statements: &[Statement<'a>], condition: &[String]) {
        for statement in statements {
            match statement {
                Statement::ExpressionStatement(s) => self.expression(&s.expression, condition),
                Statement::BlockStatement(block) => self.statements(&block.body, condition),
                Statement::IfStatement(s) => match event_condition(&s.test, self.param) {
                    Some(more) if s.alternate.is_none() => {
                        let mut all = condition.to_vec();
                        all.extend(more);
                        self.statements(std::slice::from_ref(&s.consequent), &all);
                    }
                    _ => self.deferred_in(Some(s.test.span()), |d| d.visit_statement(statement)),
                },
                _ => self.deferred_in(None, |d| d.visit_statement(statement)),
            }
        }
    }

    fn deferred_in(&mut self, condition: Option<Span>, visit: impl FnOnce(&mut Deferred<'_>)) {
        let mut deferred = Deferred { param: self.param, condition, out: Vec::new() };
        visit(&mut deferred);
        self.deferred.extend(deferred.out);
    }
}

/// `data-rz-prevent` tokens of one handler, and the decisions that markup cannot express.
fn decisions(event: &str, value: &Expression<'_>) -> (Vec<String>, Vec<Deferral>) {
    let params = match value.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => &arrow.params,
        Expression::FunctionExpression(function) => &function.params,
        _ => return (Vec::new(), Vec::new()),
    };
    let Some(BindingPattern::BindingIdentifier(param)) = params.items.first().map(|p| &p.pattern)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut decisions =
        Decisions { event, param: param.name.as_str(), prevent: Vec::new(), deferred: Vec::new() };
    match value.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => decisions.statements(&body.statements, &[]),
            body => {
                if let Some(expression) = body.as_expression() {
                    decisions.expression(expression, &[]);
                }
            }
        },
        Expression::FunctionExpression(function) => {
            if let Some(body) = &function.body {
                decisions.statements(&body.statements, &[]);
            }
        }
        _ => {}
    }
    (decisions.prevent, decisions.deferred)
}

// ---------------------------------------------------------------------------------------------
// Rewriting client code

#[derive(Default)]
struct Needs {
    /// Component-level declarations to copy into the factory.
    locals: BTreeSet<SymbolId>,
    /// Module-level declarations, of any module.
    decls: BTreeSet<Sym>,
    /// Import bindings to keep, by module.
    imports: BTreeSet<Sym>,
    helpers: BTreeSet<&'static str>,
}

/// Captures of one scope, by key, with the server expression of each.
#[derive(Default)]
struct Captures {
    keys: Vec<(String, String)>,
}

impl Captures {
    fn slot(&mut self, key: String, server: String) -> usize {
        if let Some(index) = self.keys.iter().position(|(k, _)| *k == key) {
            return index;
        }
        self.keys.push((key, server));
        self.keys.len() - 1
    }
}

struct ComponentCx<'c> {
    component: ComponentRef,
    span: Span,
    /// Function span and scope of each callback that has a scope.
    scoped: &'c [(Span, usize)],
    captures: &'c mut Vec<Captures>,
    /// Offset of the first capture in a state entry, by scope.
    access: &'c dyn Fn(usize) -> String,
    /// Parameters of the callbacks client code calls, which are never captured; `true` for an
    /// index that is a signal.
    params: &'c HashMap<SymbolId, bool>,
}

/// The identifier a member chain such as `props.data.at` starts from.
fn root_symbol(
    index: &Index<'_, '_>,
    module: usize,
    expression: &Expression<'_>,
) -> Option<SymbolId> {
    let mut current = expression.without_parentheses();
    loop {
        match current {
            Expression::StaticMemberExpression(member) => {
                current = member.object.without_parentheses()
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => {
                    current = member.object.without_parentheses();
                }
                _ => return None,
            },
            Expression::Identifier(reference) => return index.symbol(module, reference),
            _ => return None,
        }
    }
}

/// `code`, the state slot of the read `key` from the symbol `symbol`, converted to its native
/// form when the props type of `component` gives it a `Date`, `bigint`, `Map`, or `Set`.
fn restore_read(
    analysis: &Analysis<'_>,
    component: ComponentRef,
    symbol: SymbolId,
    key: &str,
    code: String,
) -> String {
    let facts = &analysis.facts[component.module].components[component.index];
    let Some(props) = &facts.props else { return code };
    if key.contains(['[', '(']) {
        return code;
    }
    let mut segments: Vec<String> =
        key.split('.').map(|s| s.trim_end_matches('?').to_string()).collect();
    match facts.function.params().items.first().map(|p| &p.pattern) {
        Some(BindingPattern::BindingIdentifier(id)) if id.symbol_id.get() == Some(symbol) => {
            segments.remove(0);
        }
        Some(BindingPattern::ObjectPattern(object)) => {
            let name = object.properties.iter().find_map(|property| match &property.value {
                BindingPattern::BindingIdentifier(id) if id.symbol_id.get() == Some(symbol) => {
                    property.key.static_name()
                }
                _ => None,
            });
            let Some(name) = name else { return code };
            segments[0] = name.to_string();
        }
        _ => return code,
    }
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    field_type(props, &segments).and_then(|ty| restore(&ty, &code, 0)).unwrap_or(code)
}

const UNSERIALIZABLE_OK: [&str; 7] = ["Date", "Map", "Set", "URL", "Array", "Object", "RegExp"];

struct Rewrite<'r, 'c, 'm, 'a> {
    index: &'r Index<'m, 'a>,
    module: usize,
    source: &'a str,
    /// The code being rewritten; what it declares itself is not captured.
    root: Span,
    component: Option<&'r mut ComponentCx<'c>>,
    edits: Vec<(Span, String)>,
    needs: &'r mut Needs,
    reports: &'r mut Vec<Report>,
}

impl<'a> Rewrite<'_, '_, '_, 'a> {
    fn text(&self, span: Span) -> &'a str {
        let source: &'a str = self.source;
        &source[span.start as usize..span.end as usize]
    }

    /// The scope of a component-level value, when `symbol` is one.
    fn capture_scope(&self, symbol: SymbolId) -> Option<usize> {
        let component = self.component.as_ref()?;
        if component.params.contains_key(&symbol) {
            return None;
        }
        let scoping = self.index.modules[self.module].semantic.scoping();
        let declared = scoping.symbol_span(symbol);
        if !component.span.contains_inclusive(declared) || self.root.contains_inclusive(declared) {
            return None;
        }
        if self.index.decl((self.module, symbol), Some(component.component)).is_some() {
            return None;
        }
        Some(
            component
                .scoped
                .iter()
                .filter(|(span, _)| span.contains_inclusive(declared))
                .min_by_key(|(span, _)| span.size())
                .map_or(0, |(_, scope)| *scope),
        )
    }

    fn check_serializable(&mut self, symbol: SymbolId, at: Span) {
        let semantic = &self.index.modules[self.module].semantic;
        let node = semantic.scoping().symbol_declaration(symbol);
        let oxc_ast::AstKind::VariableDeclarator(declarator) =
            semantic.nodes().get_node(node).kind()
        else {
            return;
        };
        let Some(Expression::NewExpression(new)) =
            declarator.init.as_ref().map(Expression::without_parentheses)
        else {
            return;
        };
        let class = match new.callee.without_parentheses() {
            Expression::Identifier(id) => id.name.as_str(),
            _ => "class instance",
        };
        if UNSERIALIZABLE_OK.contains(&class) {
            return;
        }
        let name = semantic.scoping().symbol_name(symbol).to_string();
        self.reports.push(
            Report::new(Code::HandlerCaptureUnserializable, at)
                .arg("name", name)
                .arg("what", format!("a `{class}`")),
        );
    }

    /// Reports a read of `props.data` in a route component whose loader has no result type.
    fn check_loader_read(&mut self, symbol: SymbolId, key: &str, span: Span) {
        let Some(component) = self.component.as_ref().map(|c| c.component) else { return };
        let facts = &self.index.analysis.facts[component.module];
        let function = facts.components[component.index].function;
        let props = match function.params().items.first().map(|p| &p.pattern) {
            Some(BindingPattern::BindingIdentifier(id)) if id.symbol_id.get() == Some(symbol) => {
                &id.name
            }
            _ => return,
        };
        let is_default = match facts.exports.get("default") {
            Some(Export::Local(local)) => facts.components[component.index].symbol == Some(*local),
            Some(Export::Component(index)) => *index == component.index,
            _ => false,
        };
        let loader =
            facts.endpoints.iter().find(|e| e.kind == EndpointKind::Loader && e.export == "load");
        let Some(loader) = loader.filter(|loader| is_default && !loader.typed) else { return };
        let Some(route) = &loader.route else { return };
        let data = format!("{props}.data");
        if key == data
            || key.starts_with(&format!("{data}."))
            || key.starts_with(&format!("{data}["))
        {
            self.reports.push(
                Report::new(Code::LoaderResultUntyped, span)
                    .arg("read", self.text(span).to_string())
                    .arg("route", route.clone()),
            );
        }
    }

    fn capture(&mut self, symbol: SymbolId, span: Span) -> String {
        let scope = self.capture_scope(symbol).unwrap_or(0);
        self.check_serializable(symbol, span);
        let text = self.text(span).to_string();
        let key: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        self.check_loader_read(symbol, &key, span);
        let component = self.component.as_mut().expect("captures belong to a component");
        let target = component.component;
        let slot = component.captures[scope].slot(key.clone(), text);
        let offset = if scope == 0 { 1 } else { 2 };
        let code = format!("{}[{}]", (component.access)(scope), slot + offset);
        restore_read(self.index.analysis, target, symbol, &key, code)
    }

    /// The replacement of a bare reference, or `None` when it stays.
    fn reference(&mut self, it: &IdentifierReference<'a>) -> Option<String> {
        let symbol = self.index.symbol(self.module, it)?;
        if let Some(&signal) = self.component.as_ref().and_then(|c| c.params.get(&symbol)) {
            return signal.then(|| format!("{}.v", it.name));
        }
        let component = self.component.as_ref().map(|c| c.component);
        if let Some(component) = component
            && let Some(decl) = self.index.decl((self.module, symbol), Some(component))
            && self.index.locals[&component].contains_key(&symbol)
        {
            let state = decl.is_state();
            self.needs.locals.insert(symbol);
            return state.then(|| format!("{}.v", it.name));
        }
        if self.capture_scope(symbol).is_some() {
            return Some(self.capture(symbol, it.span));
        }
        let facts = &self.index.analysis.facts[self.module];
        if let Some(import) = facts.imports.get(&symbol) {
            if facts.apis.contains_key(&symbol) {
                return None;
            }
            self.needs.imports.insert((self.module, symbol));
            if let ImportSource::Module(_) = import.source {
                let origin = self.index.origin(self.module, symbol);
                if let Some(decl) = self.index.decls[origin.0].get(&origin.1) {
                    self.needs.decls.insert(origin);
                    return decl.is_state().then(|| format!("{}.v", it.name));
                }
            }
            return None;
        }
        if let Some(decl) = self.index.decls[self.module].get(&symbol) {
            self.needs.decls.insert((self.module, symbol));
            return decl.is_state().then(|| format!("{}.v", it.name));
        }
        None
    }

    /// `props.post.title` rooted at a captured value, as one capture.
    fn chain(&mut self, expression: &Expression<'a>) -> Option<String> {
        let mut current = expression;
        loop {
            match current {
                Expression::StaticMemberExpression(member) if !member.optional => {
                    current = &member.object;
                }
                Expression::Identifier(id) => {
                    let symbol = self.index.symbol(self.module, id)?;
                    self.capture_scope(symbol)?;
                    if std::ptr::eq(current, expression) {
                        return None;
                    }
                    return Some(self.capture(symbol, expression.span()));
                }
                _ => return None,
            }
        }
    }

    fn helper(&mut self, name: &'static str) -> &'static str {
        self.needs.helpers.insert(name);
        name
    }
}

impl<'a> Visit<'a> for Rewrite<'_, '_, '_, 'a> {
    fn visit_expression(&mut self, it: &Expression<'a>) {
        if let Some(replacement) = self.chain(it) {
            self.edits.push((it.span(), replacement));
            return;
        }
        walk::walk_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        match self.index.api(self.module, &it.callee) {
            Some(Api::Untrack) => {
                let helper = self.helper("$untrack");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::Action) => self.edits.push((it.callee.span(), String::new())),
            Some(Api::Signal) => {
                let helper = self.helper("$sig");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::Store) => {
                let helper = self.helper("$store");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::Computed) => {
                let helper = self.helper("$comp");
                self.edits.push((it.callee.span(), helper.into()));
                if let Some(argument) = it.arguments.first() {
                    self.edits.push((Span::empty(argument.span().start), "() => ".into()));
                }
            }
            Some(Api::Effect) => {
                let helper = self.helper("$fx");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::OnCleanup) => {
                let helper = self.helper("$cleanup");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::UseLocation) => {
                let helper = self.helper("$loc");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::UseSearchParams) => {
                let helper = self.helper("$search");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::UseLinkState) => {
                let helper = self.helper("$link");
                self.edits.push((it.callee.span(), helper.into()));
            }
            Some(Api::BuildPaths) => {
                let helper = self.helper("$path");
                self.edits.push((it.callee.span(), helper.into()));
            }
            _ => {
                if let Expression::StaticMemberExpression(member) = it.callee.without_parentheses()
                {
                    self.visit_expression(&member.object);
                } else {
                    self.visit_expression(&it.callee);
                }
            }
        }
        for argument in &it.arguments {
            self.visit_argument(argument);
        }
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
        {
            if let Some(replacement) = self.reference(id) {
                self.edits.push((it.span, format!("{}: {replacement}", id.name)));
            }
            return;
        }
        walk::walk_object_property(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(replacement) = self.reference(it) {
            self.edits.push((it.span, replacement));
        }
    }

    fn visit_ts_type_annotation(&mut self, _: &oxc_ast::ast::TSTypeAnnotation<'a>) {}
}

/// `span` of `source` with `edits` applied; edits are sorted and do not overlap.
fn splice(source: &str, span: Span, mut edits: Vec<(Span, String)>) -> String {
    edits.sort_by_key(|(span, _)| (span.start, span.end));
    let mut out = String::new();
    let mut position = span.start;
    for (edit, text) in edits {
        if edit.start < position {
            continue;
        }
        out.push_str(&source[position as usize..edit.start as usize]);
        out.push_str(&text);
        position = edit.end;
    }
    out.push_str(&source[position as usize..span.end as usize]);
    out
}

// ---------------------------------------------------------------------------------------------
// Planning

struct Planner<'m, 'a> {
    index: Index<'m, 'a>,
    plans: Vec<ModulePlan>,
    reports: Vec<Vec<Report>>,
    /// Factory code of each client component.
    factories: HashMap<ComponentRef, (usize, String, bool)>,
    needs: Vec<Needs>,
}

pub fn plan<'a>(modules: &[Module<'a>], analysis: &Analysis<'a>, routes: &RouteTable) -> Plan {
    let mut index = Index {
        modules,
        analysis,
        decls: Vec::new(),
        locals: HashMap::new(),
        reactive: HashSet::new(),
    };
    for (module, current) in modules.iter().enumerate() {
        let mut decls = HashMap::new();
        for statement in &current.program.body {
            match statement {
                Statement::ExportDeclaration(export) => {
                    declare(&index, module, &export.declaration, true, &mut decls, false);
                }
                _ => {
                    if let Some(declaration) = statement.as_declaration() {
                        declare(&index, module, declaration, false, &mut decls, false);
                    }
                }
            }
        }
        for component in &analysis.facts[module].components {
            if let Some(symbol) = component.symbol {
                decls.remove(&symbol);
            }
        }
        index.decls.push(decls);
    }
    for (module, facts) in analysis.facts.iter().enumerate() {
        for (i, component) in facts.components.iter().enumerate() {
            let mut locals = HashMap::new();
            if let Some(body) = component.function.body() {
                for statement in &body.statements {
                    if let Some(declaration) = statement.as_declaration() {
                        declare(&index, module, declaration, false, &mut locals, true);
                    }
                }
            }
            index.locals.insert(ComponentRef { module, index: i }, locals);
        }
    }

    let mut reports: Vec<Vec<Report>> = modules.iter().map(|_| Vec::new()).collect();
    check_module_state(&index, &mut reports);
    check_untrack(&index, &mut reports);

    let mut written = HashSet::new();
    let mut deps: HashMap<Sym, Vec<Sym>> = HashMap::new();
    let state = |sym: &Sym, index: &Index<'_, 'a>| {
        index.decls[sym.0].get(&sym.1).is_some_and(Decl::is_state)
            || index
                .locals
                .iter()
                .any(|(c, l)| c.module == sym.0 && l.get(&sym.1).is_some_and(Decl::is_state))
    };
    let mut writes = Vec::new();
    for (module, decls) in index.decls.iter().enumerate() {
        for (symbol, decl) in decls {
            if matches!(decl.kind, Kind::Signal(_) | Kind::Store(_)) {
                continue;
            }
            writes.extend(writes_of(&index, module, |w| visit_decl(w, decl)));
            deps.insert((module, *symbol), refs_of(&index, module, |r| visit_decl(r, decl)));
        }
    }
    for (component, locals) in &index.locals {
        for (symbol, decl) in locals {
            if matches!(decl.kind, Kind::Signal(_) | Kind::Store(_)) {
                continue;
            }
            writes.extend(writes_of(&index, component.module, |w| visit_decl(w, decl)));
            deps.insert(
                (component.module, *symbol),
                refs_of(&index, component.module, |r| visit_decl(r, decl)),
            );
        }
        for effect in effects(&index, *component) {
            writes.extend(writes_of(&index, component.module, |w| w.visit_expression(effect)));
        }
        let Some(template) = &index.component(*component).template else { continue };
        let mut handlers = Vec::new();
        collect_handlers(&template.nodes, &mut handlers);
        for handler in handlers {
            writes.extend(writes_of(&index, component.module, |w| w.visit_expression(handler)));
        }
    }
    for sym in writes {
        if state(&sym, &index) {
            written.insert(sym);
        }
    }
    let mut reactive = written;
    loop {
        let before = reactive.len();
        for (sym, refs) in &deps {
            if !reactive.contains(sym) && refs.iter().any(|r| reactive.contains(r)) {
                reactive.insert(*sym);
            }
        }
        if reactive.len() == before {
            break;
        }
    }
    index.reactive = reactive;

    let mut planner = Planner {
        plans: modules.iter().map(|_| ModulePlan::default()).collect(),
        needs: modules.iter().map(|_| Needs::default()).collect(),
        index,
        reports,
        factories: HashMap::new(),
    };
    let mut next_id = 0;
    for module in 0..modules.len() {
        for i in 0..analysis.facts[module].components.len() {
            let component = ComponentRef { module, index: i };
            if planner.component(component, next_id) {
                next_id += 1;
            }
        }
    }
    planner.modules();
    let chunks = planner.chunks(routes);
    Plan { modules: planner.plans, chunks, reports: planner.reports }
}

/// `effect(…)` statements of a component body.
fn effects<'a>(index: &Index<'_, 'a>, component: ComponentRef) -> Vec<&'a Expression<'a>> {
    let Some(body) = index.component(component).function.body() else { return Vec::new() };
    body.statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::ExpressionStatement(s) => match s.expression.without_parentheses() {
                Expression::CallExpression(call)
                    if index.api(component.module, &call.callee) == Some(Api::Effect) =>
                {
                    Some(&s.expression)
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn collect_handlers<'a>(nodes: &[Node<'a>], out: &mut Vec<&'a Expression<'a>>) {
    for_each_element(nodes, &mut |element| {
        out.extend(element.handlers.iter().map(|h| h.value));
    });
}

fn for_each_element<'e, 'a>(nodes: &'e [Node<'a>], f: &mut dyn FnMut(&'e Element<'a>)) {
    let branch = |b: &'e Branch<'a>| match b {
        Branch::Content(c) => &c.nodes,
        Branch::Callback(c) => &c.content.nodes,
    };
    for node in nodes {
        match node {
            Node::Element(element) => {
                f(element);
                for_each_element(&element.children, f);
            }
            Node::Component(call) => {
                for value in call.props.iter().map(|p| &p.value).chain(call.children.iter()) {
                    if let PropValue::Content(c) = value {
                        for_each_element(&c.nodes, f);
                    }
                }
            }
            Node::Show(show) => {
                for_each_element(branch(&show.children), f);
                if let Some(b) = &show.fallback {
                    for_each_element(branch(b), f);
                }
            }
            Node::Switch(switch) => {
                for m in &switch.matches {
                    for_each_element(branch(&m.children), f);
                }
                if let Some(b) = &switch.fallback {
                    for_each_element(branch(b), f);
                }
            }
            Node::For(each) | Node::Repeat(each) => {
                for_each_element(&each.children.content.nodes, f);
                if let Some(b) = &each.fallback {
                    for_each_element(branch(b), f);
                }
            }
            Node::Loading(boundary) | Node::Errored(boundary) => {
                for_each_element(&boundary.children.nodes, f);
                if let Some(b) = &boundary.fallback {
                    for_each_element(branch(b), f);
                }
            }
            Node::Text(_) | Node::Expr(_) | Node::Head(_) | Node::Portal(_) => {}
        }
    }
}

enum Target {
    Element(usize),
    /// A run of text: the element before it, or its parent when it starts the parent.
    Run {
        start: usize,
    },
}

struct TextBinding<'a> {
    container: usize,
    position: usize,
    expression: &'a Expression<'a>,
}

/// How client code finds the node a binding starts from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Finder {
    Element,
    Comment,
}

/// A client range, with its code rewritten.
struct RangeCode {
    range: usize,
    scope: usize,
    mark: usize,
    /// The conditions, or the source of a list.
    sources: Vec<String>,
    /// Each branch region, with the parameters of a callback.
    branches: Vec<(usize, Option<String>)>,
    row_scope: Option<usize>,
    separated: bool,
}

fn index_var(scope: usize) -> String {
    if scope == 0 { "$i".to_string() } else { format!("$j{scope}") }
}

/// A scope id in a client template.
fn id_of(scope: usize) -> String {
    format!("${{{}.toString(36)}}", index_var(scope))
}

/// Whether `region` is `ancestor` or inside it.
fn within(layout: &Layout<'_, '_>, region: usize, ancestor: usize) -> bool {
    let mut current = Some(region);
    while let Some(r) = current {
        if r == ancestor {
            return true;
        }
        current = layout.regions[r].parent;
    }
    false
}

/// The parameter list of `function`, without parentheses.
fn params_text(source: &str, function: &ArrowFunctionExpression<'_>) -> String {
    let params = &function.params;
    let start =
        params.items.first().map(|p| p.span.start).or(params.rest.as_ref().map(|r| r.span.start));
    let end = params.rest.as_ref().map(|r| r.span.end).or(params.items.last().map(|p| p.span.end));
    match (start, end) {
        (Some(start), Some(end)) => source[start as usize..end as usize].to_string(),
        _ => String::new(),
    }
}

/// Writes the binding code of each unit: the document, a server row, or the content of a client
/// range, which client code finds under its nodes `$n`.
struct Emission<'x, 'e, 'a> {
    layout: &'x Layout<'e, 'a>,
    code: &'x BTreeMap<usize, BTreeMap<(Finder, usize), Vec<String>>>,
    ranges: &'x BTreeMap<usize, Vec<RangeCode>>,
    /// Server rows with a scope, by the unit they are in.
    rows: &'x BTreeMap<usize, Vec<(usize, usize)>>,
    scope_of: &'x dyn Fn(usize) -> usize,
    marks: &'x HashMap<u32, &'x Mark>,
    anchors: &'x HashMap<(u32, usize), (usize, usize)>,
    holes: &'x HashSet<u32>,
    range_keys: &'x HashMap<u32, (usize, usize)>,
}

impl Emission<'_, '_, '_> {
    fn has_code(&self, unit: usize) -> bool {
        self.code.get(&unit).is_some_and(|g| g.values().any(|l| !l.is_empty()))
            || self.ranges.contains_key(&unit)
            || self.rows.get(&unit).is_some_and(|rows| rows.iter().any(|&(r, _)| self.has_code(r)))
    }

    fn unit(&self, out: &mut String, unit: usize, depth: usize, rooted: bool, needs: &mut Needs) {
        let pad = "  ".repeat(depth);
        let index = index_var((self.scope_of)(unit));
        let root = if rooted { ", $n" } else { "" };
        for ((finder, mark), lines) in self.code.get(&unit).into_iter().flatten() {
            if lines.is_empty() {
                continue;
            }
            let helper = match finder {
                Finder::Element => "$q",
                Finder::Comment => "$cm",
            };
            needs.helpers.insert(helper);
            writeln!(out, "{pad}for (const $b of {helper}({index}, {mark}{root})) {{").unwrap();
            for line in lines {
                writeln!(out, "{pad}  {line}").unwrap();
            }
            writeln!(out, "{pad}}}").unwrap();
        }
        for range in self.ranges.get(&unit).into_iter().flatten() {
            self.range(out, range, depth, rooted, needs);
        }
        for &(region, scope) in self.rows.get(&unit).into_iter().flatten() {
            if !self.has_code(region) {
                continue;
            }
            needs.helpers.insert("$rows");
            writeln!(
                out,
                "{pad}for (const [$j{scope}, $r{scope}] of $rows($a, {index}, {scope})) {{"
            )
            .unwrap();
            self.unit(out, region, depth + 1, false, needs);
            writeln!(out, "{pad}}}").unwrap();
        }
    }

    fn range(
        &self,
        out: &mut String,
        range: &RangeCode,
        depth: usize,
        rooted: bool,
        needs: &mut Needs,
    ) {
        let pad = "  ".repeat(depth);
        let info = &self.layout.ranges[range.range];
        let index = index_var(range.scope);
        let root = if rooted { ", $n" } else { "" };
        let mark = range.mark;
        if info.kind == RangeKind::Show {
            let conditions: Vec<String> =
                range.sources.iter().map(|c| format!("() => ({c})")).collect();
            let keyed: usize = range
                .branches
                .iter()
                .enumerate()
                .filter(|(_, (_, params))| params.is_some())
                .map(|(x, _)| 1 << x)
                .sum();
            writeln!(
                out,
                "{pad}for (const $b of $rg({index}, {mark}{root})) $range($b, [{}], {keyed}, ($n, $at, $k, $v) => {{",
                conditions.join(", ")
            )
            .unwrap();
            for (x, (region, params)) in range.branches.iter().enumerate() {
                writeln!(out, "{pad}  if ($k === {x}) {{").unwrap();
                self.content(out, *region, "", params.as_deref(), info.svg, depth + 2, needs);
                writeln!(out, "{pad}  }}").unwrap();
            }
            if let Some(fallback) = info.fallback {
                writeln!(out, "{pad}  if ($k === {}) {{", range.branches.len()).unwrap();
                self.content(out, fallback, "", None, info.svg, depth + 2, needs);
                writeln!(out, "{pad}  }}").unwrap();
            }
            writeln!(out, "{pad}  return $n;\n{pad}}});").unwrap();
            return;
        }
        let (row, params) = &range.branches[0];
        let j = range.row_scope.map_or_else(|| "$j".to_string(), |s| format!("$j{s}"));
        let params = match params.as_deref() {
            Some(params) if !params.is_empty() => format!(", {params}"),
            _ => String::new(),
        };
        writeln!(
            out,
            "{pad}for (const $b of $rg({index}, {mark}{root})) $list($b, () => ({}), ($n, $at, {j}{params}) => {{",
            range.sources[0]
        )
        .unwrap();
        let prefix = if range.separated { "<!--,-->" } else { "" };
        self.content(out, *row, prefix, None, info.svg, depth + 1, needs);
        write!(out, "{pad}  return $n;\n{pad}}}, {}, ", u8::from(range.separated)).unwrap();
        match info.fallback {
            Some(fallback) => {
                out.push_str("($n, $at) => {\n");
                self.content(out, fallback, "", None, info.svg, depth + 1, needs);
                write!(out, "{pad}  return $n;\n{pad}}}").unwrap();
            }
            None => out.push('0'),
        }
        if let Some(scope) = range.row_scope {
            write!(out, ", [$a, {index}, {scope}]").unwrap();
        }
        out.push_str(");\n");
    }

    /// Creates the nodes of `region` when there are none to adopt, then binds them.
    #[allow(clippy::too_many_arguments)]
    fn content(
        &self,
        out: &mut String,
        region: usize,
        prefix: &str,
        params: Option<&str>,
        svg: bool,
        depth: usize,
        needs: &mut Needs,
    ) {
        let pad = "  ".repeat(depth);
        let mut html = String::from(prefix);
        let current = &self.layout.regions[region];
        self.html(current.nodes, current.key, &mut html);
        let svg = if svg { ", 1" } else { "" };
        writeln!(out, "{pad}$n ??= $h(`{html}`, $at{svg});").unwrap();
        match params {
            Some(params) => {
                writeln!(out, "{pad}(({params}) => {{").unwrap();
                self.unit(out, region, depth + 1, true, needs);
                writeln!(out, "{pad}}})($v);").unwrap();
            }
            None => self.unit(out, region, depth, true, needs),
        }
    }

    /// The HTML client code creates for `nodes`: static content, marks, anchors, and empty ranges.
    fn html(&self, nodes: &[Node<'_>], key: u32, out: &mut String) {
        let pieces: Vec<Piece> = nodes.iter().map(|n| piece(n, self.holes)).collect();
        let separators = separators(&pieces);
        for (index, node) in nodes.iter().enumerate() {
            if let Some(&(scope, mark)) = self.anchors.get(&(key, index)) {
                write!(out, "<!--{}.{}-->", id_of(scope), base36(mark)).unwrap();
            }
            if separators[index] {
                out.push_str("<!>");
            }
            match node {
                Node::Text(text) => {
                    let mut escaped = String::new();
                    html::escape_text(&mut escaped, text);
                    template_escape(out, &escaped);
                }
                Node::Element(element) => self.element(element, out),
                Node::Show(_) | Node::Switch(_) | Node::For(_) | Node::Repeat(_) => {
                    if let Some(&(scope, mark)) =
                        range_key(node).and_then(|k| self.range_keys.get(&k))
                    {
                        write!(out, "<!--[{}.{}--><!--]-->", id_of(scope), base36(mark)).unwrap();
                    }
                }
                _ => {}
            }
        }
    }

    fn element(&self, element: &Element<'_>, out: &mut String) {
        out.push('<');
        out.push_str(element.tag);
        for item in &element.attributes {
            let AttributeItem::Attribute(attribute) = item else { continue };
            match &attribute.value {
                Value::Static(value) => {
                    let mut html = format!(" {}", attribute.name);
                    html::push_attribute_value(&mut html, value);
                    template_escape(out, &html);
                }
                Value::True => {
                    out.push(' ');
                    out.push_str(attribute.name);
                }
                Value::Expr(_) => {}
            }
        }
        if let Some(mark) = self.marks.get(&element.span.start) {
            write!(out, " data-rz={}.{}", id_of(mark.scope), base36(mark.index)).unwrap();
            if let Some(prevent) = &mark.prevent {
                let mut html = String::from(" data-rz-prevent");
                html::push_attribute_value(&mut html, prevent);
                template_escape(out, &html);
            }
        }
        out.push('>');
        if html::is_void(element.tag) {
            return;
        }
        self.html(&element.children, element.span.start, out);
        out.push_str("</");
        out.push_str(element.tag);
        out.push('>');
    }
}

impl<'m, 'a> Planner<'m, 'a> {
    fn reads_reactive(&self, module: usize, expression: &Expression<'a>) -> bool {
        refs_of(&self.index, module, |r| r.visit_expression(expression))
            .iter()
            .any(|sym| self.index.reactive.contains(sym))
    }

    /// Ranges whose content client code creates, and the regions inside them: a range is a
    /// client range when its source reads client state or it sits in a client region.
    fn client_ranges(&self, module: usize, layout: &Layout<'_, 'a>) -> (Vec<bool>, Vec<bool>) {
        let mut ranges = vec![false; layout.ranges.len()];
        let mut regions = vec![false; layout.regions.len()];
        loop {
            let mut changed = false;
            for (r, range) in layout.ranges.iter().enumerate() {
                if !ranges[r]
                    && (regions[range.region]
                        || range.sources.iter().any(|e| self.reads_reactive(module, e)))
                {
                    ranges[r] = true;
                    changed = true;
                }
            }
            for region in 0..layout.regions.len() {
                let current = &layout.regions[region];
                if !regions[region]
                    && (current.range.is_some_and(|r| ranges[r])
                        || current.parent.is_some_and(|p| regions[p]))
                {
                    regions[region] = true;
                    changed = true;
                }
            }
            if !changed {
                return (ranges, regions);
            }
        }
    }

    /// Plans one component; `false` when it has no client code.
    fn component(&mut self, component: ComponentRef, id: usize) -> bool {
        let module = component.module;
        let info = self.index.component(component);
        let Some(template) = &info.template else { return false };
        let function_span = info.function.span();
        let source = self.index.modules[module].source;

        let mut layout = Layout::default();
        let root = layout.region(None, None, &template.nodes, template.span.start);
        let roots = layout.nodes_in(&template.nodes, None, root, template.span.start);
        layout.regions[root].roots = roots;

        let (client_range, client_region) = self.client_ranges(module, &layout);
        let mut reports = Vec::new();
        let mut reported = HashSet::new();
        for &(region, span, what) in &layout.unsupported {
            if client_region[region] && reported.insert(span.start) {
                reports.push(Report::new(Code::ClientRangeUnsupported, span).arg("what", what));
            }
        }
        let mut client_params: HashMap<SymbolId, bool> = HashMap::new();
        for (r, range) in layout.ranges.iter().enumerate() {
            if !client_range[r] {
                continue;
            }
            for &branch in &range.branches {
                let Some(c) = layout.regions[branch].callback else { continue };
                let function = layout.callbacks[c].function;
                if let ArrowFunctionBody::FunctionBody(body) = &function.body
                    && body.statements.len() > 1
                {
                    reports.push(
                        Report::new(Code::ClientRangeUnsupported, body.span)
                            .arg("what", "statements in a callback body"),
                    );
                }
                for (p, param) in function.params.items.iter().enumerate() {
                    let signal = range.kind == RangeKind::For
                        && p == 1
                        && matches!(param.pattern, BindingPattern::BindingIdentifier(_));
                    for id in param.pattern.get_binding_identifiers() {
                        if let Some(symbol) = id.symbol_id.get() {
                            client_params.insert(symbol, signal);
                        }
                    }
                }
            }
        }

        let attrs: Vec<usize> = (0..layout.attrs.len())
            .filter(|&i| {
                client_region[layout.attrs[i].region]
                    || self.reads_reactive(module, layout.attrs[i].expression)
            })
            .collect();
        let mut texts = Vec::new();
        for (c, container) in layout.containers.iter().enumerate() {
            for (position, node) in container.nodes.iter().enumerate() {
                if let Node::Expr(expression) = node
                    && !is_slot(expression)
                    && (client_region[container.region] || self.reads_reactive(module, expression))
                {
                    texts.push(TextBinding { container: c, position, expression });
                }
            }
        }
        let actives: Vec<usize> =
            (0..layout.actives.len()).filter(|&i| client_region[layout.actives[i].1]).collect();
        let effects = effects(&self.index, component);
        if layout.sites.is_empty()
            && attrs.is_empty()
            && texts.is_empty()
            && effects.is_empty()
            && layout.refs.is_empty()
            && !client_range.contains(&true)
        {
            return false;
        }

        let scoping = self.index.modules[module].semantic.scoping();
        let mut client_exprs: Vec<&'a Expression<'a>> = Vec::new();
        for site in &layout.sites {
            client_exprs.extend(site.element.handlers.iter().map(|h| h.value));
        }
        client_exprs.extend(attrs.iter().map(|&a| layout.attrs[a].expression));
        client_exprs.extend(texts.iter().map(|t| t.expression));
        client_exprs.extend(layout.refs.iter().map(|&(_, _, reference)| reference));
        for &a in &actives {
            if let Value::Expr(expression) = layout.actives[a].2 {
                client_exprs.push(expression);
            }
        }
        for (r, range) in layout.ranges.iter().enumerate() {
            if client_range[r] {
                client_exprs.extend(range.sources.iter().copied());
            }
        }
        let mut scoped_callbacks: BTreeSet<usize> = BTreeSet::new();
        for expression in &client_exprs {
            let mut refs = Refs { index: &self.index, module, out: Vec::new() };
            refs.visit_expression(expression);
            for (m, symbol) in refs.out {
                let declared = scoping.symbol_span(symbol);
                if m != module
                    || client_params.contains_key(&symbol)
                    || expression.span().contains_inclusive(declared)
                {
                    continue;
                }
                if let Some((c, _)) = layout
                    .callbacks
                    .iter()
                    .enumerate()
                    .filter(|(_, cb)| cb.function.span.contains_inclusive(declared))
                    .min_by_key(|(_, cb)| cb.function.span.size())
                {
                    scoped_callbacks.insert(c);
                }
            }
        }
        for (r, range) in layout.ranges.iter().enumerate() {
            if !client_range[r] || range.kind == RangeKind::Show {
                continue;
            }
            let row = range.branches[0];
            if layout.sites.iter().any(|site| within(&layout, site.region, row))
                && let Some(c) = layout.regions[row].callback
            {
                scoped_callbacks.insert(c);
            }
        }
        let scope_of_callback: HashMap<usize, usize> =
            scoped_callbacks.iter().enumerate().map(|(i, &c)| (c, i + 1)).collect();
        let region_scope = |region: usize| {
            let mut current = Some(region);
            while let Some(r) = current {
                if let Some(c) = layout.regions[r].callback
                    && let Some(&scope) = scope_of_callback.get(&c)
                {
                    return scope;
                }
                current = layout.regions[r].parent;
            }
            0
        };
        let unit_of = |region: usize| {
            let mut r = region;
            loop {
                let current = &layout.regions[r];
                if current.range.is_some_and(|x| client_range[x])
                    || current.callback.is_some_and(|c| scope_of_callback.contains_key(&c))
                {
                    return r;
                }
                match current.parent {
                    Some(parent) => r = parent,
                    None => return r,
                }
            }
        };
        let scope_count = scoped_callbacks.len() + 1;
        let mut scope_parent = vec![0usize; scope_count];
        for (&c, &scope) in &scope_of_callback {
            let region = layout.callbacks[c].region;
            scope_parent[scope] = layout.regions[region].parent.map_or(0, region_scope);
        }

        let mut marks: HashMap<usize, Mark> = HashMap::new();
        let mut next_mark = 0;
        let mut events = BTreeSet::new();
        let mut eager = !effects.is_empty() || !layout.refs.is_empty();
        for site in &layout.sites {
            let mut prevent = Vec::new();
            for handler in &site.element.handlers {
                events.insert(handler.event.clone());
                let (tokens, deferred) = decisions(&handler.event, handler.value);
                prevent.extend(tokens);
                if !site.element.eager {
                    for deferral in deferred {
                        let condition =
                            deferral.condition.map_or("control flow".to_string(), |span| {
                                format!("`{}`", &source[span.start as usize..span.end as usize])
                            });
                        self.reports[module].push(
                            Report::new(Code::EventDecisionDeferred, deferral.span)
                                .arg("method", deferral.method)
                                .arg("condition", condition),
                        );
                    }
                }
            }
            eager |= site.element.eager;
            marks.insert(
                site.node,
                Mark {
                    scope: region_scope(site.region),
                    index: next_mark,
                    prevent: (!prevent.is_empty()).then(|| prevent.join(" ")),
                },
            );
            next_mark += 1;
        }
        let mut range_marks: HashMap<usize, (usize, usize)> = HashMap::new();
        let mut separated = HashSet::new();
        for (r, range) in layout.ranges.iter().enumerate() {
            if !client_range[r] {
                continue;
            }
            range_marks.insert(r, (region_scope(range.region), next_mark));
            next_mark += 1;
            if range.kind != RangeKind::Show {
                let row = &layout.regions[range.branches[0]];
                if !matches!(row.nodes, [Node::Element(_)]) {
                    separated.insert(row.key);
                }
            }
        }

        let mut anchors: HashMap<(u32, usize), (usize, usize)> = HashMap::new();
        let locate = |target: usize,
                      region: usize,
                      marks: &mut HashMap<usize, Mark>,
                      next_mark: &mut usize|
         -> (usize, String) {
            if let Some(mark) = marks.get(&target) {
                return (mark.index, String::new());
            }
            let found = marks
                .iter()
                .filter(|(node, mark)| {
                    mark.scope == region_scope(region) && node_region(&layout, **node, region)
                })
                .filter_map(|(node, mark)| Some((mark.index, layout.path(*node, target, region)?)))
                .min_by_key(|(index, path)| (path.len(), *index));
            if let Some(found) = found {
                return found;
            }
            marks.insert(
                target,
                Mark { scope: region_scope(region), index: *next_mark, prevent: None },
            );
            *next_mark += 1;
            (*next_mark - 1, String::new())
        };

        let mut needs = Needs::default();
        let mut captures: Vec<Captures> = (0..scope_count).map(|_| Captures::default()).collect();
        let scoped: Vec<(Span, usize)> = scope_of_callback
            .iter()
            .map(|(&c, &scope)| (layout.callbacks[c].function.span, scope))
            .collect();
        let binding_access =
            |scope: usize| if scope == 0 { "$s".to_string() } else { format!("$r{scope}") };

        let mut binding_code: BTreeMap<usize, BTreeMap<(Finder, usize), Vec<String>>> =
            BTreeMap::new();
        let mut text_holes = HashSet::new();

        for &a in &attrs {
            let attr = &layout.attrs[a];
            let (mark, path) = locate(attr.node, attr.region, &mut marks, &mut next_mark);
            let mut cx = ComponentCx {
                component,
                span: function_span,
                scoped: &scoped,
                captures: &mut captures,
                access: &binding_access,
                params: &client_params,
            };
            let code = self.rewrite(
                module,
                Some(&mut cx),
                &mut needs,
                &mut reports,
                |r| r.visit_expression(attr.expression),
                attr.expression.span(),
            );
            let helper = if html::is_property(attr.name)
                && matches!(attr.tag, "input" | "select" | "textarea" | "option")
            {
                "$prop"
            } else {
                "$attr"
            };
            needs.helpers.insert(helper);
            needs.helpers.insert("$fx");
            binding_code
                .entry(unit_of(attr.region))
                .or_default()
                .entry((Finder::Element, mark))
                .or_default()
                .push(format!("$fx(() => {helper}($b{path}, {:?}, {code}));", attr.name));
        }

        for &a in &actives {
            let (node, region, href) = layout.actives[a];
            let (mark, path) = locate(node, region, &mut marks, &mut next_mark);
            let code = match href {
                Value::Static(href) => serde_json::to_string(href).unwrap(),
                Value::Expr(expression) => {
                    let mut cx = ComponentCx {
                        component,
                        span: function_span,
                        scoped: &scoped,
                        captures: &mut captures,
                        access: &binding_access,
                        params: &client_params,
                    };
                    self.rewrite(
                        module,
                        Some(&mut cx),
                        &mut needs,
                        &mut reports,
                        |r| r.visit_expression(expression),
                        expression.span(),
                    )
                }
                Value::True => continue,
            };
            needs.helpers.extend(["$ac", "$fx"]);
            binding_code
                .entry(unit_of(region))
                .or_default()
                .entry((Finder::Element, mark))
                .or_default()
                .push(format!("$fx(() => $ac($b{path}, {code}));"));
        }

        for &(node, region, reference) in &layout.refs {
            let (mark, path) = locate(node, region, &mut marks, &mut next_mark);
            let mut cx = ComponentCx {
                component,
                span: function_span,
                scoped: &scoped,
                captures: &mut captures,
                access: &binding_access,
                params: &client_params,
            };
            let code = self.rewrite(
                module,
                Some(&mut cx),
                &mut needs,
                &mut reports,
                |r| r.visit_expression(reference),
                reference.span(),
            );
            binding_code
                .entry(unit_of(region))
                .or_default()
                .entry((Finder::Element, mark))
                .or_default()
                .push(format!("({code})($b{path});"));
        }

        let mut runs: BTreeMap<(usize, usize), Vec<&TextBinding<'a>>> = BTreeMap::new();
        for text in &texts {
            let container = &layout.containers[text.container];
            let mut start = text.position;
            while start > 0
                && !matches!(piece(&container.nodes[start - 1], &HashSet::new()), Piece::Break)
            {
                start -= 1;
            }
            runs.entry((text.container, start)).or_default().push(text);
            text_holes.insert(text.expression.span().start);
        }
        let mut target_of_run: Vec<((usize, usize), Target)> = Vec::new();
        for &(c, start) in runs.keys() {
            let container = &layout.containers[c];
            let target = if let (0, Some(element)) = (start, container.element) {
                Target::Element(element)
            } else if start > 0 && layout.is_element(container.ids[start - 1]) {
                Target::Element(container.ids[start - 1])
            } else {
                Target::Run { start }
            };
            target_of_run.push(((c, start), target));
        }
        for ((c, start), target) in target_of_run {
            let container = &layout.containers[c];
            let region = container.region;
            let scope = region_scope(region);
            let pieces: Vec<Piece> =
                container.nodes.iter().map(|n| piece(n, &text_holes)).collect();
            let seps = separators(&pieces);
            let (mark, parent, prev, finder) = match target {
                Target::Element(node) => {
                    let (mark, path) = locate(node, region, &mut marks, &mut next_mark);
                    if start == 0 {
                        (mark, format!("$b{path}"), "null".to_string(), Finder::Element)
                    } else {
                        let parent = parent_path(&format!("$b{path}"));
                        (mark, parent, format!("$b{path}"), Finder::Element)
                    }
                }
                Target::Run { start } => {
                    let mark = next_mark;
                    next_mark += 1;
                    anchors.insert((container.key, start), (scope, mark));
                    (mark, "$b.parentNode".to_string(), "$b".to_string(), Finder::Comment)
                }
            };
            let mut end = start;
            while end < pieces.len() && pieces[end] != Piece::Break {
                end += 1;
            }
            let mut skip = 0;
            let mut segment: Vec<String> = Vec::new();
            let mut names: Vec<String> = Vec::new();
            let mut updates: Vec<String> = Vec::new();
            let flush = |skip: usize,
                         segment: &mut Vec<String>,
                         names: &mut Vec<String>,
                         updates: &mut Vec<String>,
                         out: &mut Vec<String>| {
                if !names.is_empty() {
                    out.push(format!(
                        "const [{}] = $text({parent}, {prev}, {skip}, [{}]);",
                        names.join(", "),
                        segment.join(", ")
                    ));
                    out.append(updates);
                }
                segment.clear();
                names.clear();
            };
            let mut out = Vec::new();
            for position in start..end {
                if seps[position] {
                    flush(skip, &mut segment, &mut names, &mut updates, &mut out);
                    skip += 1;
                }
                match pieces[position] {
                    Piece::Static(len) => segment.push(len.to_string()),
                    Piece::Binding => {
                        let Node::Expr(expression) = &container.nodes[position] else { continue };
                        let mut cx = ComponentCx {
                            component,
                            span: function_span,
                            scoped: &scoped,
                            captures: &mut captures,
                            access: &binding_access,
                            params: &client_params,
                        };
                        let code = self.rewrite(
                            module,
                            Some(&mut cx),
                            &mut needs,
                            &mut reports,
                            |r| r.visit_expression(expression),
                            expression.span(),
                        );
                        let name = format!("$t{}", expression.span().start);
                        segment.push(format!("$str({code})"));
                        updates.push(format!("$fx(() => {{ {name}.data = $str({code}); }});"));
                        names.push(name);
                    }
                    _ => {}
                }
            }
            flush(skip, &mut segment, &mut names, &mut updates, &mut out);
            needs.helpers.extend(["$text", "$str", "$fx"]);
            binding_code
                .entry(unit_of(region))
                .or_default()
                .entry((finder, mark))
                .or_default()
                .extend(out);
        }

        let mut range_code: BTreeMap<usize, Vec<RangeCode>> = BTreeMap::new();
        for (r, range) in layout.ranges.iter().enumerate() {
            let Some(&(scope, mark)) = range_marks.get(&r) else { continue };
            let mut sources = Vec::new();
            for expression in &range.sources {
                let mut cx = ComponentCx {
                    component,
                    span: function_span,
                    scoped: &scoped,
                    captures: &mut captures,
                    access: &binding_access,
                    params: &client_params,
                };
                sources.push(self.rewrite(
                    module,
                    Some(&mut cx),
                    &mut needs,
                    &mut reports,
                    |r| r.visit_expression(expression),
                    expression.span(),
                ));
            }
            let params = |region: usize| {
                layout.regions[region]
                    .callback
                    .map(|c| params_text(source, layout.callbacks[c].function))
            };
            let row = range.branches[0];
            let row_scope = layout.regions[row]
                .callback
                .and_then(|c| scope_of_callback.get(&c).copied())
                .filter(|_| range.kind != RangeKind::Show);
            needs.helpers.insert(if range.kind == RangeKind::Show { "$range" } else { "$list" });
            needs.helpers.extend(["$rg", "$h"]);
            range_code.entry(unit_of(range.region)).or_default().push(RangeCode {
                range: r,
                scope,
                mark,
                sources,
                branches: range.branches.iter().map(|&b| (b, params(b))).collect(),
                row_scope,
                separated: separated.contains(&layout.regions[row].key),
            });
        }

        let mut handler_code: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for site in &layout.sites {
            let mark = marks[&site.node].index;
            let scope = region_scope(site.region);
            let handler_access = |target: usize| -> String {
                if target == 0 {
                    return "$s".into();
                }
                let mut expression = "$r".to_string();
                let mut current = scope;
                while current != target && current != 0 {
                    expression = format!("$a[~{expression}[0]]");
                    current = scope_parent[current];
                }
                expression
            };
            let mut wraps = Vec::new();
            let mut current = Some(site.region);
            while let Some(region) = current {
                let r = &layout.regions[region];
                if let (Some(x), Some(c)) = (r.range, r.callback)
                    && client_range[x]
                    && layout.ranges[x].branches.contains(&region)
                {
                    wraps.push((x, region, c));
                }
                current = r.parent;
            }
            for handler in &site.element.handlers {
                let mut cx = ComponentCx {
                    component,
                    span: function_span,
                    scoped: &scoped,
                    captures: &mut captures,
                    access: &handler_access,
                    params: &client_params,
                };
                let mut code = self.rewrite(
                    module,
                    Some(&mut cx),
                    &mut needs,
                    &mut reports,
                    |r| r.visit_expression(handler.value),
                    handler.value.span(),
                );
                for &(x, region, c) in &wraps {
                    let range = &layout.ranges[x];
                    let args = match range.kind {
                        RangeKind::For => {
                            let entry = handler_access(region_scope(region));
                            format!("{entry}[2], {entry}[3]")
                        }
                        RangeKind::Repeat => format!("{}[2]", handler_access(region_scope(region))),
                        RangeKind::Show => {
                            let at = range.branches.iter().position(|&b| b == region).unwrap_or(0);
                            let when = range.sources[at];
                            let mut cx = ComponentCx {
                                component,
                                span: function_span,
                                scoped: &scoped,
                                captures: &mut captures,
                                access: &handler_access,
                                params: &client_params,
                            };
                            self.rewrite(
                                module,
                                Some(&mut cx),
                                &mut needs,
                                &mut reports,
                                |r| r.visit_expression(when),
                                when.span(),
                            )
                        }
                    };
                    let params = params_text(source, layout.callbacks[c].function);
                    code = format!("(({params}) => ({code}))({args})");
                }
                let key = if html::is_identifier_name(&handler.event) {
                    handler.event.clone()
                } else {
                    format!("{:?}", handler.event)
                };
                handler_code.entry(mark).or_default().push(format!("{key}: ($r) => {code}"));
            }
        }

        let mut local_code: BTreeMap<u32, String> = BTreeMap::new();
        for effect in &effects {
            let mut cx = ComponentCx {
                component,
                span: function_span,
                scoped: &scoped,
                captures: &mut captures,
                access: &binding_access,
                params: &client_params,
            };
            let code = self.rewrite(
                module,
                Some(&mut cx),
                &mut needs,
                &mut reports,
                |r| r.visit_expression(effect),
                effect.span(),
            );
            local_code.insert(effect.span().start, format!("{code};"));
        }
        let mut done: HashSet<SymbolId> = HashSet::new();
        loop {
            let pending: Vec<SymbolId> =
                needs.locals.iter().filter(|s| !done.contains(s)).copied().collect();
            if pending.is_empty() {
                break;
            }
            for symbol in pending {
                done.insert(symbol);
                let decl = self.index.locals[&component][&symbol];
                let mut cx = ComponentCx {
                    component,
                    span: function_span,
                    scoped: &scoped,
                    captures: &mut captures,
                    access: &binding_access,
                    params: &client_params,
                };
                let name = scoping.symbol_name(symbol).to_string();
                let code =
                    self.decl_code(module, &name, &decl, Some(&mut cx), &mut needs, &mut reports);
                local_code.insert(decl.span.start, code);
            }
        }
        self.reports[module].extend(reports);

        let mut server_rows: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        for (&c, &scope) in &scope_of_callback {
            let region = layout.callbacks[c].region;
            if !client_region[region] {
                let parent = layout.regions[region].parent.map_or(0, unit_of);
                server_rows.entry(parent).or_default().push((region, scope));
            }
        }
        for rows in server_rows.values_mut() {
            rows.sort_by_key(|&(_, scope)| scope);
        }
        let mut range_keys = HashMap::new();
        for (&r, &at) in &range_marks {
            range_keys.insert(layout.ranges[r].key, at);
        }
        let mut element_marks = HashMap::new();
        for (node, mark) in &marks {
            if let LKind::Element { span, .. } = layout.nodes[*node].kind {
                element_marks.insert(span, mark);
            }
        }
        let emission = Emission {
            layout: &layout,
            code: &binding_code,
            ranges: &range_code,
            rows: &server_rows,
            scope_of: &region_scope,
            marks: &element_marks,
            anchors: &anchors,
            holes: &text_holes,
            range_keys: &range_keys,
        };

        let mut code = String::new();
        writeln!(code, "export const $c{id} = ($s, $i, $a) => {{").unwrap();
        for line in local_code.values() {
            writeln!(code, "  {line}").unwrap();
        }
        emission.unit(&mut code, root, 1, false, &mut needs);
        let handlers: Vec<String> = handler_code
            .iter()
            .map(|(mark, handlers)| format!("{mark}: {{ {} }}", handlers.join(", ")))
            .collect();
        if handlers.is_empty() {
            code.push_str("  return {};\n};\n");
        } else {
            writeln!(code, "  return {{ {} }};\n}};", handlers.join(", ")).unwrap();
        }

        let reads_module_state = needs.decls.iter().any(|sym| {
            self.index.decls[sym.0].get(&sym.1).is_some_and(Decl::is_state)
                && self.index.reactive.contains(sym)
        });
        let plan = &mut self.plans[module];
        for (node, mark) in marks {
            let LKind::Element { span, .. } = layout.nodes[node].kind else { continue };
            plan.marks.insert(span, mark);
        }
        plan.bindings.extend(text_holes);
        plan.anchors.extend(anchors);
        plan.ranges.extend(range_keys);
        plan.separated.extend(separated);
        plan.components.insert(
            template.span.start,
            ComponentScope {
                id,
                events: events.into_iter().collect::<Vec<_>>().join(" "),
                captures: captures[0].keys.iter().map(|(_, server)| server.clone()).collect(),
                eager,
            },
        );
        for (&c, &scope) in &scope_of_callback {
            plan.rows.insert(
                layout.callbacks[c].content,
                RowScope {
                    scope,
                    parent: scope_parent[scope],
                    captures: captures[scope]
                        .keys
                        .iter()
                        .map(|(_, server)| server.clone())
                        .collect(),
                },
            );
        }
        let target = &mut self.needs[module];
        target.decls.extend(needs.decls);
        target.imports.extend(needs.imports);
        target.helpers.extend(needs.helpers);
        self.factories.insert(component, (id, code, reads_module_state));
        true
    }

    fn rewrite(
        &self,
        module: usize,
        component: Option<&mut ComponentCx<'_>>,
        needs: &mut Needs,
        reports: &mut Vec<Report>,
        visit: impl FnOnce(&mut Rewrite<'_, '_, 'm, 'a>),
        span: Span,
    ) -> String {
        let source = self.index.modules[module].source;
        let mut rewrite = Rewrite {
            index: &self.index,
            module,
            source,
            root: span,
            component,
            edits: Vec::new(),
            needs,
            reports,
        };
        visit(&mut rewrite);
        let edits = std::mem::take(&mut rewrite.edits);
        splice(source, span, edits)
    }

    fn decl_code(
        &self,
        module: usize,
        name: &str,
        decl: &Decl<'a>,
        component: Option<&mut ComponentCx<'_>>,
        needs: &mut Needs,
        reports: &mut Vec<Report>,
    ) -> String {
        let export = if decl.exported { "export " } else { "" };
        match decl.kind {
            Kind::Signal(init) | Kind::Store(init) => {
                let helper = if matches!(decl.kind, Kind::Store(_)) { "$store" } else { "$sig" };
                needs.helpers.insert(helper);
                let value = match (init, component) {
                    (Some(init), _) if is_constant(init) => self.rewrite(
                        module,
                        None,
                        needs,
                        reports,
                        |r| r.visit_expression(init),
                        init.span(),
                    ),
                    (None, _) => String::new(),
                    (Some(init), Some(cx)) => {
                        let slot = cx.captures[0].slot(format!("${name}"), name.to_string());
                        let code = format!("$s[{}]", slot + 1);
                        match root_symbol(&self.index, module, init) {
                            Some(symbol) => {
                                let text = &self.index.modules[module].source
                                    [init.span().start as usize..init.span().end as usize];
                                let key: String =
                                    text.chars().filter(|c| !c.is_whitespace()).collect();
                                restore_read(self.index.analysis, cx.component, symbol, &key, code)
                            }
                            None => code,
                        }
                    }
                    (Some(init), None) => self.rewrite(
                        module,
                        None,
                        needs,
                        reports,
                        |r| r.visit_expression(init),
                        init.span(),
                    ),
                };
                format!("{export}const {name} = {helper}({value});")
            }
            Kind::Computed(expression) => {
                needs.helpers.insert("$comp");
                let value = expression.map_or_else(String::new, |e| {
                    self.rewrite(
                        module,
                        component,
                        needs,
                        reports,
                        |r| r.visit_expression(e),
                        e.span(),
                    )
                });
                format!("{export}const {name} = $comp(() => {value});")
            }
            Kind::Action(expression) => {
                let value = expression.map_or_else(
                    || "() => {}".to_string(),
                    |e| {
                        self.rewrite(
                            module,
                            component,
                            needs,
                            reports,
                            |r| r.visit_expression(e),
                            e.span(),
                        )
                    },
                );
                format!("{export}const {name} = {value};")
            }
            Kind::Declarator(declarator, kind) => {
                let keyword = match kind {
                    VariableDeclarationKind::Let => "let",
                    VariableDeclarationKind::Var => "var",
                    _ => "const",
                };
                let code = self.rewrite(
                    module,
                    component,
                    needs,
                    reports,
                    |r| {
                        if let Some(init) = &declarator.init {
                            r.visit_expression(init);
                        }
                    },
                    declarator.span,
                );
                format!("{export}{keyword} {code};")
            }
            Kind::Function(function) => {
                let code = self.rewrite(
                    module,
                    component,
                    needs,
                    reports,
                    |r| r.visit_function(function, ScopeFlags::Function),
                    function.span,
                );
                format!("{export}{code}")
            }
            Kind::Class(class) => {
                let code = self.rewrite(
                    module,
                    component,
                    needs,
                    reports,
                    |r| r.visit_class(class),
                    class.span,
                );
                format!("{export}{code}")
            }
        }
    }

    /// Client code of every module: the factories and the module-level declarations they reach.
    fn modules(&mut self) {
        let mut done: HashSet<Sym> = HashSet::new();
        let mut code: Vec<BTreeMap<u32, String>> =
            self.plans.iter().map(|_| BTreeMap::new()).collect();
        loop {
            let pending: Vec<Sym> = self
                .needs
                .iter()
                .flat_map(|n| n.decls.iter().copied())
                .filter(|sym| !done.contains(sym))
                .collect();
            if pending.is_empty() {
                break;
            }
            for sym in pending {
                done.insert(sym);
                let decl = self.index.decls[sym.0][&sym.1];
                if code[sym.0].contains_key(&decl.span.start) {
                    continue;
                }
                let name =
                    self.index.modules[sym.0].semantic.scoping().symbol_name(sym.1).to_string();
                let mut needs = Needs::default();
                let mut reports = Vec::new();
                let mutation = self.index.analysis.facts[sym.0]
                    .endpoints
                    .iter()
                    .find(|e| e.kind == EndpointKind::Mutation && e.symbol == Some(sym.1))
                    .and_then(|e| e.id.as_ref());
                let text = match mutation {
                    Some(id) => {
                        needs.helpers.insert("$call");
                        let export = if decl.exported { "export " } else { "" };
                        format!("{export}const {name} = $call({id:?});")
                    }
                    None => self.decl_code(sym.0, &name, &decl, None, &mut needs, &mut reports),
                };
                code[sym.0].insert(decl.span.start, text);
                let target = &mut self.needs[sym.0];
                target.decls.extend(needs.decls);
                target.imports.extend(needs.imports);
                target.helpers.extend(needs.helpers);
                self.reports[sym.0].extend(reports);
            }
        }
        let mut factories: Vec<Vec<(usize, String)>> =
            self.plans.iter().map(|_| Vec::new()).collect();
        for (component, (id, factory, _)) in &self.factories {
            factories[component.module].push((*id, factory.clone()));
        }
        for (module, plan) in self.plans.iter_mut().enumerate() {
            let mut list = std::mem::take(&mut factories[module]);
            if code[module].is_empty() && list.is_empty() {
                continue;
            }
            list.sort();
            let needs = &self.needs[module];
            let mut out = String::new();
            if !needs.helpers.is_empty() {
                let helpers: Vec<&str> = needs.helpers.iter().copied().collect();
                writeln!(out, "import {{ {} }} from {CLIENT_RUNTIME:?};", helpers.join(", "))
                    .unwrap();
            }
            out.push_str(&imports(&self.index, module, &needs.imports));
            for text in code[module].values() {
                out.push_str(text);
                out.push('\n');
            }
            for (_, factory) in list {
                out.push_str(&factory);
            }
            plan.client = Some(out);
        }
    }

    /// The chunk of each page route: the factories of every client component it can render.
    fn chunks(&self, routes: &RouteTable) -> BTreeMap<usize, String> {
        let mut out = BTreeMap::new();
        for (node_index, node) in routes.nodes.iter().enumerate() {
            if node.is_layout {
                continue;
            }
            let mut seen: BTreeSet<(usize, usize)> = BTreeSet::new();
            let mut stack = Vec::new();
            for level in routes.chain(node_index) {
                let file = &routes.nodes[level].file;
                let Some(module) = self.index.modules.iter().position(|m| &m.path == file) else {
                    continue;
                };
                if let Some(component) = default_component(&self.index, module) {
                    stack.push(component);
                }
            }
            while let Some(component) = stack.pop() {
                if !seen.insert((component.module, component.index)) {
                    continue;
                }
                if let Some(template) = &self.index.component(component).template {
                    calls(&template.nodes, &mut stack);
                }
            }
            let mut imports: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            let mut global = Vec::new();
            for &(module, index) in &seen {
                if let Some((id, _, reads)) = self.factories.get(&ComponentRef { module, index }) {
                    imports.entry(module).or_default().push(*id);
                    if *reads {
                        global.push(*id);
                    }
                }
            }
            if imports.is_empty() {
                continue;
            }
            let mut code = String::new();
            let mut entries = Vec::new();
            for (module, ids) in &imports {
                let names: Vec<String> = ids.iter().map(|id| format!("$c{id}")).collect();
                writeln!(
                    code,
                    "import {{ {} }} from {:?};",
                    names.join(", "),
                    format!("/{}", self.index.modules[*module].path)
                )
                .unwrap();
                entries.extend(ids.iter().map(|id| format!("{id}: $c{id}")));
            }
            global.sort();
            writeln!(code, "export default {{ {} }};", entries.join(", ")).unwrap();
            let global: Vec<String> = global.iter().map(ToString::to_string).collect();
            writeln!(code, "export const g = [{}];", global.join(", ")).unwrap();
            out.insert(node_index, code);
        }
        out
    }
}

/// The parent of the node at `path`, dropping a trailing `.children[k]` step.
fn parent_path(path: &str) -> String {
    match path.rfind(".children[") {
        Some(at)
            if path.ends_with(']')
                && path[at + 10..path.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            path[..at].to_string()
        }
        _ => format!("{path}.parentNode"),
    }
}

fn node_region(layout: &Layout<'_, '_>, node: usize, region: usize) -> bool {
    let root = layout.chain(node)[0];
    layout.regions[region].roots.contains(&root)
}

fn default_component(index: &Index<'_, '_>, module: usize) -> Option<ComponentRef> {
    let facts = &index.analysis.facts[module];
    match facts.exports.get("default")? {
        Export::Component(i) => Some(ComponentRef { module, index: *i }),
        Export::Local(symbol) => facts
            .components
            .iter()
            .position(|c| c.symbol == Some(*symbol))
            .map(|i| ComponentRef { module, index: i }),
        Export::Reexport { .. } => None,
    }
}

fn calls(nodes: &[Node<'_>], out: &mut Vec<ComponentRef>) {
    for node in nodes {
        match node {
            Node::Element(element) => calls(&element.children, out),
            Node::Component(call) => {
                match &call.target {
                    CallTarget::Component(c) => out.push(*c),
                    CallTarget::Dynamic { candidates, .. } => {
                        out.extend(candidates.iter().map(|(_, c)| *c));
                    }
                }
                for value in call.props.iter().map(|p| &p.value).chain(call.children.iter()) {
                    if let PropValue::Content(content) = value {
                        calls(&content.nodes, out);
                    }
                }
            }
            Node::Show(show) => {
                calls_branch(&show.children, out);
                show.fallback.iter().for_each(|b| calls_branch(b, out));
            }
            Node::Switch(switch) => {
                switch.matches.iter().for_each(|m| calls_branch(&m.children, out));
                switch.fallback.iter().for_each(|b| calls_branch(b, out));
            }
            Node::For(each) | Node::Repeat(each) => {
                calls(&each.children.content.nodes, out);
                each.fallback.iter().for_each(|b| calls_branch(b, out));
            }
            Node::Loading(b) | Node::Errored(b) | Node::Portal(b) => {
                calls(&b.children.nodes, out);
                b.fallback.iter().for_each(|f| calls_branch(f, out));
            }
            Node::Text(_) | Node::Expr(_) | Node::Head(_) => {}
        }
    }
}

fn calls_branch(branch: &Branch<'_>, out: &mut Vec<ComponentRef>) {
    match branch {
        Branch::Content(content) => calls(&content.nodes, out),
        Branch::Callback(callback) => calls(&callback.content.nodes, out),
    }
}

fn imports(index: &Index<'_, '_>, module: usize, needed: &BTreeSet<Sym>) -> String {
    let mut out = String::new();
    for statement in &index.modules[module].program.body {
        let Statement::ImportDeclaration(declaration) = statement else { continue };
        let mut named = Vec::new();
        let mut default = None;
        let mut namespace = None;
        for specifier in declaration.specifiers.iter().flatten() {
            let Some(symbol) = specifier.local().symbol_id.get() else { continue };
            if !needed.contains(&(module, symbol)) {
                continue;
            }
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported = s.imported.name();
                    named.push(if imported == s.local.name {
                        imported.to_string()
                    } else {
                        format!("{imported} as {}", s.local.name)
                    });
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    default = Some(s.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    namespace = Some(s.local.name.to_string());
                }
            }
        }
        let mut parts = Vec::new();
        parts.extend(default);
        if let Some(namespace) = namespace {
            parts.push(format!("* as {namespace}"));
        }
        if !named.is_empty() {
            parts.push(format!("{{ {} }}", named.join(", ")));
        }
        if !parts.is_empty() {
            let source = match declaration.source.value.as_str() {
                "reze-js/server" => CLIENT_RUNTIME,
                source => source,
            };
            writeln!(out, "import {} from {source:?};", parts.join(", ")).unwrap();
        }
    }
    out
}

/// `MODULE_STATE_INITIALIZER` and `MODULE_STATE_SERVER_WRITE`.
fn check_module_state(index: &Index<'_, '_>, reports: &mut [Vec<Report>]) {
    for (module, decls) in index.decls.iter().enumerate() {
        let mut seen = HashSet::new();
        for (symbol, decl) in decls {
            if let Kind::Signal(Some(init)) | Kind::Store(Some(init)) = decl.kind
                && !is_constant(init)
                && seen.insert(decl.span.start)
            {
                let name =
                    index.modules[module].semantic.scoping().symbol_name(*symbol).to_string();
                reports[module]
                    .push(Report::new(Code::ModuleStateInitializer, init.span()).arg("name", name));
            }
        }
    }
    for component in index.locals.keys() {
        let info = index.component(*component);
        let Some(body) = info.function.body() else { continue };
        let mut writes = ServerWrites { index, module: component.module, out: Vec::new() };
        for statement in &body.statements {
            writes.visit_statement(statement);
        }
        for (sym, span) in writes.out {
            if index.decls[sym.0]
                .get(&sym.1)
                .is_some_and(|d| matches!(d.kind, Kind::Signal(_) | Kind::Store(_)))
            {
                let name = index.modules[sym.0].semantic.scoping().symbol_name(sym.1).to_string();
                reports[component.module]
                    .push(Report::new(Code::ModuleStateServerWrite, span).arg("name", name));
            }
        }
    }
}

/// Writes that run during the server render: outside nested functions.
struct ServerWrites<'i, 'm, 'a> {
    index: &'i Index<'m, 'a>,
    module: usize,
    out: Vec<(Sym, Span)>,
}

impl<'a> Visit<'a> for ServerWrites<'_, '_, 'a> {
    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(reference) = &it.left
            && let Some(symbol) = self.index.symbol(self.module, reference)
        {
            self.out.push((self.index.origin(self.module, symbol), it.span));
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(reference) = &it.argument
            && let Some(symbol) = self.index.symbol(self.module, reference)
        {
            self.out.push((self.index.origin(self.module, symbol), it.span));
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// `UNTRACK_REDUNDANT`: an `untrack` in a handler, or in the component body outside an
/// `effect`, a `computed`, and the template.
fn check_untrack(index: &Index<'_, '_>, reports: &mut [Vec<Report>]) {
    for component in index.locals.keys() {
        let Some(body) = index.component(*component).function.body() else { continue };
        let mut untracks = Untracks {
            index,
            module: component.module,
            tracking: 0,
            handler: false,
            out: Vec::new(),
        };
        for statement in &body.statements {
            untracks.visit_statement(statement);
        }
        reports[component.module].extend(untracks.out);
    }
}

struct Untracks<'i, 'm, 'a> {
    index: &'i Index<'m, 'a>,
    module: usize,
    /// Depth of code whose reads may subscribe.
    tracking: usize,
    /// The next function is a handler.
    handler: bool,
    out: Vec<Report>,
}

impl<'a> Untracks<'_, '_, 'a> {
    fn tracked(&mut self, walk: impl FnOnce(&mut Self)) {
        if std::mem::take(&mut self.handler) {
            walk(self);
        } else {
            self.tracking += 1;
            walk(self);
            self.tracking -= 1;
        }
    }
}

impl<'a> Visit<'a> for Untracks<'_, '_, 'a> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        match self.index.api(self.module, &it.callee) {
            Some(Api::Effect | Api::Computed) => {
                self.tracking += 1;
                walk::walk_call_expression(self, it);
                self.tracking -= 1;
                return;
            }
            Some(Api::Untrack) if self.tracking == 0 => {
                let mut report = Report::new(Code::UntrackRedundant, it.span);
                if let Some(Expression::ArrowFunctionExpression(arrow)) = first_argument(it)
                    && let Some(expression) = arrow.get_expression()
                {
                    let source = self.index.modules[self.module].source;
                    let span = expression.span();
                    let text = source[span.start as usize..span.end as usize].to_string();
                    report =
                        report.fix(vec![Edit { start: it.span.start, end: it.span.end, text }]);
                }
                self.out.push(report);
            }
            _ => {}
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_jsx_element(&mut self, it: &oxc_ast::ast::JSXElement<'a>) {
        self.tracking += 1;
        walk::walk_jsx_element(self, it);
        self.tracking -= 1;
    }

    fn visit_jsx_attribute(&mut self, it: &oxc_ast::ast::JSXAttribute<'a>) {
        let name = it.name.get_identifier().name;
        let is_handler =
            name.len() > 2 && name.starts_with("on") && name.as_bytes()[2].is_ascii_uppercase();
        if !is_handler {
            return walk::walk_jsx_attribute(self, it);
        }
        let tracking = std::mem::take(&mut self.tracking);
        self.handler = true;
        walk::walk_jsx_attribute(self, it);
        self.handler = false;
        self.tracking = tracking;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.tracked(|this| walk::walk_arrow_function_expression(this, it));
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        self.tracked(|this| walk::walk_function(this, it, flags));
    }
}
