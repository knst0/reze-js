use oxc_allocator::{ArenaVec, TakeIn};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk_mut};
use oxc_span::{SPAN, Span};
use oxc_syntax::scope::ScopeFlags;

use super::EmitContext;
use crate::CompileTarget;
use crate::ast::Ast;

pub fn prepare<'a>(ctx: &mut EmitContext<'a, '_>, call: &mut CallExpression<'a>) {
    let Some(Argument::ArrowFunctionExpression(loader)) = call.arguments.first_mut() else {
        return;
    };
    if !loader.r#async || loader.span != SPAN {
        return;
    }
    let ArrowFunctionBody::FunctionBody(body) = &mut loader.body else {
        unreachable!("compiler async loader has a statement body")
    };
    prepare_body(ctx, body, call.span, None);
}

pub fn prepare_action<'a>(ctx: &mut EmitContext<'a, '_>, call: &mut CallExpression<'a>) {
    let origin = call.span;
    let (parameters, body) = match call.arguments.first_mut() {
        Some(Argument::ArrowFunctionExpression(function)) if function.r#async => {
            let function = function.as_mut();
            let ArrowFunctionBody::FunctionBody(body) = &mut function.body else { return };
            (&function.params, body.as_mut())
        }
        Some(Argument::FunctionExpression(function)) if function.r#async => {
            let function = function.as_mut();
            let Some(body) = function.body.as_mut() else { return };
            (&function.params, body.as_mut())
        }
        _ => return,
    };
    let Some(parameter) = parameters.items.first().filter(|parameter| parameter.span == SPAN)
    else {
        return;
    };
    let BindingPattern::BindingIdentifier(binding) = &parameter.pattern else { return };
    let action_run = ctx.intern(binding.name.as_str());
    prepare_body(ctx, body, origin, Some(action_run));
}

fn prepare_body<'a>(
    ctx: &mut EmitContext<'a, '_>,
    body: &mut FunctionBody<'a>,
    origin: Span,
    action_run: Option<&'a str>,
) {
    let mut check = AwaitCheck(false);
    check.visit_function_body(body);
    if !check.0 {
        return;
    }
    let ast = Ast::new(ctx.allocator);
    let run = ctx.fresh("_continuation$");
    let error = ctx.fresh("_error$");
    let start = if ctx.options.target == CompileTarget::Client {
        ctx.call("reze-js/internal/reactivity", "beginContinuation", [])
    } else {
        let site = ctx.origin_site(origin);
        ctx.call("reze-js/internal/reactivity", "beginContinuation", [site])
    };
    let mut awaits = Awaits { ctx, run, action_run, await_count: 0, replay_sites: Vec::new() };
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
            ArenaVec::from_array_in(
                [Statement::new_throw_statement(SPAN, reject(ctx, run, error), &ast.builder)],
                &ast.builder,
            ),
            &ast.builder,
        ),
        &ast.builder,
    );
    let finalizer = BlockStatement::boxed(
        SPAN,
        ArenaVec::from_array_in(
            [ast.stmt(ast.call(ast.member(ast.ident(run), "end"), []))],
            &ast.builder,
        ),
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

struct AwaitCheck(bool);

impl<'a> Visit<'a> for AwaitCheck {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.0 = true;
    }
}

struct Awaits<'c, 'a, 'm> {
    ctx: &'c mut EmitContext<'a, 'm>,
    run: &'a str,
    action_run: Option<&'a str>,
    await_count: usize,
    replay_sites: Vec<Span>,
}

impl<'a> VisitMut<'a> for Awaits<'_, 'a, '_> {
    fn visit_function(&mut self, _: &mut Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &mut ArrowFunctionExpression<'a>) {}

    fn visit_catch_clause(&mut self, clause: &mut CatchClause<'a>) {
        walk_mut::walk_catch_clause(self, clause);
        let ast = Ast::new(self.ctx.allocator);
        let error;
        let binds_error = clause.param.is_some();
        let pattern = if let Some(parameter) = &mut clause.param {
            match &parameter.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    error = self.ctx.intern(binding.name.as_str());
                    None
                }
                _ => {
                    error = self.ctx.fresh("_caught$");
                    Some(std::mem::replace(
                        &mut parameter.pattern,
                        BindingPattern::new_binding_identifier(SPAN, error, &ast.builder),
                    ))
                }
            }
        } else {
            error = self.ctx.fresh("_caught$");
            clause.param = Some(CatchParameter::new(
                SPAN,
                BindingPattern::new_binding_identifier(SPAN, error, &ast.builder),
                None,
                &ast.builder,
            ));
            None
        };
        let rejected = reject(self.ctx, self.run, error);
        let statement = match pattern {
            Some(pattern) => {
                let body = clause.body.body.take_in(&self.ctx.allocator);
                clause.body.body.push(Statement::new_block_statement(SPAN, body, &ast.builder));
                let declaration = VariableDeclarator::new(
                    SPAN,
                    pattern,
                    None,
                    Some(rejected),
                    false,
                    &ast.builder,
                );
                Statement::new_variable_declaration(
                    SPAN,
                    VariableDeclarationKind::Let,
                    ArenaVec::from_array_in([declaration], &ast.builder),
                    false,
                    &ast.builder,
                )
            }
            None if binds_error && self.ctx.options.target == CompileTarget::Hydrate => {
                ast.stmt(ast.assign(ast.ident(error), rejected))
            }
            None => ast.stmt(rejected),
        };
        clause.body.body.insert(0, statement);
    }

    fn visit_try_statement(&mut self, statement: &mut TryStatement<'a>) {
        let before = self.await_count;
        self.visit_block_statement(&mut statement.block);
        let suspended = self.await_count != before;
        if let Some(handler) = &mut statement.handler {
            self.visit_catch_clause(handler);
        }
        if let Some(finalizer) = &mut statement.finalizer {
            self.visit_block_statement(finalizer);
            if suspended && statement.handler.is_none() {
                let ast = Ast::new(self.ctx.allocator);
                let error = self.ctx.fresh("_caught$");
                statement.handler = Some(CatchClause::boxed(
                    SPAN,
                    Some(CatchParameter::new(
                        SPAN,
                        BindingPattern::new_binding_identifier(SPAN, error, &ast.builder),
                        None,
                        &ast.builder,
                    )),
                    BlockStatement::boxed(
                        SPAN,
                        ArenaVec::from_array_in(
                            [Statement::new_throw_statement(
                                SPAN,
                                reject(self.ctx, self.run, error),
                                &ast.builder,
                            )],
                            &ast.builder,
                        ),
                        &ast.builder,
                    ),
                    &ast.builder,
                ));
            }
        }
    }

    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        let nested_start = self.replay_sites.len();
        walk_mut::walk_expression(self, expression);
        let Expression::AwaitExpression(awaited) = expression else { return };
        self.await_count += 1;
        let ast = Ast::new(self.ctx.allocator);
        let mut value = awaited.argument.take_in(&self.ctx.allocator);
        let mut action_suspend = None;
        if let Some(action_run) = self.action_run
            && let Expression::CallExpression(call) = &value
            && call.arguments.len() == 1
            && let Expression::StaticMemberExpression(member) = &call.callee
            && member.property.name == "suspend"
            && matches!(&member.object, Expression::Identifier(identifier) if identifier.name == action_run)
        {
            let Expression::CallExpression(mut call) = value else { unreachable!() };
            value = call.arguments.remove(0).into_expression();
            action_suspend = Some(call);
        }
        if self.ctx.options.target == CompileTarget::Hydrate {
            let mut replayed = ArenaVec::new_in(&ast.builder);
            for index in nested_start..self.replay_sites.len() {
                let span = self.replay_sites[index];
                let site = self.ctx.origin_site(span);
                let expected = self.ctx.call(
                    "reze-js/internal/hydrate",
                    "willReplayAwait",
                    [ast.ident(self.run), site],
                );
                let site = self.ctx.origin_site(span);
                let operand = self.ctx.call(
                    "reze-js/internal/hydrate",
                    "replayAwaitOperand",
                    [ast.ident(self.run), site],
                );
                let site = self.ctx.origin_site(span);
                let mut suspended =
                    ast.call(ast.member(ast.ident(self.run), "suspend"), [operand, site]);
                if let Some(action_run) = self.action_run {
                    suspended = ast.call(ast.member(ast.ident(action_run), "suspend"), [suspended]);
                }
                let awaited = Expression::new_await_expression(span, suspended, &ast.builder);
                let mut resumed = ast.call(ast.member(ast.ident(self.run), "resume"), [awaited]);
                if let Some(action_run) = self.action_run {
                    resumed = ast.call(ast.member(ast.ident(action_run), "resume"), [resumed]);
                }
                replayed.push(ast.conditional(expected, resumed, ast.undefined()));
            }
            let site = self.ctx.origin_site(awaited.span);
            let operand = self.ctx.call(
                "reze-js/internal/hydrate",
                "replayAwaitOperand",
                [ast.ident(self.run), site],
            );
            let replayed = if replayed.is_empty() {
                operand
            } else {
                replayed.push(operand);
                Expression::new_sequence_expression(SPAN, replayed, &ast.builder)
            };
            self.replay_sites.push(awaited.span);
            value = ast.conditional(ast.member(ast.ident(self.run), "replaying"), replayed, value);
        } else if self.ctx.options.target == CompileTarget::Html {
            let site = self.ctx.origin_site(awaited.span);
            let begin = self.ctx.call(
                "reze-js/internal/html",
                "beginAwaitOperand",
                [ast.ident(self.run), site],
            );
            value = Expression::new_sequence_expression(
                SPAN,
                ArenaVec::from_array_in([begin, value], &ast.builder),
                &ast.builder,
            );
        }
        let suspend = ast.member(ast.ident(self.run), "suspend");
        let suspended = if self.ctx.options.target == CompileTarget::Client {
            ast.call(suspend, [value])
        } else {
            let site = self.ctx.origin_site(awaited.span);
            ast.call(suspend, [value, site])
        };
        awaited.argument = if let Some(mut call) = action_suspend {
            call.arguments.push(Argument::from(suspended));
            Expression::CallExpression(call)
        } else {
            suspended
        };
        let original_await = expression.take_in(&self.ctx.allocator);
        *expression = ast.call(ast.member(ast.ident(self.run), "resume"), [original_await]);
    }
}

fn reject<'a>(ctx: &mut EmitContext<'a, '_>, run: &'a str, error: &'a str) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    if ctx.options.target == CompileTarget::Html {
        ctx.call("reze-js/internal/html", "rejectAwaitOperand", [ast.ident(run), ast.ident(error)])
    } else {
        ast.call(ast.member(ast.ident(run), "reject"), [ast.ident(error)])
    }
}
