use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;

use super::children::render_child;
use super::{Lowerer, Tag, attribute_name, is_function};
use crate::analyze::Intrinsic;
use crate::diagnostic::{Code, Report};
use crate::html::{clean_jsx_text, decode_entities};
use crate::ir::{Branch, Child, Embed, Flow, Jsx, Render, Source};

const FOR_ROW: &str = "one function `(item, index) => …`";

impl<'a> Lowerer<'a, '_> {
    /// A `Show`, `For`, `Switch` or `Match` tag; after an error, an empty fragment.
    pub(super) fn control_flow(&mut self, el: &JSXElement<'a>, intrinsic: Intrinsic) -> Jsx<'a> {
        self.has_jsx = true;
        self.path.push(format!("<{}>", intrinsic.name()));
        let flow = match intrinsic {
            Intrinsic::Show => self.show(el, intrinsic).map(Flow::Show),
            Intrinsic::For => self.for_flow(el),
            Intrinsic::Switch => self.switch(el),
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

    fn required_source(
        &mut self,
        el: &JSXElement<'a>,
        intrinsic: Intrinsic,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
        name: &'static str,
    ) -> Option<Source<'a>> {
        let expr = self.required(el, intrinsic, attributes, name)?;
        let getter = attributes
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .and_then(|(_, a)| match a.value.as_ref()? {
                JSXAttributeValue::ExpressionContainer(c) => c.expression.as_expression(),
                _ => None,
            })
            .and_then(|e| self.stable_getter_callee(e));
        Some(Source { expr, getter })
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

    fn fallback(&mut self, attributes: &[(&'a str, &JSXAttribute<'a>)]) -> Option<Render<'a>> {
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

    fn for_flow(&mut self, el: &JSXElement<'a>) -> Option<Flow<'a>> {
        let intrinsic = Intrinsic::For;
        let attributes =
            self.flow_attributes(el, intrinsic, &["each", "fallback", "key", "children"]);
        if let Some((_, a)) = attributes.iter().find(|(name, _)| *name == "each")
            && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
            && let Some(Expression::ArrayExpression(array)) =
                c.expression.as_expression().map(Expression::without_parentheses)
        {
            self.report(Report::new(Code::InlineEach, array.span));
        }
        let each = self.required_source(el, intrinsic, &attributes, "each");
        let map = self.row(el, &attributes);
        let fallback = self.fallback(&attributes);
        let key = match attributes.iter().rev().find(|(name, _)| *name == "key") {
            Some((_, a)) => self.attribute_value(a),
            None => None,
        };
        let (map, selectors) = map?;
        Some(Flow::For { each: each?, map, fallback, key, selectors })
    }

    /// The row function of a `For` and the selectors its comparisons read.
    fn row(
        &mut self,
        el: &JSXElement<'a>,
        attributes: &[(&'a str, &JSXAttribute<'a>)],
    ) -> Option<(Embed<'a>, oxc_allocator::Vec<'a, crate::ir::Selector<'a>>)> {
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
        let map = match map {
            Ok(map) => map,
            Err(span) => {
                self.report(
                    Report::new(Code::ControlFlowChildren, span)
                        .arg("tag", Intrinsic::For.name())
                        .arg("expected", FOR_ROW),
                );
                return None;
            }
        };
        let is_row_scope = self.enter_for(map);
        let embed = self.expr(map);
        let selectors = if is_row_scope { self.leave_for() } else { self.vec() };
        Some((embed, selectors))
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
fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(c) => c.expression.as_expression().is_some(),
        _ => true,
    }
}
