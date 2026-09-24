//! How a component uses its props, for the slots it can take as an island (SPEC §16.4). A slot
//! is server HTML on the server and the server's nodes in the browser: the two agree only when
//! the island inserts it as a child of a native element that it always renders.

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;
use serde::{Deserialize, Serialize};

use super::inert::Violation;
use crate::lower::is_native_name;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SlotUses {
    /// The first use of each prop that is not a JSX insert.
    pub keys: Vec<KeyUse>,
    /// The first use that may read any prop but those in `except`: the props object used as a
    /// value, a rest element, `arguments`.
    pub others: Option<OtherUse>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KeyUse {
    pub key: String,
    pub violation: Violation,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OtherUse {
    pub violation: Violation,
    pub except: Vec<String>,
}

impl SlotUses {
    /// Why the component cannot take the slot `name`; `None` when it only inserts it.
    pub fn failure(&self, name: &str) -> Option<&Violation> {
        self.keys.iter().find(|u| u.key == name).map(|u| &u.violation).or_else(|| {
            self.others
                .as_ref()
                .filter(|o| !o.except.iter().any(|key| key == name))
                .map(|o| &o.violation)
        })
    }
}

/// The props uses of the component function `function` with parameters `params`.
pub fn slot_uses(
    source: &str,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    function: NodeId,
    params: &FormalParameters<'_>,
    body: Option<&FunctionBody<'_>>,
) -> SlotUses {
    let mut collector = Collector { source, scoping, nodes, function, uses: SlotUses::default() };
    if let Some(body) = body {
        let mut arguments = ArgumentsRead::default();
        arguments.visit_function_body(body);
        if let Some(span) = arguments.found {
            collector.other(span, "reads the props through `arguments`", Vec::new());
        }
    }
    match params.items.first() {
        Some(param) => match &param.pattern {
            BindingPattern::BindingIdentifier(id) => collector.props_object(id.symbol_id()),
            BindingPattern::ObjectPattern(pattern) => collector.pattern(pattern),
            other => collector.other(
                other.span(),
                "destructures props in a form that hides which prop it reads",
                Vec::new(),
            ),
        },
        None => {
            if let Some(rest) = &params.rest {
                collector.other(rest.span, "collects the props in a rest parameter", Vec::new());
            }
        }
    }
    collector.uses
}

struct Collector<'s, 'a> {
    source: &'s str,
    scoping: &'s Scoping,
    nodes: &'s AstNodes<'a>,
    function: NodeId,
    uses: SlotUses,
}

impl Collector<'_, '_> {
    fn key(&mut self, key: &str, span: Span, what: &str) {
        if self.uses.keys.iter().all(|u| u.key != key) {
            let violation = Violation::at(self.source, span, what);
            self.uses.keys.push(KeyUse { key: key.to_string(), violation });
        }
    }

    /// Keeps the first such use, unless a later one reaches every prop.
    fn other(&mut self, span: Span, what: &str, except: Vec<String>) {
        let replaces =
            self.uses.others.as_ref().is_none_or(|o| !o.except.is_empty() && except.is_empty());
        if replaces {
            let violation = Violation::at(self.source, span, what);
            self.uses.others = Some(OtherUse { violation, except });
        }
    }

    /// `p`: every reference must be `p.k` or `p["k"]` in a JSX insert.
    fn props_object(&mut self, symbol: SymbolId) {
        for &reference in self.scoping.get_resolved_reference_ids(symbol) {
            let node = self.scoping.get_reference(reference).node_id();
            let AstKind::IdentifierReference(id) = self.nodes.kind(node) else { continue };
            let member = self.nodes.parent_id(node);
            let key = match self.nodes.kind(member) {
                AstKind::StaticMemberExpression(m) if !m.optional && m.object.span() == id.span => {
                    Some(m.property.name.as_str())
                }
                AstKind::ComputedMemberExpression(m)
                    if !m.optional && m.object.span() == id.span =>
                {
                    match m.expression.without_parentheses() {
                        Expression::StringLiteral(s) => Some(s.value.as_str()),
                        _ => None,
                    }
                }
                _ => None,
            };
            match key {
                Some(_) if self.is_insert(member) => {}
                Some(key) => {
                    let context = self.context(member);
                    self.key(key, context, &format!("uses `{key}` outside a JSX insert"));
                }
                None => {
                    let context = self.context(node);
                    self.other(context, "uses the props object as a value", Vec::new());
                }
            }
        }
    }

    /// `{ a, b: { c }, d = 1, ...rest }`: a binding per key read in JSX inserts only.
    fn pattern(&mut self, pattern: &ObjectPattern<'_>) {
        let mut keys = Vec::new();
        for property in &pattern.properties {
            let key = if property.computed { None } else { property.key.static_name() };
            let Some(key) = key else {
                self.other(property.span, "destructures a computed key", Vec::new());
                continue;
            };
            match &property.value {
                BindingPattern::BindingIdentifier(id) => {
                    for &reference in self.scoping.get_resolved_reference_ids(id.symbol_id()) {
                        let node = self.scoping.get_reference(reference).node_id();
                        if !self.is_insert(node) {
                            let context = self.context(node);
                            self.key(&key, context, &format!("uses `{key}` outside a JSX insert"));
                        }
                    }
                }
                BindingPattern::AssignmentPattern(_) => {
                    self.key(&key, property.span, &format!("gives `{key}` a default"));
                }
                _ => self.key(&key, property.span, &format!("destructures `{key}`")),
            }
            keys.push(key.into_owned());
        }
        let Some(rest) = &pattern.rest else { return };
        let BindingPattern::BindingIdentifier(id) = &rest.argument else {
            self.other(rest.span, "destructures the rest of the props", keys);
            return;
        };
        if let Some(&reference) = self.scoping.get_resolved_reference_ids(id.symbol_id()).first() {
            let node = self.scoping.get_reference(reference).node_id();
            let context = self.context(node);
            self.other(context, &format!("reads props through the rest `{}`", id.name), keys);
        }
    }

    /// The expression around the use at `node`, for the reason.
    fn context(&self, node: NodeId) -> Span {
        self.nodes.kind(self.parent(node)).span()
    }

    fn parent(&self, node: NodeId) -> NodeId {
        let mut parent = self.nodes.parent_id(node);
        while matches!(self.nodes.kind(parent), AstKind::ParenthesizedExpression(_)) {
            parent = self.nodes.parent_id(parent);
        }
        parent
    }

    /// Whether the value at `node` is the whole of a JSX child of a native element, in JSX the
    /// component returns without conditions, functions or components in between.
    fn is_insert(&self, node: NodeId) -> bool {
        let container = self.parent(node);
        if !matches!(self.nodes.kind(container), AstKind::JSXExpressionContainer(_)) {
            return false;
        }
        let mut current = self.nodes.parent_id(container);
        if !matches!(self.nodes.kind(current), AstKind::JSXElement(e) if is_native(e)) {
            return false;
        }
        loop {
            let parent = self.nodes.parent_id(current);
            match self.nodes.kind(parent) {
                AstKind::JSXElement(e) if is_native(e) => {}
                AstKind::JSXFragment(_)
                | AstKind::JSXExpressionContainer(_)
                | AstKind::ParenthesizedExpression(_) => {}
                AstKind::ArrowFunctionExpression(_) => return parent == self.function,
                AstKind::ReturnStatement(_) => {
                    let body = self.nodes.parent_id(parent);
                    return matches!(self.nodes.kind(body), AstKind::FunctionBody(_))
                        && self.nodes.parent_id(body) == self.function;
                }
                _ => return false,
            }
            current = parent;
        }
    }
}

fn is_native(element: &JSXElement<'_>) -> bool {
    match &element.opening_element.name {
        JSXElementName::Identifier(_) | JSXElementName::NamespacedName(_) => true,
        JSXElementName::IdentifierReference(id) => is_native_name(id.name.as_str()),
        JSXElementName::MemberExpression(_) | JSXElementName::ThisExpression(_) => false,
    }
}

/// The first `arguments` of the function, outside nested non-arrow functions.
#[derive(Default)]
struct ArgumentsRead {
    found: Option<Span>,
}

impl<'a> Visit<'a> for ArgumentsRead {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name == "arguments" && self.found.is_none() {
            self.found = Some(it.span);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
}
