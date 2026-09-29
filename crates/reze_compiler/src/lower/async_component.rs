use std::collections::{HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::props::is_declared_component;
use super::{Lowerer, has_jsx, is_component_name};
use crate::diagnostic::{Code, Report};
use crate::ir::{Hole, HoleKind, ScriptEdit};

pub struct AsyncFunction<'b, 'a> {
    pub span: Span,
    pub body: &'b FunctionBody<'a>,
    pub return_type: Option<&'b TSTypeAnnotation<'a>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Reject {
    reason: &'static str,
    span: Span,
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

#[derive(Default)]
struct ReturnCheck {
    found: bool,
}

impl<'a> Visit<'a> for ReturnCheck {
    fn visit_return_statement(&mut self, _: &ReturnStatement<'a>) {
        self.found = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

struct References<'s> {
    scoping: &'s Scoping,
    symbols: HashSet<SymbolId>,
    /// Value references in visit order, with the symbol each resolves to.
    ordered: std::vec::Vec<(ReferenceId, SymbolId)>,
}

impl<'a> Visit<'a> for References<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(reference) = it.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
        {
            self.symbols.insert(symbol);
            if self.scoping.get_reference(reference).flags().is_value() {
                self.ordered.push((reference, symbol));
            }
        }
    }
}

/// How an `async` component splits: the load step runs `statements[..=last]` once per load,
/// and the body reads each awaited value through a getter.
pub struct AsyncPlan {
    pub first: usize,
    pub last: usize,
    pub tracked: std::vec::Vec<Span>,
    /// Awaited bindings read after the last await, in declaration order.
    pub values: std::vec::Vec<String>,
}

/// The split of every `async` component in a module, collected in the analysis phase:
/// by the start of the component function its plan (or why it cannot be rewritten),
/// and every awaited-value reference in a body → (component function start, values index).
#[derive(Default)]
pub struct AsyncFacts {
    plans: HashMap<u32, Result<AsyncPlan, Reject>>,
    reads: HashMap<ReferenceId, (u32, usize)>,
}

impl AsyncFacts {
    pub fn collect(program: &Program<'_>, scoping: &Scoping, nodes: &AstNodes<'_>) -> Self {
        let mut collector = AsyncCollector { scoping, nodes, facts: AsyncFacts::default() };
        collector.visit_program(program);
        collector.facts
    }

    pub fn plan(&self, function_start: u32) -> Option<&Result<AsyncPlan, Reject>> {
        self.plans.get(&function_start)
    }

    pub fn is_read(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id.get().is_some_and(|r| self.reads.contains_key(&r))
    }

    pub fn read(&self, id: &IdentifierReference<'_>) -> Option<(u32, usize)> {
        id.reference_id.get().and_then(|r| self.reads.get(&r)).copied()
    }
}

/// Components as the lowering sees them: `async function C` with a capitalized name, or
/// `const C = …` with an async arrow with a block body or an async function expression
/// that is not itself named as a component.
struct AsyncCollector<'c, 's> {
    scoping: &'s Scoping,
    nodes: &'c AstNodes<'c>,
    facts: AsyncFacts,
}

impl AsyncCollector<'_, '_> {
    fn component(&mut self, start: u32, statements: &[Statement<'_>]) {
        let Some(result) = plan(start, statements, self.scoping, self.nodes) else { return };
        match result {
            Ok((plan, reads)) => {
                self.facts.reads.extend(reads);
                self.facts.plans.insert(start, Ok(plan));
            }
            Err(reject) => {
                self.facts.plans.insert(start, Err(reject));
            }
        }
    }
}

impl<'a> Visit<'a> for AsyncCollector<'_, '_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.r#async
            && !it.generator
            && let Some(id) = &it.id
            && is_component_name(id.name.as_str())
            && let Some(body) = &it.body
        {
            self.component(it.span.start, &body.statements);
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
                        self.component(arrow.span.start, &body.statements);
                    }
                }
                Expression::FunctionExpression(function)
                    if function.r#async
                        && !function.generator
                        && is_declared_component(function) =>
                {
                    if let Some(body) = &function.body {
                        self.component(function.span.start, &body.statements);
                    }
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

/// An awaited-value reference → (component function start, values index).
type AsyncReads = std::vec::Vec<(ReferenceId, (u32, usize))>;

/// The plan for `statements`, `None` when nothing awaits. `values` keeps declaration order,
/// deduplicated by symbol; every value-position reference after the last await is recorded
/// with its index. A value assigned anywhere is rejected (`value-reassigned`).
fn plan(
    function_start: u32,
    statements: &[Statement<'_>],
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<Result<(AsyncPlan, AsyncReads), Reject>> {
    let (first, last) = await_range(statements)?;
    let mut tracked = std::vec::Vec::new();
    for (i, statement) in statements[..=last].iter().enumerate() {
        let reject = |reason| Reject { reason, span: statement.span() };
        if has_jsx(|check| check.visit_statement(statement)) {
            return Some(Err(reject("jsx-before-await")));
        }
        if contains_return(statement) {
            return Some(Err(reject("return-before-await")));
        }
        if i < first {
            continue;
        }
        if contains_await(statement) {
            let operand = match await_operand(statement) {
                Ok(operand) => operand,
                Err(reject) => return Some(Err(reject)),
            };
            if i > first {
                tracked.push(operand.span());
            }
        } else if let Statement::VariableDeclaration(declaration) = statement {
            tracked.extend(
                declaration.declarations.iter().filter_map(|d| d.init.as_ref()).map(GetSpan::span),
            );
        } else {
            return Some(Err(reject("statement-between-awaits")));
        }
    }

    let mut declared = std::vec::Vec::new();
    statements[..=last].iter().for_each(|statement| declared_symbols(statement, &mut declared));
    let mut references =
        References { scoping, symbols: HashSet::new(), ordered: std::vec::Vec::new() };
    statements[last + 1..].iter().for_each(|statement| references.visit_statement(statement));
    let mut seen = HashSet::new();
    let kept: std::vec::Vec<(SymbolId, &str)> = declared
        .into_iter()
        .filter(|(symbol, _)| references.symbols.contains(symbol) && seen.insert(*symbol))
        .collect();
    for (symbol, _) in &kept {
        if let Some(&r) = scoping
            .get_resolved_reference_ids(*symbol)
            .iter()
            .find(|r| scoping.get_reference(**r).is_write())
        {
            let span = nodes.kind(scoping.get_reference(r).node_id()).span();
            return Some(Err(Reject { reason: "value-reassigned", span }));
        }
    }
    let mut positions = HashMap::new();
    for (index, (symbol, _)) in kept.iter().enumerate() {
        positions.insert(*symbol, index);
    }
    let values = kept.iter().map(|(_, name)| name.to_string()).collect();
    let reads = references
        .ordered
        .into_iter()
        .filter_map(|(r, symbol)| positions.get(&symbol).map(|&index| (r, (function_start, index))))
        .collect();
    Some(Ok((AsyncPlan { first, last, tracked, values }, reads)))
}

fn contains_await(statement: &Statement<'_>) -> bool {
    let mut check = AwaitCheck::default();
    check.visit_statement(statement);
    check.found
}

fn contains_return(statement: &Statement<'_>) -> bool {
    let mut check = ReturnCheck::default();
    check.visit_statement(statement);
    check.found
}

/// The first and last top-level statements that await.
fn await_range(statements: &[Statement<'_>]) -> Option<(usize, usize)> {
    let mut awaiting = statements.iter().enumerate().filter(|(_, s)| contains_await(s));
    let first = awaiting.next()?.0;
    Some((first, awaiting.next_back().map_or(first, |(i, _)| i)))
}

/// The operand of a statement that is exactly `[const|let|var x =] await operand;`.
fn await_operand<'s, 'a>(statement: &'s Statement<'a>) -> Result<&'s Expression<'a>, Reject> {
    let reject = |reason| Reject { reason, span: statement.span() };
    let (value, pattern) = match statement {
        Statement::VariableDeclaration(declaration)
            if !declaration.kind.is_using() && declaration.declarations.len() == 1 =>
        {
            let declarator = &declaration.declarations[0];
            (declarator.init.as_ref(), Some(&declarator.id))
        }
        Statement::ExpressionStatement(expression) => (Some(&expression.expression), None),
        _ => (None, None),
    };
    let Some(Expression::AwaitExpression(awaited)) = value else {
        return Err(reject("await-position"));
    };
    let mut check = AwaitCheck::default();
    if let Some(pattern) = pattern {
        check.visit_binding_pattern(pattern);
    }
    if check.found {
        return Err(reject("await-position"));
    }
    check.visit_expression(&awaited.argument);
    if check.found {
        return Err(reject("nested-await"));
    }
    Ok(&awaited.argument)
}

fn declared_symbols<'s, 'a>(statement: &'s Statement<'a>, out: &mut Vec<(SymbolId, &'s str)>) {
    let mut push = |id: &'s BindingIdentifier<'a>| out.push((id.symbol_id(), id.name.as_str()));
    match statement {
        Statement::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                declarator.id.get_binding_identifiers().into_iter().for_each(&mut push);
            }
        }
        Statement::FunctionDeclaration(function) => function.id.iter().for_each(&mut push),
        Statement::ClassDeclaration(class) => class.id.iter().for_each(&mut push),
        _ => {}
    }
}

impl<'a> Lowerer<'a, '_> {
    /// The holes turning `async` component `function` into
    /// `return asyncComponent(async (c) => { …awaits; return [values]; }, (v) => { …rest reading v()[i] });`,
    /// none when the body never awaits or has a shape that cannot be rewritten (reported).
    pub(super) fn async_component(
        &mut self,
        name: &str,
        function: &AsyncFunction<'_, 'a>,
    ) -> std::vec::Vec<Hole<'a>> {
        let analysis = self.analysis;
        match analysis.asyncs.plan(function.span.start) {
            None => std::vec::Vec::new(),
            Some(Err(reject)) => {
                let Reject { reason, span } = *reject;
                let report = Report::new(Code::AsyncComponentShape, span);
                self.report(report.arg("component", name).arg("reason", reason));
                std::vec::Vec::new()
            }
            Some(Ok(plan)) => self.async_holes(function, plan),
        }
    }

    /// The read replacing `id` at `span` with `values()[index]`; `key: ` first when `shorthand`.
    /// `None` when the component never split (it stays `async`, or never awaited).
    pub(super) fn async_read(
        &mut self,
        id: &IdentifierReference<'a>,
        span: Span,
        shorthand: bool,
    ) -> Option<Hole<'a>> {
        let (start, index) = self.analysis.asyncs.read(id)?;
        let values = *self.async_values.get(&start)?;
        Some(Hole {
            span,
            kind: HoleKind::Script(ScriptEdit::AsyncRead { values, index, shorthand }),
        })
    }

    fn async_holes(
        &mut self,
        function: &AsyncFunction<'_, 'a>,
        plan: &AsyncPlan,
    ) -> std::vec::Vec<Hole<'a>> {
        let statements = &function.body.statements;
        debug_assert!(plan.first <= plan.last && plan.last < statements.len());
        let context = (!plan.tracked.is_empty()).then(|| self.fresh("_c$"));
        let mut holes = std::vec::Vec::new();

        let start = function.span.start as usize;
        if let Some(after) = self.source[start..].strip_prefix("async") {
            let spaces = after.len() - after.trim_start().len();
            let keyword = Span::new(start as u32, (start + "async".len() + spaces) as u32);
            holes.push(insertion(keyword, ""));
        }
        if let Some(annotation) = function.return_type
            && let TSType::TSTypeReference(reference) = &annotation.type_annotation
            && let TSTypeName::IdentifierReference(id) = &reference.type_name
            && id.name.as_str() == "Promise"
            && let Some(arguments) = &reference.type_arguments
            && let [inner] = arguments.params.as_slice()
        {
            holes.push(insertion(reference.span, self.text(inner.span())));
        }
        holes.push(Hole {
            span: Span::empty(statements[0].span().start),
            kind: HoleKind::Script(ScriptEdit::AsyncOpen { context }),
        });
        if let Some(context) = context {
            for span in &plan.tracked {
                let is_object = self.source.as_bytes()[span.start as usize] == b'{';
                let (open, close) = if is_object { ("(", "))") } else { ("", ")") };
                let call = self.str(&format!("{context}.get(() => {open}"));
                holes.push(insertion(Span::empty(span.start), call));
                holes.push(insertion(Span::empty(span.end), close));
            }
        }
        let split = if plan.values.is_empty() {
            "\nreturn [];\n}, () => {".to_string()
        } else {
            let values_name = self.fresh("_v$");
            self.async_values.insert(function.span.start, values_name);
            format!("\nreturn [{}];\n}}, ({values_name}) => {{", plan.values.join(", "))
        };
        holes.push(insertion(Span::empty(statements[plan.last].span().end), self.str(&split)));
        holes.push(insertion(Span::empty(function.body.span.end - 1), "});"));
        holes
    }
}

fn insertion<'a>(span: Span, text: &'a str) -> Hole<'a> {
    Hole { span, kind: HoleKind::Script(ScriptEdit::Insert(text)) }
}
