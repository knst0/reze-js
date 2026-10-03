pub(crate) mod async_component;
mod attribute;
mod children;
mod component;
pub mod constant;
mod control_flow;
mod debug_name;
mod dynamic;
mod element;
mod hot;
mod island;
pub mod keyed;
mod prerender;
pub mod props;
mod selector;

use std::collections::HashMap;

use oxc_allocator::{Allocator, Box, Vec};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use crate::analyze::{Analysis, Intrinsic};
use crate::diagnostic::{Edit, Report};
use crate::ir::{Embed, Getter, Hole, HoleKind, HotEdit, Jsx, Placement, ScriptEdit, Source};
use crate::namer::Namer;
use island::IslandPlan;
pub use prerender::{ComponentRef, PrerenderComponent, PrerenderHole, PrerenderModule, Tree};

pub struct Lowered<'a, 'f> {
    /// The hashbang, directives and leading imports.
    pub head: Embed<'a>,
    /// `None` when nothing in the module is rewritten.
    pub body: Option<Embed<'a>>,
    pub namer: Namer<'f>,
    pub reports: std::vec::Vec<Report>,
}

pub struct Settings {
    pub debug_names: bool,
    pub hot: bool,
    pub links: bool,
}

pub struct Lowerer<'a, 'f> {
    alloc: &'a Allocator,
    source: &'a str,
    analysis: &'f Analysis<'f>,
    settings: Settings,
    namer: Namer<'f>,
    reports: std::vec::Vec<Report>,
    /// Enclosing components (`<Name>`) and elements, for diagnostics.
    path: std::vec::Vec<String>,
    has_jsx: bool,
    /// Props object names by the start of the parameter they replace.
    props_names: HashMap<u32, &'a str>,
    /// Temporaries of hoisted props defaults by the start of the default.
    props_temporaries: HashMap<u32, &'a str>,
    /// Row callbacks of the `<For>` elements being lowered, innermost last.
    for_scopes: std::vec::Vec<selector::ForScope<'a>>,
    /// Values getters of split `async` components, by the start of the component function.
    async_values: HashMap<u32, &'a str>,
    /// Imported components used only by islands, for chunk splitting.
    islands: IslandPlan,
    /// Names of the functions initializing a component's `const`, by the start of the function.
    component_inits: HashMap<u32, &'a str>,
    hot: hot::HotPlan<'a>,
}

impl<'a, 'f> Lowerer<'a, 'f> {
    pub fn new(
        alloc: &'a Allocator,
        source: &'a str,
        analysis: &'f Analysis<'f>,
        settings: Settings,
        namer: Namer<'f>,
        reports: std::vec::Vec<Report>,
    ) -> Self {
        Self {
            alloc,
            source,
            analysis,
            settings,
            namer,
            reports,
            path: std::vec::Vec::new(),
            has_jsx: false,
            props_names: HashMap::new(),
            props_temporaries: HashMap::new(),
            component_inits: HashMap::new(),
            for_scopes: std::vec::Vec::new(),
            async_values: HashMap::new(),
            islands: IslandPlan::default(),
            hot: hot::HotPlan::default(),
        }
    }

    /// The leading part up to `start` and the rest of the program, with every hole lowered.
    pub fn program(mut self, program: &Program<'a>, start: u32) -> Lowered<'a, 'f> {
        if self.settings.hot {
            self.hot = hot::plan(program);
        }
        self.islands = IslandPlan::scan(program, self.analysis.scoping);
        let end = self.source.len() as u32;
        let head = self.embed(Span::new(0, start), |finder| {
            for statement in &program.body {
                if statement.span().end <= start {
                    finder.visit_statement(statement);
                    if let Statement::ImportDeclaration(it) = statement
                        && let Some(span) = finder.lowerer.islands.prune_span(it.span.start)
                    {
                        finder
                            .holes
                            .push(Hole { span, kind: HoleKind::Script(ScriptEdit::Insert("")) });
                    }
                }
            }
        });
        let body = self.embed(Span::new(start, end), |finder| {
            for statement in &program.body {
                if statement.span().end <= start {
                    continue;
                }
                finder.visit_statement(statement);
                if let Some(&name) = finder.lowerer.hot.declarations.get(&statement.span().start) {
                    let at = Span::empty(statement.span().end);
                    let kind =
                        HoleKind::Script(ScriptEdit::Hot(HotEdit::AfterDeclaration { name }));
                    finder.holes.push(Hole { span: at, kind });
                }
            }
            if finder.lowerer.hot.accept {
                let kind = HoleKind::Script(ScriptEdit::Hot(HotEdit::Accept));
                finder.holes.push(Hole { span: Span::empty(end), kind });
            }
        });
        let is_rewritten = self.has_jsx || !body.holes.is_empty() || !head.holes.is_empty();
        Lowered {
            head,
            body: is_rewritten.then_some(body),
            namer: self.namer,
            reports: self.reports,
        }
    }

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

    /// Sorted stable-getter symbols read by `exprs`: the dependency set of a bind.
    fn dep_list<'x>(
        &self,
        exprs: impl IntoIterator<Item = &'x Expression<'x>>,
    ) -> Vec<'a, SymbolId> {
        let mut deps = std::vec::Vec::new();
        for e in exprs {
            deps.extend(self.analysis.stable_getter_deps(e));
        }
        deps.sort();
        deps.dedup();
        let mut out = self.vec();
        out.extend(deps);
        out
    }

    fn boxed<T>(&self, value: T) -> Box<'a, T> {
        Box::new_in(value, &self.alloc)
    }

    fn text(&self, span: Span) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn embed(&mut self, span: Span, visit: impl FnOnce(&mut HoleFinder<'_, 'a, 'f>)) -> Embed<'a> {
        let mut finder = HoleFinder { lowerer: self, holes: std::vec::Vec::new() };
        visit(&mut finder);
        let mut holes = finder.holes;
        holes.sort_by_key(|hole| (hole.span.start, hole.span.end));
        Embed { span, holes: Vec::from_iter_in(holes, &self.alloc) }
    }

    pub(crate) fn expr(&mut self, e: &Expression<'a>) -> Embed<'a> {
        self.embed(e.span(), |finder| finder.visit_expression(e))
    }

    /// `f` for a bare `f()`, otherwise `() => e`.
    fn getter(&mut self, e: &Expression<'a>) -> Getter<'a> {
        if let Expression::CallExpression(call) = e.without_parentheses()
            && let Expression::Identifier(id) = &call.callee
            && call.arguments.is_empty()
            && !call.optional
            && call.type_arguments.is_none()
            && !self.analysis.props.is_read(id)
            && !self.analysis.asyncs.is_read(id)
        {
            return Getter::Call(id.span);
        }
        self.thunk(e)
    }

    fn thunk(&mut self, e: &Expression<'a>) -> Getter<'a> {
        let parenthesize = self.source.as_bytes()[e.span().start as usize] == b'{';
        Getter::Thunk { body: self.expr(e), parenthesize }
    }

    fn source(&mut self, e: &Expression<'a>) -> Source<'a> {
        Source { expr: self.expr(e), getter: self.stable_getter_callee(e) }
    }

    /// `f` of `e` = `f()` reading a stable signal or computed getter that is not folded.
    fn stable_getter_callee(&self, e: &Expression<'a>) -> Option<Span> {
        let Expression::CallExpression(call) = e.without_parentheses() else { return None };
        let Expression::Identifier(id) = &call.callee else { return None };
        let is_plain_call =
            call.arguments.is_empty() && !call.optional && call.type_arguments.is_none();
        (is_plain_call
            && self.analysis.is_stable_getter(id)
            && self.analysis.folded_read(call).is_none())
        .then_some(id.span)
    }

    /// Removes `span` together with the whitespace before it.
    fn removal(&self, span: Span) -> Edit {
        let start = self.source[..span.start as usize].trim_end().len() as u32;
        Edit { start, end: span.end, text: String::new() }
    }

    fn tag_of(&self, name: &JSXElementName<'a>) -> Tag<'a> {
        if let Some(intrinsic) = self.analysis.intrinsic(name) {
            return Tag::Intrinsic(intrinsic);
        }
        let by_name = |name: &'a str, span: Span| {
            if is_native_name(name) { Tag::Native(name) } else { Tag::Component(span) }
        };
        match name {
            JSXElementName::Identifier(id) => by_name(id.name.as_str(), id.span),
            JSXElementName::IdentifierReference(id) => by_name(id.name.as_str(), id.span),
            JSXElementName::NamespacedName(n) => {
                Tag::Native(self.str(&format!("{}:{}", n.namespace.name, n.name.name)))
            }
            JSXElementName::MemberExpression(m) => Tag::Component(m.span),
            JSXElementName::ThisExpression(t) => Tag::Component(t.span),
        }
    }

    pub fn element(&mut self, el: &JSXElement<'a>) -> Jsx<'a> {
        self.has_jsx = true;
        match self.tag_of(&el.opening_element.name) {
            Tag::Native(tag) => Jsx::Template(self.template(el, tag)),
            Tag::Component(callee) => Jsx::Component(self.component(el, callee)),
            Tag::Intrinsic(intrinsic) => self.control_flow(el, intrinsic),
        }
    }

    pub fn fragment(&mut self, fragment: &JSXFragment<'a>) -> Jsx<'a> {
        self.has_jsx = true;
        let items = self.items(&fragment.children, false);
        Jsx::Fragment(self.list(items))
    }

    /// `el` as a hole at `span`, written as a block when it is a template with work.
    fn block_hole(&mut self, el: &JSXElement<'a>, span: Span) -> Hole<'a> {
        match self.element(el) {
            Jsx::Template(mut template) if template.has_work() => {
                template.placement = Placement::Block;
                Hole { span, kind: HoleKind::Jsx(Jsx::Template(template)) }
            }
            jsx => Hole { span: el.span, kind: HoleKind::Jsx(jsx) },
        }
    }
}

enum Tag<'a> {
    Native(&'a str),
    Component(Span),
    Intrinsic(Intrinsic),
}

pub fn is_component_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

pub(super) fn is_native_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-')
}

/// Whether the nodes `visit` walks contain JSX.
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

fn is_function(e: &Expression<'_>) -> bool {
    matches!(
        e.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

/// A native JSX element, parentheses aside.
fn native_element<'e, 'a>(
    lowerer: &Lowerer<'a, '_>,
    e: &'e Expression<'a>,
) -> Option<&'e JSXElement<'a>> {
    let Expression::JSXElement(el) = e.without_parentheses() else { return None };
    matches!(lowerer.tag_of(&el.opening_element.name), Tag::Native(_)).then_some(el)
}

pub fn attribute_name<'a>(lowerer: &Lowerer<'a, '_>, attr: &JSXAttribute<'a>) -> &'a str {
    match &attr.name {
        JSXAttributeName::Identifier(id) => id.name.as_str(),
        JSXAttributeName::NamespacedName(n) => {
            lowerer.str(&format!("{}:{}", n.namespace.name, n.name.name))
        }
    }
}

/// Collects the outermost holes of a JS region, lowering each.
struct HoleFinder<'l, 'a, 'f> {
    lowerer: &'l mut Lowerer<'a, 'f>,
    holes: std::vec::Vec<Hole<'a>>,
}

impl HoleFinder<'_, '_, '_> {
    fn in_component<R>(&mut self, name: Option<&str>, run: impl FnOnce(&mut Self) -> R) -> R {
        let Some(name) = name.filter(|name| is_component_name(name)) else { return run(self) };
        self.lowerer.path.push(format!("<{name}>"));
        let result = run(self);
        self.lowerer.path.pop();
        result
    }
}

impl<'a> HoleFinder<'_, 'a, '_> {
    fn push_script(&mut self, span: Span, edit: ScriptEdit<'a>) {
        self.holes.push(Hole { span, kind: HoleKind::Script(edit) });
    }

    /// The holes of `body` when it is the block body of an async component.
    fn async_holes(
        &mut self,
        component: Option<&str>,
        function: async_component::AsyncFunction<'_, 'a>,
    ) -> std::vec::Vec<Hole<'a>> {
        match component {
            Some(name) => self.lowerer.async_component(name, &function),
            None => std::vec::Vec::new(),
        }
    }
}

impl<'a> Visit<'a> for HoleFinder<'_, 'a, '_> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if self.lowerer.analysis.prunes_import(it) {
            self.push_script(it.span, ScriptEdit::Insert(""));
        }
    }

    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        let jsx = self.lowerer.element(it);
        self.holes.push(Hole { span: it.span, kind: HoleKind::Jsx(jsx) });
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        let jsx = self.lowerer.fragment(it);
        self.holes.push(Hole { span: it.span, kind: HoleKind::Jsx(jsx) });
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        match it.argument.as_ref().and_then(|argument| native_element(self.lowerer, argument)) {
            Some(el) => {
                let hole = self.lowerer.block_hole(el, it.span);
                self.holes.push(hole);
            }
            None => walk::walk_return_statement(self, it),
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let name = it.id.as_ref().map(|id| id.name.as_str());
        let component = name
            .filter(|name| is_component_name(name))
            .map(|name| self.lowerer.str(name))
            .or_else(|| self.lowerer.component_inits.get(&it.span.start).copied());
        self.in_component(name, |finder| {
            if let Some(name) = name.filter(|name| is_component_name(name)) {
                finder.lowerer.component_scope(name, &it.params);
            }
            if let Some(body) = &it.body {
                let at = Span::empty(props::block_start(body));
                if let Some(entry) = finder.lowerer.props_entry(&it.params, at, None) {
                    finder.holes.push(entry);
                }
                if it.r#async && !it.generator {
                    let function = async_component::AsyncFunction {
                        span: it.span,
                        body,
                        return_type: it.return_type.as_deref(),
                    };
                    let holes = finder.async_holes(component, function);
                    finder.holes.extend(holes);
                }
            }
            walk::walk_function(finder, it, flags);
        });
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        let async_holes = match &it.body {
            ArrowFunctionBody::FunctionBody(body) if it.r#async => {
                let component = self.lowerer.component_inits.get(&it.span.start).copied();
                let function = async_component::AsyncFunction {
                    span: it.span,
                    body,
                    return_type: it.return_type.as_deref(),
                };
                self.async_holes(component, function)
            }
            _ => std::vec::Vec::new(),
        };
        let (entry, body) = match &it.body {
            ArrowFunctionBody::FunctionBody(body) => {
                let at = Span::empty(props::block_start(body));
                (self.lowerer.props_entry(&it.params, at, None), None)
            }
            body => {
                let expression = body.as_expression();
                let entry = expression
                    .and_then(|e| self.lowerer.props_entry(&it.params, e.span(), Some(e)));
                (entry, expression)
            }
        };
        let block = body
            .filter(|_| entry.is_none())
            .and_then(|e| Some((e.span(), native_element(self.lowerer, e)?)));
        if entry.is_none() && block.is_none() {
            self.holes.extend(async_holes);
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
        if let Some(entry) = entry {
            self.holes.push(entry);
            self.holes.extend(async_holes);
            if let ArrowFunctionBody::FunctionBody(body) = &it.body {
                self.visit_function_body(body);
            }
        } else if let Some((span, el)) = block {
            let hole = self.lowerer.block_hole(el, span);
            self.holes.push(hole);
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
        } else if let Some(hole) = self.lowerer.async_read(it, it.span, false) {
            self.holes.push(hole);
        }
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some(hole) = self
                .lowerer
                .props_read(id, it.span, true)
                .or_else(|| self.lowerer.async_read(id, it.span, true))
        {
            self.holes.push(hole);
            return;
        }
        walk::walk_object_property(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(getter) = self.lowerer.analysis.folded_getter(it)
            && let Some(Expression::CallExpression(call)) =
                it.init.as_ref().map(Expression::without_parentheses)
            && let Some(value) = call.arguments.first().and_then(Argument::as_expression)
        {
            let init = self.lowerer.expr(value);
            self.push_script(it.span, ScriptEdit::ConstSignalDecl { getter, init });
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
        if let (Some(name), Some(init)) =
            (component.filter(|name| is_component_name(name)), &it.init)
        {
            let name = self.lowerer.str(name);
            self.lowerer.component_inits.insert(init.without_parentheses().span().start, name);
        }
        let hot_name = it
            .init
            .as_ref()
            .and_then(|init| self.lowerer.hot.inits.get(&init.span().start).copied());
        self.in_component(component, |finder| {
            if let (Some(name), Some(params)) = (component, params)
                && is_component_name(name)
            {
                finder.lowerer.component_scope(name, params);
            }
            match (hot_name, &it.init) {
                (Some(name), Some(init)) => {
                    let init_embed = finder.lowerer.expr(init);
                    let edit = ScriptEdit::Hot(HotEdit::WrapInit { name, init: init_embed });
                    finder.push_script(init.span(), edit);
                }
                _ => walk::walk_variable_declarator(finder, it),
            }
        });
        if let Some(hole) = self.lowerer.debug_name(it) {
            self.holes.push(hole);
        }
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some((getter, _)) = self.lowerer.analysis.folded_read(it) {
            self.push_script(it.span, ScriptEdit::ConstSignalRead { getter });
            return;
        }
        let tags = self.lowerer.element_tags(it);
        self.holes.extend(tags);
        walk::walk_call_expression(self, it);
    }

    fn visit_binary_expression(&mut self, it: &BinaryExpression<'a>) {
        match self.lowerer.selector_read(it) {
            Some(hole) => self.holes.push(hole),
            None => walk::walk_binary_expression(self, it),
        }
    }
}
