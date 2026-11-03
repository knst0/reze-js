//! A component's returned template, as the targets lower it.

use oxc_ast::ast::{ArrowFunctionExpression, Expression};
use oxc_span::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComponentRef {
    pub module: usize,
    pub index: usize,
}

pub enum Node<'a> {
    Element(Element<'a>),
    /// Static text, decoded.
    Text(String),
    /// A text hole.
    Expr(&'a Expression<'a>),
    Component(Call<'a>),
    Show(Show<'a>),
    Switch(Switch<'a>),
    For(Each<'a>),
    Repeat(Each<'a>),
    Loading(Boundary<'a>),
    Errored(Boundary<'a>),
    Portal(Boundary<'a>),
    Head(Vec<Element<'a>>),
}

pub struct Element<'a> {
    pub tag: &'a str,
    pub span: Span,
    /// In source order, which decides between a spread and an attribute of the same name.
    pub attributes: Vec<AttributeItem<'a>>,
    pub handlers: Vec<Handler<'a>>,
    pub reference: Option<&'a Expression<'a>>,
    pub eager: bool,
    pub children: Vec<Node<'a>>,
}

pub enum Value<'a> {
    /// Decoded string literal.
    Static(String),
    /// A name without a value.
    True,
    Expr(&'a Expression<'a>),
}

pub struct Attribute<'a> {
    pub name: &'a str,
    pub value: Value<'a>,
}

pub enum AttributeItem<'a> {
    Attribute(Attribute<'a>),
    Spread(&'a Expression<'a>),
}

impl<'a> Element<'a> {
    /// The `href` of an anchor that gets `aria-current="page"` and `data-active` when it points at
    /// the current path: one without a spread or an `aria-current` of its own.
    pub fn active_href(&self) -> Option<&Value<'a>> {
        if self.tag != "a" {
            return None;
        }
        let mut href = None;
        for item in &self.attributes {
            match item {
                AttributeItem::Spread(_) => return None,
                AttributeItem::Attribute(attribute) => match attribute.name {
                    "aria-current" => return None,
                    "href" => href = Some(&attribute.value),
                    _ => {}
                },
            }
        }
        match href {
            Some(Value::Static(href)) if !routable(href) => None,
            Some(Value::True) => None,
            href => href,
        }
    }
}

/// A same-origin absolute path, which a static `href` needs to ever be active.
fn routable(href: &str) -> bool {
    href.starts_with('/') && !href.starts_with("//")
}

pub struct Handler<'a> {
    /// DOM event name, `click`.
    pub event: String,
    pub value: &'a Expression<'a>,
}

pub enum PropValue<'a> {
    Value(Value<'a>),
    /// JSX written as the value.
    Content(Content<'a>),
    Spread(&'a Expression<'a>),
}

pub struct Prop<'a> {
    pub name: &'a str,
    pub value: PropValue<'a>,
}

pub enum CallTarget {
    Component(ComponentRef),
    /// `dynamic(table[key])`: the candidates by key.
    Dynamic {
        candidates: Vec<(String, ComponentRef)>,
    },
}

pub struct Call<'a> {
    pub name: String,
    pub span: Span,
    pub target: CallTarget,
    pub props: Vec<Prop<'a>>,
    pub children: Option<PropValue<'a>>,
}

/// Markup in a value position: children, a JSX prop, a branch.
pub struct Content<'a> {
    pub span: Span,
    pub nodes: Vec<Node<'a>>,
}

/// Content that receives values: `(user) => <Profile user={user} />`.
pub struct Callback<'a> {
    pub function: &'a ArrowFunctionExpression<'a>,
    /// The returned JSX, which the targets replace with the lowered `content`.
    pub content: Content<'a>,
}

pub enum Branch<'a> {
    Content(Content<'a>),
    Callback(Callback<'a>),
}

pub struct Show<'a> {
    pub when: &'a Expression<'a>,
    pub children: Branch<'a>,
    pub fallback: Option<Branch<'a>>,
}

pub struct Match<'a> {
    pub when: &'a Expression<'a>,
    pub children: Branch<'a>,
}

pub struct Switch<'a> {
    pub matches: Vec<Match<'a>>,
    pub fallback: Option<Branch<'a>>,
}

/// `<For each>` or `<Repeat count>`.
pub struct Each<'a> {
    pub source: &'a Expression<'a>,
    pub children: Callback<'a>,
    pub fallback: Option<Branch<'a>>,
}

/// `<Loading>`, `<Errored>`, `<Portal>`.
pub struct Boundary<'a> {
    pub fallback: Option<Branch<'a>>,
    pub children: Content<'a>,
}
