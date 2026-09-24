//! The local half of the static-component rules (SPEC §15.8): a component or constant is inert
//! up to its first violation, provided the program confirms its dependencies.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::symbol::SymbolId;
use serde::{Deserialize, Serialize};

use super::{Ref, TopLevel, is_blank_text};
use crate::facts::IslandMode;
use crate::lower::props::props_plan;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Violation {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

impl Violation {
    /// `` `<source of span>` what ``.
    pub(super) fn at(source: &str, span: Span, what: &str) -> Violation {
        Violation {
            start: span.start,
            end: span.end,
            message: format!("`{}` {what}", label(source, span)),
        }
    }
}

/// The source of `span`, shortened to 40 characters.
fn label(source: &str, span: Span) -> String {
    let text = &source[span.start as usize..span.end as usize];
    match text.char_indices().nth(40) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Dep {
    pub start: u32,
    pub end: u32,
    /// The dependent source text, shortened, for reasons.
    pub label: String,
    pub kind: DepKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DepKind {
    /// A read of a program binding: it must be an inert constant.
    Constant(Ref),
    /// `x.map(…)`: `x` must be an inert constant initialized with an array literal.
    ArrayConstant(Ref),
    /// `x()`: `x` must be a folded getter.
    Folded(Ref),
    /// `<C …/>`: `C` must be static, or a client component in a boundary position (§15.9).
    Element {
        /// Start of the JSX element.
        element: u32,
        callee: Ref,
        /// Whether the position can be an island boundary, with what that needs.
        boundary: Result<Boundary, Violation>,
    },
}

/// A position that can be an island boundary (§15.9, §16.3, §16.4).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Boundary {
    /// Reads of program bindings among the props: each must be an inert constant.
    pub deps: Vec<Dep>,
    pub mode: IslandMode,
    /// Props the parent renders to HTML, in source order; `children` for nested children. The
    /// island may use each only as a JSX insert.
    pub slots: Vec<String>,
}

pub struct Checked {
    pub violation: Option<Violation>,
    pub deps: Vec<Dep>,
    /// Starts of references to program bindings that stand in an inert position.
    pub sites: Vec<u32>,
}

pub enum Body<'b, 'a> {
    /// A function declaration or expression.
    Block(&'b FunctionBody<'a>),
    /// A block-bodied arrow.
    Arrow(&'b FunctionBody<'a>),
    Expression(&'b Expression<'a>),
}

type Check = Result<(), Violation>;

pub struct Checker<'s, 'a> {
    source: &'a str,
    scoping: &'s Scoping,
    top_level: &'s TopLevel,
    param: Option<SymbolId>,
    /// Destructured props bindings (§15.7) with the length of their props path.
    props: HashMap<SymbolId, usize>,
    locals: HashSet<SymbolId>,
    map_params: HashSet<SymbolId>,
    deps: Vec<Dep>,
    sites: Vec<u32>,
}

impl<'s, 'a> Checker<'s, 'a> {
    pub fn new(source: &'a str, scoping: &'s Scoping, top_level: &'s TopLevel) -> Self {
        Self {
            source,
            scoping,
            top_level,
            param: None,
            props: HashMap::new(),
            locals: HashSet::new(),
            map_params: HashSet::new(),
            deps: Vec::new(),
            sites: Vec::new(),
        }
    }

    fn finish(self, result: Check) -> Checked {
        Checked { violation: result.err(), deps: self.deps, sites: self.sites }
    }

    pub fn function(
        mut self,
        params: &FormalParameters<'a>,
        body: Body<'_, 'a>,
        is_async: bool,
        is_generator: bool,
    ) -> Checked {
        let result = self.check_function(params, body, is_async, is_generator);
        self.finish(result)
    }

    pub fn constant(mut self, init: &Expression<'a>) -> Checked {
        let result = self.expr(init);
        self.finish(result)
    }

    fn label(&self, span: Span) -> String {
        label(self.source, span)
    }

    fn violation(&self, span: Span, what: &str) -> Violation {
        Violation::at(self.source, span, what)
    }

    fn check_function(
        &mut self,
        params: &FormalParameters<'a>,
        body: Body<'_, 'a>,
        is_async: bool,
        is_generator: bool,
    ) -> Check {
        if is_async || is_generator {
            let kind = if is_async { "async" } else { "a generator" };
            return Err(Violation {
                start: params.span.start,
                end: params.span.end,
                message: format!("the component is {kind}"),
            });
        }
        let function_body = match body {
            Body::Block(block) => Some(block),
            Body::Expression(_) | Body::Arrow(_) => None,
        };
        if params.rest.is_some() || params.items.len() > 1 {
            return Err(self.violation(params.span, "takes more than one parameter"));
        }
        if let Some(param) = params.items.first() {
            match &param.pattern {
                BindingPattern::BindingIdentifier(id) => self.param = Some(id.symbol_id()),
                BindingPattern::ObjectPattern(_) => {
                    match props_plan(params, function_body, is_generator, self.scoping) {
                        Ok(Some(plan)) if plan.rest.is_none() => {
                            for binding in &plan.bindings {
                                self.props.insert(binding.symbol, binding.path.len());
                            }
                            for default in plan.bindings.iter().filter_map(|b| b.default) {
                                self.expr(default)?;
                            }
                        }
                        Ok(Some(_)) => {
                            return Err(
                                self.violation(param.span, "splits its props with a rest element")
                            );
                        }
                        Ok(None) | Err(_) => {
                            return Err(self.violation(
                                param.span,
                                "destructures props in a form that reads them once",
                            ));
                        }
                    }
                }
                _ => return Err(self.violation(param.span, "is not a plain props parameter")),
            }
        }
        match body {
            Body::Expression(e) => self.expr(e),
            Body::Block(block) | Body::Arrow(block) => self.block(block),
        }
    }

    fn block(&mut self, block: &FunctionBody<'a>) -> Check {
        let Some((last, declarations)) = block.statements.split_last() else {
            return Err(self.violation(block.span, "returns nothing"));
        };
        for statement in declarations {
            let Statement::VariableDeclaration(variables) = statement else {
                return Err(self.violation(
                    statement.span(),
                    "is a statement other than a `const` declaration",
                ));
            };
            if variables.kind != VariableDeclarationKind::Const {
                return Err(self.violation(variables.span, "is not a `const` declaration"));
            }
            for declarator in &variables.declarations {
                let (BindingPattern::BindingIdentifier(id), Some(init)) =
                    (&declarator.id, &declarator.init)
                else {
                    return Err(self.violation(declarator.span, "destructures a value"));
                };
                self.expr(init)?;
                self.locals.insert(id.symbol_id());
            }
        }
        match last {
            Statement::ReturnStatement(ret) => match &ret.argument {
                Some(argument) => self.expr(argument),
                None => Err(self.violation(ret.span, "returns nothing")),
            },
            other => Err(self.violation(other.span(), "does not end the body with `return`")),
        }
    }

    fn symbol_of(&self, id: &IdentifierReference<'_>) -> Option<SymbolId> {
        self.scoping.get_reference(id.reference_id.get()?).symbol_id()
    }

    /// The program binding `id` names, when it is top-level and not a namespace.
    fn program_ref(&self, id: &IdentifierReference<'_>) -> Option<Ref> {
        let symbol = self.symbol_of(id)?;
        if self.top_level.namespaces.contains(&symbol) {
            return None;
        }
        self.top_level.starts.get(&symbol).map(|&binding| Ref { binding, member: None })
    }

    /// `ns.member` of a namespace import.
    fn namespace_ref(&self, object: &Expression<'_>, member: &str) -> Option<Ref> {
        let Expression::Identifier(id) = object.without_parentheses() else { return None };
        let symbol = self.symbol_of(id)?;
        if !self.top_level.namespaces.contains(&symbol) {
            return None;
        }
        let binding = self.top_level.starts[&symbol];
        Some(Ref { binding, member: Some(member.to_string()) })
    }

    fn dep(&mut self, span: Span, kind: DepKind, site: u32) {
        self.sites.push(site);
        let label = self.label(span);
        self.deps.push(Dep { start: span.start, end: span.end, label, kind });
    }

    fn expr(&mut self, e: &Expression<'a>) -> Check {
        match e.without_parentheses() {
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_) => Ok(()),
            Expression::TemplateLiteral(t) => t.expressions.iter().try_for_each(|e| self.expr(e)),
            Expression::UnaryExpression(u) if u.operator != UnaryOperator::Delete => {
                self.expr(&u.argument)
            }
            Expression::BinaryExpression(b) => {
                self.expr(&b.left)?;
                self.expr(&b.right)
            }
            Expression::LogicalExpression(l) => {
                self.expr(&l.left)?;
                self.expr(&l.right)
            }
            Expression::ConditionalExpression(c) => {
                self.expr(&c.test)?;
                self.expr(&c.consequent)?;
                self.expr(&c.alternate)
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(s) => {
                            return Err(self.violation(s.span, "spreads a value"));
                        }
                        ArrayExpressionElement::Elision(_) => {}
                        _ => self.expr(element.to_expression())?,
                    }
                }
                Ok(())
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(p)
                            if !p.computed && !p.method && p.kind == PropertyKind::Init =>
                        {
                            self.expr(&p.value)?;
                        }
                        other => {
                            return Err(
                                self.violation(other.span(), "is not a plain object property")
                            );
                        }
                    }
                }
                Ok(())
            }
            Expression::Identifier(id) => self.identifier(id),
            Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
                self.member(e.without_parentheses())
            }
            Expression::CallExpression(call) => self.call(call),
            Expression::JSXElement(element) => self.element(element),
            Expression::JSXFragment(fragment) => self.children(&fragment.children),
            other => Err(self.violation(other.span(), "is not an inert expression")),
        }
    }

    fn identifier(&mut self, id: &IdentifierReference<'a>) -> Check {
        let Some(symbol) = self.symbol_of(id) else {
            if id.name == "undefined" {
                return Ok(());
            }
            return Err(self.violation(id.span, "is a global"));
        };
        if Some(symbol) == self.param
            || self.locals.contains(&symbol)
            || self.map_params.contains(&symbol)
        {
            return Ok(());
        }
        if let Some(&depth) = self.props.get(&symbol) {
            return if depth == 1 {
                Ok(())
            } else {
                Err(self.violation(id.span, "reads a prop deeper than one level"))
            };
        }
        match self.program_ref(id) {
            Some(target) => {
                self.dep(id.span, DepKind::Constant(target), id.span.start);
                Ok(())
            }
            None => Err(self.violation(id.span, "is not a constant")),
        }
    }

    /// `p.k`, `x.k…` of an inert constant, or `item.k…` of a `map` parameter.
    fn member(&mut self, e: &Expression<'a>) -> Check {
        let mut keys: Vec<&str> = Vec::new();
        let mut object = e;
        loop {
            match object.without_parentheses() {
                Expression::StaticMemberExpression(m) if !m.optional => {
                    keys.push(m.property.name.as_str());
                    object = &m.object;
                }
                Expression::ComputedMemberExpression(m) if !m.optional => {
                    match m.expression.without_parentheses() {
                        Expression::StringLiteral(s) => keys.push(s.value.as_str()),
                        Expression::NumericLiteral(_) => keys.push(""),
                        _ => return Err(self.violation(e.span(), "reads a computed key")),
                    }
                    object = &m.object;
                }
                _ => break,
            }
        }
        keys.reverse();
        let Expression::Identifier(root) = object.without_parentheses() else {
            return Err(
                self.violation(e.span(), "reads a member of a value that is not a constant")
            );
        };
        let symbol = self.symbol_of(root);
        if symbol.is_some() && symbol == self.param {
            return if keys.len() == 1 {
                Ok(())
            } else {
                Err(self.violation(e.span(), "reads a prop deeper than one level"))
            };
        }
        if symbol.is_some_and(|s| self.map_params.contains(&s)) {
            return Ok(());
        }
        let target = match self.program_ref(root) {
            Some(target) => Some(target),
            None => keys.first().and_then(|member| self.namespace_ref(object, member)),
        };
        match target {
            Some(target) => {
                self.dep(e.span(), DepKind::Constant(target), root.span.start);
                Ok(())
            }
            None => {
                Err(self.violation(e.span(), "reads a member of a value that is not a constant"))
            }
        }
    }

    fn call(&mut self, call: &CallExpression<'a>) -> Check {
        if call.optional || call.type_arguments.is_some() {
            return Err(self.violation(call.span, "is a call"));
        }
        if call.arguments.is_empty() {
            let target = match call.callee.without_parentheses() {
                Expression::Identifier(id) => self.program_ref(id).map(|t| (t, id.span.start)),
                Expression::StaticMemberExpression(m) if !m.optional => self
                    .namespace_ref(&m.object, m.property.name.as_str())
                    .map(|t| (t, m.span.start)),
                _ => None,
            };
            return match target {
                Some((target, site)) => {
                    self.dep(call.span, DepKind::Folded(target), site);
                    Ok(())
                }
                None => Err(self.violation(call.span, "is a call")),
            };
        }
        if let Expression::StaticMemberExpression(m) = call.callee.without_parentheses()
            && !m.optional
            && m.property.name == "map"
            && call.arguments.len() == 1
            && let Expression::Identifier(array) = m.object.without_parentheses()
            && let Some(target) = self.program_ref(array)
            && let Some(Expression::ArrowFunctionExpression(callback)) =
                call.arguments[0].as_expression().map(Expression::without_parentheses)
            && callback.is_expression()
            && !callback.r#async
            && callback.params.rest.is_none()
            && callback.params.items.len() <= 2
        {
            for param in &callback.params.items {
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return Err(self.violation(param.span, "destructures a `map` item"));
                };
                self.map_params.insert(id.symbol_id());
            }
            self.dep(m.object.span(), DepKind::ArrayConstant(target), array.span.start);
            let body = callback.get_expression().expect("expression body");
            return self.expr(body);
        }
        Err(self.violation(call.span, "is a call"))
    }

    fn children(&mut self, children: &[JSXChild<'a>]) -> Check {
        for child in children {
            match child {
                JSXChild::Text(_) => {}
                JSXChild::Element(element) => self.element(element)?,
                JSXChild::Fragment(fragment) => self.children(&fragment.children)?,
                JSXChild::ExpressionContainer(c) => {
                    if let Some(e) = c.expression.as_expression() {
                        self.expr(e)?;
                    }
                }
                JSXChild::Spread(s) => return Err(self.violation(s.span, "spreads children")),
            }
        }
        Ok(())
    }

    fn element(&mut self, element: &JSXElement<'a>) -> Check {
        let opening = &element.opening_element;
        let callee = match &opening.name {
            JSXElementName::Identifier(_) | JSXElementName::NamespacedName(_) => None,
            JSXElementName::IdentifierReference(id) => {
                let name = id.name.as_str();
                if name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-') {
                    None
                } else {
                    Some(self.program_ref(id).ok_or_else(|| {
                        self.violation(id.span, "is not a component of the program")
                    })?)
                }
            }
            JSXElementName::MemberExpression(member) => {
                let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object
                else {
                    return Err(self.violation(member.span, "is not a component of the program"));
                };
                let target = self
                    .symbol_of(namespace)
                    .filter(|s| self.top_level.namespaces.contains(s))
                    .map(|s| Ref {
                        binding: self.top_level.starts[&s],
                        member: Some(member.property.name.to_string()),
                    });
                Some(target.ok_or_else(|| {
                    self.violation(member.span, "is not a component of the program")
                })?)
            }
            JSXElementName::ThisExpression(t) => {
                return Err(self.violation(t.span, "is not a component of the program"));
            }
        };
        match callee {
            None => {
                let tag = tag_name(&opening.name);
                for attribute in &opening.attributes {
                    self.native_attribute(attribute, &tag)?;
                }
            }
            Some(callee) => {
                for attribute in &opening.attributes {
                    match attribute {
                        JSXAttributeItem::SpreadAttribute(s) => {
                            return Err(self.violation(s.span, "spreads props"));
                        }
                        JSXAttributeItem::Attribute(a) => self.attribute_value(a)?,
                    }
                }
                let boundary = self.boundary(element);
                let name_span = opening.name.span();
                self.sites.push(name_span.start);
                self.deps.push(Dep {
                    start: name_span.start,
                    end: name_span.end,
                    label: format!("<{}>", self.label(name_span)),
                    kind: DepKind::Element { element: element.span.start, callee, boundary },
                });
            }
        }
        self.children(&element.children)
    }

    fn native_attribute(&mut self, attribute: &JSXAttributeItem<'a>, tag: &str) -> Check {
        let a = match attribute {
            JSXAttributeItem::SpreadAttribute(s) => {
                return Err(self.violation(s.span, "spreads attributes"));
            }
            JSXAttributeItem::Attribute(a) => a,
        };
        let name = attribute_name(a);
        let behavior = if name == "ref" {
            Some("attaches a ref")
        } else if name.starts_with("on") && name.len() > 2 {
            Some("attaches an event handler")
        } else if name.starts_with("prop:") {
            Some("sets a client-only property")
        } else if name == "value" && matches!(tag, "select" | "textarea") {
            Some("sets a property the server does not render")
        } else {
            None
        };
        if let Some(behavior) = behavior {
            return Err(self.violation(a.name.span(), behavior));
        }
        self.attribute_value(a)
    }

    fn attribute_value(&mut self, a: &JSXAttribute<'a>) -> Check {
        match &a.value {
            None | Some(JSXAttributeValue::StringLiteral(_)) => Ok(()),
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(e) => self.expr(e),
                None => Ok(()),
            },
            Some(JSXAttributeValue::Element(e)) => self.element(e),
            Some(JSXAttributeValue::Fragment(f)) => self.children(&f.children),
        }
    }

    /// Whether `<C …/>` can be an island boundary (§15.9, §16.3, §16.4): no spread or ref;
    /// nested children and JSX-form values are slots, `island:load` with a mode name is the
    /// load mode, and every other value is a JSON form or an inert read of `p.k`, a constant or
    /// a `map` parameter.
    fn boundary(&self, element: &JSXElement<'a>) -> Result<Boundary, Violation> {
        let has_children = element.children.iter().any(|c| !is_blank_text(c));
        let mut boundary =
            Boundary { deps: Vec::new(), mode: IslandMode::Eager, slots: Vec::new() };
        for attribute in &element.opening_element.attributes {
            let a = match attribute {
                JSXAttributeItem::SpreadAttribute(s) => {
                    return Err(self.violation(s.span, "spreads props into an island"));
                }
                JSXAttributeItem::Attribute(a) => a,
            };
            let name = attribute_name(a);
            if name == "ref" {
                return Err(self.violation(a.span, "cannot cross an island boundary"));
            }
            if name == "children" && has_children {
                continue;
            }
            if let Some(mode) = island_load_mode(a) {
                boundary.mode = mode;
                continue;
            }
            if is_json_attribute(a) {
                continue;
            }
            if is_slot_value(a) {
                if !boundary.slots.contains(&name) {
                    boundary.slots.push(name);
                }
                continue;
            }
            let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value else {
                return Err(self.violation(a.span, "is not serializable to an island"));
            };
            let Some(e) = c.expression.as_expression() else { continue };
            match self.serializable_read(e) {
                Some(Some(dep)) => boundary.deps.push(dep),
                Some(None) => {}
                None => return Err(self.violation(e.span(), "is not serializable to an island")),
            }
        }
        if has_children {
            boundary.slots.push("children".to_string());
        }
        Ok(boundary)
    }

    /// `Some(None)` for `p.k` and `map` parameter reads, `Some(Some(dep))` for a constant read.
    fn serializable_read(&self, e: &Expression<'a>) -> Option<Option<Dep>> {
        let mut object = e.without_parentheses();
        let mut depth = 0;
        let mut first_key = None;
        loop {
            match object {
                Expression::StaticMemberExpression(m) if !m.optional => {
                    first_key = Some(m.property.name.as_str());
                    object = m.object.without_parentheses();
                }
                Expression::ComputedMemberExpression(m)
                    if !m.optional
                        && matches!(
                            m.expression.without_parentheses(),
                            Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                        ) =>
                {
                    first_key = None;
                    object = m.object.without_parentheses();
                }
                _ => break,
            }
            depth += 1;
        }
        let Expression::Identifier(root) = object else { return None };
        let symbol = self.symbol_of(root);
        if symbol.is_some() && symbol == self.param {
            return (depth == 1).then_some(None);
        }
        if let Some(&path) = symbol.and_then(|s| self.props.get(&s)) {
            return (depth == 0 && path == 1).then_some(None);
        }
        if symbol.is_some_and(|s| self.map_params.contains(&s)) {
            return Some(None);
        }
        let target = match self.program_ref(root) {
            Some(target) => target,
            None => self.namespace_ref(object, first_key?)?,
        };
        let span = e.span();
        let label = self.label(span);
        Some(Some(Dep { start: span.start, end: span.end, label, kind: DepKind::Constant(target) }))
    }
}

fn attribute_name(a: &JSXAttribute<'_>) -> String {
    match &a.name {
        JSXAttributeName::Identifier(id) => id.name.to_string(),
        JSXAttributeName::NamespacedName(n) => format!("{}:{}", n.namespace.name, n.name.name),
    }
}

fn tag_name(name: &JSXElementName<'_>) -> String {
    match name {
        JSXElementName::Identifier(id) => id.name.to_string(),
        JSXElementName::IdentifierReference(id) => id.name.to_string(),
        JSXElementName::NamespacedName(n) => format!("{}:{}", n.namespace.name, n.name.name),
        JSXElementName::MemberExpression(m) => m.property.name.to_string(),
        JSXElementName::ThisExpression(_) => "this".to_string(),
    }
}

/// The mode of `island:load="eager" | "idle" | "visible" | "interaction"` (§16.3); `None` for
/// any other attribute or value.
pub fn island_load_mode(a: &JSXAttribute<'_>) -> Option<IslandMode> {
    let JSXAttributeName::NamespacedName(name) = &a.name else { return None };
    if name.namespace.name != "island" || name.name.name != "load" {
        return None;
    }
    let value = match &a.value {
        Some(JSXAttributeValue::StringLiteral(s)) => s.value.as_str(),
        Some(JSXAttributeValue::ExpressionContainer(c)) => {
            match c.expression.as_expression()?.without_parentheses() {
                Expression::StringLiteral(s) => s.value.as_str(),
                _ => return None,
            }
        }
        _ => return None,
    };
    IslandMode::from_directive(value)
}

/// A JSX element, a fragment, or an array literal of those and text: an island slot (§16.4).
fn is_slot_value(a: &JSXAttribute<'_>) -> bool {
    match &a.value {
        Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => true,
        Some(JSXAttributeValue::ExpressionContainer(c)) => {
            c.expression.as_expression().is_some_and(is_jsx_form)
        }
        None | Some(JSXAttributeValue::StringLiteral(_)) => false,
    }
}

fn is_jsx_form(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            !matches!(
                element,
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_)
            ) && match element.to_expression().without_parentheses() {
                Expression::StringLiteral(_) => true,
                Expression::TemplateLiteral(t) => t.expressions.is_empty(),
                other => is_jsx_form(other),
            }
        }),
        _ => false,
    }
}

/// A bare attribute, a string, or `{…}` holding a JSON form: a string, a finite number other
/// than `-0`, `true`/`false`/`null`, or arrays and objects of JSON forms.
pub fn is_json_attribute(a: &JSXAttribute<'_>) -> bool {
    match &a.value {
        None | Some(JSXAttributeValue::StringLiteral(_)) => true,
        Some(JSXAttributeValue::ExpressionContainer(c)) => {
            c.expression.as_expression().is_some_and(is_json_form)
        }
        Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => false,
    }
}

fn is_json_form(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::StringLiteral(_) | Expression::BooleanLiteral(_) | Expression::NullLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::NumericLiteral(n) => n.value.is_finite(),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            matches!(u.argument.without_parentheses(), Expression::NumericLiteral(n) if n.value.is_finite() && n.value != 0.0)
        }
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            !matches!(element, ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_))
                && is_json_form(element.to_expression())
        }),
        Expression::ObjectExpression(object) => object.properties.iter().all(|property| match property {
            ObjectPropertyKind::ObjectProperty(p) => {
                !p.computed
                    && !p.method
                    && p.kind == PropertyKind::Init
                    && !matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "__proto__")
                    && is_json_form(&p.value)
            }
            ObjectPropertyKind::SpreadProperty(_) => false,
        }),
        _ => false,
    }
}
