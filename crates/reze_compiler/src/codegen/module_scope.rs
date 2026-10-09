use std::collections::HashMap;

use oxc_allocator::TakeIn;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk_mut};
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::EmitContext;
use super::html::HTML_SOURCE;
use crate::ast::Ast;

#[derive(Clone, Copy)]
enum Wrapper {
    Function,
    Proxy,
}

struct Binding<'a> {
    name: &'a str,
    symbol: SymbolId,
    wrapper: Wrapper,
    exported: bool,
}

pub fn apply<'a>(ctx: &mut EmitContext<'a, '_>, program: &mut Program<'a>) {
    let old = program.body.take_in(&ctx.allocator);
    let moved: Vec<bool> = old.iter().map(|statement| is_moved(ctx, statement)).collect();
    if !moved.contains(&true) {
        for statement in old {
            program.body.push(statement);
        }
        return;
    }

    let mut bindings = Vec::new();
    let mut init = Vec::new();
    let mut body = Vec::new();
    let mut at = None;
    for (statement, moved) in old.into_iter().zip(moved) {
        if !moved {
            body.push(statement);
            continue;
        }
        at.get_or_insert(body.len());
        match statement {
            Statement::ExportDeclaration(mut export) => {
                let statement = Statement::from(export.declaration.take_in(&ctx.allocator));
                collect_bindings(ctx, &statement, true, &mut bindings);
                init.push(statement);
            }
            statement => {
                collect_bindings(ctx, &statement, false, &mut bindings);
                init.push(statement);
            }
        }
    }

    let state = ctx.fresh("_m$");
    let module_id = ctx.options.module_id.as_deref().unwrap_or_default();
    let ast = Ast::new(ctx.allocator);
    let mut returned = Vec::new();
    for binding in &bindings {
        returned.push(ast.prop(binding.name, ast.ident(binding.name)));
    }
    init.push(ast.return_stmt(ast.object(returned)));
    let initializer = ctx.call(
        HTML_SOURCE,
        "moduleState",
        [ast.string(module_id), ast.block_arrow([], init)],
    );
    let mut lowered = vec![ast.declaration(VariableDeclarationKind::Const, state, Some(initializer))];
    let mut proxy_helper = None;
    for binding in bindings.iter().filter(|binding| binding.exported) {
        let name = binding.name;
        let text = match binding.wrapper {
            Wrapper::Function => {
                format!("export function {name}(...args) {{ return {state}().{name}(...args); }}")
            }
            Wrapper::Proxy => {
                let proxy = *proxy_helper.get_or_insert_with(|| ctx.helper_name(HTML_SOURCE, "moduleProxy"));
                format!("export const {name} = {proxy}(() => {state}().{name});")
            }
        };
        lowered.extend(parse(ctx, &text));
    }

    let symbols: HashMap<SymbolId, &'a str> =
        bindings.iter().map(|binding| (binding.symbol, binding.name)).collect();
    let mut rewrite = Rewrite { ast, scoping: ctx.scoping, symbols: &symbols, state };
    for statement in &mut body {
        rewrite.visit_statement(statement);
    }
    let at = at.unwrap_or(body.len());
    body.splice(at..at, lowered);
    for statement in body {
        program.body.push(statement);
    }
}

fn is_moved(ctx: &EmitContext<'_, '_>, statement: &Statement<'_>) -> bool {
    match statement {
        Statement::VariableDeclaration(declaration) => declaration_moved(ctx, declaration),
        Statement::ExportDeclaration(export) => {
            matches!(&export.declaration, Declaration::VariableDeclaration(declaration) if declaration_moved(ctx, declaration))
        }
        Statement::ExpressionStatement(statement) => {
            let Expression::CallExpression(call) = &statement.expression else { return false };
            matches!(
                ctx.facts.imported(ctx.scoping, &call.callee),
                Some("effect" | "effectScope" | "provideContext")
            ) && !awaits(&statement.expression)
        }
        _ => false,
    }
}

fn declaration_moved(ctx: &EmitContext<'_, '_>, declaration: &VariableDeclaration<'_>) -> bool {
    declaration.declarations.iter().all(|declarator| {
        declarator.init.as_ref().is_some_and(|init| {
            wrapper(ctx, init).is_some() && !awaits(init)
        })
    })
}

fn wrapper(ctx: &EmitContext<'_, '_>, init: &Expression<'_>) -> Option<Wrapper> {
    let Expression::CallExpression(call) = init else { return None };
    match ctx.facts.imported(ctx.scoping, &call.callee)? {
        "signal" | "computed" | "action" | "selector" => Some(Wrapper::Function),
        "store" | "createContext" => Some(Wrapper::Proxy),
        _ => None,
    }
}

fn collect_bindings<'a>(
    ctx: &EmitContext<'a, '_>,
    statement: &Statement<'a>,
    exported: bool,
    bindings: &mut Vec<Binding<'a>>,
) {
    let Statement::VariableDeclaration(declaration) = statement else { return };
    for declarator in &declaration.declarations {
        let Some(wrapper) = declarator.init.as_ref().and_then(|init| wrapper(ctx, init)) else {
            continue;
        };
        for identifier in declarator.id.get_binding_identifiers() {
            bindings.push(Binding {
                name: ctx.intern(identifier.name.as_str()),
                symbol: identifier.symbol_id(),
                wrapper,
                exported,
            });
        }
    }
}

fn awaits(expression: &Expression<'_>) -> bool {
    let mut finder = AwaitFinder(false);
    finder.visit_expression(expression);
    finder.0
}

struct AwaitFinder(bool);

impl<'a> Visit<'a> for AwaitFinder {
    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.0 = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

struct Rewrite<'a, 's> {
    ast: Ast<'a>,
    scoping: &'s oxc_semantic::Scoping,
    symbols: &'s HashMap<SymbolId, &'a str>,
    state: &'a str,
}

impl<'a> Rewrite<'a, '_> {
    fn binding(&self, identifier: &IdentifierReference<'a>) -> Option<&'a str> {
        let reference = identifier.reference_id.get()?;
        let symbol = self.scoping.get_reference(reference).symbol_id()?;
        self.symbols.get(&symbol).copied()
    }
}

impl<'a> VisitMut<'a> for Rewrite<'a, '_> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::Identifier(identifier) = expression
            && let Some(name) = self.binding(identifier)
        {
            let state = self.ast.call(self.ast.ident(self.state), []);
            *expression = self.ast.member(state, name);
            return;
        }
        walk_mut::walk_expression(self, expression);
    }

    fn visit_object_property(&mut self, property: &mut ObjectProperty<'a>) {
        walk_mut::walk_object_property(self, property);
        if property.shorthand && !matches!(property.value, Expression::Identifier(_)) {
            property.shorthand = false;
        }
    }
}

pub(super) fn parse<'a>(ctx: &EmitContext<'a, '_>, text: &str) -> Vec<Statement<'a>> {
    let source = ctx.intern(text);
    Parser::new(ctx.allocator, source, SourceType::mjs()).parse().program.body.into_iter().collect()
}
