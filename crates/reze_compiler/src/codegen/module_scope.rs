use oxc_allocator::{ArenaVec, CloneIn, TakeIn};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk};
use oxc_span::{GetSpan, GetSpanMut, SPAN};
use oxc_syntax::scope::ScopeFlags;

use super::EmitContext;
use crate::ast::Ast;
use crate::frontend::analysis::RuntimeCallKind;

pub fn apply<'a>(ctx: &mut EmitContext<'a, '_>, program: &mut Program<'a>) {
    let old = program.body.take_in(&ctx.allocator);
    for statement in old {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                variables(ctx, declaration, &mut program.body, None);
            }
            Statement::ClassDeclaration(class) if class_needs_scope(&class) => {
                let declaration = class_variable(ctx, class);
                variables(ctx, declaration, &mut program.body, None);
            }
            Statement::ExportDeclaration(mut export) => {
                match export.declaration.take_in(&ctx.allocator) {
                    Declaration::VariableDeclaration(declaration) => {
                        variables(ctx, declaration, &mut program.body, Some(export));
                    }
                    Declaration::ClassDeclaration(class) if class_needs_scope(&class) => {
                        let declaration = class_variable(ctx, class);
                        variables(ctx, declaration, &mut program.body, Some(export));
                    }
                    declaration => {
                        export.declaration = declaration;
                        program.body.push(Statement::ExportDeclaration(export));
                    }
                }
            }
            Statement::ExpressionStatement(mut statement) => {
                let clear =
                    scope_expression(ctx, &mut statement.expression, None, &mut program.body);
                program.body.push(Statement::ExpressionStatement(statement));
                if let Some(temporary) = clear {
                    let ast = Ast::new(ctx.allocator);
                    program.body.push(ast.stmt(ast.assign(ast.ident(temporary), ast.undefined())));
                }
            }
            Statement::ExportDefaultDeclaration(mut export) => {
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration
                    && class_needs_scope(class)
                {
                    let ExportDefaultDeclarationKind::ClassDeclaration(mut class) =
                        export.declaration.take_in(&ctx.allocator)
                    else {
                        unreachable!()
                    };
                    if let Some(binding) = &class.id {
                        let name = ctx.intern(binding.name.as_str());
                        let declaration = class_variable(ctx, class);
                        variables(ctx, declaration, &mut program.body, None);
                        let ast = Ast::new(ctx.allocator);
                        program.body.push(Statement::new_export_named_declaration(
                            SPAN,
                            ArenaVec::from_array_in(
                                [ExportSpecifier::new(
                                    SPAN,
                                    ModuleExportName::new_identifier_reference(
                                        SPAN,
                                        name,
                                        &ast.builder,
                                    ),
                                    ModuleExportName::new_identifier_name(
                                        SPAN,
                                        "default",
                                        &ast.builder,
                                    ),
                                    ImportOrExportKind::Value,
                                    &ast.builder,
                                )],
                                &ast.builder,
                            ),
                            ImportOrExportKind::Value,
                            &ast.builder,
                        ));
                        continue;
                    }
                    class.r#type = ClassType::ClassExpression;
                    export.declaration = ExportDefaultDeclarationKind::ClassExpression(class);
                }
                let clear = export.declaration.as_expression_mut().and_then(|value| {
                    scope_expression(ctx, value, Some("default"), &mut program.body)
                });
                program.body.push(Statement::ExportDefaultDeclaration(export));
                if let Some(temporary) = clear {
                    let ast = Ast::new(ctx.allocator);
                    program.body.push(ast.stmt(ast.assign(ast.ident(temporary), ast.undefined())));
                }
            }
            statement @ (Statement::BlockStatement(_)
            | Statement::IfStatement(_)
            | Statement::SwitchStatement(_)
            | Statement::WhileStatement(_)
            | Statement::DoWhileStatement(_)
            | Statement::ForStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::LabeledStatement(_)
            | Statement::TryStatement(_)
            | Statement::ThrowStatement(_)) => program.body.push(scope_statement(ctx, statement)),
            statement => program.body.push(statement),
        }
    }
}

fn class_needs_scope(class: &Class<'_>) -> bool {
    let mut check = Initializer::default();
    check.visit_class(class);
    check.call
}

fn class_variable<'a>(
    ctx: &mut EmitContext<'a, '_>,
    mut class: oxc_allocator::ArenaBox<'a, Class<'a>>,
) -> oxc_allocator::ArenaBox<'a, VariableDeclaration<'a>> {
    let ast = Ast::new(ctx.allocator);
    let name = ctx.intern(class.id.as_ref().expect("named class declaration").name.as_str());
    class.r#type = ClassType::ClassExpression;
    VariableDeclaration::boxed(
        SPAN,
        VariableDeclarationKind::Let,
        ArenaVec::from_array_in(
            [VariableDeclarator::new(
                SPAN,
                BindingPattern::new_binding_identifier(SPAN, name, &ast.builder),
                None,
                Some(Expression::ClassExpression(class)),
                false,
                &ast.builder,
            )],
            &ast.builder,
        ),
        false,
        &ast.builder,
    )
}

fn scope_statement<'a>(
    ctx: &mut EmitContext<'a, '_>,
    mut statement: Statement<'a>,
) -> Statement<'a> {
    let mut check = Initializer::default();
    check.visit_statement(&statement);
    if !check.call {
        return statement;
    }
    let ast = Ast::new(ctx.allocator);
    let frame = ctx.fresh("_moduleScope$");
    if check.awaited {
        ModuleAwaits { ctx, frame, await_count: 0 }.visit_statement(&mut statement);
    }
    let module_id =
        ast.string(ctx.options.module_id.as_deref().expect("selected target has a module ID"));
    let begin = ctx.call("reze-js/internal/reactivity", "beginModuleScope", [module_id]);
    ctx.changed = true;
    Statement::new_block_statement(
        SPAN,
        ArenaVec::from_array_in(
            [
                ast.declaration(VariableDeclarationKind::Const, frame, Some(begin)),
                Statement::new_try_statement(
                    SPAN,
                    BlockStatement::boxed(
                        SPAN,
                        ArenaVec::from_array_in([statement], &ast.builder),
                        &ast.builder,
                    ),
                    None,
                    Some(BlockStatement::boxed(
                        SPAN,
                        ArenaVec::from_array_in(
                            [ast.stmt(ast.call(ast.member(ast.ident(frame), "end"), []))],
                            &ast.builder,
                        ),
                        &ast.builder,
                    )),
                    &ast.builder,
                ),
            ],
            &ast.builder,
        ),
        &ast.builder,
    )
}

fn variables<'a>(
    ctx: &mut EmitContext<'a, '_>,
    mut declaration: oxc_allocator::ArenaBox<'a, VariableDeclaration<'a>>,
    output: &mut ArenaVec<'a, Statement<'a>>,
    export: Option<oxc_allocator::ArenaBox<'a, ExportDeclaration<'a>>>,
) {
    let ast = Ast::new(ctx.allocator);
    let declarations = declaration.declarations.take_in(&ctx.allocator);
    for mut variable in declarations {
        let name = match &variable.id {
            BindingPattern::BindingIdentifier(binding) => Some(ctx.intern(binding.name.as_str())),
            _ => None,
        };
        let clear =
            variable.init.as_mut().and_then(|value| scope_expression(ctx, value, name, output));
        let mut next = declaration.clone_in(ctx.allocator);
        next.declarations.push(variable);
        if let Some(export) = &export {
            let mut export = export.clone_in(ctx.allocator);
            export.declaration = Declaration::VariableDeclaration(next);
            output.push(Statement::ExportDeclaration(export));
        } else {
            output.push(Statement::VariableDeclaration(next));
        }
        if let Some(temporary) = clear {
            output.push(ast.stmt(ast.assign(ast.ident(temporary), ast.undefined())));
        }
    }
}

fn scope_expression<'a>(
    ctx: &mut EmitContext<'a, '_>,
    expression: &mut Expression<'a>,
    inferred_name: Option<&str>,
    output: &mut ArenaVec<'a, Statement<'a>>,
) -> Option<&'a str> {
    let mut check = Initializer::default();
    check.visit_expression(expression);
    if !check.call {
        return None;
    }
    if let Expression::CallExpression(call) = expression.without_parentheses()
        && matches!(
            ctx.facts.runtime_calls.get(&call.node_id.get()),
            Some(RuntimeCallKind::Resource | RuntimeCallKind::UniqueId)
        )
    {
        let mut arguments = Initializer::default();
        for argument in &call.arguments {
            arguments.visit_argument(argument);
        }
        if !arguments.call {
            return None;
        }
    }
    let ast = Ast::new(ctx.allocator);
    let origin = expression.span();
    let module_id =
        ast.string(ctx.options.module_id.as_deref().expect("selected target has a module ID"));
    let mut value = expression.take_in(&ctx.allocator);
    if let Some(name) = inferred_name
        && matches!(value.without_parentheses(), Expression::ClassExpression(class) if class.id.is_none())
    {
        value = ast.index(ast.object([ast.prop(name, value)]), ast.string(name));
    }
    ctx.changed = true;
    if !check.awaited {
        *expression = ctx.call(
            "reze-js/internal/reactivity",
            "withModuleScope",
            [module_id, ast.arrow([], value)],
        );
        *expression.span_mut() = origin;
        return None;
    }
    let frame = ctx.fresh("_moduleScope$");
    let result = ctx.fresh("_moduleValue$");
    let error = ctx.fresh("_moduleError$");
    ModuleAwaits { ctx, frame, await_count: 0 }.visit_expression(&mut value);
    let begin = ctx.call("reze-js/internal/reactivity", "beginModuleScope", [module_id]);
    output.push(ast.declaration(VariableDeclarationKind::Let, result, None));
    output.push(ast.declaration(VariableDeclarationKind::Let, frame, Some(begin)));
    output.push(Statement::new_try_statement(
        SPAN,
        BlockStatement::boxed(
            SPAN,
            ArenaVec::from_array_in([ast.stmt(ast.assign(ast.ident(result), value))], &ast.builder),
            &ast.builder,
        ),
        Some(CatchClause::boxed(
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
                    [
                        ast.stmt(
                            ast.call(ast.member(ast.ident(frame), "reject"), [ast.ident(error)]),
                        ),
                        Statement::new_throw_statement(SPAN, ast.ident(error), &ast.builder),
                    ],
                    &ast.builder,
                ),
                &ast.builder,
            ),
            &ast.builder,
        )),
        Some(BlockStatement::boxed(
            SPAN,
            ArenaVec::from_array_in(
                [
                    ast.stmt(ast.call(ast.member(ast.ident(frame), "end"), [])),
                    ast.stmt(ast.assign(ast.ident(frame), ast.undefined())),
                ],
                &ast.builder,
            ),
            &ast.builder,
        )),
        &ast.builder,
    ));
    *expression = ast.ident(result);
    *expression.span_mut() = origin;
    Some(result)
}

#[derive(Default)]
struct Initializer {
    call: bool,
    awaited: bool,
}

impl<'a> Visit<'a> for Initializer {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, class: &Class<'a>) {
        self.call |= class.heritage.is_some();
        walk::walk_class(self, class);
    }
    fn visit_property_definition(&mut self, property: &PropertyDefinition<'a>) {
        for decorator in &property.decorators {
            self.visit_decorator(decorator);
        }
        if property.computed {
            self.call = true;
            self.visit_property_key(&property.key);
        }
        if property.r#static
            && let Some(value) = &property.value
        {
            self.visit_expression(value);
        }
    }
    fn visit_accessor_property(&mut self, property: &AccessorProperty<'a>) {
        for decorator in &property.decorators {
            self.visit_decorator(decorator);
        }
        if property.computed {
            self.call = true;
            self.visit_property_key(&property.key);
        }
        if property.r#static
            && let Some(value) = &property.value
        {
            self.visit_expression(value);
        }
    }
    fn visit_decorator(&mut self, decorator: &Decorator<'a>) {
        self.call = true;
        walk::walk_decorator(self, decorator);
    }
    fn visit_static_member_expression(&mut self, expression: &StaticMemberExpression<'a>) {
        self.call = true;
        walk::walk_static_member_expression(self, expression);
    }
    fn visit_computed_member_expression(&mut self, expression: &ComputedMemberExpression<'a>) {
        self.call = true;
        walk::walk_computed_member_expression(self, expression);
    }
    fn visit_call_expression(&mut self, expression: &CallExpression<'a>) {
        self.call = true;
        walk::walk_call_expression(self, expression);
    }
    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        self.call = true;
        walk::walk_new_expression(self, expression);
    }
    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        self.call = true;
        walk::walk_import_expression(self, expression);
    }
    fn visit_await_expression(&mut self, expression: &AwaitExpression<'a>) {
        self.awaited = true;
        self.call = true;
        self.visit_expression(&expression.argument);
    }
}

struct ModuleAwaits<'c, 'a, 'm> {
    ctx: &'c mut EmitContext<'a, 'm>,
    frame: &'a str,
    await_count: usize,
}

impl<'a> VisitMut<'a> for ModuleAwaits<'_, 'a, '_> {
    fn visit_function(&mut self, _: &mut Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &mut ArrowFunctionExpression<'a>) {}

    fn visit_catch_clause(&mut self, clause: &mut CatchClause<'a>) {
        oxc_ast_visit::walk_mut::walk_catch_clause(self, clause);
        let ast = Ast::new(self.ctx.allocator);
        let restore = if let Some(parameter) = &mut clause.param
            && !matches!(parameter.pattern, BindingPattern::BindingIdentifier(_))
        {
            let error = self.ctx.fresh("_caught$");
            let pattern = std::mem::replace(
                &mut parameter.pattern,
                BindingPattern::new_binding_identifier(SPAN, error, &ast.builder),
            );
            let body = clause.body.body.take_in(&self.ctx.allocator);
            clause.body.body.push(Statement::new_block_statement(SPAN, body, &ast.builder));
            let rejected =
                ast.call(ast.member(ast.ident(self.frame), "reject"), [ast.ident(error)]);
            let declaration =
                VariableDeclarator::new(SPAN, pattern, None, Some(rejected), false, &ast.builder);
            Statement::new_variable_declaration(
                SPAN,
                VariableDeclarationKind::Let,
                ArenaVec::from_array_in([declaration], &ast.builder),
                false,
                &ast.builder,
            )
        } else {
            ast.stmt(ast.call(ast.member(ast.ident(self.frame), "resume"), [ast.undefined()]))
        };
        clause.body.body.insert(0, restore);
    }

    fn visit_try_statement(&mut self, statement: &mut TryStatement<'a>) {
        let before = self.await_count;
        self.visit_block_statement(&mut statement.block);
        if let Some(handler) = &mut statement.handler {
            self.visit_catch_clause(handler);
        }
        let suspended = self.await_count != before;
        if let Some(finalizer) = &mut statement.finalizer {
            self.visit_block_statement(finalizer);
            if suspended {
                let ast = Ast::new(self.ctx.allocator);
                finalizer.body.insert(
                    0,
                    ast.stmt(
                        ast.call(ast.member(ast.ident(self.frame), "resume"), [ast.undefined()]),
                    ),
                );
            }
        }
    }

    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        oxc_ast_visit::walk_mut::walk_expression(self, expression);
        let Expression::AwaitExpression(awaited) = expression else { return };
        self.await_count += 1;
        let ast = Ast::new(self.ctx.allocator);
        let site = self.ctx.origin_site(awaited.span);
        let value = awaited.argument.take_in(&self.ctx.allocator);
        awaited.argument = ast.call(ast.member(ast.ident(self.frame), "suspend"), [value, site]);
        let original = expression.take_in(&self.ctx.allocator);
        *expression = ast.call(ast.member(ast.ident(self.frame), "resume"), [original]);
    }
}
