//! Attributes of native elements (SPEC §7.2–§7.4, §7.6, §7.7).

use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::component::PropsBuilder;
use super::constant::{
    ClassKeys, Literal, is_dynamic, literal, literal_truthy, static_property, static_style,
};
use super::element::TemplateBuilder;
use super::types::{StaticKind, static_kind};
use super::{Lowerer, attribute_name, is_function};
use crate::diagnostic::{Code, Edit, Report};
use crate::html::{
    attribute_namespace, decode_entities, escape_attribute, event_name, is_delegated_event,
    is_property, suggest_attribute,
};
use crate::ir::{
    AssignTarget, Bind, BindTarget, Handler, MemberKey, NodeId, Op, PropHtml, RefTarget, Value,
};

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
    facts: &crate::analyze::Facts,
    pieces: &mut std::vec::Vec<ClassPiece<'b, 'a>>,
) -> Option<()> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => pieces.push(ClassPiece::Static(s.value.as_str())),
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let (key, value) = static_property(property)?;
                pieces.push(match literal_truthy(value, facts) {
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
                class_pieces(element.as_expression()?, facts, pieces)?;
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
    facts: &crate::analyze::Facts,
) -> Option<ClassToggles<'b, 'a>> {
    let mut pieces = std::vec::Vec::new();
    for value in values {
        match value {
            AttrValue::Str(s) => pieces.push(ClassPiece::Static(s)),
            AttrValue::Expr(e) => class_pieces(e, facts, &mut pieces)?,
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
    Prop(&'a str, PropHtml),
    /// A property whose literal initial value is written as a template attribute.
    InlineProp(&'a str, PropHtml),
    /// A property set after the element's children exist (`<select value>`, `<textarea value>`).
    LateProp(&'a str, PropHtml),
    Style,
}

fn is_class_like(name: &str) -> bool {
    matches!(name, "class" | "className" | "classList")
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
            .filter(|&(i, (_, name))| !is_overridden[i] && is_class_like(name))
            .map(|(_, (a, _))| *a)
            .collect();
        for (a, name) in attrs.iter().zip(&names) {
            if *name == "className" || *name == "classList" {
                self.class_alias(a, name);
            }
        }

        let mut deferred = std::vec::Vec::new();
        let mut is_class_done = false;
        for (i, (a, name)) in attrs.iter().zip(&names).enumerate() {
            if is_overridden[i] {
                continue;
            }
            if is_class_like(name) {
                if !is_class_done {
                    self.class(builder, node, &class_sources);
                    is_class_done = true;
                }
                continue;
            }
            self.attribute(builder, node, tag, a, name, is_svg, &mut deferred);
        }
        deferred
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
                    Report::new(
                        Code::DuplicateAttribute,
                        attrs[i].span,
                        format!(
                            "`{name}` is set twice on this element; the last one wins, so the \
                             earlier one is dead code. Remove it or merge the values."
                        ),
                    )
                    .label(attrs[previous].span, "overridden here")
                    .fix(format!("remove the earlier `{name}`"), vec![removal])
                    .data("attribute", *name),
                );
            }
        }
        is_overridden
    }

    fn class_alias(&mut self, a: &JSXAttribute<'a>, name: &str) {
        let name_span = a.name.span();
        self.report(
            Report::new(
                Code::ClassAlias,
                name_span,
                format!(
                    "`{name}` is a legacy alias: Reze has one `class` attribute that takes strings, \
                     objects and arrays. Rename it to `class`; it was compiled as `class`."
                ),
            )
            .fix(
                format!("rename `{name}` to `class`"),
                vec![Edit { start: name_span.start, end: name_span.end, text: "class".into() }],
            )
            .data("attribute", name),
        );
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
            AttrValue::Expr(e) => keys.add(e, self.facts).is_some(),
            AttrValue::Bare | AttrValue::Jsx(_) => true,
        });
        if is_static {
            let class = keys.attribute();
            if !class.is_empty() {
                builder.html.push_str(" class=\"");
                escape_attribute(&mut builder.html, &class);
                builder.html.push('"');
            }
            return;
        }
        if let Some(classes) = class_toggles(&values, self.facts) {
            self.class_toggle_ops(builder, node, classes);
            return;
        }

        let is_reactive = values.iter().any(|value| match value {
            AttrValue::Expr(e) => is_dynamic(e, false, self.facts),
            _ => false,
        });
        let is_string = matches!(
            values.as_slice(),
            [AttrValue::Expr(only)]
                if static_kind(only, self.facts, self.scoping) == Some(StaticKind::String)
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
        let inside = (!classes.static_tokens.is_empty()).then(|| {
            builder.html.push_str(" class=\"");
            escape_attribute(&mut builder.html, &classes.static_tokens.join(" "));
            let at = builder.html.len() as u32;
            builder.html.push('"');
            at
        });
        builder.reference(node);
        let reported = self.reports.len();
        let mut server = self.vec();
        for (token, value) in &classes.toggles {
            server.push((*token, self.expr(value)));
        }
        self.reports.truncate(reported);
        for (token, value) in classes.toggles {
            let target = BindTarget::ClassToggle(token);
            let toggle = Value::Truthy(self.expr(value));
            if is_dynamic(value, false, self.facts) {
                builder.binds.push(Bind { node, target, value: toggle });
            } else {
                builder.ops.push(Op::Set { node, target, value: toggle });
            }
        }
        builder.ops.push(Op::ServerClass { node, toggles: Value::ClassToggles(server), inside });
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
        if !self.facts.is_getter(id) {
            return;
        }
        let name = id.name.as_str();
        self.report(
            Report::new(
                Code::SignalNotCalled,
                id.span,
                format!(
                    "`{name}` is a signal getter passed without calling it: the DOM receives the \
                     function, not its value, and never updates. Call it: `{name}()`."
                ),
            )
            .fix(
                format!("call `{name}()`"),
                vec![Edit { start: id.span.end, end: id.span.end, text: "()".into() }],
            )
            .data("signal", name),
        );
    }

    #[allow(clippy::too_many_arguments)]
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
            && let Some(style) = static_style(e, self.facts)
        {
            builder.html.push_str(" style=\"");
            escape_attribute(&mut builder.html, &style);
            builder.html.push('"');
            return;
        }
        if self.inline_literal(builder, kind, &value) {
            return;
        }

        let target = match kind {
            Kind::Attr(n) => BindTarget::Attr(n),
            Kind::AttrNs(ns, n) => BindTarget::AttrNs(ns, n),
            Kind::Bool(n) => BindTarget::Bool(n),
            Kind::Prop(name, html) | Kind::InlineProp(name, html) | Kind::LateProp(name, html) => {
                BindTarget::Prop { name, html }
            }
            Kind::Style => BindTarget::Style,
        };
        builder.reference(node);
        if let AttrValue::Expr(e) = value
            && is_dynamic(e, false, self.facts)
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
            return Kind::Prop(n, PropHtml::None);
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
                "value" if tag == "textarea" => Kind::LateProp(name, PropHtml::Text),
                "value" if tag == "select" => Kind::LateProp(name, PropHtml::None),
                "value" => Kind::InlineProp(name, PropHtml::Attr),
                "checked" | "selected" => Kind::InlineProp(name, PropHtml::Bool),
                "innerHTML" => Kind::Prop(name, PropHtml::Html),
                _ => Kind::Prop(name, PropHtml::Text),
            };
        }
        if name == "key" {
            let removal = self.removal(a.span);
            self.report(
                Report::new(
                    Code::KeyOnElement,
                    a.span,
                    "`key` does nothing on a native element and renders as a useless attribute; \
                     rows of a list are keyed by `<For>`. Remove it.",
                )
                .fix("remove `key`", vec![removal]),
            );
        } else if !name.contains(['-', ':'])
            && let Some(suggestion) = suggest_attribute(name)
        {
            let name_span = a.name.span();
            self.report(
                Report::new(
                    Code::UnknownAttribute,
                    name_span,
                    format!(
                        "`{name}` is not a known attribute and renders as written, so the browser \
                         ignores it. Did you mean `{suggestion}`?"
                    ),
                )
                .fix(
                    format!("rename to `{suggestion}`"),
                    vec![Edit {
                        start: name_span.start,
                        end: name_span.end,
                        text: suggestion.into(),
                    }],
                )
                .data("attribute", name)
                .data("suggestion", suggestion),
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
            Kind::Attr(n) | Kind::AttrNs(_, n) | Kind::InlineProp(n, _) | Kind::Bool(n) => n,
            Kind::Style => "style",
            Kind::Prop(..) | Kind::LateProp(..) => return false,
        };
        let literal = match value {
            AttrValue::Bare => Literal::Bool(true),
            AttrValue::Str(s) => Literal::Str(s.to_string()),
            AttrValue::Expr(e) => match literal(e, self.facts) {
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
                html.push_str("=\"true\"");
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
                html.push_str("=\"");
                escape_attribute(html, &s);
                html.push('"');
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
        self.report(
            Report::new(
                Code::EventNameLowercase,
                name_span,
                format!(
                    "`{name}` passes a function to a lower-case attribute, which never attaches a \
                     listener; only `{camel}` or `on:{rest}` does. It was compiled as `{camel}`."
                ),
            )
            .fix(
                format!("rename to `{camel}`"),
                vec![Edit { start: name_span.start, end: name_span.end, text: camel.clone() }],
            )
            .data("attribute", name),
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
                builder.html.push_str("=\"");
                escape_attribute(&mut builder.html, s);
                builder.html.push('"');
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
            Expression::Identifier(id) if !self.facts.props.is_read(id) => {
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
                    let is_reactive = is_dynamic(&s.argument, false, self.facts);
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
                    let key = if name == "className" || name == "classList" {
                        self.class_alias(a, name);
                        "class"
                    } else {
                        name
                    };
                    if let Some(prop) = self.prop(key, a, false) {
                        props.push(prop);
                    }
                }
            }
        }
        builder.reference(node);
        builder.ops.push(Op::Spread { node, props: props.finish(), is_svg, has_children });
    }
}
