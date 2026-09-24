//! Async components (SPEC §7.9): the plan that decides whether an `async` function that renders
//! JSX can be rewritten into its synchronous shape.

use std::collections::HashSet;

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::{Lowerer, has_jsx};
use crate::diagnostic::{Code, Report};
use crate::ir::{AsyncComponent, AsyncHead, AsyncStep, ReturnType};

/// Why an async component keeps its `async` form; `data.reason` of ASYNC_COMPONENT_SHAPE.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reject {
    NoTopLevelAwait,
    Directives,
    Using,
    NestedAwait,
    EarlyExit,
    NoFinalReturn,
    LocalCrossesAwait,
}

impl Reject {
    fn describe(self) -> &'static str {
        match self {
            Reject::NoTopLevelAwait => "no top-level `await`",
            Reject::Directives => "function directives",
            Reject::Using => "a `using` declaration awaits",
            Reject::NestedAwait => "an `await` outside a top-level `const x = await …`",
            Reject::EarlyExit => "a `return`/`throw` before the last `await`",
            Reject::NoFinalReturn => "the body does not end with `return …;`",
            Reject::LocalCrossesAwait => "a local declared before an `await` is used after it",
        }
    }
}

struct Awaited<'b, 'a> {
    statement: usize,
    kind: &'static str,
    pattern: Span,
    annotation: Option<Span>,
    argument: &'b Expression<'a>,
}

struct Plan<'b, 'a> {
    awaits: std::vec::Vec<Awaited<'b, 'a>>,
    /// Index of the final `return` statement.
    return_index: usize,
    result: &'b Expression<'a>,
}

impl<'a> Lowerer<'a, '_> {
    pub(super) fn async_function(&mut self, func: &Function<'a>) -> Option<AsyncComponent<'a>> {
        if !func.r#async || func.generator {
            return None;
        }
        let body = func.body.as_ref()?;
        let plan = self.plan(body, func.span)?;
        let head = AsyncHead::Function {
            id: func.id.as_ref().map(|id| id.span),
            type_parameters: func.type_parameters.as_ref().map(|t| t.span),
        };
        Some(self.async_component(head, &func.params, func.return_type.as_deref(), body, plan))
    }

    pub(super) fn async_arrow(
        &mut self,
        arrow: &ArrowFunctionExpression<'a>,
    ) -> Option<AsyncComponent<'a>> {
        let ArrowFunctionBody::FunctionBody(body) = &arrow.body else { return None };
        if !arrow.r#async {
            return None;
        }
        let plan = self.plan(body, arrow.span)?;
        let head =
            AsyncHead::Arrow { type_parameters: arrow.type_parameters.as_ref().map(|t| t.span) };
        Some(self.async_component(head, &arrow.params, arrow.return_type.as_deref(), body, plan))
    }

    fn plan<'b>(&mut self, body: &'b FunctionBody<'a>, span: Span) -> Option<Plan<'b, 'a>> {
        if !has_jsx(|check| check.visit_function_body(body)) {
            return None;
        }
        match plan(body, self.scoping) {
            Ok(plan) => Some(plan),
            Err(Reject::NoTopLevelAwait) => None,
            Err(reason) => {
                self.report(
                    Report::new(
                        Code::AsyncComponentShape,
                        Span::new(span.start, span.start + 5),
                        format!(
                            "This async component was left as written because of {}; it returns a \
                             Promise, which renders nothing. Use top-level `const x = await …;` \
                             declarations and a final `return …;`.",
                            reason.describe()
                        ),
                    )
                    .data("reason", reason.describe()),
                );
                None
            }
        }
    }

    fn async_component(
        &mut self,
        head: AsyncHead,
        params: &FormalParameters<'a>,
        return_type: Option<&TSTypeAnnotation<'a>>,
        body: &FunctionBody<'a>,
        plan: Plan<'_, 'a>,
    ) -> AsyncComponent<'a> {
        let params = self.params(params);
        let return_type = self.return_type(return_type);
        let mut steps = self.vec();
        let mut segment_start = 0;
        for awaited in &plan.awaits {
            let mut before = self.vec();
            for statement in &body.statements[segment_start..awaited.statement] {
                before.push(self.stmt(statement));
            }
            steps.push(AsyncStep {
                before,
                kind: awaited.kind,
                pattern: awaited.pattern,
                annotation: awaited.annotation,
                argument: self.expr(awaited.argument),
            });
            segment_start = awaited.statement + 1;
        }
        let mut tail = self.vec();
        for statement in &body.statements[segment_start..plan.return_index] {
            tail.push(self.stmt(statement));
        }
        let result = self.expr(plan.result);
        AsyncComponent { head, params, return_type, steps, tail, result }
    }

    /// `Promise<X>` unwraps to `X`: the rewritten component is synchronous.
    fn return_type(&mut self, annotation: Option<&TSTypeAnnotation<'a>>) -> ReturnType {
        let Some(annotation) = annotation else { return ReturnType::None };
        let TSType::TSTypeReference(reference) = &annotation.type_annotation else {
            return ReturnType::Verbatim(annotation.span);
        };
        let is_promise = matches!(
            &reference.type_name,
            TSTypeName::IdentifierReference(id) if id.name.as_str() == "Promise"
        );
        if !is_promise {
            return ReturnType::Verbatim(annotation.span);
        }
        if let Some(arguments) = &reference.type_arguments
            && let [inner] = arguments.params.as_slice()
        {
            return ReturnType::Unwrapped(inner.span());
        }
        self.report(Report::new(
            Code::AsyncReturnType,
            annotation.span,
            "The compiled component is synchronous but this `Promise` annotation cannot be \
             unwrapped, so it was kept as written. Annotate `Promise<JSX.Element>` or remove it.",
        ));
        ReturnType::Verbatim(annotation.span)
    }
}

fn plan<'b, 'a>(body: &'b FunctionBody<'a>, scoping: &Scoping) -> Result<Plan<'b, 'a>, Reject> {
    let mut awaits = std::vec::Vec::new();
    for (index, statement) in body.statements.iter().enumerate() {
        if let Statement::VariableDeclaration(declaration) = statement
            && let [declarator] = declaration.declarations.as_slice()
            && let Some(Expression::AwaitExpression(awaited)) =
                declarator.init.as_ref().map(Expression::without_parentheses)
        {
            let kind = match declaration.kind {
                VariableDeclarationKind::Const => "const",
                VariableDeclarationKind::Let => "let",
                VariableDeclarationKind::Var => "var",
                VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                    return Err(Reject::Using);
                }
            };
            awaits.push(Awaited {
                statement: index,
                kind,
                pattern: declarator.id.span(),
                annotation: declarator.type_annotation.as_ref().map(|a| a.span),
                argument: &awaited.argument,
            });
            continue;
        }
        if contains(statement, Find::Await) {
            return Err(Reject::NestedAwait);
        }
    }
    let Some(last) = awaits.last().map(|a| a.statement) else {
        return Err(Reject::NoTopLevelAwait);
    };
    if !body.directives.is_empty() {
        return Err(Reject::Directives);
    }
    let is_await = |index: usize| awaits.iter().any(|a| a.statement == index);
    if body.statements[..last]
        .iter()
        .enumerate()
        .any(|(index, statement)| !is_await(index) && contains(statement, Find::Exit))
    {
        return Err(Reject::EarlyExit);
    }
    let return_index = body.statements[last + 1..]
        .iter()
        .rposition(|statement| !matches!(statement, Statement::EmptyStatement(_)))
        .map(|offset| last + 1 + offset)
        .ok_or(Reject::NoFinalReturn)?;
    let Statement::ReturnStatement(ret) = &body.statements[return_index] else {
        return Err(Reject::NoFinalReturn);
    };
    let result = ret.argument.as_ref().ok_or(Reject::NoFinalReturn)?;

    let mut awaited_bindings = Bindings::default();
    for awaited in &awaits {
        let Statement::VariableDeclaration(declaration) = &body.statements[awaited.statement]
        else {
            continue;
        };
        awaited_bindings.visit_binding_pattern(&declaration.declarations[0].id);
    }
    let mut segment_start = 0;
    for awaited in &awaits {
        let mut declared = Bindings::default();
        for statement in &body.statements[segment_start..awaited.statement] {
            declared.visit_statement(statement);
        }
        let mut used = Uses { scoping, symbols: HashSet::new() };
        for statement in &body.statements[awaited.statement + 1..] {
            used.visit_statement(statement);
        }
        if declared.symbols.iter().any(|symbol| {
            used.symbols.contains(symbol) && !awaited_bindings.symbols.contains(symbol)
        }) {
            return Err(Reject::LocalCrossesAwait);
        }
        segment_start = awaited.statement + 1;
    }
    Ok(Plan { awaits, return_index, result })
}

#[derive(Clone, Copy)]
enum Find {
    Await,
    Exit,
}

/// Whether `statement` holds an `await` (or `return`/`throw`) outside nested functions.
fn contains(statement: &Statement<'_>, find: Find) -> bool {
    struct Check {
        find: Find,
        found: bool,
    }
    impl<'a> Visit<'a> for Check {
        fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
            self.found |= matches!(self.find, Find::Await);
        }
        fn visit_return_statement(&mut self, _: &ReturnStatement<'a>) {
            self.found |= matches!(self.find, Find::Exit);
        }
        fn visit_throw_statement(&mut self, _: &ThrowStatement<'a>) {
            self.found |= matches!(self.find, Find::Exit);
        }
        fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    }
    let mut check = Check { find, found: false };
    check.visit_statement(statement);
    check.found
}

/// Symbols a statement list declares outside nested functions.
#[derive(Default)]
struct Bindings {
    symbols: HashSet<SymbolId>,
}

impl<'a> Visit<'a> for Bindings {
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        if let Some(symbol) = it.symbol_id.get() {
            self.symbols.insert(symbol);
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, _: ScopeFlags) {
        if let Some(id) = &it.id {
            self.visit_binding_identifier(id);
        }
    }

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, it: &Class<'a>) {
        if let Some(id) = &it.id {
            self.visit_binding_identifier(id);
        }
    }
}

/// Symbols referenced anywhere in a statement list, closures included.
struct Uses<'s> {
    scoping: &'s Scoping,
    symbols: HashSet<SymbolId>,
}

impl<'a> Visit<'a> for Uses<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(reference) = it.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
        {
            self.symbols.insert(symbol);
        }
    }
}
