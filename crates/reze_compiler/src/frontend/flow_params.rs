use std::collections::{HashMap, HashSet};

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast::builder::AstBuilder;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::symbol::SymbolId;

use super::analysis::{Intrinsic, Primitive};
use super::pure::is_meaningful;
use crate::diagnostic::{Code, Edit, Report};
use crate::exports;

#[derive(Default)]
pub struct Plan {
    getters: HashSet<SymbolId>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.getters.is_empty()
    }
}

pub fn collect(program: &Program<'_>, scoping: &Scoping, reports: &mut Vec<Report>) -> Plan {
    let mut plan = Plan::default();
    let mut bindings = Bindings::default();
    bindings.visit_program(program);
    let mut flows = Flows {
        scoping,
        tags: &FlowTags::scan(program),
        bindings: &bindings,
        plan: &mut plan,
        reports,
    };
    flows.visit_program(program);
    plan
}

pub fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    scoping: &Scoping,
    plan: Plan,
) -> bool {
    if plan.is_empty() {
        return false;
    }
    let mut rewrite = Rewrite { alloc: allocator, scoping, getters: &plan.getters, changed: false };
    rewrite.visit_program(program);
    rewrite.changed
}

struct FlowTags {
    tags: HashMap<SymbolId, Intrinsic>,
    namespaces: HashMap<SymbolId, &'static str>,
}

impl FlowTags {
    fn scan(program: &Program<'_>) -> Self {
        let mut view = FlowTags { tags: HashMap::new(), namespaces: HashMap::new() };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.import_kind.is_type() {
                continue;
            };
            let Some(home) = exports::module(import.source.value.as_str()) else { continue };
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        if named.import_kind.is_type() {
                            continue;
                        }
                        if let Some(Primitive::Intrinsic(intrinsic)) =
                            exports::primitive_named(home, named.imported.name().as_str())
                            && matches!(
                                intrinsic,
                                Intrinsic::Show | Intrinsic::For | Intrinsic::Match
                            )
                        {
                            view.tags.insert(named.local.symbol_id(), intrinsic);
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                        view.namespaces.insert(namespace.local.symbol_id(), home);
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                }
            }
        }
        view
    }

    fn intrinsic(&self, scoping: &Scoping, name: &JSXElementName<'_>) -> Option<Intrinsic> {
        match name {
            JSXElementName::IdentifierReference(id) => {
                let symbol = symbol_of(scoping, id)?;
                self.tags.get(&symbol).copied()
            }
            JSXElementName::MemberExpression(member) => {
                let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object
                else {
                    return None;
                };
                let source = self.namespaces.get(&symbol_of(scoping, namespace)?)?;
                match exports::primitive_named(source, member.property.name.as_str()) {
                    Some(Primitive::Intrinsic(intrinsic))
                        if matches!(
                            intrinsic,
                            Intrinsic::Show | Intrinsic::For | Intrinsic::Match
                        ) =>
                    {
                        Some(intrinsic)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

fn symbol_of(scoping: &Scoping, id: &IdentifierReference<'_>) -> Option<SymbolId> {
    scoping.get_reference(id.reference_id.get()?).symbol_id()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyedShape {
    True,
    False,
    Function,
    Other,
}

#[derive(Default)]
struct Bindings {
    shapes: HashMap<SymbolId, KeyedShape>,
    aliases: HashMap<SymbolId, oxc_syntax::reference::ReferenceId>,
}

impl<'a> Visit<'a> for Bindings {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && let Some(init) = it.init.as_ref()
        {
            let symbol = id.symbol_id();
            match unveiled(init) {
                Expression::BooleanLiteral(literal) => {
                    self.shapes.insert(
                        symbol,
                        if literal.value { KeyedShape::True } else { KeyedShape::False },
                    );
                }
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                    self.shapes.insert(symbol, KeyedShape::Function);
                }
                Expression::Identifier(target) => {
                    if let Some(reference) = target.reference_id.get() {
                        self.aliases.insert(symbol, reference);
                    }
                }
                _ => {
                    self.shapes.insert(symbol, KeyedShape::Other);
                }
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        if let Some(id) = &it.id {
            self.shapes.insert(id.symbol_id(), KeyedShape::Function);
        }
        walk::walk_function(self, it, flags);
    }
}

fn unveiled<'ast, 'r>(mut expression: &'r Expression<'ast>) -> &'r Expression<'ast> {
    loop {
        expression = expression.without_parentheses();
        expression = match expression {
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            _ => return expression,
        };
    }
}

fn is_function_literal(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

fn attr_name(attribute: &JSXAttribute<'_>) -> String {
    match &attribute.name {
        JSXAttributeName::Identifier(id) => id.name.as_str().to_string(),
        JSXAttributeName::NamespacedName(n) => {
            format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
        }
    }
}

fn plain_param(function: &Expression<'_>, index: usize) -> Option<(SymbolId, String)> {
    let params = match function.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => &arrow.params,
        Expression::FunctionExpression(function) => &function.params,
        _ => return None,
    };
    match &params.items.get(index)?.pattern {
        BindingPattern::BindingIdentifier(id) => {
            Some((id.symbol_id(), id.name.as_str().to_string()))
        }
        _ => None,
    }
}

struct Flows<'s, 't, 'b, 'p> {
    scoping: &'s Scoping,
    tags: &'t FlowTags,
    bindings: &'b Bindings,
    plan: &'p mut Plan,
    reports: &'p mut Vec<Report>,
}

impl<'a> Visit<'a> for Flows<'_, '_, '_, '_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        match self.tags.intrinsic(self.scoping, &it.opening_element.name) {
            Some(Intrinsic::Show | Intrinsic::Match) => self.value_param(it),
            Some(Intrinsic::For) => self.row_params(it),
            _ => {}
        }
        walk::walk_jsx_element(self, it);
    }
}

impl Flows<'_, '_, '_, '_> {
    fn value_param(&mut self, el: &JSXElement<'_>) {
        let mut children = el.children.iter().filter(|child| is_meaningful(child));
        let (Some(first), None) = (children.next(), children.next()) else { return };
        let JSXChild::ExpressionContainer(container) = first else { return };
        let Some(function) = container.expression.as_expression() else { return };
        if !is_function_literal(function) {
            return;
        }
        let Some((symbol, name)) = plain_param(function, 0) else { return };
        self.plan.getters.insert(symbol);
        Called { scoping: self.scoping, getters: &[(symbol, name)], reports: self.reports }
            .visit_expression(function);
    }

    fn row_params(&mut self, el: &JSXElement<'_>) {
        let Some(map) = row_function(el) else { return };
        if !is_function_literal(map) {
            self.reports.push(
                Report::new(Code::ControlFlowChildren, map.span())
                    .arg("tag", Intrinsic::For.name().to_string())
                    .arg("expected", "one function `(item, index) => …`"),
            );
            return;
        }
        let positions = match self.row_mode(el) {
            Some(RowMode::Identity) => [None, Some(1)],
            Some(RowMode::Indexed) => [Some(0), None],
            Some(RowMode::Keyed) => [Some(0), Some(1)],
            None => return,
        };
        let mut getters = Vec::new();
        for position in positions.into_iter().flatten() {
            if let Some(getter) = plain_param(map, position) {
                self.plan.getters.insert(getter.0);
                getters.push(getter);
            }
        }
        Called { scoping: self.scoping, getters: &getters, reports: self.reports }
            .visit_expression(map);
    }

    fn row_mode(&mut self, el: &JSXElement<'_>) -> Option<RowMode> {
        let mut keyed_attrs = el
            .opening_element
            .attributes
            .iter()
            .filter_map(|item| match item {
                JSXAttributeItem::Attribute(attribute) => Some(&**attribute),
                JSXAttributeItem::SpreadAttribute(_) => None,
            })
            .filter(|attribute| attr_name(attribute) == "keyed");
        let Some(keyed) = keyed_attrs.next_back() else { return Some(RowMode::Identity) };
        let Some(value) = &keyed.value else { return Some(RowMode::Identity) };
        match value {
            JSXAttributeValue::StringLiteral(_)
            | JSXAttributeValue::Element(_)
            | JSXAttributeValue::Fragment(_) => Some(RowMode::Identity),
            JSXAttributeValue::ExpressionContainer(container) => {
                let Some(expression) = container.expression.as_expression() else {
                    return Some(RowMode::Identity);
                };
                match unveiled(expression) {
                    Expression::BooleanLiteral(literal) => {
                        Some(if literal.value { RowMode::Identity } else { RowMode::Indexed })
                    }
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                        Some(RowMode::Keyed)
                    }
                    Expression::Identifier(id) => self.keyed_name(keyed.span, id),
                    Expression::NullLiteral(_)
                    | Expression::StringLiteral(_)
                    | Expression::TemplateLiteral(_)
                    | Expression::NumericLiteral(_)
                    | Expression::BigIntLiteral(_)
                    | Expression::RegExpLiteral(_)
                    | Expression::ArrayExpression(_)
                    | Expression::ObjectExpression(_) => Some(RowMode::Identity),
                    Expression::UnaryExpression(unary) if is_void_or_typeof(unary) => {
                        Some(RowMode::Identity)
                    }
                    _ => {
                        self.reports.push(Report::new(Code::ForKeyedDynamic, keyed.span));
                        None
                    }
                }
            }
        }
    }

    fn keyed_name(&mut self, span: Span, id: &IdentifierReference<'_>) -> Option<RowMode> {
        let reference = id.reference_id.get()?;
        let name = id.name.as_str();
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else {
            return (name == "undefined").then_some(RowMode::Identity).or_else(|| {
                self.reports.push(Report::new(Code::ForKeyedDynamic, span));
                None
            });
        };
        match self.bindings.shape_of(self.scoping, symbol) {
            KeyedShape::True => Some(RowMode::Identity),
            KeyedShape::False => Some(RowMode::Indexed),
            KeyedShape::Function => Some(RowMode::Keyed),
            KeyedShape::Other => {
                self.reports.push(Report::new(Code::ForKeyedDynamic, span));
                None
            }
        }
    }
}

impl Bindings {
    fn shape_of(&self, scoping: &Scoping, symbol: SymbolId) -> KeyedShape {
        if let Some(&shape) = self.shapes.get(&symbol) {
            return shape;
        }
        if let Some(&target) = self.aliases.get(&symbol)
            && let Some(target) = scoping.get_reference(target).symbol_id()
        {
            return self.shapes.get(&target).copied().unwrap_or(KeyedShape::Other);
        }
        KeyedShape::Other
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowMode {
    Identity,
    Indexed,
    Keyed,
}

fn is_void_or_typeof(unary: &UnaryExpression<'_>) -> bool {
    matches!(
        unary.operator,
        oxc_syntax::operator::UnaryOperator::Void | oxc_syntax::operator::UnaryOperator::Typeof
    )
}

fn row_function<'b, 'x>(el: &'b JSXElement<'x>) -> Option<&'b Expression<'x>> {
    let mut children = el.children.iter().filter(|child| is_meaningful(child));
    match (children.next(), children.next()) {
        (None, _) => {}
        (Some(JSXChild::ExpressionContainer(container)), None) => {
            return container.expression.as_expression();
        }
        _ => return None,
    }
    let attribute = el.opening_element.attributes.iter().find_map(|item| match item {
        JSXAttributeItem::Attribute(attribute)
            if matches!(&attribute.name, JSXAttributeName::Identifier(id) if id.name == "children") =>
        {
            Some(&**attribute)
        }
        _ => None,
    })?;
    match &attribute.value {
        Some(JSXAttributeValue::ExpressionContainer(container)) => {
            container.expression.as_expression()
        }
        _ => None,
    }
}

struct Called<'s, 'g> {
    scoping: &'s Scoping,
    getters: &'g [(SymbolId, String)],
    reports: &'g mut Vec<Report>,
}

impl<'a> Visit<'a> for Called<'_, '_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = it.callee.without_parentheses()
            && let Some(reference) = id.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
            && let Some((_, name)) = self.getters.iter().find(|(known, _)| *known == symbol)
        {
            self.reports.push(
                Report::new(Code::FlowParamCalled, it.span)
                    .arg("param", name.clone())
                    .fix(vec![Edit { start: it.span.start, end: it.span.end, text: name.clone() }]),
            );
        }
        walk::walk_call_expression(self, it);
    }
}

struct Rewrite<'a, 's> {
    alloc: &'a Allocator,
    scoping: &'s Scoping,
    getters: &'s HashSet<SymbolId>,
    changed: bool,
}

impl<'a, 's> VisitMut<'a> for Rewrite<'a, 's> {
    fn visit_call_expression(&mut self, it: &mut CallExpression<'a>) {
        let direct = matches!(&it.callee.without_parentheses(), Expression::Identifier(id) if self.is_getter(id));
        if direct {
            for argument in &mut it.arguments {
                self.visit_argument(argument);
            }
            return;
        }
        walk_mut::walk_call_expression(self, it);
    }

    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        if let Expression::Identifier(id) = it
            && let Some(reference) = id.reference_id.get()
        {
            let read = self.scoping.get_reference(reference);
            if !read.flags().is_write()
                && read.symbol_id().is_some_and(|symbol| self.getters.contains(&symbol))
            {
                let builder = AstBuilder::new(self.alloc);
                let span = id.span;
                let name: &'a str = self.alloc.alloc_str(id.name.as_str());
                let callee = Expression::new_identifier(span, name, &builder);
                *it = Expression::new_call_expression(
                    span,
                    callee,
                    None,
                    ArenaVec::new_in(&builder),
                    false,
                    &builder,
                );
                self.changed = true;
                return;
            }
        }
        walk_mut::walk_expression(self, it);
    }
}

impl Rewrite<'_, '_> {
    fn is_getter(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id
            .get()
            .and_then(|reference| self.scoping.get_reference(reference).symbol_id())
            .is_some_and(|symbol| self.getters.contains(&symbol))
    }
}
