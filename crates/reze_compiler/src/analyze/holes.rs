use std::collections::HashSet;

use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AwaitExpression, CallExpression, Class,
    ConditionalExpression, Expression, Function, ImportExpression, JSXElement, JSXFragment,
    LogicalExpression, LogicalOperator, NewExpression, ObjectPropertyKind,
    TaggedTemplateExpression, UpdateExpression, YieldExpression,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::{Api, Context};
use crate::diagnostic::{Code, Edit, Report};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Text,
    Attribute,
    /// A component prop, where functions and JSX values pass through.
    Prop,
    /// `when`, `each`, `count`.
    Condition,
}

/// The methods of `Date`, `Set`, and `Map` that a hole may read through.
const READ_METHODS: &[&str] = &["get", "has", "getTime", "valueOf", "toISOString", "toJSON"];

const ARRAY_METHODS: &[&str] = &[
    "at",
    "concat",
    "copyWithin",
    "entries",
    "every",
    "fill",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "forEach",
    "includes",
    "indexOf",
    "join",
    "keys",
    "lastIndexOf",
    "map",
    "pop",
    "push",
    "reduce",
    "reduceRight",
    "reverse",
    "shift",
    "slice",
    "some",
    "sort",
    "splice",
    "toReversed",
    "toSorted",
    "toSpliced",
    "unshift",
    "values",
    "with",
];

fn is_jsx(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
}

fn contains_jsx(expression: &Expression<'_>) -> bool {
    let mut finder = JsxFinder(false);
    finder.visit_expression(expression);
    finder.0
}

struct JsxFinder(bool);

impl<'a> Visit<'a> for JsxFinder {
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.0 = true;
    }
    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.0 = true;
    }
}

struct Consume<'s>(&'s mut HashSet<u32>);

impl<'a> Visit<'a> for Consume<'_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        self.0.insert(it.span.start);
        walk::walk_jsx_element(self, it);
    }
    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        self.0.insert(it.span.start);
        walk::walk_jsx_fragment(self, it);
    }
}

/// Marks `element` and every JSX inside it as reported, so it is not reported again as JSX outside
/// a template.
pub fn consume(consumed: &mut HashSet<u32>, element: &JSXElement<'_>) {
    Consume(consumed).visit_jsx_element(element);
}

/// Marks every JSX inside `expression` as reported.
pub fn consume_expression(consumed: &mut HashSet<u32>, expression: &Expression<'_>) {
    Consume(consumed).visit_expression(expression);
}

pub fn check<'a>(
    cx: &Context<'_, 'a>,
    module: usize,
    expression: &'a Expression<'a>,
    position: Position,
    container: Option<Span>,
    reports: &mut Vec<Report>,
    consumed: &mut HashSet<u32>,
) {
    let mut checker = HoleCheck {
        cx,
        module,
        source: cx.modules[module].source,
        root: expression.without_parentheses().span(),
        container,
        reports,
        consumed,
        unsupported: false,
    };
    if position == Position::Prop {
        checker.value(expression);
    } else {
        checker.visit_expression(expression);
    }
}

struct HoleCheck<'c, 'm, 'a> {
    cx: &'c Context<'m, 'a>,
    module: usize,
    source: &'a str,
    root: Span,
    /// The `{…}` around a text hole, which a fix replaces.
    container: Option<Span>,
    reports: &'c mut Vec<Report>,
    consumed: &'c mut HashSet<u32>,
    unsupported: bool,
}

impl<'a> HoleCheck<'_, '_, 'a> {
    fn text(&self, span: Span) -> &'a str {
        let source: &'a str = self.source;
        &source[span.start as usize..span.end as usize]
    }

    /// Prop values: functions and JSX at the top level, or inside array and object literals, are
    /// values rather than holes.
    fn value(&mut self, expression: &Expression<'a>) {
        match expression.without_parentheses() {
            Expression::ArrowFunctionExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::JSXElement(_)
            | Expression::JSXFragment(_) => {}
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element.as_expression() {
                        Some(expression) => self.value(expression),
                        None => walk::walk_array_expression_element(self, element),
                    }
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            self.visit_property_key(&property.key);
                            self.value(&property.value);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument)
                        }
                    }
                }
            }
            other => self.visit_expression(other),
        }
    }

    fn unsupported(&mut self, span: Span, reason: String) {
        if self.unsupported {
            return;
        }
        self.unsupported = true;
        self.reports.push(Report::new(Code::ServerHoleUnsupported, span).arg("reason", reason));
    }

    fn consume_expression(&mut self, expression: &Expression<'a>) {
        Consume(self.consumed).visit_expression(expression);
    }

    fn import_edit(&self, api: Api, name: &str) -> Option<Edit> {
        let imported = self.cx.facts[self.module].apis.values().any(|a| *a == api);
        (!imported).then(|| Edit {
            start: 0,
            end: 0,
            text: format!("import {{ {name} }} from \"reze-js\";\n"),
        })
    }

    /// Edits that replace the whole hole with `replacement`, when the expression is the hole.
    fn rewrite(&self, span: Span, replacement: String, api: Api, name: &str) -> Option<Vec<Edit>> {
        let container = self.container?;
        if span != self.root {
            return None;
        }
        let mut edits: Vec<Edit> = self.import_edit(api, name).into_iter().collect();
        edits.push(Edit { start: container.start, end: container.end, text: replacement });
        Some(edits)
    }

    fn child(&self, expression: &Expression<'_>) -> String {
        let text = self.text(expression.span());
        if is_jsx(expression) { text.to_string() } else { format!("{{{text}}}") }
    }
}

impl<'a> Visit<'a> for HoleCheck<'_, '_, 'a> {
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {}
    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {}

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        if !is_jsx(&it.consequent) && !is_jsx(&it.alternate) {
            walk::walk_conditional_expression(self, it);
            return;
        }
        let fallback = match it.alternate.without_parentheses() {
            Expression::NullLiteral(_) => String::new(),
            Expression::Identifier(id) if id.name == "undefined" => String::new(),
            alternate => format!(" fallback={{{}}}", self.text(alternate.span())),
        };
        let replacement = format!(
            "<Show when={{{}}}{fallback}>{}</Show>",
            self.text(it.test.span()),
            self.child(&it.consequent)
        );
        let mut report = Report::new(Code::JsxConditionalExpression, it.span).arg("operator", "?:");
        if let Some(edits) = self.rewrite(it.span, replacement, Api::Show, "Show") {
            report = report.fix(edits);
        }
        self.reports.push(report);
        self.consume_expression(&it.consequent);
        self.consume_expression(&it.alternate);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        if !is_jsx(&it.left) && !is_jsx(&it.right) {
            walk::walk_logical_expression(self, it);
            return;
        }
        let mut report = Report::new(Code::JsxConditionalExpression, it.span)
            .arg("operator", it.operator.as_str());
        if it.operator == LogicalOperator::And {
            let replacement = format!(
                "<Show when={{{}}}>{}</Show>",
                self.text(it.left.span()),
                self.child(&it.right)
            );
            if let Some(edits) = self.rewrite(it.span, replacement, Api::Show, "Show") {
                report = report.fix(edits);
            }
        }
        self.reports.push(report);
        self.consume_expression(&it.left);
        self.consume_expression(&it.right);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let method = it.callee.get_member_expr().and_then(|member| member.static_property_name());
        if let (Some(method), Some(member)) = (method, it.callee.get_member_expr())
            && ARRAY_METHODS.contains(&method)
        {
            let renders = it.arguments.iter().any(|a| a.as_expression().is_some_and(contains_jsx));
            if renders {
                let mut report =
                    Report::new(Code::JsxListExpression, it.span).arg("method", method);
                if method == "map"
                    && let [argument] = it.arguments.as_slice()
                    && let Some(Expression::ArrowFunctionExpression(arrow)) =
                        argument.as_expression()
                {
                    let replacement = format!(
                        "<For each={{{}}}>{{{}}}</For>",
                        self.text(member.object().span()),
                        self.text(arrow.span)
                    );
                    if let Some(edits) = self.rewrite(it.span, replacement, Api::For, "For") {
                        report = report.fix(edits);
                    }
                }
                self.reports.push(report);
            } else {
                self.reports.push(Report::new(Code::JsxArrayMethod, it.span).arg("method", method));
            }
            for argument in &it.arguments {
                if let Some(expression) = argument.as_expression() {
                    self.consume_expression(expression);
                }
            }
            return;
        }
        if let (Some("get" | "has"), Some(member)) = (method, it.callee.get_member_expr())
            && let Expression::Identifier(object) = member.object().without_parentheses()
            && self.cx.initializer_api(self.module, object) == Some(Api::UseSearchParams)
        {
            walk::walk_arguments(self, &it.arguments);
            return;
        }
        if self.cx.api(self.module, &it.callee) == Some(Api::BuildPaths) {
            walk::walk_arguments(self, &it.arguments);
            return;
        }
        if let Some(member) = it.callee.get_member_expr()
            && method.is_some_and(|method| READ_METHODS.contains(&method))
        {
            self.visit_expression(member.object());
            walk::walk_arguments(self, &it.arguments);
            return;
        }
        let callee = self.text(it.callee.span());
        self.unsupported(
            it.span,
            format!("`{callee}()` calls a function the compiler does not analyze"),
        );
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        let callee = self.text(it.callee.span());
        self.unsupported(it.span, format!("`new {callee}` has no server lowering"));
    }

    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        self.unsupported(
            it.span,
            "A tagged template calls a function the compiler does not analyze".into(),
        );
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.unsupported(it.span, "A function is not a value a template can render".into());
    }

    fn visit_function(&mut self, it: &Function<'a>, _: ScopeFlags) {
        self.unsupported(it.span, "A function is not a value a template can render".into());
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        self.unsupported(it.span, "A class is not a value a template can render".into());
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        self.unsupported(
            it.span,
            "An assignment in a template writes state while rendering".into(),
        );
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        self.unsupported(it.span, "An update in a template writes state while rendering".into());
    }

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        self.unsupported(it.span, "`await` in a template has no server lowering".into());
    }

    fn visit_yield_expression(&mut self, it: &YieldExpression<'a>) {
        self.unsupported(it.span, "`yield` in a template has no server lowering".into());
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        self.unsupported(it.span, "`import()` in a template has no server lowering".into());
    }
}
