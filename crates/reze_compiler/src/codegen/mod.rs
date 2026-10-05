mod calls;
mod client;
mod composite;
mod continuation;
pub mod hot;
mod html;
mod hydrate;
mod module_scope;
mod native;
mod optimize;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

use oxc_allocator::{Allocator, ArenaVec, TakeIn};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, GetSpanMut, SPAN, Span};
use oxc_str::Ident;
use oxc_syntax::node::NodeId;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};

use crate::ast::Ast;
use crate::frontend::{Namer, SharedFacts};
use crate::imports::HelperImports;
use crate::ir::{ModuleIr, sites::SiteId, view::*};
use crate::{CompileTarget, Options, RUNTIME_MODULE};

pub struct EmitContext<'a, 'm> {
    pub allocator: &'a Allocator,
    pub options: &'m Options,
    pub source: &'m str,
    pub filename: &'m str,
    pub ir: &'m ModuleIr,
    pub cold: bool,
    facts: &'m SharedFacts,
    scoping: &'m Scoping,
    changed: bool,
    namer: Namer<'a>,
    helpers: HelperImports<'a>,
    expressions: RefCell<HashMap<NodeId, Expression<'a>>>,
    sites: HashMap<u32, &'a str>,
    positions: HashMap<u32, (u32, u32)>,
    hoisted: Vec<Statement<'a>>,
    delegated: BTreeSet<String>,
}

impl<'a, 'm> EmitContext<'a, 'm> {
    pub fn new(
        allocator: &'a Allocator,
        options: &'m Options,
        source: &'m str,
        filename: &'m str,
        ir: &'m ModuleIr,
        facts: &'m SharedFacts,
        scoping: &'m Scoping,
        namer: Namer<'a>,
        helpers: HelperImports<'a>,
        cold: bool,
    ) -> Self {
        Self {
            allocator,
            options,
            source,
            filename,
            ir,
            facts,
            scoping,
            namer,
            helpers,
            cold,
            changed: false,
            expressions: RefCell::new(HashMap::new()),
            sites: HashMap::new(),
            positions: source_positions(source, ir),
            hoisted: Vec::new(),
            delegated: BTreeSet::new(),
        }
    }

    pub fn emit(mut self, program: &mut Program<'a>, hot_plan: Option<&hot::HotPlan>) -> bool {
        Replace { ctx: &mut self }.visit_program(program);
        if self.options.target != CompileTarget::Client {
            module_scope::apply(&mut self, program);
            let ast = Ast::new(self.allocator);
            let module = ast.string(
                self.ir.module_id.as_deref().expect("managed target has canonical module identity"),
            );
            let source = if self.options.target == CompileTarget::Html {
                "reze-js/internal/html"
            } else {
                "reze-js/internal/hydrate"
            };
            let mark = self.call(source, "markModule", [module]);
            self.hoisted.push(ast.stmt(mark));
            self.changed = true;
        }
        program.body.retain(|statement| !matches!(statement,
            Statement::ImportDeclaration(import) if self.ir.islands.prunes_import(import) || self.facts.prunes_import(import)
        ));
        if !self.delegated.is_empty() {
            let ast = Ast::new(self.allocator);
            let events = ast.array(self.delegated.iter().map(|event| ast.string(event)));
            let call = if self.options.target == CompileTarget::Hydrate {
                let module = ast.string(
                    self.ir
                        .module_id
                        .as_deref()
                        .expect("managed target has canonical module identity"),
                );
                self.call("reze-js/internal/hydrate", "stageDelegation", [module, events])
            } else {
                self.call(RUNTIME_MODULE, "delegateEvents", [events])
            };
            program.body.push(ast.stmt(call));
        }
        if let Some(plan) = hot_plan {
            self.changed |= hot::apply(
                self.allocator,
                program,
                plan,
                self.filename,
                &mut self.namer,
                &mut self.helpers,
            );
        }
        self.helpers.install(self.allocator, program);
        if !self
            .scoping
            .scope_descendants_from_root()
            .any(|scope| self.scoping.scope_flags(scope).contains_direct_eval())
        {
            self.changed |= optimize::prune_imports(program, self.scoping);
            optimize::hoist_templates(&mut self, program);
        }
        if !self.hoisted.is_empty() {
            let ast = Ast::new(self.allocator);
            let at = program
                .body
                .iter()
                .rposition(|statement| matches!(statement, Statement::ImportDeclaration(_)))
                .map_or(0, |index| index + 1);
            let old = std::mem::replace(&mut program.body, ArenaVec::new_in(&ast.builder));
            let mut body = ArenaVec::with_capacity_in(old.len() + self.hoisted.len(), &ast.builder);
            let mut old = old.into_iter();
            body.extend(old.by_ref().take(at));
            body.extend(self.hoisted);
            body.extend(old);
            program.body = body;
        }
        self.changed
    }

    pub fn intern(&self, value: &str) -> &'a str {
        self.allocator.alloc_str(value)
    }

    pub fn fresh(&mut self, base: &str) -> &'a str {
        self.allocator.alloc_str(&self.namer.fresh(base))
    }

    pub fn helper(&mut self, source: &str, export: &str) -> Expression<'a> {
        let name = self.helpers.require(self.allocator, &mut self.namer, source, export);
        Ast::new(self.allocator).ident(name)
    }

    pub fn call(
        &mut self,
        source: &str,
        export: &str,
        args: impl IntoIterator<Item = Expression<'a>>,
    ) -> Expression<'a> {
        Ast::new(self.allocator).call(self.helper(source, export), args)
    }

    pub fn expr(&self, reference: ExprRef) -> Expression<'a> {
        self.expressions
            .borrow_mut()
            .remove(&reference.id)
            .expect("retained source expression is moved exactly once")
    }

    pub fn delegate(&mut self, event: &str) {
        self.delegated.insert(event.to_string());
    }

    pub fn view(&mut self, id: ViewId) -> Expression<'a> {
        let view = self.ir.view(id);
        let mut expression = if let Some(expression) = composite::emit(self, view) {
            expression
        } else {
            let ViewKind::Element(element) = &view.kind else {
                unreachable!("composite view emitted above")
            };
            match self.options.target {
                CompileTarget::Client => client::emit(self, element),
                CompileTarget::Hydrate => hydrate::emit(self, view, element),
                CompileTarget::Html => html::emit(self, view, element),
            }
        };
        *expression.span_mut() = view.origin;
        if let Expression::CallExpression(call) = &mut expression {
            match &mut call.callee {
                Expression::Identifier(callee) if callee.span == SPAN => callee.span = view.origin,
                Expression::ArrowFunctionExpression(arrow) if arrow.span == SPAN => {
                    if let ArrowFunctionBody::FunctionBody(body) = &mut arrow.body {
                        body.span = view.origin;
                        if let Some(statement) = body.statements.first_mut() {
                            *statement.span_mut() = view.origin;
                        }
                    }
                }
                _ => {}
            }
        }
        expression
    }

    pub fn getter(&self, dynamic: &Dynamic) -> Expression<'a> {
        let value = self.expr(dynamic.expr);
        match dynamic.getter {
            Some(GetterKind::Call(_)) => call_callee(value),
            _ => Ast::new(self.allocator).arrow([], value),
        }
    }

    pub fn flow_getter(&self, source: &FlowSource) -> Expression<'a> {
        let value = self.expr(source.expr);
        if source.getter.is_some() {
            call_callee(value)
        } else {
            Ast::new(self.allocator).arrow([], value)
        }
    }

    pub fn value(&mut self, value: &AttrValue) -> Expression<'a> {
        let ast = Ast::new(self.allocator);
        match value {
            AttrValue::True => ast.boolean(true),
            AttrValue::Str(value) => ast.string(value),
            AttrValue::Expr(value) => self.expr(*value),
            AttrValue::View(id) => self.view(*id),
            AttrValue::Dynamic(dynamic) => self.expr(dynamic.expr),
            AttrValue::Truthy(value) => ast.unary(
                UnaryOperator::LogicalNot,
                ast.unary(UnaryOperator::LogicalNot, self.expr(*value)),
            ),
            AttrValue::ClassParts(parts) => ast.array(parts.iter().map(|part| self.value(part))),
            AttrValue::TextParts(parts) => self.text(parts),
        }
    }

    fn text(&self, parts: &[TextPart]) -> Expression<'a> {
        let ast = Ast::new(self.allocator);
        let mut value = ast.string("");
        for part in parts {
            let next = match part {
                TextPart::Static(text) => ast.string(text),
                TextPart::Dynamic(dynamic) => self.expr(dynamic.expr),
            };
            value = ast.binary(value, BinaryOperator::Addition, next);
        }
        value
    }

    pub fn child(&mut self, child: &Child) -> Expression<'a> {
        let ast = Ast::new(self.allocator);
        match child {
            Child::StaticText(text) => ast.string(text),
            Child::View(id) => self.view(*id),
            Child::Dynamic(dynamic) if dynamic.mode == crate::ir::schedule::ValueMode::Once => {
                self.expr(dynamic.expr)
            }
            Child::Dynamic(dynamic) => self.getter(dynamic),
            Child::Conditional(branch) => {
                let test = self.expr(branch.test);
                let consequent = self.child(&branch.consequent);
                let alternate = branch.alternate.as_ref().map(|child| self.child(child));
                if self.options.target == CompileTarget::Client {
                    let test = if branch.test_is_boolean {
                        test
                    } else {
                        ast.unary(
                            UnaryOperator::LogicalNot,
                            ast.unary(UnaryOperator::LogicalNot, test),
                        )
                    };
                    let memo = self.fresh("_c$");
                    let computed =
                        self.call("reze-js/internal/reactivity", "computed", [ast.arrow([], test)]);
                    let value = ast.conditional(
                        ast.call(ast.ident(memo), []),
                        consequent,
                        alternate.unwrap_or_else(|| ast.boolean(false)),
                    );
                    return ast.call(
                        ast.block_arrow(
                            [],
                            [
                                ast.declaration(
                                    VariableDeclarationKind::Const,
                                    memo,
                                    Some(computed),
                                ),
                                ast.return_stmt(ast.arrow([], value)),
                            ],
                        ),
                        [],
                    );
                }
                let site = self.range_site(branch.origin, "branch");
                let mut args = vec![site, ast.arrow([], test), ast.arrow([], consequent)];
                args.extend(alternate.map(|value| ast.arrow([], value)));
                if self.options.target == CompileTarget::Html {
                    self.call("reze-js/internal/html", "hShow", args)
                } else {
                    self.call("reze-js/internal/hydrate", "prepareShow", args)
                }
            }
        }
    }

    pub fn assign_ref(
        &self,
        target: &crate::ir::view::AssignTarget,
        value: Expression<'a>,
    ) -> Expression<'a> {
        let ast = Ast::new(self.allocator);
        let target = match target {
            crate::ir::view::AssignTarget::Identifier(name) => ast.ident(self.intern(name)),
            crate::ir::view::AssignTarget::Member { object, key } => {
                let object = self.expr(*object);
                match key {
                    MemberKey::Static(name) if name.starts_with('#') => {
                        let private = PrivateIdentifier::new(
                            SPAN,
                            Ident::from(self.intern(&name[1..])),
                            &ast.builder,
                        );
                        Expression::new_private_field_expression(
                            SPAN,
                            object,
                            private,
                            false,
                            &ast.builder,
                        )
                    }
                    MemberKey::Static(name) if crate::html::is_identifier_name(name) => {
                        ast.member(object, self.intern(name))
                    }
                    MemberKey::Static(name) => ast.index(object, ast.string(name)),
                    MemberKey::Computed(key) => ast.index(object, self.expr(*key)),
                }
            }
        };
        ast.assign(target, value)
    }

    pub fn site(&mut self, view: &View) -> Expression<'a> {
        let name = self.site_name(view);
        Ast::new(self.allocator).ident(name)
    }

    pub fn site_name(&mut self, view: &View) -> &'a str {
        let site = view.site.as_ref().expect("managed view has an original source site");
        if let Some(&name) = self.sites.get(&site.ordinal) {
            return name;
        }
        let layout = layout(Ast::new(self.allocator), view);
        self.hoist_site(site, view.origin, layout)
    }

    fn origin_site(&mut self, origin: Span) -> Expression<'a> {
        let site = self
            .ir
            .callback_sites
            .get(&(origin.start, origin.end))
            .expect("managed call retains its original source site");
        let name = if let Some(&name) = self.sites.get(&site.ordinal) {
            name
        } else {
            self.hoist_site(site, origin, None)
        };
        Ast::new(self.allocator).ident(name)
    }

    fn range_site(&mut self, origin: Span, kind: &str) -> Expression<'a> {
        let site = self
            .ir
            .callback_sites
            .get(&(origin.start, origin.end))
            .expect("optimized range retains its original source site");
        let name = if let Some(&name) = self.sites.get(&site.ordinal) {
            name
        } else {
            let ast = Ast::new(self.allocator);
            self.hoist_site(site, origin, Some(ast.object([ast.prop("range", ast.string(kind))])))
        };
        Ast::new(self.allocator).ident(name)
    }

    fn hoist_site(
        &mut self,
        site: &SiteId,
        origin: Span,
        layout: Option<Expression<'a>>,
    ) -> &'a str {
        let ast = Ast::new(self.allocator);
        let name = self.fresh("_site$");
        let (line, column) = self.positions[&origin.start];
        let mut fields = vec![
            ast.prop(
                "key",
                ast.string(
                    &self.ir.sites.as_ref().expect("managed target has source sites").key(*site),
                ),
            ),
            ast.prop(
                "module",
                ast.string(
                    self.ir
                        .module_id
                        .as_deref()
                        .expect("managed target has canonical module identity"),
                ),
            ),
            ast.prop("ordinal", ast.number(f64::from(site.ordinal))),
            ast.prop("line", ast.number(f64::from(line))),
            ast.prop("column", ast.number(f64::from(column))),
        ];
        if let Some(layout) = layout {
            fields.push(ast.prop("layout", layout));
        }
        let value = ast.object(fields);
        self.hoisted.push(ast.declaration(VariableDeclarationKind::Const, name, Some(value)));
        self.sites.insert(site.ordinal, name);
        name
    }
}

fn call_callee(expression: Expression<'_>) -> Expression<'_> {
    let Expression::CallExpression(call) = unparenthesize(expression) else {
        unreachable!("getter proof identifies a call")
    };
    call.unbox().callee
}

fn unparenthesize(mut expression: Expression<'_>) -> Expression<'_> {
    while let Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = parenthesized.unbox().expression;
    }
    expression
}

fn namespace(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Html => "",
        Namespace::Svg => "svg",
        Namespace::MathMl => "math",
    }
}

fn layout<'a>(ast: Ast<'a>, view: &View) -> Option<Expression<'a>> {
    let range = match &view.kind {
        ViewKind::Element(element) => {
            let nodes = element.statics.nodes.iter().enumerate().map(|(index, node)| {
                let mut fields = vec![
                    ast.prop("parent", node.parent.map_or_else(|| ast.null(), |parent| ast.number(f64::from(parent)))),
                    ast.prop("children", ast.array(node.children.iter().map(|child| ast.number(f64::from(*child))))),
                    ast.prop("kind", ast.string(match node.kind { StaticNodeKind::Element => "element", StaticNodeKind::Text => "text", StaticNodeKind::Marker => "marker" })),
                ];
                match node.kind {
                    StaticNodeKind::Element => {
                        fields.push(ast.prop("tag", ast.string(&node.tag)));
                        fields.push(ast.prop("ns", ast.string(namespace(node.ns))));
                        fields.push(ast.prop("attrs", ast.array(node.attrs.iter().map(|attr| ast.array([
                            ast.string(&attr.name), attr.value.as_ref().map_or_else(|| ast.null(), |value| ast.string(value)),
                        ])))));
                    }
                    StaticNodeKind::Text => {
                        fields.push(ast.prop("text", ast.string(&node.text)));
                        if element.props.iter().any(|prop| matches!(prop, ElementProp::Attr(attr) if attr.node as usize == index && matches!(attr.target, AttrTarget::Text))) {
                            fields.push(ast.prop("dynamic", ast.boolean(true)));
                        }
                    }
                    StaticNodeKind::Marker => {}
                }
                ast.object(fields)
            });
            let inserts = element.inserts.iter().map(|insert| {
                ast.object([
                    ast.prop("slot", ast.number(f64::from(insert.slot))),
                    ast.prop("parent", ast.number(f64::from(insert.parent))),
                    ast.prop(
                        "anchor",
                        match insert.anchor {
                            Anchor::Only => ast.string("only"),
                            Anchor::End => ast.string("end"),
                            Anchor::Before(index) => ast.number(f64::from(index)),
                        },
                    ),
                ])
            });
            return Some(ast.object([
                ast.prop("tag", ast.string(&element.tag)),
                ast.prop("ns", ast.string(namespace(element.namespace))),
                ast.prop("nodes", ast.array(nodes)),
                ast.prop("inserts", ast.array(inserts)),
            ]));
        }
        ViewKind::Fragment(_) => "fragment",
        ViewKind::Component(component) if component.island.is_some() => "island",
        ViewKind::Component(_) => return None,
        ViewKind::Flow(FlowView::For { .. } | FlowView::Repeat { .. } | FlowView::Rows { .. }) => {
            "list"
        }
        ViewKind::Flow(FlowView::Portal { .. }) => "portal",
        ViewKind::Flow(_) => "branch",
    };
    Some(ast.object([ast.prop("range", ast.string(range))]))
}

fn source_positions(source: &str, ir: &ModuleIr) -> HashMap<u32, (u32, u32)> {
    if ir.sites.is_none() {
        return HashMap::new();
    }
    let mut offsets: Vec<u32> = ir
        .views
        .iter()
        .filter(|view| view.site.is_some())
        .map(|view| view.origin.start)
        .chain(ir.callback_sites.keys().map(|(start, _)| *start))
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut out = HashMap::with_capacity(offsets.len());
    let mut wanted = offsets.into_iter().peekable();
    let (mut line, mut column, mut previous_cr) = (1, 0, false);
    for (offset, character) in source.char_indices().chain(std::iter::once((source.len(), '\0'))) {
        if wanted.peek() == Some(&(offset as u32)) {
            out.insert(offset as u32, (line, column));
            wanted.next();
        }
        if wanted.peek().is_none() {
            break;
        }
        match character {
            '\n' if previous_cr => {}
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                line += 1;
                column = 0;
            }
            character => column += character.len_utf16() as u32,
        }
        previous_cr = character == '\r';
    }
    out
}

struct Replace<'c, 'a, 'm> {
    ctx: &'c mut EmitContext<'a, 'm>,
}

impl<'a> VisitMut<'a> for Replace<'_, 'a, '_> {
    fn visit_statement(&mut self, statement: &mut Statement<'a>) {
        let inline_return = matches!(statement,
            Statement::ReturnStatement(returned) if returned.argument.as_ref().is_some_and(|argument|
                matches!(argument.without_parentheses(), Expression::JSXElement(_) | Expression::JSXFragment(_)))
        );
        walk_mut::walk_statement(self, statement);
        if !inline_return {
            return;
        }
        let Statement::ReturnStatement(returned) = statement else { unreachable!() };
        let argument = returned.argument.take().expect("view return has an argument");
        match inline_view_body(argument) {
            Ok(body) => {
                let ArrowFunctionBody::FunctionBody(body) = body else { unreachable!() };
                let ast = Ast::new(self.ctx.allocator);
                *statement =
                    Statement::new_block_statement(SPAN, body.unbox().statements, &ast.builder);
            }
            Err(argument) => returned.argument = Some(argument),
        }
    }

    fn visit_arrow_function_expression(&mut self, arrow: &mut ArrowFunctionExpression<'a>) {
        let inline_expression = arrow.body.as_expression().is_some_and(|expression| {
            matches!(
                expression.without_parentheses(),
                Expression::JSXElement(_) | Expression::JSXFragment(_)
            )
        });
        walk_mut::walk_arrow_function_expression(self, arrow);
        if inline_expression {
            let expression = arrow
                .body
                .as_expression_mut()
                .expect("view arrow has an expression body")
                .take_in(&self.ctx.allocator);
            arrow.body = match inline_view_body(expression) {
                Ok(body) => body,
                Err(expression) => ArrowFunctionBody::from(expression),
            };
        }
    }

    fn visit_call_expression(&mut self, call: &mut CallExpression<'a>) {
        if self.ctx.facts.primitive(self.ctx.scoping, &call.callee)
            == Some(crate::frontend::analysis::Primitive::Action)
        {
            continuation::prepare_action(self.ctx, call);
        }
        if let Some(kind) = self.ctx.facts.runtime_calls.get(&call.node_id.get()).copied() {
            calls::rewrite(self.ctx, call, kind);
        }
        walk_mut::walk_call_expression(self, call);
    }

    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Some(tag) = self.ctx.facts.dynamic_tags.get(&expression.node_id()).copied() {
            let span = expression.span();
            let value = expression.take_in(&self.ctx.allocator);
            *expression = calls::native_type(self.ctx, tag, value);
            *expression.span_mut() = span;
            self.ctx.changed = true;
            return;
        }
        if let Expression::CallExpression(call) = expression
            && self.ctx.facts.folded_read(call).is_some()
        {
            *expression = call.callee.take_in(&self.ctx.allocator);
            self.ctx.changed = true;
            return;
        }
        if matches!(expression, Expression::JSXElement(_) | Expression::JSXFragment(_)) {
            let span = expression.span();
            let view = self.ctx.ir.by_span[&(span.start, span.end)];
            let mut wanted = HashSet::new();
            let mut seen = HashSet::new();
            collect_references(
                self.ctx.ir,
                view,
                self.ctx.options.target != CompileTarget::Html,
                &mut seen,
                &mut wanted,
            );
            Harvest { ctx: self.ctx, wanted }.visit_expression(expression);
            *expression = self.ctx.view(view);
            self.ctx.changed = true;
        } else {
            walk_mut::walk_expression(self, expression);
        }
    }

    fn visit_variable_declarator(&mut self, declarator: &mut VariableDeclarator<'a>) {
        if self.ctx.facts.folded_getter(declarator).is_some() {
            let BindingPattern::ArrayPattern(pattern) = declarator.id.take_in(&self.ctx.allocator)
            else {
                unreachable!("folded signal has a getter binding")
            };
            let mut pattern = pattern.unbox();
            declarator.id = pattern.elements.remove(0).expect("folded signal getter exists");
            let initializer = declarator.init.take().expect("folded signal has an initializer");
            let Expression::CallExpression(call) = unparenthesize(initializer) else {
                unreachable!("folded signal has a factory call")
            };
            let mut call = call.unbox();
            declarator.init = Some(call.arguments.remove(0).into_expression());
            self.ctx.changed = true;
        }
        if self.ctx.options.debug_names && self.ctx.options.target != CompileTarget::Html {
            let binding = match &declarator.id {
                BindingPattern::BindingIdentifier(binding) => Some(binding.as_ref()),
                BindingPattern::ArrayPattern(pattern) => match pattern.elements.first() {
                    Some(Some(BindingPattern::BindingIdentifier(binding))) => {
                        Some(binding.as_ref())
                    }
                    _ => None,
                },
                _ => None,
            };
            if let Some(binding) = binding
                && let Some(call) = declarator.init.as_mut().and_then(call_mut)
                && call.arguments.len() < 2
                && !call
                    .arguments
                    .iter()
                    .any(|argument| matches!(argument, Argument::SpreadElement(_)))
                && matches!(
                    self.ctx.facts.primitive(self.ctx.scoping, &call.callee),
                    Some(
                        crate::frontend::analysis::Primitive::Signal
                            | crate::frontend::analysis::Primitive::Computed
                            | crate::frontend::analysis::Primitive::Action
                    )
                )
            {
                let ast = Ast::new(self.ctx.allocator);
                if call.arguments.is_empty() {
                    call.arguments.push(Argument::from(ast.undefined()));
                }
                call.arguments.push(Argument::from(
                    ast.object([ast.prop("name", ast.string(binding.name.as_str()))]),
                ));
                self.ctx.changed = true;
            }
        }
        walk_mut::walk_variable_declarator(self, declarator);
    }
}

fn inline_view_body(expression: Expression<'_>) -> Result<ArrowFunctionBody<'_>, Expression<'_>> {
    let expression = unparenthesize(expression);
    let is_inline_block = matches!(&expression,
        Expression::CallExpression(call) if call.arguments.is_empty()
            && matches!(&call.callee, Expression::ArrowFunctionExpression(arrow)
                if !arrow.r#async && arrow.params.items.is_empty() && arrow.params.rest.is_none()
                    && matches!(&arrow.body, ArrowFunctionBody::FunctionBody(body) if body.directives.is_empty()))
    );
    if !is_inline_block {
        return Err(expression);
    }
    let Expression::CallExpression(call) = expression else { unreachable!() };
    let Expression::ArrowFunctionExpression(arrow) = call.unbox().callee else { unreachable!() };
    Ok(arrow.unbox().body)
}

fn call_mut<'e, 'a>(expression: &'e mut Expression<'a>) -> Option<&'e mut CallExpression<'a>> {
    match expression {
        Expression::CallExpression(call) => Some(call),
        Expression::ParenthesizedExpression(parenthesized) => {
            call_mut(&mut parenthesized.expression)
        }
        _ => None,
    }
}

fn collect_references(
    ir: &ModuleIr,
    id: ViewId,
    client: bool,
    seen: &mut HashSet<ViewId>,
    wanted: &mut HashSet<NodeId>,
) {
    if !seen.insert(id) {
        return;
    }
    ir.view(id).for_each_reference(client, &mut |reference| match reference {
        ViewReference::Expression(reference) => {
            wanted.insert(reference.id);
        }
        ViewReference::View(id) => collect_references(ir, id, client, seen, wanted),
    });
}

struct Harvest<'c, 'a, 'm> {
    ctx: &'c mut EmitContext<'a, 'm>,
    wanted: HashSet<NodeId>,
}

impl<'a> VisitMut<'a> for Harvest<'_, 'a, '_> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        let id = expression.node_id();
        if self.wanted.remove(&id) {
            Replace { ctx: self.ctx }.visit_expression(expression);
            self.ctx.expressions.borrow_mut().insert(id, expression.take_in(&self.ctx.allocator));
        } else {
            walk_mut::walk_expression(self, expression);
        }
    }

    fn visit_jsx_element_name(&mut self, name: &mut JSXElementName<'a>) {
        let id = name.node_id();
        if self.wanted.remove(&id) {
            let value = jsx_name(self.ctx.allocator, name.take_in(&self.ctx.allocator));
            self.ctx.expressions.borrow_mut().insert(id, value);
        }
    }

    fn visit_jsx_attribute(&mut self, attribute: &mut JSXAttribute<'a>) {
        let ast = Ast::new(self.ctx.allocator);
        let id = attribute.node_id();
        if self.wanted.remove(&id) {
            self.ctx.expressions.borrow_mut().insert(id, ast.boolean(true));
            return;
        }
        if let Some(value) = &attribute.value {
            let id = value.node_id();
            if self.wanted.remove(&id) {
                let mut expression = match attribute.value.take().expect("attribute value present")
                {
                    JSXAttributeValue::StringLiteral(value) => {
                        let decoded = crate::html::decode_entities(value.value.as_str());
                        Expression::new_string_literal(
                            value.span,
                            oxc_str::Str::from_str_in(&decoded, &ast.builder),
                            None,
                            &ast.builder,
                        )
                    }
                    JSXAttributeValue::Element(element) => Expression::JSXElement(element),
                    JSXAttributeValue::Fragment(fragment) => Expression::JSXFragment(fragment),
                    JSXAttributeValue::ExpressionContainer(container) => {
                        match container.unbox().expression {
                            JSXExpression::EmptyExpression(_) => ast.undefined(),
                            expression => expression.into_expression(),
                        }
                    }
                };
                Replace { ctx: self.ctx }.visit_expression(&mut expression);
                self.ctx.expressions.borrow_mut().insert(id, expression);
                return;
            }
        }
        walk_mut::walk_jsx_attribute(self, attribute);
    }
}

fn jsx_name<'a>(allocator: &'a Allocator, name: JSXElementName<'a>) -> Expression<'a> {
    let ast = Ast::new(allocator);
    match name {
        JSXElementName::IdentifierReference(identifier) => Expression::Identifier(identifier),
        JSXElementName::ThisExpression(this) => Expression::ThisExpression(this),
        JSXElementName::MemberExpression(member) => jsx_member(allocator, member.unbox()),
        JSXElementName::Identifier(identifier) => Expression::new_identifier(
            identifier.span,
            Ident::from(allocator.alloc_str(identifier.name.as_str())),
            &ast.builder,
        ),
        JSXElementName::NamespacedName(_) => unreachable!("namespaced JSX names are native tags"),
    }
}

fn jsx_member<'a>(allocator: &'a Allocator, member: JSXMemberExpression<'a>) -> Expression<'a> {
    let ast = Ast::new(allocator);
    let object = match member.object {
        JSXMemberExpressionObject::IdentifierReference(identifier) => {
            Expression::Identifier(identifier)
        }
        JSXMemberExpressionObject::ThisExpression(this) => Expression::ThisExpression(this),
        JSXMemberExpressionObject::MemberExpression(member) => {
            jsx_member(allocator, member.unbox())
        }
    };
    let property = IdentifierName::new(
        member.property.span,
        Ident::from(allocator.alloc_str(member.property.name.as_str())),
        &ast.builder,
    );
    Expression::new_static_member_expression(member.span, object, property, false, &ast.builder)
}

pub fn serialize_source_map(map: oxc_sourcemap::SourceMap<'_>) -> String {
    if !map.get_names().any(|name| name.starts_with('<')) {
        return map.to_json_string();
    }
    let mut parts = map.into_parts();
    let mut names = Vec::with_capacity(parts.names.len());
    let mut next = 0;
    parts.names.retain(|name| {
        let keep = !name.starts_with('<');
        names.push(keep.then_some(next));
        next += u32::from(keep);
        keep
    });
    for token in &mut parts.tokens {
        if let Some(name) = token.get_name_id() {
            *token = oxc_sourcemap::Token::new(
                token.get_dst_line(),
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
                token.get_source_id(),
                names[name as usize],
            );
        }
    }
    parts.token_chunks = None;
    oxc_sourcemap::SourceMap::from_parts(parts).to_json_string()
}
