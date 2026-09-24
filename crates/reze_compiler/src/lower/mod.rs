mod async_component;
mod attribute;
mod children;
mod component;
pub mod constant;
mod element;

use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::Facts;
use crate::diagnostic::{Code, Edit, Report};
use crate::ir::{Embed, Getter, Hole, HoleKind, Jsx};

pub struct Lowerer<'a, 'f> {
    alloc: &'a Allocator,
    source: &'a str,
    facts: &'f Facts,
    scoping: &'f Scoping,
    optimize: bool,
    reports: &'f mut std::vec::Vec<Report>,
    /// Enclosing components (`<Name>`) and elements, for diagnostics.
    path: std::vec::Vec<String>,
    has_jsx: bool,
}

impl<'a, 'f> Lowerer<'a, 'f> {
    pub fn new(
        alloc: &'a Allocator,
        source: &'a str,
        facts: &'f Facts,
        scoping: &'f Scoping,
        optimize: bool,
        reports: &'f mut std::vec::Vec<Report>,
    ) -> Self {
        Self {
            alloc,
            source,
            facts,
            scoping,
            optimize,
            reports,
            path: std::vec::Vec::new(),
            has_jsx: false,
        }
    }

    /// The program from `start` on with every hole compiled; `None` when there is no JSX.
    pub fn program(mut self, program: &Program<'a>, start: u32) -> Option<Embed<'a>> {
        let end = self.source.len() as u32;
        let embed = self.embed(Span::new(start, end), |finder| {
            for statement in &program.body {
                if statement.span().start >= start {
                    finder.visit_statement(statement);
                }
            }
        });
        self.has_jsx.then_some(embed)
    }

    fn report(&mut self, mut report: Report) {
        report.path = self.path.clone();
        self.reports.push(report);
    }

    fn str(&self, s: &str) -> &'a str {
        self.alloc.alloc_str(s)
    }

    fn vec<T>(&self) -> Vec<'a, T> {
        Vec::new_in(&self.alloc)
    }

    fn boxed<T>(&self, value: T) -> Box<'a, T> {
        Box::new_in(value, &self.alloc)
    }

    fn embed(&mut self, span: Span, visit: impl FnOnce(&mut HoleFinder<'_, 'a, 'f>)) -> Embed<'a> {
        let mut finder = HoleFinder { lowerer: self, holes: std::vec::Vec::new() };
        visit(&mut finder);
        let mut holes = finder.holes;
        holes.sort_by_key(|hole| hole.span.start);
        Embed { span, holes: Vec::from_iter_in(holes, &self.alloc) }
    }

    fn expr(&mut self, e: &Expression<'a>) -> Embed<'a> {
        self.embed(e.span(), |finder| finder.visit_expression(e))
    }

    fn stmt(&mut self, statement: &Statement<'a>) -> Embed<'a> {
        self.embed(statement.span(), |finder| finder.visit_statement(statement))
    }

    fn params(&mut self, params: &FormalParameters<'a>) -> Embed<'a> {
        self.embed(params.span, |finder| finder.visit_formal_parameters(params))
    }

    /// `f` for a bare `f()`, otherwise `() => e`.
    fn getter(&mut self, e: &Expression<'a>) -> Getter<'a> {
        if let Expression::CallExpression(call) = e.without_parentheses()
            && let Expression::Identifier(id) = &call.callee
            && call.arguments.is_empty()
            && !call.optional
            && call.type_arguments.is_none()
        {
            return Getter::Call(id.span);
        }
        let parenthesize = self.source.as_bytes()[e.span().start as usize] == b'{';
        Getter::Thunk { body: self.expr(e), parenthesize }
    }

    /// Removes `span` together with the whitespace before it.
    fn removal(&self, span: Span) -> Edit {
        let before = &self.source[..span.start as usize];
        let start = before.trim_end().len() as u32;
        Edit { start, end: span.end, text: String::new() }
    }

    fn component_scope(&mut self, name: &str, params: &FormalParameters<'a>, body_has_jsx: bool) {
        if !body_has_jsx {
            return;
        }
        if let Some(first) = params.items.first()
            && let BindingPattern::ObjectPattern(pattern) = &first.pattern
        {
            self.report(
                Report::new(
                    Code::PropsDestructured,
                    pattern.span,
                    format!(
                        "`{name}` destructures its props: each value is read once when the component \
                         runs and never updates. Take `props` and read `props.x` where it is used."
                    ),
                )
                .data("component", name),
            );
        }
    }
}

fn is_component_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

/// Whether the nodes `visit` walks contain any JSX.
pub fn has_jsx(visit: impl FnOnce(&mut JsxCheck)) -> bool {
    let mut check = JsxCheck { found: false };
    visit(&mut check);
    check.found
}

pub struct JsxCheck {
    found: bool,
}

impl<'a> Visit<'a> for JsxCheck {
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found = true;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found = true;
    }
}

/// Collects the outermost holes of a JS region, lowering each.
struct HoleFinder<'l, 'a, 'f> {
    lowerer: &'l mut Lowerer<'a, 'f>,
    holes: std::vec::Vec<Hole<'a>>,
}

impl HoleFinder<'_, '_, '_> {
    fn in_component<R>(&mut self, name: Option<&str>, run: impl FnOnce(&mut Self) -> R) -> R {
        let entered = name.filter(|n| is_component_name(n)).map(|n| format!("<{n}>"));
        let is_entered = entered.is_some();
        if let Some(entry) = entered {
            self.lowerer.path.push(entry);
        }
        let result = run(self);
        if is_entered {
            self.lowerer.path.pop();
        }
        result
    }
}

impl<'a> Visit<'a> for HoleFinder<'_, 'a, '_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        let jsx = self.lowerer.element(it);
        self.holes.push(Hole { span: it.span, kind: HoleKind::Jsx(jsx) });
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        let jsx = self.lowerer.fragment(it);
        self.holes.push(Hole { span: it.span, kind: HoleKind::Jsx(jsx) });
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let name = it.id.as_ref().map(|id| id.name.as_str());
        self.in_component(name, |finder| {
            if let (Some(name), Some(body)) = (name, &it.body)
                && is_component_name(name)
            {
                finder.lowerer.component_scope(
                    name,
                    &it.params,
                    has_jsx(|c| c.visit_function_body(body)),
                );
            }
            match finder.lowerer.async_function(it) {
                Some(component) => finder.holes.push(Hole {
                    span: it.span,
                    kind: HoleKind::AsyncComponent(finder.lowerer.boxed(component)),
                }),
                None => walk::walk_function(finder, it, flags),
            }
        });
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        match self.lowerer.async_arrow(it) {
            Some(component) => self.holes.push(Hole {
                span: it.span,
                kind: HoleKind::AsyncComponent(self.lowerer.boxed(component)),
            }),
            None => walk::walk_arrow_function_expression(self, it),
        }
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(getter) = self.lowerer.facts.folded_getter(it)
            && let Some(init) = &it.init
            && let Expression::CallExpression(call) = init.without_parentheses()
            && let Some(value) = call.arguments.first().and_then(Argument::as_expression)
        {
            let init = self.lowerer.expr(value);
            self.holes
                .push(Hole { span: it.span, kind: HoleKind::ConstSignalDecl { getter, init } });
            return;
        }
        let name = match &it.id {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        };
        let arrow = match it.init.as_ref().map(Expression::without_parentheses) {
            Some(Expression::ArrowFunctionExpression(arrow)) => Some(arrow),
            _ => None,
        };
        let component = name.filter(|_| arrow.is_some());
        self.in_component(component, |finder| {
            if let (Some(name), Some(arrow)) = (component, arrow)
                && is_component_name(name)
            {
                let body_has_jsx = has_jsx(|c| c.visit_arrow_function_body(&arrow.body));
                finder.lowerer.component_scope(name, &arrow.params, body_has_jsx);
            }
            walk::walk_variable_declarator(finder, it);
        });
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && it.arguments.is_empty()
            && !it.optional
            && self.lowerer.facts.is_folded_read(id)
        {
            self.holes
                .push(Hole { span: it.span, kind: HoleKind::ConstSignalRead { getter: id.span } });
            return;
        }
        walk::walk_call_expression(self, it);
    }
}

/// Native element tag, or the callee span of a component.
enum Tag<'a> {
    Native(&'a str),
    Component(Span),
}

impl<'a> Lowerer<'a, '_> {
    fn tag_of(&self, name: &JSXElementName<'a>) -> Tag<'a> {
        let by_name = |name: &'a str, span: Span| {
            if name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-') {
                Tag::Native(name)
            } else {
                Tag::Component(span)
            }
        };
        match name {
            JSXElementName::Identifier(id) => by_name(id.name.as_str(), id.span),
            JSXElementName::IdentifierReference(id) => by_name(id.name.as_str(), id.span),
            JSXElementName::NamespacedName(n) => Tag::Native(self.str(&format!(
                "{}:{}",
                n.namespace.name.as_str(),
                n.name.name.as_str()
            ))),
            JSXElementName::MemberExpression(m) => Tag::Component(m.span),
            JSXElementName::ThisExpression(t) => Tag::Component(t.span),
        }
    }

    pub fn element(&mut self, el: &JSXElement<'a>) -> Jsx<'a> {
        self.has_jsx = true;
        match self.tag_of(&el.opening_element.name) {
            Tag::Native(tag) => Jsx::Template(self.template(el, tag)),
            Tag::Component(callee) => Jsx::Component(self.component(el, callee)),
        }
    }

    pub fn fragment(&mut self, fragment: &JSXFragment<'a>) -> Jsx<'a> {
        self.has_jsx = true;
        let items = self.items(&fragment.children, false);
        Jsx::Fragment(self.list(items))
    }
}

pub fn attribute_name<'a>(lowerer: &Lowerer<'a, '_>, attr: &JSXAttribute<'a>) -> &'a str {
    match &attr.name {
        JSXAttributeName::Identifier(id) => id.name.as_str(),
        JSXAttributeName::NamespacedName(n) => {
            lowerer.str(&format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str()))
        }
    }
}

fn is_function(e: &Expression<'_>) -> bool {
    matches!(
        e.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}
