use std::collections::HashSet;

use oxc_allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, SPAN, Span};
use oxc_str::Ident;
use oxc_syntax::number::NumberBase;
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;

use super::Namer;
use super::analysis::AsyncFacts;
use super::pure::{is_component_name, is_declared_component};
use crate::diagnostic::{Code, Report};
use crate::imports::HelperImports;

#[derive(Default)]
pub struct Plan {
    entries: Vec<Entry>,
}

#[derive(Clone)]
struct Entry {
    function_start: u32,
    function_span: Span,
    head: (u32, u32),
    first: (u32, u32),
    last: (u32, u32),
    context: bool,
    values: Vec<String>,
    reads: Vec<(ReferenceId, usize)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry(&self, start: u32) -> Option<Entry> {
        self.entries.iter().find(|entry| entry.function_start == start).cloned()
    }
}

pub fn collect(
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) -> Plan {
    let facts = AsyncFacts::collect(program, scoping, nodes);
    let mut plan = Plan::default();
    Collector { facts: &facts, reports, plan: &mut plan }.visit_program(program);
    plan
}

pub fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    plan: Plan,
    namer: &mut Namer<'a>,
    helpers: &mut HelperImports<'a>,
) -> bool {
    if plan.entries.is_empty() {
        return false;
    }
    let async_component =
        helpers.require(allocator, namer, crate::RUNTIME_MODULE, "asyncComponent");
    let mut rewrite = Rewrite {
        alloc: allocator,
        plan: &plan,
        namer,
        async_component,
        done: HashSet::new(),
        changed: false,
    };
    rewrite.visit_program(program);
    rewrite.changed
}

struct Collector<'f, 'r, 'p> {
    facts: &'f AsyncFacts,
    reports: &'r mut Vec<Report>,
    plan: &'p mut Plan,
}

impl Collector<'_, '_, '_> {
    fn component(&mut self, span: Span, name: &str, statements: &[Statement<'_>]) {
        let start = span.start;
        let Some(result) = self.facts.plan(start) else { return };
        match result {
            Err(reject) => {
                self.reports.push(
                    Report::new(Code::AsyncComponentShape, reject.span)
                        .arg("component", name)
                        .arg("reason", reject.reason),
                );
            }
            Ok(found) => {
                let head = statements[0].span();
                let first = statements[found.first].span();
                let last = statements[found.last].span();
                let mut reads = Vec::new();
                let mut scan = ReadScan { facts: self.facts, start, reads: &mut reads };
                for statement in &statements[found.last + 1..] {
                    scan.visit_statement(statement);
                }
                self.plan.entries.push(Entry {
                    function_start: start,
                    function_span: span,
                    head: (head.start, head.end),
                    first: (first.start, first.end),
                    last: (last.start, last.end),
                    context: !found.tracked.is_empty(),
                    values: found.values.clone(),
                    reads,
                });
            }
        }
    }
}

impl<'a> Visit<'a> for Collector<'_, '_, '_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.r#async
            && !it.generator
            && let Some(id) = &it.id
            && is_component_name(id.name.as_str())
            && let Some(body) = &it.body
        {
            self.component(it.span, id.name.as_str(), &body.statements);
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && is_component_name(id.name.as_str())
            && let Some(init) = it.init.as_ref().map(Expression::without_parentheses)
        {
            match init {
                Expression::ArrowFunctionExpression(arrow) if arrow.r#async => {
                    if let ArrowFunctionBody::FunctionBody(body) = &arrow.body {
                        self.component(arrow.span, id.name.as_str(), &body.statements);
                    }
                }
                Expression::FunctionExpression(function)
                    if function.r#async
                        && !function.generator
                        && is_declared_component(function) =>
                {
                    if let Some(body) = &function.body {
                        self.component(function.span, id.name.as_str(), &body.statements);
                    }
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

struct ReadScan<'f, 'r> {
    facts: &'f AsyncFacts,
    start: u32,
    reads: &'r mut Vec<(ReferenceId, usize)>,
}

impl<'a> Visit<'a> for ReadScan<'_, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some((start, index)) = self.facts.read(it)
            && start == self.start
            && let Some(reference) = it.reference_id.get()
        {
            self.reads.push((reference, index));
        }
    }
}

struct Rewrite<'x, 'p, 'a> {
    alloc: &'a Allocator,
    plan: &'p Plan,
    namer: &'x mut Namer<'a>,
    async_component: &'a str,
    done: HashSet<u32>,
    changed: bool,
}

impl<'a> VisitMut<'a> for Rewrite<'_, '_, 'a> {
    fn visit_function(&mut self, it: &mut Function<'a>, flags: ScopeFlags) {
        if it.span != SPAN && !self.done.contains(&it.span.start) {
            if let Some(entry) = self.plan.entry(it.span.start) {
                if split_function(self.alloc, self.namer, self.async_component, it, &entry) {
                    self.done.insert(it.span.start);
                    self.changed = true;
                }
            }
        }
        walk_mut::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &mut ArrowFunctionExpression<'a>) {
        if it.span != SPAN && !self.done.contains(&it.span.start) {
            if let Some(entry) = self.plan.entry(it.span.start) {
                if split_arrow(self.alloc, self.namer, self.async_component, it, &entry) {
                    self.done.insert(it.span.start);
                    self.changed = true;
                }
            }
        }
        walk_mut::walk_arrow_function_expression(self, it);
    }
}

fn split_function<'a>(
    alloc: &'a Allocator,
    namer: &mut Namer<'a>,
    async_component: &'a str,
    it: &mut Function<'a>,
    entry: &Entry,
) -> bool {
    let Some(body) = it.body.as_mut() else { return false };
    if !split_body(alloc, namer, async_component, body, entry) {
        return false;
    }
    it.r#async = false;
    narrow_return(alloc, &mut it.return_type);
    true
}

fn split_arrow<'a>(
    alloc: &'a Allocator,
    namer: &mut Namer<'a>,
    async_component: &'a str,
    it: &mut ArrowFunctionExpression<'a>,
    entry: &Entry,
) -> bool {
    let ArrowFunctionBody::FunctionBody(body) = &mut it.body else { return false };
    if !split_body(alloc, namer, async_component, body, entry) {
        return false;
    }
    it.r#async = false;
    narrow_return(alloc, &mut it.return_type);
    true
}

fn split_body<'a>(
    alloc: &'a Allocator,
    namer: &mut Namer<'a>,
    async_component: &'a str,
    body: &mut FunctionBody<'a>,
    entry: &Entry,
) -> bool {
    let mut head = None;
    let mut first = None;
    let mut last = None;
    for (index, statement) in body.statements.iter().enumerate() {
        let span = statement.span();
        if span.start == entry.head.0 && span.end == entry.head.1 {
            head = Some(index);
        }
        if span.start == entry.first.0 && span.end == entry.first.1 {
            first = Some(index);
        }
        if span.start == entry.last.0 && span.end == entry.last.1 {
            last = Some(index);
        }
    }
    let (Some(head), Some(first), Some(last)) = (head, first, last) else { return false };
    let context: Option<&'a str> = entry.context.then(|| alloc.alloc_str(&namer.fresh("_c$")));
    if let Some(context) = context {
        for (index, statement) in body.statements.iter_mut().enumerate() {
            if index < first || index > last {
                continue;
            }
            if has_await(statement) {
                if index == first {
                    continue;
                }
                wrap_await_operand(alloc, context, statement);
            } else if let Statement::VariableDeclaration(declaration) = statement {
                for declarator in declaration.declarations.iter_mut() {
                    if let Some(init) = declarator.init.as_mut() {
                        wrap_tracked(alloc, context, init);
                    }
                }
            }
        }
    }
    let builder = AstBuilder::new(alloc);
    let taken = std::mem::replace(&mut body.statements, ArenaVec::new_in(&builder));
    let mut outside = ArenaVec::new_in(&builder);
    let mut load = ArenaVec::new_in(&builder);
    let mut rest = ArenaVec::new_in(&builder);
    for (index, statement) in taken.into_iter().enumerate() {
        if index < head {
            outside.push(statement);
        } else if index <= last {
            load.push(statement);
        } else {
            rest.push(statement);
        }
    }
    let values: Option<&'a str> =
        (!entry.values.is_empty()).then(|| alloc.alloc_str(&namer.fresh("_v$")));
    let mut rewrite = ReadRewrite { alloc, values, reads: &entry.reads };
    for statement in rest.iter_mut() {
        rewrite.visit_statement(statement);
    }
    let mut elements = ArenaVec::new_in(&builder);
    for name in &entry.values {
        let text: &'a str = alloc.alloc_str(name);
        elements.push(ArrayExpressionElement::from(reference(alloc, SPAN, text)));
    }
    let array = Expression::new_array_expression(SPAN, elements, &builder);
    load.push(Statement::new_return_statement(SPAN, Some(array), &builder));
    let load_arrow = loader(alloc, true, context, load);
    let body_arrow = loader(alloc, false, values, rest);
    let mut arguments = ArenaVec::new_in(&builder);
    arguments.push(Argument::from(load_arrow));
    arguments.push(Argument::from(body_arrow));
    let call = Expression::new_call_expression(
        entry.function_span,
        reference(alloc, SPAN, async_component),
        None,
        arguments,
        false,
        &builder,
    );
    outside.push(Statement::new_return_statement(SPAN, Some(call), &builder));
    body.statements = outside;
    true
}

fn narrow_return<'a>(
    alloc: &'a Allocator,
    returned: &mut Option<ArenaBox<'a, TSTypeAnnotation<'a>>>,
) {
    let Some(annotation) = returned else { return };
    let TSType::TSTypeReference(reference) = &mut annotation.type_annotation else { return };
    let TSTypeName::IdentifierReference(name) = &reference.type_name else { return };
    if name.name.as_str() != "Promise" {
        return;
    }
    let Some(arguments) = reference.type_arguments.as_mut() else { return };
    if arguments.params.len() != 1 {
        return;
    }
    let taken = std::mem::replace(&mut arguments.params, ArenaVec::new_in(&alloc));
    let Some(inner) = taken.into_iter().next() else { return };
    annotation.type_annotation = inner;
}

fn wrap_await_operand<'a>(alloc: &'a Allocator, context: &'a str, statement: &mut Statement<'a>) {
    let argument = match statement {
        Statement::VariableDeclaration(declaration) if declaration.declarations.len() == 1 => {
            let mut declarators = declaration.declarations.iter_mut();
            let (Some(declarator), None) = (declarators.next(), declarators.next()) else { return };
            let Some(init) = declarator.init.as_mut() else { return };
            let Expression::AwaitExpression(awaited) = init else { return };
            &mut awaited.argument
        }
        Statement::ExpressionStatement(expression) => {
            let Expression::AwaitExpression(awaited) = &mut expression.expression else { return };
            &mut awaited.argument
        }
        _ => return,
    };
    wrap_tracked(alloc, context, argument);
}

fn wrap_tracked<'a>(alloc: &'a Allocator, context: &'a str, slot: &mut Expression<'a>) {
    let builder = AstBuilder::new(alloc);
    let span = slot.span();
    let inner = std::mem::replace(slot, Expression::new_null_literal(span, &builder));
    let params = FormalParameters::boxed(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        ArenaVec::new_in(&builder),
        None,
        &builder,
    );
    let thunk = Expression::new_arrow_function_expression(
        SPAN,
        false,
        None,
        params,
        None,
        ArrowFunctionBody::from(inner),
        &builder,
    );
    let mut arguments = ArenaVec::new_in(&builder);
    arguments.push(Argument::from(thunk));
    let property = IdentifierName::new(SPAN, Ident::from("get"), &builder);
    let callee = Expression::new_static_member_expression(
        SPAN,
        reference(alloc, SPAN, context),
        property,
        false,
        &builder,
    );
    *slot = Expression::new_call_expression(SPAN, callee, None, arguments, false, &builder);
}

struct ReadRewrite<'x, 'a> {
    alloc: &'a Allocator,
    values: Option<&'a str>,
    reads: &'x [(ReferenceId, usize)],
}

impl ReadRewrite<'_, '_> {
    fn lookup(&self, reference: ReferenceId) -> Option<usize> {
        self.reads.iter().find(|(read, _)| *read == reference).map(|(_, index)| *index)
    }
}

impl<'a> VisitMut<'a> for ReadRewrite<'_, 'a> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        let hit = match it {
            Expression::Identifier(id) => id
                .reference_id
                .get()
                .and_then(|reference| self.lookup(reference).map(|index| (id.span, index))),
            _ => None,
        };
        if let Some((span, index)) = hit
            && let Some(values) = self.values
        {
            *it = values_read(self.alloc, span, values, index);
            return;
        }
        walk_mut::walk_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &mut ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some(reference) = id.reference_id.get()
            && let Some(index) = self.lookup(reference)
            && let Some(values) = self.values
        {
            let span = id.span;
            it.shorthand = false;
            it.value = values_read(self.alloc, span, values, index);
        }
        walk_mut::walk_object_property(self, it);
    }
}

fn reference<'a>(alloc: &'a Allocator, span: Span, name: &'a str) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_identifier(span, Ident::from(name), &builder)
}

fn values_read<'a>(
    alloc: &'a Allocator,
    span: Span,
    values: &'a str,
    index: usize,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    let empty: ArenaVec<'a, Argument<'a>> = ArenaVec::new_in(&builder);
    let call = Expression::new_call_expression(
        span,
        reference(alloc, SPAN, values),
        None,
        empty,
        false,
        &builder,
    );
    let index_expr =
        Expression::new_numeric_literal(SPAN, index as f64, None, NumberBase::Decimal, &builder);
    Expression::new_computed_member_expression(span, call, index_expr, false, &builder)
}

fn loader<'a>(
    alloc: &'a Allocator,
    is_async: bool,
    param: Option<&'a str>,
    statements: ArenaVec<'a, Statement<'a>>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    let mut items = ArenaVec::new_in(&builder);
    if let Some(name) = param {
        let pattern = BindingPattern::new_binding_identifier(SPAN, Ident::from(name), &builder);
        items.push(FormalParameter::new(
            SPAN,
            ArenaVec::new_in(&builder),
            pattern,
            None,
            None,
            false,
            None,
            false,
            false,
            &builder,
        ));
    }
    let params = FormalParameters::boxed(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        items,
        None,
        &builder,
    );
    let body = ArrowFunctionBody::new_function_body(
        SPAN,
        ArenaVec::new_in(&builder),
        statements,
        &builder,
    );
    Expression::new_arrow_function_expression(SPAN, is_async, None, params, None, body, &builder)
}

#[derive(Default)]
struct AwaitCheck {
    found: bool,
}

impl<'a> Visit<'a> for AwaitCheck {
    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.found = true;
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if it.r#await {
            self.found = true;
        } else {
            walk::walk_for_of_statement(self, it);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

fn has_await(statement: &Statement<'_>) -> bool {
    let mut check = AwaitCheck::default();
    check.visit_statement(statement);
    check.found
}
