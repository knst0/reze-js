use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::component::PropsBuilder;
use super::constant::{
    ClassKeys, Literal, is_dynamic, literal, literal_truthy, static_property, static_style,
};
use super::element::TemplateBuilder;
use super::{Lowerer, attribute_name, is_function};
use crate::analyze::Analysis;
use crate::diagnostic::{Code, Edit, Report};
use crate::html::{
    attribute_namespace, decode_entities, event_name, is_delegated_event, is_property,
    push_attribute_value, suggest_attribute,
};
use crate::ir::{AssignTarget, Bind, BindTarget, Handler, MemberKey, NodeId, Op, RefTarget, Value};
use crate::kind::{Kind as ValueKind, static_kind};

enum ClassPiece<'b, 'a> {
    Static(&'a str),
    Off(&'a str),
    Toggle(&'a str, &'b Expression<'a>),
}

struct ClassToggles<'b, 'a> {
    static_tokens: std::vec::Vec<&'a str>,
    toggles: std::vec::Vec<(&'a str, &'b Expression<'a>)>,
}

fn class_pieces<'b, 'a>(
    e: &'b Expression<'a>,
    analysis: &Analysis<'_>,
    pieces: &mut std::vec::Vec<ClassPiece<'b, 'a>>,
) -> Option<()> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => pieces.push(ClassPiece::Static(s.value.as_str())),
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let (key, value) = static_property(property)?;
                pieces.push(match literal_truthy(value, analysis) {
                    Some(true) => ClassPiece::Static(key),
                    Some(false) => ClassPiece::Off(key),
                    None if key.split_whitespace().count() == 1 && key.trim() == key => {
                        ClassPiece::Toggle(key, value)
                    }
                    None => return None,
                });
            }
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                class_pieces(element.as_expression()?, analysis, pieces)?;
            }
        }
        _ => return None,
    }
    Some(())
}

/// Splits class sources into static tokens and single-token toggles (SPEC §7.3); `None` when a
/// source is not a string, an object with static keys or an array of those, or when a token is
/// both static and toggled, toggled twice, or switched off by a literal.
fn class_toggles<'b, 'a>(
    values: &[AttrValue<'b, 'a>],
    analysis: &Analysis<'_>,
) -> Option<ClassToggles<'b, 'a>> {
    let mut pieces = std::vec::Vec::new();
    for value in values {
        match value {
            AttrValue::Str(s) => pieces.push(ClassPiece::Static(s)),
            AttrValue::Expr(e) => class_pieces(e, analysis, &mut pieces)?,
            AttrValue::Bare | AttrValue::Jsx(_) => {}
        }
    }
    let mut static_tokens: std::vec::Vec<&'a str> = std::vec::Vec::new();
    let mut off_tokens: std::vec::Vec<&'a str> = std::vec::Vec::new();
    let mut toggles: std::vec::Vec<(&'a str, &'b Expression<'a>)> = std::vec::Vec::new();
    for piece in pieces {
        match piece {
            ClassPiece::Static(key) => {
                for token in key.split_whitespace() {
                    if !static_tokens.contains(&token) {
                        static_tokens.push(token);
                    }
                }
            }
            ClassPiece::Off(key) => off_tokens.extend(key.split_whitespace()),
            ClassPiece::Toggle(token, value) => {
                if toggles.iter().any(|(t, _)| *t == token) {
                    return None;
                }
                toggles.push((token, value));
            }
        }
    }
    let is_toggled = |token: &str| toggles.iter().any(|(t, _)| *t == token);
    let has_conflict = static_tokens.iter().any(|t| off_tokens.contains(t) || is_toggled(t))
        || off_tokens.iter().any(|t| is_toggled(t));
    if toggles.is_empty() || has_conflict {
        return None;
    }
    Some(ClassToggles { static_tokens, toggles })
}

/// Whether a literal href may be a router path: none of empty, `#…` (except a hash-history `#/…`), `?…`, `//…` or `scheme:…`.
fn is_routable_href(href: &str) -> bool {
    let Some(first) = href.bytes().next() else { return false };
    if first == b'?' || (first == b'#' && !href.starts_with("#/")) || href.starts_with("//") {
        return false;
    }
    let scheme_end =
        href.bytes().position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-')));
    !(first.is_ascii_alphabetic() && scheme_end.is_some_and(|end| href.as_bytes()[end] == b':'))
}

enum AttrValue<'b, 'a> {
    Bare,
    Str(&'a str),
    Expr(&'b Expression<'a>),
    Jsx(crate::ir::Jsx<'a>),
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Attr(&'a str),
    AttrNs(&'static str, &'a str),
    Bool(&'a str),
    Prop(&'a str),
    /// A property whose literal initial value is written as a template attribute.
    InlineProp(&'a str),
    /// A property set after the element's children exist (`<select value>`, `<textarea value>`).
    LateProp(&'a str),
    Style,
}

impl<'a> Lowerer<'a, '_> {
    /// Returns the ops that must run after the element's children are inserted.
    pub(super) fn attributes(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        tag: &str,
        attrs: &[JSXAttributeItem<'a>],
        is_svg: bool,
    ) -> std::vec::Vec<Op<'a>> {
        let attrs: std::vec::Vec<&JSXAttribute<'a>> = attrs
            .iter()
            .filter_map(|item| match item {
                JSXAttributeItem::Attribute(a) => Some(&**a),
                JSXAttributeItem::SpreadAttribute(_) => None,
            })
            .collect();
        let names: std::vec::Vec<&'a str> = attrs.iter().map(|a| attribute_name(self, a)).collect();
        let is_overridden = self.duplicates(&attrs, &names);
        let class_sources: std::vec::Vec<&JSXAttribute<'a>> = attrs
            .iter()
            .zip(&names)
            .enumerate()
            .filter(|&(i, (_, name))| !is_overridden[i] && *name == "class")
            .map(|(_, (a, _))| *a)
            .collect();
        let link = self.claimed_href(tag, &attrs, &names, &is_overridden);
        let mut link_href = None;
        let mut deferred = std::vec::Vec::new();
        let mut is_class_done = false;
        for (i, (a, name)) in attrs.iter().zip(&names).enumerate() {
            if is_overridden[i] {
                continue;
            }
            if *name == "class" {
                if !is_class_done {
                    self.class(builder, node, &class_sources);
                    is_class_done = true;
                }
                continue;
            }
            if link == Some(i)
                && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                && let Some(e) = c.expression.as_expression()
                && is_dynamic(e, false, self.analysis)
            {
                self.check_signal_called(e);
                link_href = Some(self.getter(e));
                continue;
            }
            self.attribute(builder, node, tag, a, name, is_svg, &mut deferred);
        }
        if link.is_some() {
            builder.reference(node);
            builder.ops.push(Op::Link { node, href: link_href });
        }
        deferred
    }

    /// Index of the `href` that makes this element a claimed `<a>` (see `Settings::links`).
    fn claimed_href(
        &self,
        tag: &str,
        attrs: &[&JSXAttribute<'a>],
        names: &[&'a str],
        is_overridden: &[bool],
    ) -> Option<usize> {
        if !self.settings.links || tag != "a" {
            return None;
        }
        let is_aria_current = |name: &&str| {
            matches!(*name, "aria-current" | "attr:aria-current" | "prop:ariaCurrent")
        };
        if names.iter().any(is_aria_current) {
            return None;
        }
        let i = (0..attrs.len()).find(|&i| names[i] == "href" && !is_overridden[i])?;
        let is_routable = match &attrs[i].value {
            Some(JSXAttributeValue::StringLiteral(s)) => {
                is_routable_href(&decode_entities(s.value.as_str()))
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                match literal(c.expression.as_expression()?, self.analysis) {
                    Some(Literal::Str(s)) => is_routable_href(&s),
                    Some(Literal::Bool(_) | Literal::Nullish) => false,
                    None => true,
                }
            }
            None | Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => false,
        };
        is_routable.then_some(i)
    }

    /// Flags every attribute a later one with the same name overrides.
    fn duplicates(
        &mut self,
        attrs: &[&JSXAttribute<'a>],
        names: &[&'a str],
    ) -> std::vec::Vec<bool> {
        let mut is_overridden = vec![false; attrs.len()];
        let mut last: HashMap<&str, usize> = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            if let Some(previous) = last.insert(name, i) {
                is_overridden[previous] = true;
                let removal = self.removal(attrs[previous].span);
                self.report(
                    Report::new(Code::DuplicateAttribute, attrs[i].span)
                        .arg("attribute", *name)
                        .label(attrs[previous].span, "overridden here")
                        .fix(vec![removal]),
                );
            }
        }
        is_overridden
    }

    /// All class sources of one element as a single `class` (SPEC §7.3).
    fn class(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        sources: &[&JSXAttribute<'a>],
    ) {
        let mut values: std::vec::Vec<AttrValue<'_, 'a>> = std::vec::Vec::new();
        for a in sources {
            match self.attr_value(a) {
                Some(AttrValue::Expr(e)) => {
                    self.check_signal_called(e);
                    values.push(AttrValue::Expr(e));
                }
                Some(value @ AttrValue::Str(_)) => values.push(value),
                Some(AttrValue::Bare | AttrValue::Jsx(_)) | None => {}
            }
        }
        if values.is_empty() {
            return;
        }

        let mut keys = ClassKeys::default();
        let is_static = values.iter().all(|value| match value {
            AttrValue::Str(s) => {
                keys.set(s, !s.is_empty());
                true
            }
            AttrValue::Expr(e) => keys.add(e, self.analysis).is_some(),
            AttrValue::Bare | AttrValue::Jsx(_) => true,
        });
        if is_static {
            let class = keys.attribute();
            if !class.is_empty() {
                builder.html.push_str(" class");
                push_attribute_value(&mut builder.html, &class);
            }
            return;
        }
        if let Some(classes) = class_toggles(&values, self.analysis) {
            self.class_toggle_ops(builder, node, classes);
            return;
        }

        let is_reactive = values.iter().any(|value| match value {
            AttrValue::Expr(e) => is_dynamic(e, false, self.analysis),
            _ => false,
        });
        let is_string = matches!(
            values.as_slice(),
            [AttrValue::Expr(only)]
                if static_kind(only, self.analysis) == Some(ValueKind::String)
        );
        let mut parts = self.vec();
        for value in values {
            parts.push(match value {
                AttrValue::Str(s) => Value::Str(s),
                AttrValue::Expr(e) => Value::Expr(self.expr(e)),
                AttrValue::Bare | AttrValue::Jsx(_) => continue,
            });
        }
        let target = if is_string { BindTarget::Attr("class") } else { BindTarget::Class };
        let value = if parts.len() == 1 {
            parts.pop().expect("one part")
        } else {
            Value::ClassParts(parts)
        };
        builder.reference(node);
        if is_reactive {
            builder.binds.push(Bind { node, target, value });
        } else {
            builder.ops.push(Op::Set { node, target, value });
        }
    }

    fn class_toggle_ops(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        classes: ClassToggles<'_, 'a>,
    ) {
        if !classes.static_tokens.is_empty() {
            builder.html.push_str(" class");
            push_attribute_value(&mut builder.html, &classes.static_tokens.join(" "));
        }
        builder.reference(node);
        for (token, value) in classes.toggles {
            let target = BindTarget::ClassToggle(token);
            let toggle = Value::Truthy(self.expr(value));
            if is_dynamic(value, false, self.analysis) {
                builder.binds.push(Bind { node, target, value: toggle });
            } else {
                builder.ops.push(Op::Set { node, target, value: toggle });
            }
        }
    }

    fn attr_value<'b>(&mut self, a: &'b JSXAttribute<'a>) -> Option<AttrValue<'b, 'a>> {
        Some(match &a.value {
            None => AttrValue::Bare,
            Some(JSXAttributeValue::StringLiteral(s)) => {
                AttrValue::Str(self.str(&decode_entities(s.value.as_str())))
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                AttrValue::Expr(c.expression.as_expression()?)
            }
            Some(JSXAttributeValue::Element(e)) => AttrValue::Jsx(self.element(e)),
            Some(JSXAttributeValue::Fragment(f)) => AttrValue::Jsx(self.fragment(f)),
        })
    }

    /// `title={count}` where `count` is a signal getter (SIGNAL_NOT_CALLED).
    fn check_signal_called(&mut self, e: &Expression<'a>) {
        let Expression::Identifier(id) = e.without_parentheses() else { return };
        if !self.analysis.is_getter(id) {
            return;
        }
        let call = Edit { start: id.span.end, end: id.span.end, text: String::from("()") };
        self.report(
            Report::new(Code::SignalNotCalled, id.span)
                .arg("signal", id.name.as_str())
                .fix(vec![call]),
        );
    }

    #[allow(clippy::too_many_arguments, reason = "the element's lowering state travels together")]
    fn attribute(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        tag: &str,
        a: &JSXAttribute<'a>,
        name: &'a str,
        is_svg: bool,
        deferred: &mut std::vec::Vec<Op<'a>>,
    ) {
        if name == "children" {
            return;
        }
        let Some(value) = self.attr_value(a) else { return };
        if name == "ref" {
            if let AttrValue::Expr(e) = value {
                let target = self.ref_target(e);
                builder.reference(node);
                builder.ops.push(Op::Ref { node, target });
            }
            return;
        }
        if let Some(event) = name.strip_prefix("on:") {
            self.event(builder, node, event, false, value);
            return;
        }
        if name.len() > 2 && name.starts_with("on") && !name.contains(':') {
            let third = name.as_bytes()[2];
            if third.is_ascii_uppercase() {
                self.event(builder, node, &event_name(&name[2..]), true, value);
                return;
            }
            if third.is_ascii_lowercase() && matches!(value, AttrValue::Expr(_)) {
                self.event_lowercase(a, name);
                self.event(builder, node, &event_name(&name[2..]), true, value);
                return;
            }
        }

        let kind = self.kind(a, name, tag, is_svg);
        if let AttrValue::Expr(e) = &value {
            self.check_signal_called(e);
        }
        if let (Kind::Style, AttrValue::Expr(e)) = (kind, &value)
            && let Some(style) = static_style(e, self.analysis)
        {
            builder.html.push_str(" style");
            push_attribute_value(&mut builder.html, &style);
            return;
        }
        if self.inline_literal(builder, kind, &value) {
            return;
        }

        let target = match kind {
            Kind::Attr(n) => BindTarget::Attr(n),
            Kind::AttrNs(ns, n) => BindTarget::AttrNs(ns, n),
            Kind::Bool(n) => BindTarget::Bool(n),
            Kind::Prop(name) | Kind::InlineProp(name) | Kind::LateProp(name) => {
                BindTarget::Prop { name }
            }
            Kind::Style => BindTarget::Style,
        };
        builder.reference(node);
        if let AttrValue::Expr(e) = value
            && is_dynamic(e, false, self.analysis)
        {
            let value = Value::Expr(self.expr(e));
            builder.binds.push(Bind { node, target, value });
            return;
        }
        let value = match value {
            AttrValue::Bare => Value::True,
            AttrValue::Str(s) => Value::Str(s),
            AttrValue::Expr(e) => Value::Expr(self.expr(e)),
            AttrValue::Jsx(jsx) => Value::Jsx(jsx),
        };
        let op = Op::Set { node, target, value };
        if matches!(kind, Kind::LateProp(..)) { deferred.push(op) } else { builder.ops.push(op) }
    }

    fn kind(&mut self, a: &JSXAttribute<'a>, name: &'a str, tag: &str, is_svg: bool) -> Kind<'a> {
        if name == "style" {
            return Kind::Style;
        }
        if let Some(n) = name.strip_prefix("prop:") {
            return Kind::Prop(n);
        }
        if let Some(n) = name.strip_prefix("attr:") {
            return Kind::Attr(n);
        }
        if let Some(n) = name.strip_prefix("bool:") {
            return Kind::Bool(n);
        }
        if let Some(ns) = name.split_once(':').and_then(|(prefix, _)| attribute_namespace(prefix)) {
            return Kind::AttrNs(ns, name);
        }
        if !is_svg && is_property(name) {
            return match name {
                "value" if tag == "textarea" || tag == "select" => Kind::LateProp(name),
                "value" | "checked" | "selected" => Kind::InlineProp(name),
                _ => Kind::Prop(name),
            };
        }
        if name == "key" {
            let removal = self.removal(a.span);
            self.report(Report::new(Code::KeyOnElement, a.span).fix(vec![removal]));
        } else if !name.contains(['-', ':'])
            && let Some(suggestion) = suggest_attribute(name)
        {
            let name_span = a.name.span();
            let rename =
                Edit { start: name_span.start, end: name_span.end, text: suggestion.to_string() };
            self.report(
                Report::new(Code::UnknownAttribute, name_span)
                    .arg("attribute", name)
                    .arg("suggestion", suggestion)
                    .fix(vec![rename]),
            );
        }
        Kind::Attr(name)
    }

    /// Writes a literal value straight into the template; `false` when it is not one.
    fn inline_literal(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        kind: Kind<'a>,
        value: &AttrValue<'_, 'a>,
    ) -> bool {
        let html_name = match kind {
            Kind::Attr(n) | Kind::AttrNs(_, n) | Kind::InlineProp(n) | Kind::Bool(n) => n,
            Kind::Style => "style",
            Kind::Prop(..) | Kind::LateProp(..) => return false,
        };
        let literal = match value {
            AttrValue::Bare => Literal::Bool(true),
            AttrValue::Str(s) => Literal::Str(s.to_string()),
            AttrValue::Expr(e) => match literal(e, self.analysis) {
                Some(literal) => literal,
                None => return false,
            },
            AttrValue::Jsx(_) => return false,
        };
        let is_bool = matches!(kind, Kind::Bool(_));
        let is_bare_value = matches!(value, AttrValue::Bare);
        let html = &mut builder.html;
        match literal {
            Literal::Bool(true)
                if is_bare_value || is_bool || matches!(kind, Kind::InlineProp(..)) =>
            {
                html.push(' ');
                html.push_str(html_name);
            }
            Literal::Bool(true) => {
                html.push(' ');
                html.push_str(html_name);
                html.push_str("=true");
            }
            Literal::Str(s) if is_bool => {
                if !s.is_empty() {
                    html.push(' ');
                    html.push_str(html_name);
                }
            }
            Literal::Str(s) => {
                html.push(' ');
                html.push_str(html_name);
                push_attribute_value(html, &s);
            }
            Literal::Bool(false) | Literal::Nullish => {}
        }
        true
    }

    fn event_lowercase(&mut self, a: &JSXAttribute<'a>, name: &str) {
        let name_span = a.name.span();
        let mut camel = String::from("on");
        let rest = &name[2..];
        camel.push(rest.as_bytes()[0].to_ascii_uppercase() as char);
        camel.push_str(&rest[1..]);
        let rename = Edit { start: name_span.start, end: name_span.end, text: camel.clone() };
        self.report(
            Report::new(Code::EventNameLowercase, name_span)
                .arg("attribute", name)
                .arg("suggestion", camel)
                .fix(vec![rename]),
        );
    }

    fn event(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        event: &str,
        is_delegatable: bool,
        value: AttrValue<'_, 'a>,
    ) {
        let handler = match value {
            AttrValue::Expr(e) => e,
            AttrValue::Str(s) => {
                builder.html.push_str(" on");
                builder.html.push_str(event);
                push_attribute_value(&mut builder.html, s);
                return;
            }
            AttrValue::Bare | AttrValue::Jsx(_) => return,
        };
        let event = self.str(event);
        let handler = if !(is_delegatable && is_delegated_event(event)) {
            Handler::Direct(self.expr(handler))
        } else if is_function(handler) {
            Handler::Delegated { handler: self.expr(handler), data: None }
        } else if let Expression::ArrayExpression(array) = handler.without_parentheses()
            && array.elements.len() == 2
            && let (Some(f), Some(data)) =
                (array.elements[0].as_expression(), array.elements[1].as_expression())
        {
            Handler::Delegated { handler: self.expr(f), data: Some(self.expr(data)) }
        } else {
            Handler::DelegatedDynamic(self.expr(handler))
        };
        builder.reference(node);
        builder.ops.push(Op::Event { node, event, handler });
    }

    /// How a `ref` value receives the element (SPEC §7.6).
    pub(super) fn ref_target(&mut self, e: &Expression<'a>) -> RefTarget<'a> {
        if is_function(e) {
            return RefTarget::Callback(self.expr(e));
        }
        match self.assign_target(e) {
            Some(target) => RefTarget::Assign(target),
            None => RefTarget::Expr(self.expr(e)),
        }
    }

    /// `e` as an assignment target, when it is one.
    pub(super) fn assign_target(&mut self, e: &Expression<'a>) -> Option<AssignTarget<'a>> {
        Some(match e.without_parentheses() {
            Expression::Identifier(id) if !self.analysis.props.is_read(id) => {
                AssignTarget::Identifier(id.span)
            }
            Expression::StaticMemberExpression(m) if !m.optional => AssignTarget::Member {
                object: self.expr(&m.object),
                key: MemberKey::Static(m.property.name.as_str()),
            },
            Expression::ComputedMemberExpression(m) if !m.optional => AssignTarget::Member {
                object: self.expr(&m.object),
                key: MemberKey::Computed(self.expr(&m.expression)),
            },
            Expression::PrivateFieldExpression(m) if !m.optional => AssignTarget::Member {
                object: self.expr(&m.object),
                key: MemberKey::Static(self.str(&format!("#{}", m.field.name.as_str()))),
            },
            _ => return None,
        })
    }

    /// An element with a spread: every attribute goes through `spread`; children stay compiled.
    pub(super) fn spread(
        &mut self,
        builder: &mut TemplateBuilder<'a>,
        node: NodeId,
        attrs: &[JSXAttributeItem<'a>],
        is_svg: bool,
        has_children: bool,
    ) {
        let mut props = PropsBuilder::new(self.alloc);
        for attr in attrs {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    let is_reactive = is_dynamic(&s.argument, false, self.analysis);
                    let value = self.expr(&s.argument);
                    props.spread(value, is_reactive);
                }
                JSXAttributeItem::Attribute(a) => {
                    let name = attribute_name(self, a);
                    if name == "ref" {
                        if let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                            && let Some(e) = c.expression.as_expression()
                        {
                            let target = self.ref_target(e);
                            builder.ops.push(Op::Ref { node, target });
                        }
                        continue;
                    }
                    if name == "children" && has_children {
                        continue;
                    }
                    if let Some(prop) = self.prop(name, a, false) {
                        props.push(prop);
                    }
                }
            }
        }
        builder.reference(node);
        builder.ops.push(Op::Spread { node, props: props.finish(), is_svg, has_children });
    }
}
