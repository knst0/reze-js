mod async_component;
mod attribute;
mod children;
mod component;
mod computed;
pub mod constant;
mod element;
mod island;
pub mod props;
pub mod store;

use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::analyze::Facts;
use crate::diagnostic::{Edit, Report};
use crate::ir::{Embed, Getter, Hole, HoleKind, Jsx};
use crate::namer::Namer;

/// What lowering leaves for emission.
pub struct Lowered<'a, 'f> {
    /// The hashbang, directives and leading imports, with their holes.
    pub head: Embed<'a>,
    /// `None` when nothing in the module is rewritten.
    pub body: Option<Embed<'a>>,
    pub namer: Namer<'f>,
    pub reports: std::vec::Vec<Report>,
}

pub struct Lowerer<'a, 'f> {
    alloc: &'a Allocator,
    source: &'a str,
    facts: &'f Facts,
    scoping: &'f Scoping,
    nodes: &'f AstNodes<'a>,
    optimize: bool,
    namer: Namer<'f>,
    reports: std::vec::Vec<Report>,
    /// Enclosing components (`<Name>`) and elements, for diagnostics.
    path: std::vec::Vec<String>,
    has_jsx: bool,
    /// Props object names by the start of the parameter they replace.
    props_names: std::collections::HashMap<u32, &'a str>,
    /// Temporaries of hoisted props defaults by the start of the default (SPEC §16.7).
    props_temporaries: std::collections::HashMap<u32, &'a str>,
    store_names: store::StoreNames<'a>,
    computed_names: computed::ComputedNames<'a, 'f>,
}

impl<'a, 'f> Lowerer<'a, 'f> {
    pub fn new(
        alloc: &'a Allocator,
        source: &'a str,
        facts: &'f Facts,
        scoping: &'f Scoping,
        nodes: &'f AstNodes<'a>,
        optimize: bool,
        namer: Namer<'f>,
        reports: std::vec::Vec<Report>,
    ) -> Self {
        Self {
            alloc,
            source,
            facts,
            scoping,
            nodes,
            optimize,
            namer,
            reports,
            path: std::vec::Vec::new(),
            has_jsx: false,
            props_names: std::collections::HashMap::new(),
            props_temporaries: std::collections::HashMap::new(),
            store_names: store::StoreNames::default(),
            computed_names: computed::ComputedNames::default(),
        }
    }

    /// The leading part up to `start` and the program from `start` on, with every hole compiled.
    pub fn program(mut self, program: &Program<'a>, start: u32) -> Lowered<'a, 'f> {
        let end = self.source.len() as u32;
        let head = self.embed(Span::new(0, start), |finder| {
            for statement in &program.body {
                if let Statement::ImportDeclaration(import) = statement
                    && import.span.end <= start
                {
                    finder.visit_import_declaration(import);
                }
            }
        });
        let embed = self.embed(Span::new(start, end), |finder| {
            for statement in &program.body {
                if statement.span().start >= start {
                    finder.visit_statement(statement);
                }
            }
        });
        let is_rewritten = self.has_jsx || !embed.holes.is_empty() || !head.holes.is_empty();
        let body = is_rewritten.then_some(embed);
        Lowered { head, body, namer: self.namer, reports: self.reports }
    }

    /// A fresh identifier, reserved for the whole module.
    fn fresh(&mut self, base: &str) -> &'a str {
        let name = self.namer.fresh(base);
        self.alloc.alloc_str(&name)
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

    pub(crate) fn expr(&mut self, e: &Expression<'a>) -> Embed<'a> {
        self.embed(e.span(), |finder| finder.visit_expression(e))
    }

    pub(crate) fn stmt(&mut self, statement: &Statement<'a>) -> Embed<'a> {
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
            && self.facts.inlined_body(call, self.nodes).is_none()
            && !self.facts.program.computed_reads.contains_key(&id.span.start)
            && self.facts.props.read(id).is_none()
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
}

fn is_component_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

pub fn is_native_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-')
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
            if let Some(name) = name.filter(|n| is_component_name(n)) {
                finder.lowerer.component_scope(name, &it.params);
            }
            match finder.lowerer.async_function(it) {
                Some(component) => finder.holes.push(Hole {
                    span: it.span,
                    kind: HoleKind::AsyncComponent(finder.lowerer.boxed(component)),
                }),
                None => {
                    if let Some(body) = &it.body
                        && let Some(entry) = finder.lowerer.props_entry(
                            &it.params,
                            Span::empty(props::block_start(body)),
                            None,
                        )
                    {
                        finder.holes.push(entry);
                    }
                    walk::walk_function(finder, it, flags);
                }
            }
        });
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let Some(component) = self.lowerer.async_arrow(it) {
            let kind = HoleKind::AsyncComponent(self.lowerer.boxed(component));
            self.holes.push(Hole { span: it.span, kind });
            return;
        }
        let entry = match &it.body {
            ArrowFunctionBody::FunctionBody(body) => {
                let at = Span::empty(props::block_start(body));
                self.lowerer.props_entry(&it.params, at, None)
            }
            body => body.as_expression().and_then(|expression| {
                self.lowerer.props_entry(&it.params, expression.span(), Some(expression))
            }),
        };
        let Some(entry) = entry else {
            walk::walk_arrow_function_expression(self, it);
            return;
        };
        self.holes.push(entry);
        if let ArrowFunctionBody::FunctionBody(_) = &it.body {
            walk::walk_arrow_function_expression(self, it);
            return;
        }
        if let Some(type_parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(type_parameters);
        }
        self.visit_formal_parameters(&it.params);
        if let Some(return_type) = &it.return_type {
            self.visit_ts_type_annotation(return_type);
        }
    }

    fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
        let Some(hole) = self.lowerer.props_param(it) else {
            walk::walk_formal_parameter(self, it);
            return;
        };
        self.holes.push(hole);
        if let Some(annotation) = &it.type_annotation {
            self.visit_ts_type_annotation(annotation);
        }
        if let Some(initializer) = &it.initializer {
            self.visit_expression(initializer);
        }
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(hole) = self.lowerer.props_read(it, it.span, false) {
            self.holes.push(hole);
        }
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some(hole) = self.lowerer.props_read(id, it.span, true)
        {
            self.holes.push(hole);
            return;
        }
        walk::walk_object_property(self, it);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if let Some(span) = self.lowerer.facts.removed_declaration(it) {
            self.holes.push(Hole { span, kind: HoleKind::Remove });
            return;
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(hole) = self.lowerer.store_declaration(it) {
            self.holes.push(hole);
            return;
        }
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
        let params = match it.init.as_ref().map(Expression::without_parentheses) {
            Some(Expression::ArrowFunctionExpression(arrow)) => Some(&*arrow.params),
            Some(Expression::FunctionExpression(function))
                if props::is_declared_component(function) =>
            {
                Some(&*function.params)
            }
            _ => None,
        };
        let component = name.filter(|_| params.is_some());
        self.in_component(component, |finder| {
            if let (Some(name), Some(params)) = (component, params)
                && is_component_name(name)
            {
                finder.lowerer.component_scope(name, params);
            }
            walk::walk_variable_declarator(finder, it);
        });
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(hole) = self.lowerer.store_set(it) {
            self.holes.push(hole);
            return;
        }
        if let Some(hole) = self.lowerer.store_method_call(it) {
            self.holes.push(hole);
            return;
        }
        if let Some(body) = self.lowerer.facts.inlined_body(it, self.lowerer.nodes) {
            let body = self.lowerer.expr(body);
            self.holes.push(Hole { span: it.span, kind: HoleKind::ComputedInline { body } });
            return;
        }
        if let Some(hole) = self.lowerer.computed_read(it) {
            self.holes.push(hole);
            return;
        }
        if let Some((getter, _)) = self.lowerer.facts.folded_callee(it) {
            self.holes.push(Hole { span: it.span, kind: HoleKind::ConstSignalRead { getter } });
            return;
        }
        if let Some(root) = self.lowerer.island_root(it) {
            self.holes.push(root);
            return;
        }
        walk::walk_call_expression(self, it);
    }
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some(hole) = self.lowerer.store_read(it.span) {
            self.holes.push(hole);
            return;
        }
        if let Some(mut steps) = self.lowerer.member_steps(&it.object) {
            steps.push(crate::ir::ArraySuffix::Key(it.property.name.as_str()));
            if let Some(hole) = self.lowerer.array_read(it.span, steps) {
                self.holes.push(hole);
                return;
            }
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if let Some(hole) = self.lowerer.store_read(it.span) {
            self.holes.push(hole);
            return;
        }
        if let Some(mut steps) = self.lowerer.member_steps(&it.object) {
            match it.expression.without_parentheses() {
                Expression::StringLiteral(key) => {
                    steps.push(crate::ir::ArraySuffix::Key(self.lowerer.str(&key.value)))
                }
                _ => steps.push(crate::ir::ArraySuffix::Index(self.lowerer.expr(&it.expression))),
            }
            if let Some(hole) = self.lowerer.array_read(it.span, steps) {
                self.holes.push(hole);
                return;
            }
        }
        walk::walk_computed_member_expression(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let Some(hole) =
            self.lowerer.store_assignment(it).or_else(|| self.lowerer.array_assignment(it))
        {
            self.holes.push(hole);
            return;
        }
        if let Some(hole) = self.lowerer.form_assignment(it) {
            self.holes.push(hole);
            return;
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let Some(hole) = self.lowerer.store_update(it).or_else(|| self.lowerer.array_update(it))
        {
            self.holes.push(hole);
            return;
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if let Some(hole) = self.lowerer.store_import(it) {
            self.holes.push(hole);
        }
    }

    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        match self.lowerer.store_export_declaration(it) {
            Some(hole) => self.holes.push(hole),
            None => walk::walk_export_declaration(self, it),
        }
    }

    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        if let Some(span) = self.lowerer.facts.removed_export(it) {
            self.holes.push(Hole { span, kind: HoleKind::Remove });
            return;
        }
        match self.lowerer.store_export_specifiers(it) {
            Some(hole) => self.holes.push(hole),
            None => walk::walk_export_named_declaration(self, it),
        }
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
            if is_native_name(name) { Tag::Native(name) } else { Tag::Component(span) }
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
