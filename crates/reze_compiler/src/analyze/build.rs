use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::{
    ArrowFunctionExpression, Expression, JSXAttribute, JSXAttributeItem, JSXAttributeValue,
    JSXChild, JSXElement, JSXElementName, JSXMemberExpressionObject, ObjectPropertyKind,
    PropertyKey,
};
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};

use super::holes::{self, Position};
use super::{Api, ComponentFunction, Context, ImportSource};
use crate::diagnostic::{Code, Report};
use crate::html;
use crate::view::{
    Attribute, AttributeItem, Boundary, Branch, Call, CallTarget, Callback, ComponentRef, Content,
    Each, Element, Handler, Match, Node, Prop, PropValue, Show, Switch, Value,
};

pub struct Builder<'c, 'm, 'a> {
    cx: &'c Context<'m, 'a>,
    module: usize,
    source: &'a str,
    pub reports: &'c mut Vec<Report>,
    pub consumed: &'c mut HashSet<u32>,
    values: &'c mut HashMap<u32, Content<'a>>,
    path: Vec<String>,
}

fn is_jsx(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
}

fn is_blank(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => html::clean_jsx_text(&text.value).is_empty(),
        JSXChild::ExpressionContainer(container) => container.expression.as_expression().is_none(),
        _ => false,
    }
}

fn attribute_value<'a>(attribute: &'a JSXAttribute<'a>) -> Option<&'a Expression<'a>> {
    match &attribute.value {
        Some(JSXAttributeValue::ExpressionContainer(container)) => {
            container.expression.as_expression()
        }
        _ => None,
    }
}

impl<'c, 'm, 'a> Builder<'c, 'm, 'a> {
    pub fn new(
        cx: &'c Context<'m, 'a>,
        module: usize,
        reports: &'c mut Vec<Report>,
        consumed: &'c mut HashSet<u32>,
        values: &'c mut HashMap<u32, Content<'a>>,
        component: String,
    ) -> Self {
        let source = cx.modules[module].source;
        Self { cx, module, source, reports, consumed, values, path: vec![format!("<{component}>")] }
    }

    fn report(&mut self, report: Report) {
        let mut report = report;
        report.path = self.path.clone();
        self.reports.push(report);
    }

    fn text(&self, span: Span) -> &'a str {
        let source: &'a str = self.source;
        &source[span.start as usize..span.end as usize]
    }

    fn hole(
        &mut self,
        expression: &'a Expression<'a>,
        position: Position,
        container: Option<Span>,
    ) {
        let mut reports = Vec::new();
        holes::check(
            self.cx,
            self.module,
            expression,
            position,
            container,
            &mut reports,
            self.consumed,
        );
        for report in reports {
            self.report(report);
        }
    }

    pub fn template(&mut self, returned: &'a Expression<'a>) -> Option<Content<'a>> {
        self.jsx_content(returned)
    }

    fn jsx_content(&mut self, expression: &'a Expression<'a>) -> Option<Content<'a>> {
        match expression.without_parentheses() {
            Expression::JSXElement(element) => Some(self.element_content(element)),
            Expression::JSXFragment(fragment) => {
                self.consumed.insert(fragment.span.start);
                let nodes = self.children(&fragment.children);
                Some(Content { span: fragment.span, nodes })
            }
            _ => None,
        }
    }

    fn element_content(&mut self, element: &'a JSXElement<'a>) -> Content<'a> {
        let mut nodes = Vec::new();
        self.element(element, &mut nodes);
        Content { span: element.span, nodes }
    }

    fn children(&mut self, children: &'a [JSXChild<'a>]) -> Vec<Node<'a>> {
        let mut out: Vec<Node<'a>> = Vec::new();
        for child in children {
            match child {
                JSXChild::Text(text) => {
                    let cleaned = html::clean_jsx_text(&text.value);
                    if cleaned.is_empty() {
                        continue;
                    }
                    let decoded = html::decode_entities(&cleaned).into_owned();
                    match out.last_mut() {
                        Some(Node::Text(previous)) => previous.push_str(&decoded),
                        _ => out.push(Node::Text(decoded)),
                    }
                }
                JSXChild::Element(element) => self.element(element, &mut out),
                JSXChild::Fragment(fragment) => {
                    self.consumed.insert(fragment.span.start);
                    let nodes = self.children(&fragment.children);
                    out.extend(nodes);
                }
                JSXChild::ExpressionContainer(container) => {
                    let Some(expression) = container.expression.as_expression() else { continue };
                    if let Some(content) = self.jsx_content(expression) {
                        out.extend(content.nodes);
                        continue;
                    }
                    match static_text(expression) {
                        Some(None) => continue,
                        Some(Some(text)) => {
                            match out.last_mut() {
                                Some(Node::Text(previous)) => previous.push_str(text),
                                _ => out.push(Node::Text(text.to_string())),
                            }
                            continue;
                        }
                        None => {}
                    }
                    self.hole(expression, Position::Text, Some(container.span));
                    out.push(Node::Expr(expression));
                }
                JSXChild::Spread(spread) => {
                    self.report(Report::new(Code::SpreadChild, spread.span))
                }
            }
        }
        out
    }

    fn element(&mut self, element: &'a JSXElement<'a>, out: &mut Vec<Node<'a>>) {
        self.consumed.insert(element.span.start);
        match &element.opening_element.name {
            JSXElementName::Identifier(id) => {
                let tag = self.text(id.span);
                if tag == "head" {
                    let head = self.head(element);
                    out.push(Node::Head(head));
                } else {
                    let element = self.intrinsic(element, tag);
                    out.push(Node::Element(element));
                }
            }
            JSXElementName::NamespacedName(name) => {
                let element = self.intrinsic(element, self.text(name.span));
                out.push(Node::Element(element));
            }
            JSXElementName::IdentifierReference(reference) => {
                let name = reference.name.to_string();
                if let Some(api) = self.cx.api_of_reference(self.module, reference)
                    && api.is_tag()
                {
                    self.control(api, element, &name, out);
                    return;
                }
                let symbol = self.cx.symbol(self.module, reference);
                if let Some(component) = symbol.and_then(|s| self.cx.component(self.module, s)) {
                    let call = self.call(element, CallTarget::Component(component), name);
                    out.push(Node::Component(call));
                    return;
                }
                match symbol.and_then(|s| self.dynamic(s)) {
                    Some(Ok(target)) => {
                        let call = self.call(element, target, name);
                        out.push(Node::Component(call));
                        return;
                    }
                    Some(Err(report)) => {
                        self.report(report);
                        self.mark_consumed(element);
                        return;
                    }
                    None => {}
                }
                self.unknown(element, symbol, name);
            }
            JSXElementName::MemberExpression(member) => {
                let name = self.text(member.span).to_string();
                let target = match &member.object {
                    JSXMemberExpressionObject::IdentifierReference(object) => self
                        .cx
                        .symbol(self.module, object)
                        .and_then(|symbol| self.cx.facts[self.module].imports.get(&symbol))
                        .and_then(|import| match (&import.source, import.name.as_str()) {
                            (ImportSource::Module(target), "*") => {
                                self.cx.exported(*target, &member.property.name)
                            }
                            _ => None,
                        }),
                    _ => None,
                };
                match target {
                    Some(component) => {
                        let call = self.call(element, CallTarget::Component(component), name);
                        out.push(Node::Component(call));
                    }
                    None => self.unknown(element, None, name),
                }
            }
            JSXElementName::ThisExpression(_) => self.unknown(element, None, "this".into()),
        }
    }

    fn unknown(&mut self, element: &'a JSXElement<'a>, symbol: Option<SymbolId>, name: String) {
        let span = element.opening_element.name.span();
        let package =
            symbol.and_then(|s| self.cx.facts[self.module].imports.get(&s)).and_then(|import| {
                match &import.source {
                    ImportSource::Package(package) => Some(package.clone()),
                    ImportSource::Module(_) => None,
                }
            });
        match package {
            Some(package) => self.report(
                Report::new(Code::PackageSourceMissing, span)
                    .arg("package", package)
                    .arg("name", name),
            ),
            None => self.report(Report::new(Code::UnknownComponent, span).arg("name", name)),
        }
        self.mark_consumed(element);
    }

    fn mark_consumed(&mut self, element: &'a JSXElement<'a>) {
        holes::consume(self.consumed, element);
    }

    /// `const Widget = dynamic(table[key])` with `table` an object literal of components; `None` when
    /// the symbol is not declared by `dynamic`.
    fn dynamic(&mut self, symbol: SymbolId) -> Option<Result<CallTarget, Report>> {
        let semantic = &self.cx.modules[self.module].semantic;
        let declaration = semantic.scoping().symbol_declaration(symbol);
        let AstKind::VariableDeclarator(declarator) = semantic.nodes().get_node(declaration).kind()
        else {
            return None;
        };
        let Some(Expression::CallExpression(call)) =
            declarator.init.as_ref().map(Expression::without_parentheses)
        else {
            return None;
        };
        if self.cx.api(self.module, &call.callee) != Some(Api::Dynamic) {
            return None;
        }
        let unbounded = Report::new(Code::DynamicComponentUnbounded, call.span);
        let argument = call.arguments.first().and_then(|a| a.as_expression());
        let Some(Expression::ComputedMemberExpression(member)) =
            argument.map(Expression::without_parentheses)
        else {
            return Some(Err(unbounded));
        };
        let Expression::Identifier(table) = member.object.without_parentheses() else {
            self.report(unbounded);
            return None;
        };
        let candidates = self
            .cx
            .symbol(self.module, table)
            .and_then(|table| {
                let node = semantic.scoping().symbol_declaration(table);
                match semantic.nodes().get_node(node).kind() {
                    AstKind::VariableDeclarator(d) => {
                        match d.init.as_ref()?.without_parentheses() {
                            Expression::ObjectExpression(object) => Some(object),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            })
            .and_then(|object| {
                object
                    .properties
                    .iter()
                    .map(|property| {
                        let ObjectPropertyKind::ObjectProperty(property) = property else {
                            return None;
                        };
                        let key = match &property.key {
                            PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                            PropertyKey::StringLiteral(s) => s.value.to_string(),
                            _ => return None,
                        };
                        let Expression::Identifier(value) = property.value.without_parentheses()
                        else {
                            return None;
                        };
                        let component =
                            self.cx.component(self.module, self.cx.symbol(self.module, value)?)?;
                        Some((key, component))
                    })
                    .collect::<Option<Vec<(String, ComponentRef)>>>()
            });
        Some(match candidates {
            Some(candidates) => Ok(CallTarget::Dynamic { candidates }),
            None => Err(unbounded),
        })
    }

    fn intrinsic(&mut self, element: &'a JSXElement<'a>, tag: &'a str) -> Element<'a> {
        self.path.push(tag.to_string());
        let mut attributes = Vec::new();
        let mut handlers = Vec::new();
        let mut reference = None;
        let mut eager = false;
        for item in &element.opening_element.attributes {
            let attribute = match item {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    self.hole(&spread.argument, Position::Attribute, None);
                    attributes.push(AttributeItem::Spread(&spread.argument));
                    continue;
                }
                JSXAttributeItem::Attribute(attribute) => attribute,
            };
            let name = self.text(attribute.name.span());
            if let Some(event) = name.strip_prefix("on")
                && event.starts_with(|c: char| c.is_ascii_uppercase())
                && let Some(value) = attribute_value(attribute)
            {
                handlers.push(Handler { event: html::event_name(event), value });
                continue;
            }
            match name {
                "ref" => {
                    reference = attribute_value(attribute);
                    continue;
                }
                "eager" => {
                    eager = true;
                    continue;
                }
                _ => {}
            }
            let value = match &attribute.value {
                None => Value::True,
                Some(JSXAttributeValue::StringLiteral(literal)) => {
                    Value::Static(html::decode_entities(&literal.value).into_owned())
                }
                Some(JSXAttributeValue::ExpressionContainer(container)) => {
                    let Some(expression) = container.expression.as_expression() else { continue };
                    self.hole(expression, Position::Attribute, None);
                    Value::Expr(expression)
                }
                Some(_) => continue,
            };
            attributes.push(AttributeItem::Attribute(Attribute { name, value }));
        }
        let children = self.children(&element.children);
        self.path.pop();
        Element { tag, span: element.span, attributes, handlers, reference, eager, children }
    }

    fn head(&mut self, element: &'a JSXElement<'a>) -> Vec<Element<'a>> {
        self.path.push("head".into());
        let mut out = Vec::new();
        for node in self.children(&element.children) {
            if let Node::Element(child) = node {
                out.push(child);
            }
        }
        self.path.pop();
        out
    }

    fn prop_value(&mut self, attribute: &'a JSXAttribute<'a>) -> Option<PropValue<'a>> {
        Some(match &attribute.value {
            None => PropValue::Value(Value::True),
            Some(JSXAttributeValue::StringLiteral(literal)) => {
                PropValue::Value(Value::Static(html::decode_entities(&literal.value).into_owned()))
            }
            Some(JSXAttributeValue::Element(element)) => {
                PropValue::Content(self.element_content(element))
            }
            Some(JSXAttributeValue::Fragment(fragment)) => {
                self.consumed.insert(fragment.span.start);
                let nodes = self.children(&fragment.children);
                PropValue::Content(Content { span: fragment.span, nodes })
            }
            Some(JSXAttributeValue::ExpressionContainer(container)) => {
                let expression = container.expression.as_expression()?;
                self.expression_prop(expression)
            }
        })
    }

    fn expression_prop(&mut self, expression: &'a Expression<'a>) -> PropValue<'a> {
        if let Some(content) = self.jsx_content(expression) {
            return PropValue::Content(content);
        }
        self.hole(expression, Position::Prop, None);
        self.values_in(expression);
        PropValue::Value(Value::Expr(expression))
    }

    /// Registers JSX that a prop expression returns or holds, such as a render prop's result.
    fn values_in(&mut self, expression: &'a Expression<'a>) {
        match expression.without_parentheses() {
            Expression::JSXElement(_) | Expression::JSXFragment(_) => {
                if let Some(content) = self.jsx_content(expression) {
                    self.values.insert(content.span.start, content);
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                if let Some(returned) = returned(arrow) {
                    self.values_in(returned);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    if let Some(expression) = element.as_expression() {
                        self.values_in(expression);
                    }
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    if let ObjectPropertyKind::ObjectProperty(property) = property {
                        self.values_in(&property.value);
                    }
                }
            }
            _ => {}
        }
    }

    fn call(&mut self, element: &'a JSXElement<'a>, target: CallTarget, name: String) -> Call<'a> {
        self.path.push(format!("<{name}>"));
        let mut props = Vec::new();
        for item in &element.opening_element.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    props.push(Prop { name: "", value: PropValue::Spread(&spread.argument) });
                }
                JSXAttributeItem::Attribute(attribute) => {
                    let name = self.text(attribute.name.span());
                    if let Some(value) = self.prop_value(attribute) {
                        props.push(Prop { name, value });
                    }
                }
            }
        }
        let children = self.call_children(element);
        self.path.pop();
        Call { name, span: element.span, target, props, children }
    }

    fn call_children(&mut self, element: &'a JSXElement<'a>) -> Option<PropValue<'a>> {
        let significant: Vec<&'a JSXChild<'a>> =
            element.children.iter().filter(|child| !is_blank(child)).collect();
        if significant.is_empty() {
            return None;
        }
        if let [JSXChild::ExpressionContainer(container)] = significant.as_slice()
            && let Some(expression) = container.expression.as_expression()
            && !is_jsx(expression)
        {
            return Some(self.expression_prop(expression));
        }
        let span = children_span(element);
        let nodes = self.children(&element.children);
        Some(PropValue::Content(Content { span, nodes }))
    }

    fn callback(
        &mut self,
        arrow: &'a ArrowFunctionExpression<'a>,
        tag: &str,
    ) -> Option<Callback<'a>> {
        let Some(returned) = returned(arrow) else {
            self.report(
                Report::new(Code::ControlFlowChildren, arrow.span)
                    .arg("tag", tag)
                    .arg("expected", "a callback that returns markup"),
            );
            return None;
        };
        let content = match self.jsx_content(returned) {
            Some(content) => content,
            None => {
                self.hole(returned, Position::Text, None);
                Content { span: returned.span(), nodes: vec![Node::Expr(returned)] }
            }
        };
        Some(Callback { function: arrow, content })
    }

    fn branch_value(&mut self, attribute: &'a JSXAttribute<'a>, tag: &str) -> Option<Branch<'a>> {
        match &attribute.value {
            Some(JSXAttributeValue::ExpressionContainer(container)) => {
                let expression = container.expression.as_expression()?;
                if let Expression::ArrowFunctionExpression(arrow) = expression.without_parentheses()
                {
                    return self.callback(arrow, tag).map(Branch::Callback);
                }
                if let Some(content) = self.jsx_content(expression) {
                    return Some(Branch::Content(content));
                }
                self.hole(expression, Position::Text, None);
                Some(Branch::Content(Content {
                    span: expression.span(),
                    nodes: vec![Node::Expr(expression)],
                }))
            }
            Some(JSXAttributeValue::StringLiteral(literal)) => Some(Branch::Content(Content {
                span: literal.span,
                nodes: vec![Node::Text(html::decode_entities(&literal.value).into_owned())],
            })),
            Some(JSXAttributeValue::Element(element)) => {
                Some(Branch::Content(self.element_content(element)))
            }
            Some(JSXAttributeValue::Fragment(fragment)) => {
                self.consumed.insert(fragment.span.start);
                let nodes = self.children(&fragment.children);
                Some(Branch::Content(Content { span: fragment.span, nodes }))
            }
            None => None,
        }
    }

    /// Children of `<Show>` and the like: markup, or one callback.
    fn branch_children(
        &mut self,
        element: &'a JSXElement<'a>,
        tag: &str,
        callback: bool,
    ) -> Option<Branch<'a>> {
        let significant: Vec<&'a JSXChild<'a>> =
            element.children.iter().filter(|child| !is_blank(child)).collect();
        let arrows = significant.iter().filter_map(|child| match child {
            JSXChild::ExpressionContainer(container) => {
                match container.expression.as_expression()?.without_parentheses() {
                    Expression::ArrowFunctionExpression(arrow) => Some(&**arrow),
                    _ => None,
                }
            }
            _ => None,
        });
        let arrows: Vec<&'a ArrowFunctionExpression<'a>> = arrows.collect();
        match (arrows.as_slice(), significant.len()) {
            ([arrow], 1) if callback => self.callback(arrow, tag).map(Branch::Callback),
            ([], _) => {
                let span = children_span(element);
                let nodes = self.children(&element.children);
                Some(Branch::Content(Content { span, nodes }))
            }
            _ => {
                let expected = if callback { "markup or one callback" } else { "markup" };
                self.report(
                    Report::new(Code::ControlFlowChildren, element.opening_element.span)
                        .arg("tag", tag.to_string())
                        .arg("expected", expected),
                );
                self.mark_consumed(element);
                None
            }
        }
    }

    fn control(
        &mut self,
        api: Api,
        element: &'a JSXElement<'a>,
        tag: &str,
        out: &mut Vec<Node<'a>>,
    ) {
        let allowed: &[&str] = match api {
            Api::Show | Api::Match => &["when", "fallback"],
            Api::Switch => &["fallback"],
            Api::For => &["each", "key", "fallback"],
            Api::Repeat => &["count", "fallback"],
            Api::Loading | Api::Errored => &["fallback"],
            Api::Portal => &["mount"],
            _ => &[],
        };
        let mut attributes: HashMap<&'a str, &'a JSXAttribute<'a>> = HashMap::new();
        for item in &element.opening_element.attributes {
            match item {
                JSXAttributeItem::Attribute(attribute) => {
                    let name = self.text(attribute.name.span());
                    if allowed.contains(&name) && !(api == Api::Match && name == "fallback") {
                        attributes.insert(name, attribute);
                    } else {
                        self.report(
                            Report::new(Code::ControlFlowAttribute, attribute.span)
                                .arg("tag", tag.to_string())
                                .arg("attribute", name),
                        );
                    }
                }
                JSXAttributeItem::SpreadAttribute(spread) => self.report(
                    Report::new(Code::ControlFlowAttribute, spread.span)
                        .arg("tag", tag.to_string())
                        .arg("attribute", "{...}"),
                ),
            }
        }
        let required = match api {
            Api::Show | Api::Match => Some("when"),
            Api::For => Some("each"),
            Api::Repeat => Some("count"),
            _ => None,
        };
        let source = match required {
            Some(name) => match attributes.get(name).and_then(|a| attribute_value(a)) {
                Some(expression) => {
                    self.hole(expression, Position::Condition, None);
                    Some(expression)
                }
                None => {
                    self.report(
                        Report::new(Code::ControlFlowMissing, element.opening_element.span)
                            .arg("tag", tag.to_string())
                            .arg("attribute", name),
                    );
                    self.mark_consumed(element);
                    return;
                }
            },
            None => None,
        };
        self.path.push(format!("<{tag}>"));
        let fallback = attributes.get("fallback").and_then(|a| self.branch_value(a, tag));
        let node = match api {
            Api::Show | Api::Match => {
                let Some(children) = self.branch_children(element, tag, true) else {
                    self.path.pop();
                    return;
                };
                Some(Node::Show(Show { when: source.unwrap(), children, fallback }))
            }
            Api::Switch => {
                let mut matches = Vec::new();
                for child in &element.children {
                    if is_blank(child) {
                        continue;
                    }
                    let matched = match child {
                        JSXChild::Element(child) => match &child.opening_element.name {
                            JSXElementName::IdentifierReference(r) => {
                                (self.cx.api_of_reference(self.module, r) == Some(Api::Match))
                                    .then_some(&**child)
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    match matched {
                        Some(child) => {
                            let mut nodes = Vec::new();
                            self.consumed.insert(child.span.start);
                            self.control(Api::Match, child, "Match", &mut nodes);
                            if let Some(Node::Show(show)) = nodes.pop() {
                                matches.push(Match { when: show.when, children: show.children });
                            }
                        }
                        None => {
                            self.report(
                                Report::new(Code::ControlFlowChildren, child.span())
                                    .arg("tag", "Switch")
                                    .arg("expected", "only `<Match>` elements"),
                            );
                            if let JSXChild::Element(child) = child {
                                self.mark_consumed(child);
                            }
                        }
                    }
                }
                Some(Node::Switch(Switch { matches, fallback }))
            }
            Api::For | Api::Repeat => {
                let significant: Vec<&'a JSXChild<'a>> =
                    element.children.iter().filter(|child| !is_blank(child)).collect();
                let callback = match significant.as_slice() {
                    [JSXChild::ExpressionContainer(container)] => {
                        match container
                            .expression
                            .as_expression()
                            .map(Expression::without_parentheses)
                        {
                            Some(Expression::ArrowFunctionExpression(arrow)) => {
                                self.callback(arrow, tag)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                match callback {
                    Some(children) => {
                        let each = Each { source: source.unwrap(), children, fallback };
                        Some(if api == Api::For { Node::For(each) } else { Node::Repeat(each) })
                    }
                    None => {
                        self.report(
                            Report::new(Code::ControlFlowChildren, element.opening_element.span)
                                .arg("tag", tag.to_string())
                                .arg("expected", "one callback that renders a row"),
                        );
                        self.mark_consumed(element);
                        None
                    }
                }
            }
            Api::Loading | Api::Errored | Api::Portal => {
                let span_children = children_span(element);
                let nodes = self.children(&element.children);
                let boundary =
                    Boundary { fallback, children: Content { span: span_children, nodes } };
                Some(match api {
                    Api::Loading => Node::Loading(boundary),
                    Api::Errored => Node::Errored(boundary),
                    _ => Node::Portal(boundary),
                })
            }
            _ => None,
        };
        self.path.pop();
        if let Some(node) = node {
            out.push(node);
        }
    }
}

/// A text hole whose output is known at build time: `Some(None)` renders nothing.
fn static_text<'a>(expression: &'a Expression<'a>) -> Option<Option<&'a str>> {
    match expression.without_parentheses() {
        Expression::StringLiteral(literal) => Some(Some(literal.value.as_str())),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            template.quasis.first()?.value.cooked.as_ref().map(|cooked| Some(cooked.as_str()))
        }
        Expression::NullLiteral(_) | Expression::BooleanLiteral(_) => Some(None),
        Expression::Identifier(id) if id.name == "undefined" => Some(None),
        _ => None,
    }
}

fn children_span(element: &JSXElement<'_>) -> Span {
    let end = element.closing_element.as_ref().map_or(element.span.end, |c| c.span.start);
    Span::new(element.opening_element.span.end, end)
}

fn returned<'a>(arrow: &'a ArrowFunctionExpression<'a>) -> Option<&'a Expression<'a>> {
    ComponentFunction::Arrow(arrow).returned()
}
