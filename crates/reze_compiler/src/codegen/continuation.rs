use oxc_allocator::{ArenaVec, TakeIn};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::SPAN;
use oxc_syntax::scope::ScopeFlags;

use super::EmitContext;
use crate::CompileTarget;
use crate::ast::Ast;

pub fn prepare<'a>(ctx: &mut EmitContext<'a, '_>, call: &mut CallExpression<'a>) {
    let Some(Argument::ArrowFunctionExpression(loader)) = call.arguments.first_mut() else { return };
    if !loader.r#async || loader.span != SPAN {
        return;
    }
    let ArrowFunctionBody::FunctionBody(body) = &mut loader.body else {
        unreachable!("compiler async loader has a statement body")
    };
    let ast = Ast::new(ctx.allocator);
    let run = ctx.fresh("_continuation$");
    let error = ctx.fresh("_error$");
    let start = if ctx.options.target == CompileTarget::Client {
        ctx.call("reze-js/internal/reactivity", "beginContinuation", [])
    } else {
        let site = ctx.origin_site(call.span);
        ctx.call("reze-js/internal/reactivity", "beginContinuation", [site])
    };
    let mut awaits = Awaits { ctx, run };
    for statement in &mut body.statements {
        awaits.visit_statement(statement);
    }
    let statements = body.statements.take_in(&ctx.allocator);
    let handler = CatchClause::boxed(
        SPAN,
        Some(CatchParameter::new(
            SPAN,
            BindingPattern::new_binding_identifier(SPAN, error, &ast.builder),
            None,
            &ast.builder,
        )),
        BlockStatement::boxed(
            SPAN,
            ArenaVec::from_array_in([
                ast.stmt(ast.call(ast.member(ast.ident(run), "reject"), [ast.ident(error)])),
                Statement::new_throw_statement(SPAN, ast.ident(error), &ast.builder),
            ], &ast.builder),
            &ast.builder,
        ),
        &ast.builder,
    );
    let finalizer = BlockStatement::boxed(
        SPAN,
        ArenaVec::from_array_in([
            ast.stmt(ast.call(ast.member(ast.ident(run), "end"), [])),
        ], &ast.builder),
        &ast.builder,
    );
    body.statements.push(ast.declaration(VariableDeclarationKind::Const, run, Some(start)));
    body.statements.push(Statement::new_try_statement(
        SPAN,
        BlockStatement::boxed(SPAN, statements, &ast.builder),
        Some(handler),
        Some(finalizer),
        &ast.builder,
    ));
    ctx.changed = true;
}

struct Awaits<'c, 'a, 'm> {
    ctx: &'c mut EmitContext<'a, 'm>,
    run: &'a str,
}

impl<'a> VisitMut<'a> for Awaits<'_, 'a, '_> {
    fn visit_function(&mut self, _: &mut Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &mut ArrowFunctionExpression<'a>) {}

    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        walk_mut::walk_expression(self, expression);
        let Expression::AwaitExpression(awaited) = expression else { return };
        let ast = Ast::new(self.ctx.allocator);
        let mut value = awaited.argument.take_in(&self.ctx.allocator);
        if self.ctx.options.target == CompileTarget::Hydrate {
            value = ast.conditional(
                ast.member(ast.ident(self.run), "replaying"),
                ast.undefined(),
                value,
            );
        }
        let suspend = ast.member(ast.ident(self.run), "suspend");
        awaited.argument = if self.ctx.options.target == CompileTarget::Client {
            ast.call(suspend, [value])
        } else {
            let site = self.ctx.origin_site(awaited.span);
            ast.call(suspend, [value, site])
        };
        let original_await = expression.take_in(&self.ctx.allocator);
        *expression = ast.call(ast.member(ast.ident(self.run), "resume"), [original_await]);
    }
}
