use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;

use super::children::render_child;
use super::constant::static_text;
use super::{Lowerer, Tag, attribute_name, is_function};
use crate::analyze::Intrinsic;
use crate::diagnostic::{Code, Report};
use crate::html::{clean_jsx_text, decode_entities};
use crate::ir::{Branch, Child, Embed, Flow, Jsx, Render, Source};
use crate::kind::{Kind, static_kind};

const FOR_ROW: &str = "one function `(item, index) => …`";
const REPEAT_ROW: &str = "one function `(index) => …`";

impl<'a> Lowerer<'a, '_> {
    /// A `Show`, `For`, `Repeat`, `Switch`, `Match`, `Loading` or `Errored` tag; after an error, an empty fragment.
    pub(super) fn control_flow(&mut self, el: &JSXElement<'a>, intrinsic: Intrinsic) -> Jsx<'a> {
        self.has_jsx = true;
        self.path.push(format!("<{}>", intrinsic.name()));
        let flow = match intrinsic {
            Intrinsic::Show => self.show(el, intrinsic).map(Flow::Show),
            Intrinsic::For => self.for_flow(el),
            Intrinsic::Repeat => self.repeat(el),
            Intrinsic::Switch => self.switch(el),
            Intrinsic::Loading => self.loading(el),
            Intrinsic::Errored => self.errored(el),
            Intrinsic::Portal => self.portal(el),
            Intrinsic::Match => {
                self.report(Report::new(Code::MatchOutsideSwitch, el.opening_element.span));
                None
            }
        };
        self.path.pop();
        match flow {
            Some(flow) => Jsx::Flow(self.boxed(flow)),
            None => Jsx::Fragment(self.vec()),
        }
    }

    /// The accepted attributes of `el` in source order; every other one is reported.
    fn flow_attributes<'b>(
        &mut self,
        el: &'b JSXElement<'a>,
        intrinsic: Intrinsic,
        accepted: &[&str],
    ) -> std::vec::Vec<(&'a str, &'b JSXAttribute<'a>)> {
        let mut attributes = std::vec::Vec::new();
        for item in &el.opening_element.attributes {
            let (attribute, span) = match item {
                JSXAttributeItem::SpreadAttribute(spread) => ("{...}", spread.span),
                JSXAttributeItem::Attribute(a) => {
                    let name = attribute_name(self, a);
                    if accepted.contains(&name) {
                        attributes.push((name, &**a));
                        continue;
                    }
                    (name, a.span)
                }
            };
            let removal = self.removal(span);
            self.report(
                Report::new(Code::ControlFlowAttribute, span)
                    .arg("tag", intrinsic.name())
                    .arg("attribute", attribute)
                    .fix(vec![removal]),
            );
        }
        attributes
    }

    /// The value of a required attribute; reports CONTROL_FLOW_MISSING when it has none.
    fn required(
        &mut self,
        el: &JSXElement<'a>,
        intrinsic: Intrinsic,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
        name: &'static str,
    ) -> Option<Embed<'a>> {
        let value = attributes
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .and_then(|(_, a)| self.attribute_value(a));
        if value.is_none() {
            self.report(
                Report::new(Code::ControlFlowMissing, el.opening_element.span)
                    .arg("tag", intrinsic.name())
                    .arg("attribute", name),
            );
        }
        value
    }

    fn optional_source(
        &mut self,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
        name: &'static str,
    ) -> Option<Source<'a>> {
        let (_, a) = attributes.iter().rev().find(|(n, _)| *n == name)?;
        let expr = self.attribute_value(a)?;
        let getter = self.stable_getter(a);
        Some(Source { expr, getter })
    }

    fn required_source(
        &mut self,
        el: &JSXElement<'a>,
        intrinsic: Intrinsic,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
        name: &'static str,
    ) -> Option<Source<'a>> {
        let expr = self.required(el, intrinsic, attributes, name)?;
        let (_, a) = attributes.iter().rev().find(|(n, _)| *n == name)?;
        let getter = self.stable_getter(a);
        Some(Source { expr, getter })
    }

    fn stable_getter(&self, a: &JSXAttribute<'a>) -> Option<oxc_span::Span> {
        match a.value.as_ref()? {
            JSXAttributeValue::ExpressionContainer(c) => c.expression.as_expression(),
            _ => None,
        }
        .and_then(|e| self.stable_getter_callee(e))
    }

    fn attribute_value(&mut self, a: &JSXAttribute<'a>) -> Option<Embed<'a>> {
        match a.value.as_ref()? {
            JSXAttributeValue::ExpressionContainer(c) => {
                Some(self.expr(c.expression.as_expression()?))
            }
            JSXAttributeValue::StringLiteral(s) => Some(Embed { span: s.span, holes: self.vec() }),
            JSXAttributeValue::Element(e) => {
                Some(self.embed(e.span, |finder| finder.visit_jsx_element(e)))
            }
            JSXAttributeValue::Fragment(f) => {
                Some(self.embed(f.span, |finder| finder.visit_jsx_fragment(f)))
            }
        }
    }

    pub(super) fn fallback(
        &mut self,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
    ) -> Option<Render<'a>> {
        let (_, a) = attributes.iter().rev().find(|(name, _)| *name == "fallback")?;
        Some(match a.value.as_ref()? {
            JSXAttributeValue::ExpressionContainer(c) => self.render(c.expression.as_expression()?),
            JSXAttributeValue::StringLiteral(s) => {
                Render::Child(Child::Text(self.str(&decode_entities(s.value.as_str()))))
            }
            JSXAttributeValue::Element(e) => render_child(Child::Jsx(self.element(e))),
            JSXAttributeValue::Fragment(f) => render_child(Child::Jsx(self.fragment(f))),
        })
    }

    /// What a `Show` or `Match` renders: its one function child as is, or its children.
    fn case_children(&mut self, el: &JSXElement<'a>, intrinsic: Intrinsic) -> Option<Render<'a>> {
        let mut children = el.children.iter().filter(|child| is_meaningful(child));
        let Some(first) = children.next() else {
            self.report(
                Report::new(Code::ControlFlowChildren, el.opening_element.span)
                    .arg("tag", intrinsic.name())
                    .arg("expected", "at least one child"),
            );
            return None;
        };
        if children.next().is_none()
            && let JSXChild::ExpressionContainer(c) = first
            && let Some(e) = c.expression.as_expression().filter(|e| is_function(e))
        {
            return Some(Render::Function(self.expr(e)));
        }
        let items = self.items(&el.children, false);
        Some(render_child(Child::Jsx(Jsx::Fragment(self.list(items)))))
    }

    fn show(&mut self, el: &JSXElement<'a>, intrinsic: Intrinsic) -> Option<Branch<'a>> {
        let attributes = self.flow_attributes(el, intrinsic, &["when", "fallback"]);
        let when = self.required_source(el, intrinsic, &attributes, "when");
        let child = self.case_children(el, intrinsic);
        let fallback = self.fallback(&attributes);
        Some(Branch { when: when?, child: child?, fallback })
    }

    fn loading(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::Loading;
        let attributes = self.flow_attributes(el, intrinsic, &["fallback"]);
        let child = self.case_children(el, intrinsic);
        let fallback = self.fallback(&attributes);
        Some(Flow::Loading { child: child?, fallback })
    }

    fn errored(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::Errored;
        let attributes = self.flow_attributes(el, intrinsic, &["fallback"]);
        let child = self.case_children(el, intrinsic);
        let fallback = self.fallback_function(&attributes).or_else(|| self.fallback(&attributes));
        Some(Flow::Errored { child: child?, fallback })
    }

    fn portal(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::Portal;
        let attributes = self.flow_attributes(el, intrinsic, &["mount"]);
        let child = self.case_children(el, intrinsic);
        let mount = self.optional_source(&attributes, "mount");
        Some(Flow::Portal { child: child?, mount })
    }

    /// A `fallback` written as a function, which takes the arguments of its flow, passed as is.
    fn fallback_function(
        &mut self,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
    ) -> Option<Render<'a>> {
        let (_, a) = attributes.iter().rev().find(|(name, _)| *name == "fallback")?;
        let JSXAttributeValue::ExpressionContainer(c) = a.value.as_ref()? else { return None };
        let function = c.expression.as_expression().filter(|e| is_function(e))?;
        Some(Render::Function(self.expr(function)))
    }

    fn for_flow(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::For;
        let attributes =
            self.flow_attributes(el, intrinsic, &["each", "fallback", "keyed", "children"]);
        let keyed = attributes.iter().rev().find(|(name, _)| *name == "keyed").map(|(_, a)| *a);
        if !keyed.is_some_and(is_indexed)
            && let Some((_, a)) = attributes.iter().find(|(name, _)| *name == "each")
            && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
            && let Some(Expression::ArrayExpression(array)) =
                c.expression.as_expression().map(Expression::without_parentheses)
        {
            self.report(Report::new(Code::InlineEach, array.span));
        }
        let each = self.required_source(el, intrinsic, &attributes, "each");
        let map = self.row(el, &attributes);
        let fallback = self.fallback(&attributes);
        let key = match keyed {
            Some(a) => self.attribute_value(a),
            None => None,
        };
        let (map, selectors) = map?;
        Some(Flow::For { each: each?, map, fallback, key, selectors })
    }

    fn repeat(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::Repeat;
        let attributes = self.flow_attributes(el, intrinsic, &["count", "fallback", "children"]);
        let count = self.required_source(el, intrinsic, &attributes, "count");
        let row = self.row_function(el, &attributes, intrinsic, REPEAT_ROW);
        let fallback = self.fallback(&attributes);
        let row = row?;
        let times = self.static_count(&attributes).filter(|_| self.is_unrollable_row(row));
        let map = self.expr(row);
        match times {
            Some(times) => Some(Flow::Rows { times, map }),
            None => Some(Flow::Repeat { count: count?, map, fallback }),
        }
    }

    /// The `count` of a `Repeat` when it is a positive integer known at compile time.
    fn static_count(&self, attributes: &[(&'a str, &JSXAttribute<'a>)]) -> Option<u32> {
        let (_, a) = attributes.iter().rev().find(|(name, _)| *name == "count")?;
        let JSXAttributeValue::ExpressionContainer(c) = a.value.as_ref()? else { return None };
        let count = c.expression.as_expression()?;
        if static_kind(count, self.analysis) != Some(Kind::Numeric) {
            return None;
        }
        static_text(count, self.analysis)?.parse().ok().filter(|times| *times > 0)
    }

    /// Whether calling `row` outside a root and outside `untrack` is unobservable: it is an arrow with at most one
    /// plain parameter and a JSX body that creates nodes without running user code, and `Array` is the global.
    fn is_unrollable_row(&self, row: &Expression<'a>) -> bool {
        let Expression::ArrowFunctionExpression(arrow) = row.without_parentheses() else {
            return false;
        };
        let params = &arrow.params;
        let has_plain_params = params.rest.is_none()
            && params.items.len() <= 1
            && params.items.iter().all(|param| {
                matches!(param.pattern, BindingPattern::BindingIdentifier(_))
                    && param.initializer.is_none()
            });
        let Some(body) = arrow.body.as_expression().map(Expression::without_parentheses) else {
            return false;
        };
        let mut eager = EagerCheck { found: false };
        eager.visit_expression(body);
        has_plain_params
            && matches!(body, Expression::JSXElement(_) | Expression::JSXFragment(_))
            && !eager.found
            && !self.analysis.scoping.symbol_names().any(|name| name == "Array")
    }

    /// The row function of a `For` and the selectors its comparisons read.
    fn row(
        &mut self,
        el: &JSXElement<'a>,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
    ) -> Option<(Embed<'a>, oxc_allocator::Vec<'a, crate::ir::Selector<'a>>)> {
        let map = self.row_function(el, attributes, Intrinsic::For, FOR_ROW)?;
        let is_row_scope = self.enter_for(map);
        let embed = self.expr(map);
        let selectors = if is_row_scope { self.leave_for() } else { self.vec() };
        Some((embed, selectors))
    }

    /// The one child expression of `el`, or else its `children` attribute; reports `expected` when there is none.
    fn row_function<'b>(
        &mut self,
        el: &'b JSXElement<'a>,
        attributes: &[(&'a str, &'b JSXAttribute<'a>)],
        intrinsic: Intrinsic,
        expected: &'static str,
    ) -> Option<&'b Expression<'a>> {
        let mut children = el.children.iter().filter(|child| is_meaningful(child));
        let nested = match (children.next(), children.next()) {
            (None, _) => None,
            (Some(JSXChild::ExpressionContainer(c)), None) => c.expression.as_expression().map(Ok),
            (Some(child), _) => Some(Err(child.span())),
        };
        let attribute = attributes.iter().find(|(name, _)| *name == "children").map(|(_, a)| a);
        let map = match (nested, attribute) {
            (Some(Ok(e)), attribute) => {
                if let Some(a) = attribute {
                    self.children_ignored(a.span);
                }
                Ok(e)
            }
            (Some(Err(span)), _) => Err(span),
            (None, Some(a)) => match &a.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    c.expression.as_expression().ok_or(a.span)
                }
                _ => Err(a.span),
            },
            (None, None) => Err(el.opening_element.span),
        };
        match map {
            Ok(map) => Some(map),
            Err(span) => {
                self.report(
                    Report::new(Code::ControlFlowChildren, span)
                        .arg("tag", intrinsic.name())
                        .arg("expected", expected),
                );
                None
            }
        }
    }

    fn switch(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::Switch;
        let attributes = self.flow_attributes(el, intrinsic, &["fallback"]);
        let mut whens = self.vec();
        let mut children = self.vec();
        let mut is_valid = true;
        for child in el.children.iter().filter(|child| is_meaningful(child)) {
            let case = match child {
                JSXChild::Element(case)
                    if matches!(
                        self.tag_of(&case.opening_element.name),
                        Tag::Intrinsic(Intrinsic::Match)
                    ) =>
                {
                    case
                }
                other => {
                    if is_valid {
                        self.report(
                            Report::new(Code::ControlFlowChildren, other.span())
                                .arg("tag", intrinsic.name())
                                .arg("expected", "only `<Match>` elements"),
                        );
                    }
                    is_valid = false;
                    continue;
                }
            };
            self.path.push(String::from("<Match>"));
            let case_attributes = self.flow_attributes(case, Intrinsic::Match, &["when"]);
            let when = self.required_source(case, Intrinsic::Match, &case_attributes, "when");
            let render = self.case_children(case, Intrinsic::Match);
            self.path.pop();
            match (when, render) {
                (Some(when), Some(render)) => {
                    whens.push(when);
                    children.push(render);
                }
                _ => is_valid = false,
            }
        }
        let fallback = self.fallback(&attributes);
        is_valid.then_some(Flow::Switch { whens, children, fallback })
    }
}

/// Whitespace-only text and empty `{}` render nothing.
pub(super) fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(c) => c.expression.as_expression().is_some(),
        _ => true,
    }
}

/// Whether `a` is `keyed={false}`: rows follow positions, so an inline `each` reuses them.
fn is_indexed(a: &JSXAttribute<'_>) -> bool {
    matches!(
        &a.value,
        Some(JSXAttributeValue::ExpressionContainer(c))
            if matches!(
                c.expression.as_expression().map(Expression::without_parentheses),
                Some(Expression::BooleanLiteral(literal)) if !literal.value
            )
    )
}

/// Finds what creating JSX runs eagerly: a spread, a `ref`, a namespaced attribute or an event handler expression.
struct EagerCheck {
    found: bool,
}

impl<'a> Visit<'a> for EagerCheck {
    fn visit_jsx_spread_attribute(&mut self, _: &JSXSpreadAttribute<'a>) {
        self.found = true;
    }

    fn visit_jsx_spread_child(&mut self, _: &JSXSpreadChild<'a>) {
        self.found = true;
    }

    fn visit_jsx_attribute(&mut self, it: &JSXAttribute<'a>) {
        let JSXAttributeName::Identifier(name) = &it.name else {
            self.found = true;
            return;
        };
        let is_handler = name.name.starts_with("on")
            && matches!(&it.value, Some(JSXAttributeValue::ExpressionContainer(c))
                if c.expression.as_expression().is_none_or(|e| !is_function(e)));
        if name.name == "ref" || is_handler {
            self.found = true;
        } else {
            walk::walk_jsx_attribute(self, it);
        }
    }
}
