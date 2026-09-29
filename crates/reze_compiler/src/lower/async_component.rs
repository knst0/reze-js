use std::collections::HashSet;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::{Lowerer, has_jsx};
use crate::diagnostic::{Code, Report};
use crate::ir::{Hole, HoleKind, ScriptEdit};

pub struct AsyncFunction<'b, 'a> {
    pub span: Span,
    pub body: &'b FunctionBody<'a>,
    pub return_type: Option<&'b TSTypeAnnotation<'a>>,
}

struct Reject {
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
}

impl<'a> Visit<'a> for References<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(reference) = it.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
        {
            self.symbols.insert(symbol);
        }
    }
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
    /// `return asyncComponent(async (c) => { …awaits; return [values]; }, ([values]) => { …rest });`,
    /// none when the body never awaits or has a shape that cannot be rewritten (reported).
    pub(super) fn async_component(
        &mut self,
        name: &str,
        function: &AsyncFunction<'_, 'a>,
    ) -> std::vec::Vec<Hole<'a>> {
        let Some((first, last)) = await_range(&function.body.statements) else {
            return std::vec::Vec::new();
        };
        match self.async_holes(function, first, last) {
            Ok(holes) => holes,
            Err(Reject { reason, span }) => {
                let report = Report::new(Code::AsyncComponentShape, span);
                self.report(report.arg("component", name).arg("reason", reason));
                std::vec::Vec::new()
            }
        }
    }

    fn async_holes(
        &mut self,
        function: &AsyncFunction<'_, 'a>,
        first: usize,
        last: usize,
    ) -> Result<std::vec::Vec<Hole<'a>>, Reject> {
        let statements = &function.body.statements;
        let mut tracked = std::vec::Vec::new();
        for (i, statement) in statements[..=last].iter().enumerate() {
            let reject = |reason| Reject { reason, span: statement.span() };
            if has_jsx(|check| check.visit_statement(statement)) {
                return Err(reject("jsx-before-await"));
            }
            if contains_return(statement) {
                return Err(reject("return-before-await"));
            }
            if i < first {
                continue;
            }
            if contains_await(statement) {
                let operand = await_operand(statement)?;
                if i > first {
                    tracked.push(operand.span());
                }
            } else if let Statement::VariableDeclaration(declaration) = statement {
                tracked.extend(
                    declaration
                        .declarations
                        .iter()
                        .filter_map(|d| d.init.as_ref())
                        .map(GetSpan::span),
                );
            } else {
                return Err(reject("statement-between-awaits"));
            }
        }

        let mut declared = std::vec::Vec::new();
        statements[..=last].iter().for_each(|statement| declared_symbols(statement, &mut declared));
        let mut references = References { scoping: self.analysis.scoping, symbols: HashSet::new() };
        statements[last + 1..].iter().for_each(|statement| references.visit_statement(statement));
        let mut seen = HashSet::new();
        let values: std::vec::Vec<&str> = declared
            .into_iter()
            .filter(|(symbol, _)| references.symbols.contains(symbol) && seen.insert(*symbol))
            .map(|(_, name)| name)
            .collect();
        let values = values.join(", ");

        let context = (!tracked.is_empty()).then(|| self.fresh("_c$"));
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
            for span in tracked {
                let is_object = self.source.as_bytes()[span.start as usize] == b'{';
                let (open, close) = if is_object { ("(", "))") } else { ("", ")") };
                let call = self.str(&format!("{context}.get(() => {open}"));
                holes.push(insertion(Span::empty(span.start), call));
                holes.push(insertion(Span::empty(span.end), close));
            }
        }
        let split = if values.is_empty() {
            "\nreturn [];\n}, () => {".to_string()
        } else {
            format!("\nreturn [{values}];\n}}, ([{values}]) => {{")
        };
        holes.push(insertion(Span::empty(statements[last].span().end), self.str(&split)));
        holes.push(insertion(Span::empty(function.body.span.end - 1), "});"));
        Ok(holes)
    }
}

fn insertion<'a>(span: Span, text: &'a str) -> Hole<'a> {
    Hole { span, kind: HoleKind::Script(ScriptEdit::Insert(text)) }
}
