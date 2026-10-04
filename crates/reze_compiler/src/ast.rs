use oxc_allocator::{Allocator, ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::SPAN;
use oxc_str::{Ident, Str};
use oxc_syntax::{
    number::NumberBase,
    operator::{AssignmentOperator, BinaryOperator, UnaryOperator},
};

pub struct Ast<'a> {
    pub builder: AstBuilder<'a>,
}

impl<'a> Ast<'a> {
    pub fn new(allocator: &'a Allocator) -> Self {
        Self { builder: AstBuilder::new(allocator) }
    }

    pub fn ident(&self, name: &'a str) -> Expression<'a> {
        Expression::new_identifier(SPAN, Ident::from(name), &self.builder)
    }

    pub fn string(&self, value: &str) -> Expression<'a> {
        Expression::new_string_literal(SPAN, Str::from_str_in(value, &self.builder), None, &self.builder)
    }

    pub fn number(&self, value: f64) -> Expression<'a> {
        Expression::new_numeric_literal(SPAN, value, None, NumberBase::Decimal, &self.builder)
    }

    pub fn boolean(&self, value: bool) -> Expression<'a> {
        Expression::new_boolean_literal(SPAN, value, &self.builder)
    }

    pub fn null(&self) -> Expression<'a> {
        Expression::new_null_literal(SPAN, &self.builder)
    }

    pub fn undefined(&self) -> Expression<'a> {
        self.unary(UnaryOperator::Void, self.number(0.0))
    }

    pub fn call(
        &self,
        callee: Expression<'a>,
        args: impl IntoIterator<Item = Expression<'a>>,
    ) -> Expression<'a> {
        let args = ArenaVec::from_iter_in(args.into_iter().map(Argument::from), &self.builder);
        Expression::new_call_expression(SPAN, callee, None, args, false, &self.builder)
    }

    pub fn member(&self, object: Expression<'a>, name: &'a str) -> Expression<'a> {
        let property = IdentifierName::new(SPAN, Ident::from(name), &self.builder);
        Expression::new_static_member_expression(SPAN, object, property, false, &self.builder)
    }

    pub fn index(&self, object: Expression<'a>, key: Expression<'a>) -> Expression<'a> {
        Expression::new_computed_member_expression(SPAN, object, key, false, &self.builder)
    }

    pub fn assign(&self, target: Expression<'a>, value: Expression<'a>) -> Expression<'a> {
        let target = match target {
            Expression::Identifier(node) => AssignmentTarget::AssignmentTargetIdentifier(node),
            Expression::StaticMemberExpression(node) => AssignmentTarget::StaticMemberExpression(node),
            Expression::ComputedMemberExpression(node) => AssignmentTarget::ComputedMemberExpression(node),
            Expression::PrivateFieldExpression(node) => AssignmentTarget::PrivateFieldExpression(node),
            _ => unreachable!("generated assignment requires a reference"),
        };
        Expression::new_assignment_expression(SPAN, AssignmentOperator::Assign, target, value, &self.builder)
    }

    pub fn unary(&self, op: UnaryOperator, value: Expression<'a>) -> Expression<'a> {
        Expression::new_unary_expression(SPAN, op, value, &self.builder)
    }

    pub fn binary(&self, left: Expression<'a>, op: BinaryOperator, right: Expression<'a>) -> Expression<'a> {
        Expression::new_binary_expression(SPAN, left, op, right, &self.builder)
    }

    pub fn conditional(
        &self,
        test: Expression<'a>,
        consequent: Expression<'a>,
        alternate: Expression<'a>,
    ) -> Expression<'a> {
        Expression::new_conditional_expression(SPAN, test, consequent, alternate, &self.builder)
    }

    pub fn array(&self, values: impl IntoIterator<Item = Expression<'a>>) -> Expression<'a> {
        let values = ArenaVec::from_iter_in(values.into_iter().map(ArrayExpressionElement::from), &self.builder);
        Expression::new_array_expression(SPAN, values, &self.builder)
    }

    pub fn object(&self, props: impl IntoIterator<Item = ObjectPropertyKind<'a>>) -> Expression<'a> {
        let props = ArenaVec::from_iter_in(props, &self.builder);
        Expression::new_object_expression(SPAN, props, &self.builder)
    }

    fn property(&self, key: &str, value: Expression<'a>, kind: PropertyKind) -> ObjectPropertyKind<'a> {
        let name = PropertyKey::new_string_literal(SPAN, Str::from_str_in(key, &self.builder), None, &self.builder);
        ObjectPropertyKind::new_object_property(
            SPAN, kind, name, value, false, false, key == "__proto__", &self.builder,
        )
    }

    pub fn prop(&self, key: &str, value: Expression<'a>) -> ObjectPropertyKind<'a> {
        self.property(key, value, PropertyKind::Init)
    }

    pub fn getter(&self, key: &str, value: Expression<'a>) -> ObjectPropertyKind<'a> {
        let body = FunctionBody::boxed(
            SPAN,
            ArenaVec::new_in(&self.builder),
            ArenaVec::from_array_in([self.return_stmt(value)], &self.builder),
            &self.builder,
        );
        let value = Expression::new_function_expression(
            SPAN,
            FunctionType::FunctionExpression,
            None,
            false,
            false,
            false,
            None,
            None,
            self.params([], FormalParameterKind::FormalParameter),
            None,
            Some(body),
            &self.builder,
        );
        self.property(key, value, PropertyKind::Get)
    }

    fn params(
        &self,
        names: impl IntoIterator<Item = &'a str>,
        kind: FormalParameterKind,
    ) -> oxc_allocator::ArenaBox<'a, FormalParameters<'a>> {
        let params = names.into_iter().map(|name| {
            let pattern = BindingPattern::new_binding_identifier(SPAN, Ident::from(name), &self.builder);
            FormalParameter::new(
                SPAN, ArenaVec::new_in(&self.builder), pattern, None, None, false, None, false, false,
                &self.builder,
            )
        });
        FormalParameters::boxed(SPAN, kind, ArenaVec::from_iter_in(params, &self.builder), None, &self.builder)
    }

    pub fn arrow(&self, params: impl IntoIterator<Item = &'a str>, value: Expression<'a>) -> Expression<'a> {
        Expression::new_arrow_function_expression(
            SPAN, false, None, self.params(params, FormalParameterKind::ArrowFormalParameters), None,
            ArrowFunctionBody::from(value), &self.builder,
        )
    }

    pub fn block_arrow(
        &self,
        params: impl IntoIterator<Item = &'a str>,
        statements: impl IntoIterator<Item = Statement<'a>>,
    ) -> Expression<'a> {
        let body = ArrowFunctionBody::new_function_body(
            SPAN, ArenaVec::new_in(&self.builder), ArenaVec::from_iter_in(statements, &self.builder),
            &self.builder,
        );
        Expression::new_arrow_function_expression(
            SPAN, false, None, self.params(params, FormalParameterKind::ArrowFormalParameters), None,
            body, &self.builder,
        )
    }

    pub fn stmt(&self, value: Expression<'a>) -> Statement<'a> {
        Statement::new_expression_statement(SPAN, value, &self.builder)
    }

    pub fn return_stmt(&self, value: Expression<'a>) -> Statement<'a> {
        Statement::new_return_statement(SPAN, Some(value), &self.builder)
    }

    pub fn declaration(
        &self,
        kind: VariableDeclarationKind,
        name: &'a str,
        value: Option<Expression<'a>>,
    ) -> Statement<'a> {
        let pattern = BindingPattern::new_binding_identifier(SPAN, Ident::from(name), &self.builder);
        let decl = VariableDeclarator::new(SPAN, pattern, None, value, false, &self.builder);
        Statement::new_variable_declaration(
            SPAN, kind, ArenaVec::from_array_in([decl], &self.builder), false, &self.builder,
        )
    }

    pub fn block(&self, statements: impl IntoIterator<Item = Statement<'a>>) -> Statement<'a> {
        Statement::new_block_statement(SPAN, ArenaVec::from_iter_in(statements, &self.builder), &self.builder)
    }

    pub fn if_stmt(&self, test: Expression<'a>, body: Statement<'a>) -> Statement<'a> {
        Statement::new_if_statement(SPAN, test, body, None, &self.builder)
    }
}
