use std::collections::{HashMap, HashSet};

use oxc_allocator::{Allocator, Box as ArenaBox, TakeIn, Vec as ArenaVec};
use oxc_ast::AstKind;
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{SPAN, Span};
use oxc_str::Ident;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use super::analysis::{AsyncFacts, exported_symbols};
use super::dsl::PreScan;
use super::imports::{Syntax, allows, home_of};
use super::pure::{is_foldable_signal_shape, is_meaningful};
use super::Namer;
use crate::diagnostic::{Code, Report};
use crate::imports::HelperImports;

#[derive(Default)]
pub struct Plan {
    fors: Vec<ForPlan>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.fors.is_empty()
    }

    fn entry(&self, start: u32, end: u32) -> Option<&ForPlan> {
        self.fors.iter().find(|plan| plan.for_span == (start, end))
    }
}

struct ForPlan {
    for_span: (u32, u32),
    comparisons: Vec<Comparison>,
}

struct Comparison {
    span: (u32, u32),
    source_name: String,
    source_span: (u32, u32),
}

/// A reactive getter eligible as a selector source. `converts` tells whether
/// bare reads of it become calls before the rewrite runs (`$signal`/
/// `$computed` do through the DSL normalization; direct `signal()`/
/// `computed()` getters never rewrite bare reads, so only hand-written calls
/// qualify for them).
struct Getter {
    symbol: SymbolId,
    name: String,
    signal: bool,
    converts: bool,
    foldable_shape: bool,
    setter: Option<SymbolId>,
}

pub fn collect(
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    pre: &PreScan,
    _reports: &mut Vec<Report>,
) -> Plan {
    let imports = ImportView::scan(program);
    let mut getters: HashMap<SymbolId, Getter> = HashMap::new();
    let mut declarators = Declarators { scoping, pre, imports: &imports, getters: &mut getters };
    declarators.visit_program(program);
    let mut called = HashSet::new();
    let mut calls = Called { called: &mut called };
    calls.visit_program(program);
    let exported = exported_symbols(program, scoping);
    let asyncs = AsyncFacts::collect(program, scoping, nodes);
    let selectable: HashSet<SymbolId> = getters
        .values()
        .filter(|getter| is_selectable(scoping, nodes, &called, &exported, &asyncs, getter))
        .map(|getter| getter.symbol)
        .collect();
    let mut plan = Plan::default();
    let mut fors = Fors {
        scoping,
        imports: &imports,
        getters: &getters,
        selectable: &selectable,
        plan: &mut plan,
    };
    fors.visit_program(program);
    plan
}

/// A getter stays a live function exactly when constant folding would not
/// replace it. Folding is `analysis` territory; this mirrors its gates on
/// the original semantic so the rewrite never wraps a source the backend
/// folds into a value (which would turn `selector(source)` into
/// `selector(value)`). Computeds are never folded. Any doubt resolves to
/// selectable: a missed optimization is always safe, a spurious one is not.
fn is_selectable(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    called: &HashSet<ReferenceId>,
    exported: &HashSet<SymbolId>,
    asyncs: &AsyncFacts,
    getter: &Getter,
) -> bool {
    if !getter.signal || !getter.foldable_shape {
        return true;
    }
    if getter.setter.is_some_and(|setter| !scoping.get_resolved_reference_ids(setter).is_empty()) {
        return true;
    }
    if exported.contains(&getter.symbol) {
        return true;
    }
    for &reference in scoping.get_resolved_reference_ids(getter.symbol) {
        if asyncs.is_awaited_value(reference) {
            return true;
        }
        if getter.converts {
            if !converted_call(nodes, scoping, reference, called) {
                return true;
            }
        } else if !called.contains(&reference) {
            return true;
        }
    }
    false
}

fn converted_call(
    nodes: &AstNodes<'_>,
    scoping: &Scoping,
    reference: ReferenceId,
    called: &HashSet<ReferenceId>,
) -> bool {
    if called.contains(&reference) {
        return true;
    }
    let reference = scoping.get_reference(reference);
    let flags = reference.flags();
    if flags.is_write() || flags.is_type_only() {
        return false;
    }
    match nodes.parent_kind(reference.node_id()) {
        AstKind::JSXOpeningElement(_)
        | AstKind::JSXClosingElement(_)
        | AstKind::JSXMemberExpression(_)
        | AstKind::ImportSpecifier(_)
        | AstKind::ExportSpecifier(_)
        | AstKind::ExportDefaultDeclaration(_) => false,
        _ => true,
    }
}

pub fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    plan: Plan,
    namer: &mut Namer<'a>,
    helpers: &mut HelperImports<'a>,
    reports: &mut Vec<Report>,
) -> bool {
    if plan.is_empty() {
        return false;
    }
    let mut rewrite = Pass {
        alloc: allocator,
        plan: &plan,
        namer,
        helpers,
        helper: None,
        sels: HashMap::new(),
        reports,
        changed: false,
    };
    rewrite.visit_program(program);
    if !rewrite.changed {
        return false;
    }
    let helper = rewrite.helper.expect("a rewrite requires the selector helper");
    let mut wrap = Wrap { alloc: allocator, helper, sels: &rewrite.sels };
    wrap.visit_program(program);
    true
}

#[derive(Clone)]
struct SelectorUse {
    name: String,
    source: String,
    source_span: Span,
}

/// Pass one: rewrite planned comparisons inside each planned `For` row.
/// Nested row functions are never entered: their comparisons belong to the
/// nested `For` plan and are handled when the walk reaches it.
struct Pass<'x, 'p, 'a> {
    alloc: &'a Allocator,
    plan: &'p Plan,
    namer: &'x mut Namer<'a>,
    helpers: &'x mut HelperImports<'a>,
    helper: Option<&'a str>,
    sels: HashMap<(u32, u32), Vec<SelectorUse>>,
    reports: &'x mut Vec<Report>,
    changed: bool,
}

impl<'a> VisitMut<'a> for Pass<'_, '_, 'a> {
    fn visit_jsx_element(&mut self, it: &mut JSXElement<'a>) {
        let span = it.span;
        let plan = self.plan;
        if let Some(entry) = plan.entry(span.start, span.end) {
            rewrite_row(self, it, entry);
        }
        walk_mut::walk_jsx_element(self, it);
    }
}

fn rewrite_row<'x, 'r, 'a>(pass: &mut Pass<'x, '_, 'a>, el: &mut JSXElement<'a>, entry: &'r ForPlan) {
    let Some(body) = row_body(el) else { return };
    let mut row = Row {
        alloc: pass.alloc,
        entry,
        namer: pass.namer,
        helpers: pass.helpers,
        helper: &mut pass.helper,
        sels: &mut pass.sels,
        reports: pass.reports,
        changed: &mut pass.changed,
    };
    match body {
        RowBody::Block(statements) => {
            for statement in statements {
                row.visit_statement(statement);
            }
        }
        RowBody::Expression(expression) => {
            row.visit_expression(expression);
        }
    }
}

enum RowBody<'b, 'a> {
    Block(&'b mut ArenaVec<'a, Statement<'a>>),
    Expression(&'b mut Expression<'a>),
}

/// The statements or the single expression of the row function, without
/// entering it (the caller already holds it mutably).
fn row_body<'b, 'a>(el: &'b mut JSXElement<'a>) -> Option<RowBody<'b, 'a>> {
    let function = row_function(el)?;
    match function {
        RowFunction::Arrow(arrow) => match &mut arrow.body {
            ArrowFunctionBody::FunctionBody(body) => Some(RowBody::Block(&mut body.statements)),
            expression => Some(RowBody::Expression(expression.as_expression_mut()?)),
        },
        RowFunction::Function(function) => {
            Some(RowBody::Block(&mut function.body.as_mut()?.statements))
        }
    }
}

struct Row<'x, 'p, 'a> {
    alloc: &'a Allocator,
    entry: &'p ForPlan,
    namer: &'x mut Namer<'a>,
    helpers: &'x mut HelperImports<'a>,
    helper: &'x mut Option<&'a str>,
    sels: &'x mut HashMap<(u32, u32), Vec<SelectorUse>>,
    reports: &'x mut Vec<Report>,
    changed: &'x mut bool,
}

impl<'a> VisitMut<'a> for Row<'_, '_, 'a> {
    fn visit_function(&mut self, _: &mut Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &mut ArrowFunctionExpression<'a>) {}

    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        let Expression::BinaryExpression(binary) = it else {
            walk_mut::walk_expression(self, it);
            return;
        };
        let negated = match binary.operator {
            BinaryOperator::StrictEquality => false,
            BinaryOperator::StrictInequality => true,
            _ => {
                walk_mut::walk_expression(self, it);
                return;
            }
        };
        let span = binary.span;
        let Some(comparison) =
            self.entry.comparisons.iter().find(|one| one.span == (span.start, span.end))
        else {
            walk_mut::walk_expression(self, it);
            return;
        };
        if !rewrite_comparison(self, it, comparison, negated) {
            walk_mut::walk_expression(self, it);
        }
    }
}

fn rewrite_comparison<'x, 'p, 'a>(
    row: &mut Row<'x, 'p, 'a>,
    it: &mut Expression<'a>,
    comparison: &Comparison,
    negated: bool,
) -> bool {
    let source_span = Span::new(comparison.source_span.0, comparison.source_span.1);
    let Expression::BinaryExpression(binary) = it else { return false };
    if binary.operator != BinaryOperator::StrictEquality
        && binary.operator != BinaryOperator::StrictInequality
    {
        return false;
    }
    let builder = AstBuilder::new(row.alloc);
    let key = if is_source_call(&binary.left, &comparison.source_name, source_span) {
        std::mem::replace(&mut binary.right, Expression::new_null_literal(SPAN, &builder))
    } else if is_source_call(&binary.right, &comparison.source_name, source_span) {
        std::mem::replace(&mut binary.left, Expression::new_null_literal(SPAN, &builder))
    } else {
        return false;
    };
    let name = selector_name(row, comparison, source_span);
    let span = binary.span;
    let mut arguments = ArenaVec::new_in(&builder);
    arguments.push(Argument::from(key));
    let mut lookup = Expression::new_call_expression(
        span,
        ident(row.alloc, SPAN, name),
        None,
        arguments,
        false,
        &builder,
    );
    if negated {
        lookup = Expression::new_unary_expression(span, UnaryOperator::LogicalNot, lookup, &builder);
    }
    *it = lookup;
    row.reports.push(
        Report::new(Code::AutoSelector, span).arg("signal", comparison.source_name.as_str()),
    );
    *row.changed = true;
    true
}

fn selector_name<'x, 'p, 'a>(
    row: &mut Row<'x, 'p, 'a>,
    comparison: &Comparison,
    source_span: Span,
) -> &'a str {
    let for_span = row.entry.for_span;
    if let Some(found) = row
        .sels
        .get(&for_span)
        .and_then(|uses| uses.iter().find(|uses| uses.source == comparison.source_name))
    {
        let name: &'a str = row.alloc.alloc_str(&found.name);
        return name;
    }
    let name = row.namer.fresh("_sel$");
    let text: &'a str = row.alloc.alloc_str(&name);
    row.sels.entry(for_span).or_default().push(SelectorUse {
        name,
        source: comparison.source_name.clone(),
        source_span,
    });
    if row.helper.is_none() {
        *row.helper = Some(row.helpers.require(row.alloc, row.namer, "reze-js", "selector"));
    }
    text
}

/// The post-DSL shape of the planned source operand: the zero-arg call the
/// DSL normalization produced (or the hand-written one), matched by span and
/// callee name at the same position, so shadowing cannot misroute it.
fn is_source_call(operand: &Expression<'_>, name: &str, span: Span) -> bool {
    let Expression::CallExpression(call) = operand.without_parentheses() else { return false };
    if !call.arguments.is_empty() || call.optional || call.type_arguments.is_some() {
        return false;
    }
    if call.span != span {
        return false;
    }
    match &call.callee {
        Expression::Identifier(id) => id.name.as_str() == name,
        _ => false,
    }
}

/// Pass two: wrap each rewritten `For` in its selector scope.
struct Wrap<'x, 'a> {
    alloc: &'a Allocator,
    helper: &'a str,
    sels: &'x HashMap<(u32, u32), Vec<SelectorUse>>,
}

impl<'a> VisitMut<'a> for Wrap<'_, 'a> {
    fn visit_jsx_child(&mut self, child: &mut JSXChild<'a>) {
        walk_mut::walk_jsx_child(self, child);
        let JSXChild::Element(element) = child else { return };
        let span = element.span;
        let Some(uses) = self.sels.get(&(span.start, span.end)) else { return };
        if uses.is_empty() { return; }
        let JSXChild::Element(element) = child.take_in(&self.alloc) else { unreachable!() };
        let expression = iife(
            self.alloc, self.helper, Expression::JSXElement(element), uses,
        );
        *child = JSXChild::new_expression_container(
            span, JSXExpression::from(expression), &AstBuilder::new(self.alloc),
        );
    }

    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);
        let Expression::JSXElement(element) = it else {
            return;
        };
        let span = element.span;
        let Some(uses) = self.sels.get(&(span.start, span.end)) else {
            return;
        };
        if uses.is_empty() {
            return;
        }
        let builder = AstBuilder::new(self.alloc);
        let taken =
            std::mem::replace(it, Expression::new_null_literal(SPAN, &builder));
        *it = iife(self.alloc, self.helper, taken, uses);
    }
}

/// `((sel, …) => for_)(selector(source), …)`: the selectors evaluate before
/// the rows in the same owner, and the rows close over them.
fn iife<'a>(
    alloc: &'a Allocator,
    helper: &'a str,
    for_: Expression<'a>,
    uses: &[SelectorUse],
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    let mut params = ArenaVec::new_in(&builder);
    let mut arguments = ArenaVec::new_in(&builder);
    for uses in uses {
        let name: &'a str = alloc.alloc_str(&uses.name);
        let pattern = BindingPattern::new_binding_identifier(SPAN, Ident::from(name), &builder);
        params.push(FormalParameter::new(
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
        let source: &'a str = alloc.alloc_str(&uses.source);
        let mut inner = ArenaVec::new_in(&builder);
        inner.push(Argument::from(ident(alloc, uses.source_span, source)));
        arguments.push(Argument::from(Expression::new_call_expression(
            uses.source_span,
            ident(alloc, SPAN, helper),
            None,
            inner,
            false,
            &builder,
        )));
    }
    let params = FormalParameters::boxed(
        SPAN,
        FormalParameterKind::ArrowFormalParameters,
        params,
        None,
        &builder,
    );
    let arrow = Expression::new_arrow_function_expression(
        SPAN,
        false,
        None,
        params,
        None,
        ArrowFunctionBody::from(for_),
        &builder,
    );
    Expression::new_call_expression(SPAN, arrow, None, arguments, false, &builder)
}

fn ident<'a>(alloc: &'a Allocator, span: Span, name: &'a str) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_identifier(span, Ident::from(name), &builder)
}

enum RowFunction<'b, 'a> {
    Arrow(&'b mut ArenaBox<'a, ArrowFunctionExpression<'a>>),
    Function(&'b mut ArenaBox<'a, Function<'a>>),
}

/// The row function of a `For`, from its single meaningful child or its
/// `children` attribute: an arrow or plain function, never async, never a
/// generator.
fn row_function<'b, 'a>(el: &'b mut JSXElement<'a>) -> Option<RowFunction<'b, 'a>> {
    if el.children.iter().filter(|child| is_meaningful(child)).count() == 1 {
        let position = el.children.iter().position(|child| {
            matches!(child, JSXChild::ExpressionContainer(_)) && is_meaningful(child)
        });
        let index = position?;
        let JSXChild::ExpressionContainer(container) = &mut el.children[index] else {
            return None;
        };
        let expression = container.expression.as_expression_mut()?;
        return as_row_function(expression);
    }
    if el.children.iter().any(|child| is_meaningful(child)) {
        return None;
    }
    let attribute = el
        .opening_element
        .attributes
        .iter_mut()
        .rev()
        .find_map(|item| match item {
            JSXAttributeItem::Attribute(a)
                if matches!(&a.name, JSXAttributeName::Identifier(id) if id.name == "children") =>
            {
                Some(&mut **a)
            }
            _ => None,
        })?;
    let JSXAttributeValue::ExpressionContainer(container) = attribute.value.as_mut()? else {
        return None;
    };
    as_row_function(container.expression.as_expression_mut()?)
}

fn as_row_function<'b, 'a>(expression: &'b mut Expression<'a>) -> Option<RowFunction<'b, 'a>> {
    match expression.without_parentheses_mut() {
        Expression::ArrowFunctionExpression(arrow) if !arrow.r#async => {
            Some(RowFunction::Arrow(arrow))
        }
        Expression::FunctionExpression(function)
            if !function.r#async && !function.generator =>
        {
            Some(RowFunction::Function(function))
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Signal,
    Computed,
    For,
}

struct ImportView {
    named: HashMap<SymbolId, ImportKind>,
    namespaces: HashMap<SymbolId, &'static str>,
}

/// The imports a selector needs: `signal`/`computed` factories a source can
/// be declared through, and the `For` tag itself, each by named import or by
/// namespace home. This mirrors the `named`/`namespaces` maps of `analysis`
/// restricted to the three names; `$signal`/`$computed` inits resolve
/// through the DSL prescan instead.
impl ImportView {
    fn scan(program: &Program<'_>) -> Self {
        let mut view = ImportView { named: HashMap::new(), namespaces: HashMap::new() };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.import_kind.is_type() {
                continue;
            }
            let Some(home) = home_of(import.source.value.as_str()) else { continue };
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        if named.import_kind.is_type() {
                            continue;
                        }
                        let name = named.imported.name();
                        let kind = match name.as_str() {
                            "signal" => ImportKind::Signal,
                            "computed" => ImportKind::Computed,
                            "For" => ImportKind::For,
                            _ => continue,
                        };
                        if allows(home, name.as_str()) {
                            view.named.insert(named.local.symbol_id(), kind);
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

    fn factory(&self, scoping: &Scoping, callee: &Expression<'_>) -> Option<bool> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => match self.named.get(
                &scoping.get_reference(id.reference_id.get()?).symbol_id()?,
            )? {
                ImportKind::Signal => Some(true),
                ImportKind::Computed => Some(false),
                ImportKind::For => None,
            },
            Expression::StaticMemberExpression(member) if !member.optional => {
                let Expression::Identifier(namespace) = &member.object else { return None };
                let symbol =
                    scoping.get_reference(namespace.reference_id.get()?).symbol_id()?;
                let source = self.namespaces.get(&symbol)?;
                let name = member.property.name.as_str();
                if !matches!(name, "signal" | "computed") || !allows(source, name) {
                    return None;
                }
                Some(name == "signal")
            }
            _ => None,
        }
    }

    fn is_for(&self, scoping: &Scoping, name: &JSXElementName<'_>) -> bool {
        match name {
            JSXElementName::IdentifierReference(id) => id
                .reference_id
                .get()
                .and_then(|r| scoping.get_reference(r).symbol_id())
                .is_some_and(|symbol| self.named.get(&symbol) == Some(&ImportKind::For)),
            JSXElementName::MemberExpression(member) => {
                let JSXMemberExpressionObject::IdentifierReference(namespace) = &member.object
                else {
                    return false;
                };
                let source = namespace
                    .reference_id
                    .get()
                    .and_then(|r| scoping.get_reference(r).symbol_id())
                    .and_then(|symbol| self.namespaces.get(&symbol));
                member.property.name.as_str() == "For"
                    && source.is_some_and(|source| allows(source, "For"))
            }
            _ => false,
        }
    }
}

struct Declarators<'s, 'p, 'g> {
    scoping: &'s Scoping,
    pre: &'p PreScan,
    imports: &'p ImportView,
    getters: &'g mut HashMap<SymbolId, Getter>,
}

impl Declarators<'_, '_, '_> {
    fn declare(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        call: &CallExpression<'_>,
        signal: bool,
        converts: bool,
    ) {
        let (getter, setter) = match &declarator.id {
            BindingPattern::BindingIdentifier(id) if !signal || converts => (Some(id.symbol_id()), None),
            BindingPattern::ArrayPattern(pattern) if signal => {
                let getter = match pattern.elements.first() {
                    Some(Some(BindingPattern::BindingIdentifier(id))) => id.symbol_id(),
                    _ => return,
                };
                let setter = match pattern.elements.get(1) {
                    Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
                    _ => None,
                };
                (Some(getter), setter)
            }
            _ => return,
        };
        let Some(getter) = getter else { return };
        let name = self.scoping.symbol_name(getter).to_string();
        let foldable_shape = signal && is_foldable_signal_shape(declarator, call);
        self.getters.insert(getter, Getter {
            symbol: getter,
            name,
            signal,
            converts,
            foldable_shape,
            setter,
        });
    }
}

impl<'a> Visit<'a> for Declarators<'_, '_, '_> {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(Expression::CallExpression(call)) =
            it.init.as_ref().map(Expression::without_parentheses)
        {
            if let Some(plan) = self.pre.decls.get(&it.span.start) {
                match plan.primitive {
                    Syntax::Signal => self.declare(it, call, true, true),
                    Syntax::Computed => self.declare(it, call, false, true),
                    Syntax::Action => {}
                }
            } else if let Some(signal) = self.imports.factory(self.scoping, &call.callee) {
                self.declare(it, call, signal, false);
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

/// Zero-arg bare calls, the shape folding counts: every converted bare read
/// becomes one.
struct Called<'c> {
    called: &'c mut HashSet<ReferenceId>,
}

impl<'a> Visit<'a> for Called<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && it.arguments.is_empty()
            && !it.optional
            && it.type_arguments.is_none()
            && let Some(reference) = id.reference_id.get()
        {
            self.called.insert(reference);
        }
        walk::walk_call_expression(self, it);
    }
}

struct Fors<'s, 'p, 'l> {
    scoping: &'s Scoping,
    imports: &'p ImportView,
    getters: &'p HashMap<SymbolId, Getter>,
    selectable: &'p HashSet<SymbolId>,
    plan: &'l mut Plan,
}

impl<'a> Visit<'a> for Fors<'_, '_, '_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if self.imports.is_for(self.scoping, &it.opening_element.name) {
            self.for_element(it);
        }
        walk::walk_jsx_element(self, it);
    }
}

impl Fors<'_, '_, '_> {
    fn for_element(&mut self, el: &JSXElement<'_>) {
        let Some(shape) = row_shape(el) else { return };
        let mut scan = Scan {
            scoping: self.scoping,
            getters: self.getters,
            selectable: self.selectable,
            params: &shape.params,
            row_span: shape.span,
            comparisons: Vec::new(),
        };
        match shape.body {
            RowShapeBody::Block(statements) => {
                for statement in statements {
                    scan.visit_statement(statement);
                }
            }
            RowShapeBody::Expression(expression) => {
                scan.visit_expression(expression);
            }
        };
        if !scan.comparisons.is_empty() {
            self.plan.fors.push(ForPlan {
                for_span: (el.span.start, el.span.end),
                comparisons: scan.comparisons,
            });
        }
    }
}

struct RowShape<'b, 'a> {
    params: Vec<SymbolId>,
    span: Span,
    body: RowShapeBody<'b, 'a>,
}

enum RowShapeBody<'b, 'a> {
    Block(&'b [Statement<'a>]),
    Expression(&'b Expression<'a>),
}

/// The row scope of a `For`: every plain parameter, the function span the
/// declared-outside check keys on, and the body to scan.
fn row_shape<'b, 'a>(el: &'b JSXElement<'a>) -> Option<RowShape<'b, 'a>> {
    let function = row_child(el)?;
    match function {
        FunctionChild::Arrow(arrow) if !arrow.r#async => {
            let params = arrow
                .params
                .items
                .iter()
                .filter_map(|param| match &param.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                    _ => None,
                })
                .collect();
            let body = match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => {
                    RowShapeBody::Block(&body.statements)
                }
                _ => RowShapeBody::Expression(arrow.body.as_expression()?),
            };
            Some(RowShape { params, span: arrow.span, body })
        }
        FunctionChild::Function(function) if !function.r#async && !function.generator => {
            let params = function
                .params
                .items
                .iter()
                .filter_map(|param| match &param.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                    _ => None,
                })
                .collect();
            let body = RowShapeBody::Block(&function.body.as_ref()?.statements);
            Some(RowShape { params, span: function.span, body })
        }
        _ => None,
    }
}

enum FunctionChild<'b, 'a> {
    Arrow(&'b ArrowFunctionExpression<'a>),
    Function(&'b Function<'a>),
}

fn row_child<'b, 'a>(el: &'b JSXElement<'a>) -> Option<FunctionChild<'b, 'a>> {
    let mut meaningful = el.children.iter().filter(|child| is_meaningful(child));
    match meaningful.next() {
        None => {}
        Some(first) => {
            if meaningful.next().is_some() {
                return None;
            }
            let JSXChild::ExpressionContainer(container) = first else { return None };
            return as_row_child(container.expression.as_expression()?);
        }
    }
    let attribute = el
        .opening_element
        .attributes
        .iter()
        .rev()
        .find_map(|item| match item {
            JSXAttributeItem::Attribute(a)
                if matches!(&a.name, JSXAttributeName::Identifier(id) if id.name == "children") =>
            {
                Some(&**a)
            }
            _ => None,
        })?;
    let JSXAttributeValue::ExpressionContainer(container) = attribute.value.as_ref()? else {
        return None;
    };
    as_row_child(container.expression.as_expression()?)
}

fn as_row_child<'b, 'a>(expression: &'b Expression<'a>) -> Option<FunctionChild<'b, 'a>> {
    match expression.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => Some(FunctionChild::Arrow(arrow)),
        Expression::FunctionExpression(function) => Some(FunctionChild::Function(function)),
        _ => None,
    }
}

struct Scan<'s, 'p> {
    scoping: &'s Scoping,
    getters: &'p HashMap<SymbolId, Getter>,
    selectable: &'p HashSet<SymbolId>,
    params: &'p [SymbolId],
    row_span: Span,
    comparisons: Vec<Comparison>,
}

impl<'a> Visit<'a> for Scan<'_, '_> {
    fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_binary_expression(&mut self, it: &BinaryExpression<'a>) {
        if matches!(it.operator, BinaryOperator::StrictEquality | BinaryOperator::StrictInequality)
            && let Some(comparison) = self.comparison(it)
        {
            self.comparisons.push(comparison);
        }
        walk::walk_binary_expression(self, it);
    }
}

impl Scan<'_, '_> {
    fn comparison(&self, it: &BinaryExpression<'_>) -> Option<Comparison> {
        self.split(&it.left, &it.right).or_else(|| self.split(&it.right, &it.left)).map(
            |(source_name, source_span)| Comparison {
                span: (it.span.start, it.span.end),
                source_name,
                source_span: (source_span.start, source_span.end),
            },
        )
    }

    /// The source operand with its original span, or `None`. A bare read
    /// qualifies only for DSL-managed getters (it becomes the call); a
    /// hand-written plain call qualifies for every getter.
    fn split(
        &self,
        source_side: &Expression<'_>,
        key_side: &Expression<'_>,
    ) -> Option<(String, Span)> {
        let (symbol, span) = match source_side.without_parentheses() {
            Expression::Identifier(id) => {
                let reference = id.reference_id.get()?;
                let symbol = self.scoping.get_reference(reference).symbol_id()?;
                if self.scoping.get_reference(reference).flags().is_write() {
                    return None;
                }
                (symbol, id.span)
            }
            Expression::CallExpression(call) => {
                if !call.arguments.is_empty() || call.optional || call.type_arguments.is_some() {
                    return None;
                }
                let Expression::Identifier(id) = &call.callee else { return None };
                let reference = id.reference_id.get()?;
                let symbol = self.scoping.get_reference(reference).symbol_id()?;
                (symbol, call.span)
            }
            _ => return None,
        };
        let getter = self.getters.get(&symbol)?;
        if !self.selectable.contains(&symbol) {
            return None;
        }
        if matches!(source_side.without_parentheses(), Expression::Identifier(_))
            && !getter.converts
        {
            return None;
        }
        if self.row_span.contains_inclusive(self.scoping.symbol_span(symbol)) {
            return None;
        }
        let mut reads_param = false;
        if !is_row_path(key_side, self.params, self.scoping, &mut reads_param) || !reads_param {
            return None;
        }
        Some((getter.name.clone(), span))
    }
}

/// Calls of row parameters, reads of them, static member reads of them, and
/// literals. Reads cover both shapes: the raw value (`row`, `row.id`,
/// `index`) and the accessor (`row()`, `row().id`, `index()`).
fn is_row_path(
    e: &Expression<'_>,
    params: &[SymbolId],
    scoping: &Scoping,
    reads_param: &mut bool,
) -> bool {
    match e.without_parentheses() {
        Expression::Identifier(id) => {
            let is_param = id
                .reference_id
                .get()
                .and_then(|r| scoping.get_reference(r).symbol_id())
                .is_some_and(|symbol| params.contains(&symbol));
            *reads_param |= is_param;
            is_param
        }
        Expression::CallExpression(call) => {
            let Expression::Identifier(id) = &call.callee else { return false };
            let is_param = id
                .reference_id
                .get()
                .and_then(|r| scoping.get_reference(r).symbol_id())
                .is_some_and(|symbol| params.contains(&symbol));
            let is_param_call = is_param
                && call.arguments.is_empty()
                && !call.optional
                && call.type_arguments.is_none();
            *reads_param |= is_param_call;
            is_param_call
        }
        Expression::StaticMemberExpression(member) => {
            !member.optional && is_row_path(&member.object, params, scoping, reads_param)
        }
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        _ => false,
    }
}
