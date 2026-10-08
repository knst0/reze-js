use std::collections::{HashMap, HashSet};

use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, SPAN, Span};
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use oxc_allocator::{Allocator, Box as ArenaBox, TakeIn, Vec as ArenaVec};
use oxc_ast_visit::{VisitMut, walk_mut};

use super::Namer;
use super::imports::{ImportResult, Syntax, SyntaxImports};
use super::pure::is_component_name;
use crate::diagnostic::{Code, Edit, Report};
use crate::module_facts::ImportedGetters;

pub struct DeclPlan {
    pub primitive: Syntax,
    pub setter: Option<String>,
    pub keyword: Option<Span>,
}

#[derive(Default)]
pub struct PreScan {
    pub run: String,
    pub decls: HashMap<u32, DeclPlan>,
    pub by_symbol: HashMap<SymbolId, u32>,
    pub syntax_inits: HashSet<(u32, u32)>,
    pub action_callees: HashSet<(u32, u32)>,
    pub discarded: HashSet<u32>,
}

pub fn prescan(
    program: &Program<'_>,
    scoping: &Scoping,
    syntax: &SyntaxImports,
    namer: &mut Namer,
    reports: &mut Vec<Report>,
) -> PreScan {
    if syntax.declared.is_empty() && syntax.namespaces.is_empty() {
        return PreScan::default();
    }
    let mut declarations = Declarations {
        scoping,
        syntax,
        namer,
        variables: HashMap::new(),
        declarators: HashMap::new(),
        consumed: HashSet::new(),
        action_callees: HashSet::new(),
        reports: Vec::new(),
        in_for_init: false,
        patterns: Vec::new(),
        has_action: false,
    };
    declarations.visit_program(program);
    let Declarations {
        variables,
        declarators,
        reports: decl_reports,
        action_callees,
        patterns,
        has_action,
        ..
    } = declarations;
    reports.extend(decl_reports);
    resolve_patterns(program, scoping, patterns, reports);
    let mut decls = HashMap::new();
    let mut by_symbol = HashMap::new();
    let mut inits = HashSet::new();
    let default_exported = super::analysis::default_exported_symbols(program, scoping);
    for (symbol, variable) in &variables {
        if default_exported.contains(symbol) {
            reports.push(
                Report::new(Code::SignalDefaultExport, variable.declarator)
                    .arg("signal", scoping.symbol_name(*symbol))
                    .arg("primitive", variable.primitive.name()),
            );
        }
        if let Some(plan) = declarators.get(&variable.declarator.start) {
            decls.insert(
                variable.declarator.start,
                DeclPlan {
                    primitive: variable.primitive,
                    setter: variable.setter.clone(),
                    keyword: plan.keyword,
                },
            );
            by_symbol.insert(*symbol, variable.declarator.start);
            inits.insert((variable.init.start, variable.init.end));
        }
    }
    let mut marker = TopMarker { discarded: HashSet::new() };
    marker.visit_program(program);
    let run = if has_action { namer.fresh("_a$") } else { String::new() };
    PreScan {
        run,
        decls,
        by_symbol,
        syntax_inits: inits,
        action_callees,
        discarded: marker.discarded,
    }
}
struct Reactive {
    declarator: Span,
    init: Span,
    primitive: Syntax,
    setter: Option<String>,
}

struct Declarator {
    keyword: Option<Span>,
}

struct PatternPlan {
    pattern: Span,
    name: String,
    getter: SymbolId,
    setter: Option<SymbolId>,
    keyword: Option<Span>,
}

struct PendingPattern {
    span: Span,
    primitive: Syntax,
    plan: Option<PatternPlan>,
}

fn pattern_plan(
    declaration: &VariableDeclaration<'_>,
    declarator: &VariableDeclarator<'_>,
    primitive: Syntax,
) -> Option<PatternPlan> {
    let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
    if pattern.rest.is_some() || pattern.elements.len() > 2 {
        return None;
    }
    let Some(Some(BindingPattern::BindingIdentifier(getter))) = pattern.elements.first() else {
        return None;
    };
    let setter = match pattern.elements.get(1) {
        None => None,
        Some(Some(BindingPattern::BindingIdentifier(setter))) if primitive == Syntax::Signal => {
            Some(setter.symbol_id())
        }
        Some(_) => return None,
    };
    let keyword = (declaration.kind == VariableDeclarationKind::Const)
        .then(|| {
            (declaration.declarations.len() == 1).then(|| Span::sized(declaration.span.start, 5))
        })
        .flatten();
    if declaration.kind == VariableDeclarationKind::Const && keyword.is_none() && setter.is_some() {
        return None;
    }
    Some(PatternPlan {
        pattern: declarator.id.span(),
        name: getter.name.to_string(),
        getter: getter.symbol_id(),
        setter,
        keyword,
    })
}

struct CallSite {
    call: Span,
    is_optional: bool,
    argument_count: usize,
    sole_value_argument: Option<Span>,
    is_statement: bool,
}

#[derive(Default)]
struct CallSites {
    calls: HashMap<ReferenceId, CallSite>,
    statements: HashSet<u32>,
}

impl<'a> Visit<'a> for CallSites {
    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let Some(Expression::CallExpression(call)) =
            it.get_expression().map(Expression::without_parentheses)
        {
            self.statements.insert(call.span.start);
        }
        walk::walk_arrow_function_expression(self, it);
    }

    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        if let Expression::CallExpression(call) = it.expression.without_parentheses() {
            self.statements.insert(call.span.start);
        }
        walk::walk_expression_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &it.callee
            && let Some(reference) = callee.reference_id.get()
        {
            let sole_value_argument = match it.arguments.as_slice() {
                [argument] => argument
                    .as_expression()
                    .filter(|expression| {
                        !matches!(
                            expression.without_parentheses(),
                            Expression::ArrowFunctionExpression(_)
                                | Expression::FunctionExpression(_)
                        )
                    })
                    .map(GetSpan::span),
                _ => None,
            };
            self.calls.insert(
                reference,
                CallSite {
                    call: it.span,
                    is_optional: it.optional,
                    argument_count: it.arguments.len(),
                    sole_value_argument,
                    is_statement: self.statements.contains(&it.span.start),
                },
            );
        }
        walk::walk_call_expression(self, it);
    }
}

fn pattern_edits(plan: &PatternPlan, sites: &CallSites, scoping: &Scoping) -> Option<Vec<Edit>> {
    let edit = |span: Span, text: String| Edit { start: span.start, end: span.end, text };
    let mut edits = vec![edit(plan.pattern, plan.name.clone())];
    for reference in scoping.get_resolved_reference_ids(plan.getter) {
        let site = sites.calls.get(reference)?;
        if site.argument_count != 0 || site.is_optional {
            return None;
        }
        edits.push(edit(site.call, plan.name.clone()));
    }
    if let Some(setter) = plan.setter {
        let references = scoping.get_resolved_reference_ids(setter);
        for reference in references {
            let site = sites.calls.get(reference)?;
            let argument = site.sole_value_argument?;
            if !site.is_statement || site.is_optional {
                return None;
            }
            edits.push(edit(
                Span::new(site.call.start, argument.start),
                format!("{} = ", plan.name),
            ));
            edits.push(edit(Span::new(argument.end, site.call.end), String::new()));
        }
        if !references.is_empty()
            && let Some(keyword) = plan.keyword
        {
            edits.push(edit(keyword, String::from("let")));
        }
    }
    Some(edits)
}

fn resolve_patterns(
    program: &Program<'_>,
    scoping: &Scoping,
    patterns: Vec<PendingPattern>,
    reports: &mut Vec<Report>,
) {
    if patterns.is_empty() {
        return;
    }
    let mut sites = CallSites::default();
    sites.visit_program(program);
    for pattern in patterns {
        let name = pattern.plan.as_ref().map_or("name", |plan| plan.name.as_str());
        let mut report = Report::new(Code::SignalPattern, pattern.span)
            .arg("primitive", pattern.primitive.name())
            .arg("name", name);
        if let Some(plan) = &pattern.plan
            && let Some(edits) = pattern_edits(plan, &sites, scoping)
        {
            report = report.fix(edits);
        }
        reports.push(report);
    }
}

struct Declarations<'p, 's, 'a> {
    scoping: &'s Scoping,
    syntax: &'p SyntaxImports,
    namer: &'p mut Namer<'a>,
    variables: HashMap<SymbolId, Reactive>,
    declarators: HashMap<u32, Declarator>,
    consumed: HashSet<u32>,
    action_callees: HashSet<(u32, u32)>,
    reports: Vec<Report>,
    in_for_init: bool,
    patterns: Vec<PendingPattern>,
    has_action: bool,
}

fn callee_syntax(
    scoping: &Scoping,
    syntax: &SyntaxImports,
    callee: &Expression<'_>,
) -> Option<Syntax> {
    match callee.without_parentheses() {
        Expression::Identifier(id) => symbol_of(scoping, id).and_then(|s| syntax.syntax_of(s)),
        Expression::StaticMemberExpression(member) => namespace_member(scoping, syntax, member),
        _ => None,
    }
}

fn misused(primitive: Syntax, span: Span) -> Report {
    match primitive {
        Syntax::Action => Report::new(Code::ActionNotCalled, span),
        _ => Report::new(Code::SignalNotDeclared, span).arg("primitive", primitive.name()),
    }
}

impl Declarations<'_, '_, '_> {
    fn is_written(&self, symbol: SymbolId) -> bool {
        self.scoping
            .get_resolved_reference_ids(symbol)
            .iter()
            .any(|&r| self.scoping.get_reference(r).flags().is_write())
    }

    fn declare(
        &mut self,
        declaration: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
    ) {
        let Some(init) = &declarator.init else { return };
        let Some(call) = syntax_call(init) else { return };
        let Some(primitive) = callee_syntax(self.scoping, self.syntax, &call.callee) else {
            return;
        };
        if primitive == Syntax::Action {
            return;
        }
        self.consumed.insert(call.callee.without_parentheses().span().start);
        if !matches!(
            declaration.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Const
        ) {
            self.reports.push(
                Report::new(Code::SignalNotDeclared, call.span).arg("primitive", primitive.name()),
            );
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            let plan = pattern_plan(declaration, declarator, primitive);
            self.patterns.push(PendingPattern { span: declarator.id.span(), primitive, plan });
            return;
        };
        let symbol = id.symbol_id();
        let name = self.scoping.symbol_name(symbol);
        let setter = (primitive == Syntax::Signal && self.is_written(symbol)).then(|| {
            let mut base = String::from("set");
            let mut chars = name.chars();
            base.extend(chars.next().map(|c| c.to_ascii_uppercase()));
            base.push_str(chars.as_str());
            self.namer.fresh(&base)
        });
        let is_sole = declaration.declarations.len() == 1;
        let keyword =
            (is_sole && !self.in_for_init && declaration.kind == VariableDeclarationKind::Let)
                .then(|| Span::sized(declaration.span.start, 3));
        self.variables.insert(
            symbol,
            Reactive { declarator: declarator.span, init: call.span, primitive, setter },
        );
        self.declarators.insert(declarator.span.start, Declarator { keyword });
    }
}

fn syntax_call<'e, 'a>(init: &'e Expression<'a>) -> Option<&'e CallExpression<'a>> {
    match init.without_parentheses() {
        Expression::CallExpression(call) => Some(call),
        _ => None,
    }
}

fn symbol_of(scoping: &Scoping, id: &IdentifierReference<'_>) -> Option<SymbolId> {
    scoping.get_reference(id.reference_id.get()?).symbol_id()
}

fn namespace_member(
    scoping: &Scoping,
    syntax: &SyntaxImports,
    member: &StaticMemberExpression<'_>,
) -> Option<Syntax> {
    if member.optional {
        return None;
    }
    let primitive = [Syntax::Signal, Syntax::Computed, Syntax::Action]
        .into_iter()
        .find(|syntax| syntax.name() == member.property.name.as_str())?;
    let Expression::Identifier(object) = &member.object else { return None };
    let symbol = symbol_of(scoping, object)?;
    let source = syntax.namespaces.get(&symbol)?;
    (crate::exports::syntax_named(source, primitive.name()) == Some(primitive)).then_some(primitive)
}

impl<'a> Visit<'a> for Declarations<'_, '_, '_> {
    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        if let Some(init) = &it.init {
            let outer = std::mem::replace(&mut self.in_for_init, true);
            self.visit_for_statement_init(init);
            self.in_for_init = outer;
        }
        if let Some(test) = &it.test {
            self.visit_expression(test);
        }
        if let Some(update) = &it.update {
            self.visit_expression(update);
        }
        self.visit_statement(&it.body);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        for declarator in &it.declarations {
            self.declare(it, declarator);
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if callee_syntax(self.scoping, self.syntax, &it.callee) == Some(Syntax::Action) {
            let callee = it.callee.without_parentheses().span();
            self.consumed.insert(callee.start);
            self.action_callees.insert((callee.start, callee.end));
            self.has_action = true;
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(primitive) = symbol_of(self.scoping, it).and_then(|s| self.syntax.syntax_of(s))
            && !self.consumed.contains(&it.span.start)
        {
            self.reports.push(misused(primitive, it.span));
        }
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some(primitive) = namespace_member(self.scoping, self.syntax, it) {
            if !self.consumed.contains(&it.span.start) {
                self.reports.push(misused(primitive, it.span));
            }
            return;
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_ts_type(&mut self, _: &TSType<'a>) {}
}

pub fn scan(
    program: &Program<'_>,
    scoping: &Scoping,
    pre: &PreScan,
    source: &str,
    reports: &mut Vec<Report>,
) {
    if pre.decls.is_empty() && pre.action_callees.is_empty() {
        return;
    }
    let mut collect = DeclCollect { pre, map: HashMap::new() };
    collect.visit_program(program);
    let mut marker = TopMarker { discarded: HashSet::new() };
    marker.visit_program(program);
    let mut analyzer = Analyzer {
        scoping,
        source,
        pre,
        decls: collect.map,
        discarded: marker.discarded,
        frame: None,
        later: HashMap::new(),
        component_inits: HashSet::new(),
        reports: Vec::new(),
    };
    analyzer.visit_program(program);
    reports.extend(analyzer.reports);
}

struct DeclCollect<'p> {
    pre: &'p PreScan,
    map: HashMap<SymbolId, ReactiveInfo>,
}

struct ReactiveInfo {
    primitive: Syntax,
}
impl<'a> Visit<'a> for DeclCollect<'_> {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(plan) = self.pre.decls.get(&it.span.start)
            && let Some(init) = &it.init
            && let Some(call) = syntax_call(init)
            && self.pre.syntax_inits.contains(&(call.span.start, call.span.end))
        {
            let symbol = match plan.primitive {
                Syntax::Signal => match &it.id {
                    BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                    BindingPattern::ArrayPattern(pattern) => match pattern.elements.first() {
                        Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
                        _ => None,
                    },
                    _ => None,
                },
                Syntax::Computed => match &it.id {
                    BindingPattern::BindingIdentifier(id) => Some(id.symbol_id()),
                    _ => None,
                },
                Syntax::Action => None,
            };
            if let Some(symbol) = symbol {
                self.map.insert(symbol, ReactiveInfo { primitive: plan.primitive });
            }
        }
        walk::walk_variable_declarator(self, it);
    }
}

struct TopMarker {
    discarded: HashSet<u32>,
}

impl TopMarker {
    fn mark_discarded(&mut self, e: &Expression<'_>) {
        self.discarded.insert(e.span().start);
        match e {
            Expression::ParenthesizedExpression(inner) => self.mark_discarded(&inner.expression),
            Expression::SequenceExpression(sequence) => {
                for element in &sequence.expressions {
                    self.mark_discarded(element);
                }
            }
            Expression::LogicalExpression(logical) => self.mark_discarded(&logical.right),
            Expression::ConditionalExpression(conditional) => {
                self.mark_discarded(&conditional.consequent);
                self.mark_discarded(&conditional.alternate);
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for TopMarker {
    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        self.mark_discarded(&it.expression);
        walk::walk_expression_statement(self, it);
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        if let Some(init) = it.init.as_ref().and_then(ForStatementInit::as_expression) {
            self.mark_discarded(init);
        }
        if let Some(update) = &it.update {
            self.mark_discarded(update);
        }
        walk::walk_for_statement(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        if it.operator == UnaryOperator::Void {
            self.mark_discarded(&it.argument);
        }
        walk::walk_unary_expression(self, it);
    }
}

#[derive(Default)]
struct ActionFrame {
    nested: u32,
    later: Option<&'static str>,
}

struct Analyzer<'p, 's> {
    scoping: &'s Scoping,
    source: &'p str,
    pre: &'p PreScan,
    decls: HashMap<SymbolId, ReactiveInfo>,
    discarded: HashSet<u32>,
    frame: Option<ActionFrame>,
    later: HashMap<u32, &'static str>,
    component_inits: HashSet<u32>,
    reports: Vec<Report>,
}

impl Analyzer<'_, '_> {
    fn reactive_of(&self, id: &IdentifierReference<'_>) -> Option<(SymbolId, Syntax)> {
        let symbol = symbol_of(self.scoping, id)?;
        self.decls.get(&symbol).map(|info| (symbol, info.primitive))
    }

    fn name(&self, symbol: SymbolId) -> &str {
        self.scoping.symbol_name(symbol)
    }

    fn computed_written(&mut self, span: Span, symbol: SymbolId) {
        self.reports
            .push(Report::new(Code::ComputedWritten, span).arg("computed", self.name(symbol)));
    }

    fn update(&mut self, it: &UpdateExpression<'_>, symbol: SymbolId) {
        if !self.discarded.contains(&it.span.start) {
            self.reports.push(
                Report::new(Code::SignalUpdateInExpression, it.span)
                    .arg("signal", self.name(symbol)),
            );
            return;
        }
    }

    fn action(&mut self, call: &CallExpression<'_>) {
        let mut arguments = call.arguments.iter();
        let body = arguments.next();
        match body.and_then(Argument::as_expression).map(Expression::without_parentheses) {
            Some(Expression::ArrowFunctionExpression(arrow)) => {
                self.action_arrow(&arrow.params, &arrow.body);
            }
            Some(Expression::FunctionExpression(function)) if function.generator => {
                self.reports.push(
                    Report::new(Code::ActionUnsupported, function.span)
                        .arg("construct", "generator"),
                );
            }
            Some(Expression::FunctionExpression(function)) => {
                self.action_function(&function.params, function.body.as_deref());
            }
            _ => {
                let span = body.map_or(call.span, GetSpan::span);
                self.reports.push(Report::new(Code::ActionArgument, span));
                if let Some(body) = body {
                    self.visit_argument(body);
                }
            }
        }
        for argument in arguments {
            self.visit_argument(argument);
        }
    }

    fn action_arrow(&mut self, params: &FormalParameters<'_>, body: &ArrowFunctionBody<'_>) {
        let outer = self.frame.replace(ActionFrame::default());
        self.visit_formal_parameters(params);
        match body {
            ArrowFunctionBody::FunctionBody(block) => self.action_block(block),
            _ => {
                if let Some(expression) = body.as_expression() {
                    self.visit_expression(expression);
                }
            }
        }
        self.frame = outer;
    }

    fn action_function(&mut self, params: &FormalParameters<'_>, body: Option<&FunctionBody<'_>>) {
        let outer = self.frame.replace(ActionFrame::default());
        self.visit_formal_parameters(params);
        if let Some(block) = body {
            self.action_block(block);
        }
        self.frame = outer;
    }

    fn action_block(&mut self, body: &FunctionBody<'_>) {
        for statement in &body.statements {
            self.visit_statement(statement);
        }
    }

    fn enter(&mut self, start: u32, is_async: bool) -> Option<Option<&'static str>> {
        let later = self.later.get(&start).copied();
        let frame = self.frame.as_mut()?;
        let outer = frame.later;
        frame.nested += 1;
        frame.later = outer.or(later).or(is_async.then_some("an async function"));
        Some(outer)
    }

    fn leave(&mut self, outer: Option<Option<&'static str>>) {
        if let (Some(frame), Some(later)) = (self.frame.as_mut(), outer) {
            frame.nested -= 1;
            frame.later = later;
        }
    }

    fn in_action_body(&self) -> bool {
        self.frame.as_ref().is_some_and(|frame| frame.nested == 0)
    }

    fn check_nested_write(&mut self, span: Span, target_is_member: bool) {
        if let (true, Some(via)) = (target_is_member, self.frame.as_ref().and_then(|f| f.later)) {
            self.reports.push(Report::new(Code::ActionNestedWrite, span).arg("via", via));
        }
    }

    fn computed_value(&mut self, value: &Expression<'_>) {
        match value.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                let mut report = Report::new(Code::ComputedFunction, arrow.span);
                if let (false, true, Some(body)) =
                    (arrow.r#async, arrow.params.is_empty(), arrow.get_expression())
                {
                    let edit = Edit {
                        start: arrow.span.start,
                        end: body.span().start,
                        text: String::new(),
                    };
                    report = report.fix(vec![edit]);
                }
                self.reports.push(report);
            }
            Expression::FunctionExpression(function) => {
                self.reports.push(Report::new(Code::ComputedFunction, function.span));
            }
            _ => {
                if suspends(value) {
                    self.reports.push(Report::new(Code::ComputedAwait, value.span()));
                }
            }
        }
    }

    fn check_read_once(&mut self, body: &FunctionBody<'_>) {
        for statement in &body.statements {
            let Statement::VariableDeclaration(declaration) = statement else { continue };
            for declarator in &declaration.declarations {
                let Some(init) = &declarator.init else { continue };
                if self.pre.decls.contains_key(&declarator.span.start) {
                    continue;
                }
                let mut first = FirstRead { analyzer: self, found: None };
                first.visit_expression(init);
                let Some((span, symbol)) = first.found else { continue };
                let variable = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                    pattern => self.source
                        [pattern.span().start as usize..pattern.span().end as usize]
                        .to_string(),
                };
                let report = Report::new(Code::SignalReadOnce, span)
                    .arg("variable", variable)
                    .arg("signal", self.name(symbol));
                self.reports.push(report);
            }
        }
    }
}

struct FirstRead<'r, 'p, 's> {
    analyzer: &'r Analyzer<'p, 's>,
    found: Option<(Span, SymbolId)>,
}

impl<'a> Visit<'a> for FirstRead<'_, '_, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if self.found.is_none()
            && let Some((symbol, _)) = self.analyzer.reactive_of(it)
            && let Some(reference) = it.reference_id.get()
            && !self.analyzer.scoping.get_reference(reference).flags().is_write()
        {
            self.found = Some((it.span, symbol));
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &Class<'a>) {}

    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {}

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {}

    fn visit_ts_type(&mut self, _: &TSType<'a>) {}
}

impl<'a> Visit<'a> for Analyzer<'_, '_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let callee = it.callee.without_parentheses().span();
        if self.pre.action_callees.contains(&(callee.start, callee.end)) {
            self.action(it);
            return;
        }
        if let (true, Some(via)) = (self.frame.is_some(), runs_later(&it.callee)) {
            for argument in &it.arguments {
                if let Some(
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_),
                ) = argument.as_expression().map(Expression::without_parentheses)
                {
                    self.later.insert(argument.span().start, via);
                }
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if it.r#await && self.in_action_body() {
            let span = Span::sized(it.span.start, 9);
            self.reports
                .push(Report::new(Code::ActionUnsupported, span).arg("construct", "for await"));
        }
        walk::walk_for_of_statement(self, it);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if it.kind == VariableDeclarationKind::AwaitUsing && self.in_action_body() {
            let span = Span::sized(it.span.start, 11);
            self.reports
                .push(Report::new(Code::ActionUnsupported, span).arg("construct", "await using"));
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(plan) = self.pre.decls.get(&it.span.start)
            && let Some(init) = &it.init
            && let Some(call) = syntax_call(init)
        {
            let mut arguments = call.arguments.iter();
            if plan.primitive == Syntax::Computed
                && let Some(value) = arguments.next()
            {
                match value.as_expression() {
                    Some(value) => {
                        if let Some(inner) = synthesized_computed_body(value) {
                            self.computed_value(inner);
                            self.visit_expression(inner);
                        } else {
                            self.computed_value(value);
                        }
                    }
                    None => self.visit_argument(value),
                }
            }
            for argument in arguments {
                self.visit_argument(argument);
            }
            return;
        }
        if let (BindingPattern::BindingIdentifier(id), Some(init)) = (&it.id, &it.init)
            && is_component_name(id.name.as_str())
            && matches!(
                init.without_parentheses(),
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            )
        {
            self.component_inits.insert(init.without_parentheses().span().start);
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let is_component = it.id.as_ref().is_some_and(|id| is_component_name(id.name.as_str()))
            || self.component_inits.contains(&it.span.start);
        if let (true, Some(body)) = (is_component, &it.body) {
            self.check_read_once(body);
        }
        let outer = self.enter(it.span.start, it.r#async);
        walk::walk_function(self, it, flags);
        self.leave(outer);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let ArrowFunctionBody::FunctionBody(body) = &it.body
            && self.component_inits.contains(&it.span.start)
        {
            self.check_read_once(body);
        }
        let outer = self.enter(it.span.start, it.r#async);
        walk::walk_arrow_function_expression(self, it);
        self.leave(outer);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(id) = &it.left {
            let found = self.reactive_of(id).map(|(symbol, primitive)| (symbol, primitive));
            if let Some((symbol, primitive)) = found {
                match primitive {
                    Syntax::Signal => {
                        self.visit_expression(&it.right);
                        return;
                    }
                    Syntax::Computed => {
                        self.computed_written(it.span, symbol);
                        return;
                    }
                    Syntax::Action => {}
                }
            }
        }
        self.check_nested_write(it.span, it.left.is_member_expression());
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &it.argument {
            let found = self.reactive_of(id).map(|(symbol, primitive)| (symbol, primitive));
            if let Some((symbol, primitive)) = found {
                match primitive {
                    Syntax::Signal => {
                        self.update(it, symbol);
                        return;
                    }
                    Syntax::Computed => {
                        self.computed_written(it.span, symbol);
                        return;
                    }
                    Syntax::Action => {}
                }
            }
        }
        self.check_nested_write(it.span, it.argument.is_member_expression());
        walk::walk_update_expression(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        if it.operator == UnaryOperator::Delete {
            self.check_nested_write(it.span, it.argument.is_member_expression());
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let Some((symbol, primitive)) =
            self.reactive_of(it).map(|(symbol, primitive)| (symbol, primitive))
        else {
            return;
        };
        let is_write =
            it.reference_id.get().is_some_and(|r| self.scoping.get_reference(r).flags().is_write());
        match (is_write, primitive) {
            (false, _) => {}
            (true, Syntax::Signal) => {
                self.reports.push(
                    Report::new(Code::SignalAssignPattern, it.span)
                        .arg("signal", self.name(symbol)),
                );
            }
            (true, Syntax::Computed) => {
                self.computed_written(it.span, symbol);
            }
            (true, Syntax::Action) => {}
        }
    }

    fn visit_jsx_element_name(&mut self, it: &JSXElementName<'a>) {
        if !matches!(it, JSXElementName::IdentifierReference(_)) {
            walk::walk_jsx_element_name(self, it);
        }
    }

    fn visit_ts_type(&mut self, _: &TSType<'a>) {}

    fn visit_import_declaration(&mut self, _: &ImportDeclaration<'a>) {}

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        walk::walk_static_member_expression(self, it);
    }
}

pub(crate) fn is_never_function(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::UnaryExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_) => true,
        Expression::AssignmentExpression(assignment) => match assignment.operator {
            AssignmentOperator::Assign => is_never_function(&assignment.right),
            AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalNullish => false,
            _ => true,
        },
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.last().is_some_and(is_never_function)
        }
        Expression::ConditionalExpression(conditional) => {
            is_never_function(&conditional.consequent) && is_never_function(&conditional.alternate)
        }
        _ => false,
    }
}

pub(crate) fn is_tight(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::Identifier(_)
            | Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::ThisExpression(_)
            | Expression::CallExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ParenthesizedExpression(_)
            | Expression::TemplateLiteral(_)
            | Expression::ArrayExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::UnaryExpression(_)
            | Expression::AwaitExpression(_)
            | Expression::ChainExpression(_)
            | Expression::TSNonNullExpression(_)
    )
}

#[derive(Default)]
struct Suspends(bool);

impl<'a> Visit<'a> for Suspends {
    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.0 = true;
    }
    fn visit_yield_expression(&mut self, _: &YieldExpression<'a>) {
        self.0 = true;
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

pub(crate) fn suspends(e: &Expression<'_>) -> bool {
    let mut check = Suspends::default();
    check.visit_expression(e);
    check.0
}
fn synthesized_computed_body<'e, 'a>(value: &'e Expression<'a>) -> Option<&'e Expression<'a>> {
    let Expression::ArrowFunctionExpression(arrow) = value.without_parentheses() else {
        return None;
    };
    if arrow.r#async || !arrow.params.items.is_empty() || arrow.params.rest.is_some() {
        return None;
    }
    let body = arrow.get_expression()?;
    (arrow.span == SPAN).then_some(body)
}

pub(crate) fn block_awaits(block: &BlockStatement<'_>) -> bool {
    let mut check = Suspends::default();
    check.visit_block_statement(block);
    check.0
}

pub(crate) fn runs_later(callee: &Expression<'_>) -> Option<&'static str> {
    match callee.without_parentheses() {
        Expression::StaticMemberExpression(member) => match member.property.name.as_str() {
            "then" => Some("a `.then` callback"),
            "catch" => Some("a `.catch` callback"),
            "finally" => Some("a `.finally` callback"),
            _ => None,
        },
        Expression::Identifier(id) => match id.name.as_str() {
            "setTimeout" => Some("a `setTimeout` callback"),
            "setInterval" => Some("a `setInterval` callback"),
            "queueMicrotask" => Some("a `queueMicrotask` callback"),
            "requestAnimationFrame" => Some("a `requestAnimationFrame` callback"),
            "requestIdleCallback" => Some("a `requestIdleCallback` callback"),
            _ => None,
        },
        _ => None,
    }
}

fn init_call_mut<'e, 'a>(init: &'e mut Expression<'a>) -> Option<&'e mut CallExpression<'a>> {
    let mut current = init;
    loop {
        match current {
            Expression::CallExpression(call) => return Some(call),
            Expression::ParenthesizedExpression(inner) => {
                current = &mut inner.expression;
            }
            _ => return None,
        }
    }
}
fn dummy<'a>(alloc: &'a Allocator, span: Span) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_null_literal(span, &builder)
}

fn take<'a>(alloc: &'a Allocator, it: &mut Expression<'a>) -> Expression<'a> {
    let span = it.span();
    std::mem::replace(it, dummy(alloc, span))
}

fn ident_expr<'a>(alloc: &'a Allocator, span: Span, name: &'a str) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_identifier(span, name, &builder)
}

fn call_expr<'a>(
    alloc: &'a Allocator,
    span: Span,
    callee: Expression<'a>,
    args: ArenaVec<'a, Argument<'a>>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_call_expression(span, callee, None, args, false, &builder)
}

fn getter_call<'a>(alloc: &'a Allocator, span: Span, name: &'a str) -> Expression<'a> {
    call_expr(alloc, span, ident_expr(alloc, span, name), ArenaVec::new_in(&alloc))
}

fn member_expr<'a>(
    alloc: &'a Allocator,
    span: Span,
    object: Expression<'a>,
    prop: &'static str,
    prop_span: Span,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_static_member_expression(
        span,
        object,
        IdentifierName::new(prop_span, prop, &builder),
        false,
        &builder,
    )
}

fn empty_params<'a>(alloc: &'a Allocator, span: Span) -> ArenaBox<'a, FormalParameters<'a>> {
    let builder = AstBuilder::new(alloc);
    FormalParameters::boxed(
        span,
        FormalParameterKind::ArrowFormalParameters,
        ArenaVec::new_in(&alloc),
        None,
        &builder,
    )
}

fn arrow_expr<'a>(
    alloc: &'a Allocator,
    span: Span,
    params_span: Span,
    body: Expression<'a>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_arrow_function_expression(
        span,
        false,
        None,
        empty_params(alloc, params_span),
        None,
        ArrowFunctionBody::from(body),
        &builder,
    )
}

fn run_param<'a>(alloc: &'a Allocator, span: Span, run: &'a str) -> FormalParameter<'a> {
    let builder = AstBuilder::new(alloc);
    FormalParameter::new(
        span,
        ArenaVec::new_in(&alloc),
        BindingPattern::new_binding_identifier(span, run, &builder),
        None,
        None,
        false,
        None,
        false,
        false,
        &builder,
    )
}

fn expr_statement<'a>(
    alloc: &'a Allocator,
    span: Span,
    expression: Expression<'a>,
) -> Statement<'a> {
    let builder = AstBuilder::new(alloc);
    Statement::new_expression_statement(span, expression, &builder)
}

fn dummy_pat<'a>(alloc: &'a Allocator) -> BindingPattern<'a> {
    let builder = AstBuilder::new(alloc);
    let empty: &'a str = alloc.alloc_str("");
    BindingPattern::new_binding_identifier(Span::empty(0), empty, &builder)
}

pub(crate) fn normalize_ast<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    scoping: &Scoping,
    syntax: &SyntaxImports,
    pre: &PreScan,
    outcome: &ImportResult<'a>,
    imported_getters: &ImportedGetters,
    source: &str,
) -> bool {
    if pre.decls.is_empty() && pre.action_callees.is_empty() && imported_getters.is_empty() {
        return false;
    }
    let mut norm = Normalizer {
        alloc: allocator,
        scoping,
        syntax,
        pre,
        source,
        targets: &outcome.targets,
        locals: &outcome.locals,
        imported_getters,
        action_depth: None,
        changed: false,
    };
    let split = split_exported_setters(allocator, program, pre);
    norm.visit_program(program);
    split || norm.changed
}

fn split_exported_setters<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    pre: &PreScan,
) -> bool {
    let builder = AstBuilder::new(allocator);
    let body = std::mem::replace(&mut program.body, ArenaVec::new_in(&builder));
    let mut changed = false;
    for statement in body {
        let Statement::ExportDeclaration(export) = statement else {
            program.body.push(statement);
            continue;
        };
        let export = export.unbox();
        let span = export.span;
        match export.declaration {
            Declaration::VariableDeclaration(decl) if declares_setter(&decl, pre) => {
                let mut specifiers = ArenaVec::new_in(&builder);
                for declarator in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        specifiers.push(ExportSpecifier::new(
                            SPAN,
                            ModuleExportName::new_identifier_reference(SPAN, id.name, &builder),
                            ModuleExportName::new_identifier_name(SPAN, id.name, &builder),
                            ImportOrExportKind::Value,
                            &builder,
                        ));
                    }
                }
                program.body.push(Statement::VariableDeclaration(decl));
                program.body.push(Statement::new_export_named_declaration(
                    SPAN,
                    specifiers,
                    ImportOrExportKind::Value,
                    &builder,
                ));
                changed = true;
            }
            declaration => {
                program.body.push(Statement::new_export_declaration(span, declaration, &builder));
            }
        }
    }
    changed
}

fn declares_setter(declaration: &VariableDeclaration<'_>, pre: &PreScan) -> bool {
    declaration.declarations.iter().all(|d| matches!(d.id, BindingPattern::BindingIdentifier(_)))
        && declaration
            .declarations
            .iter()
            .any(|d| pre.decls.get(&d.span.start).is_some_and(|plan| plan.setter.is_some()))
}

struct Normalizer<'x, 'p, 's, 'a> {
    alloc: &'a Allocator,
    scoping: &'s Scoping,
    syntax: &'p SyntaxImports,
    pre: &'p PreScan,
    source: &'p str,
    targets: &'x HashMap<SymbolId, &'a str>,
    locals: &'x [Option<&'a str>; 3],
    imported_getters: &'x ImportedGetters,
    action_depth: Option<u32>,
    changed: bool,
}

impl<'x, 'p, 's, 'a> Normalizer<'x, 'p, 's, 'a> {
    fn decl_plan(&self, symbol: SymbolId) -> Option<&DeclPlan> {
        let start = self.pre.by_symbol.get(&symbol)?;
        self.pre.decls.get(start)
    }

    fn runtime_local(&self, callee: &Expression<'_>) -> Option<&'a str> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => {
                let symbol = symbol_of(self.scoping, id)?;
                self.targets.get(&symbol).copied()
            }
            Expression::StaticMemberExpression(member) => {
                let syntax = namespace_member(self.scoping, self.syntax, member)?;
                self.locals[syntax as usize]
            }
            _ => None,
        }
    }

    fn norm_declarator(&mut self, it: &mut VariableDeclarator<'a>) {
        let Some(plan) = self.pre.decls.get(&it.span.start) else { return };
        let (primitive, setter) = (plan.primitive, plan.setter.clone());
        match primitive {
            Syntax::Signal => self.signal_declarator(it, setter.as_deref()),
            Syntax::Computed => self.computed_declarator(it),
            Syntax::Action => {}
        }
    }

    fn rename_callee(&mut self, call: &mut CallExpression<'a>) -> bool {
        let Some(runtime) = self.runtime_local(&call.callee) else { return false };
        let span = call.callee.without_parentheses().span();
        call.callee = ident_expr(self.alloc, span, runtime);
        self.changed = true;
        true
    }

    fn move_annotation(
        &mut self,
        annotation: &mut Option<ArenaBox<'a, TSTypeAnnotation<'a>>>,
        call: &mut CallExpression<'a>,
    ) {
        if call.type_arguments.is_some() {
            return;
        }
        let Some(annotation) = annotation.take() else { return };
        let annotation = annotation.unbox();
        let span = annotation.span;
        let builder = AstBuilder::new(self.alloc);
        call.type_arguments = Some(TSTypeParameterInstantiation::boxed(
            span,
            ArenaVec::from_iter_in([annotation.type_annotation], &builder),
            &builder,
        ));
        self.changed = true;
    }

    fn signal_declarator(&mut self, it: &mut VariableDeclarator<'a>, setter: Option<&str>) {
        let Some(init) = it.init.as_mut() else { return };
        let Some(call) = init_call_mut(init) else { return };
        if !self.pre.syntax_inits.contains(&(call.span.start, call.span.end)) {
            return;
        }
        if !self.rename_callee(call) {
            return;
        }
        self.move_annotation(&mut it.type_annotation, call);
        let old_id = std::mem::replace(&mut it.id, dummy_pat(self.alloc));
        let BindingPattern::BindingIdentifier(id) = old_id else {
            it.id = old_id;
            return;
        };
        let span = id.span;
        let builder = AstBuilder::new(self.alloc);
        let mut elements = ArenaVec::new_in(&builder);
        elements.push(Some(BindingPattern::BindingIdentifier(id)));
        if let Some(setter) = setter {
            let text: &'a str = self.alloc.alloc_str(setter);
            elements.push(Some(BindingPattern::new_binding_identifier(SPAN, text, &builder)));
        }
        it.id = BindingPattern::new_array_pattern(span, elements, None, &builder);
        self.changed = true;
        for argument in call.arguments.iter_mut() {
            self.visit_argument(argument);
        }
    }

    fn computed_declarator(&mut self, it: &mut VariableDeclarator<'a>) {
        let Some(init) = it.init.as_mut() else { return };
        let Some(call) = init_call_mut(init) else { return };
        if !self.pre.syntax_inits.contains(&(call.span.start, call.span.end)) {
            return;
        }
        if !self.rename_callee(call) {
            return;
        }
        self.move_annotation(&mut it.type_annotation, call);
        for argument in call.arguments.iter_mut() {
            self.visit_argument(argument);
        }
        let taken = std::mem::replace(&mut call.arguments, ArenaVec::new_in(&self.alloc));
        let mut into = taken.into_iter();
        let Some(first) = into.next() else { return };
        if first.as_expression().is_none() {
            let mut args = ArenaVec::new_in(&self.alloc);
            args.push(first);
            for argument in into {
                args.push(argument);
            }
            call.arguments = args;
            return;
        }
        let Ok(value) = Expression::try_from(first) else {
            let mut args = ArenaVec::new_in(&self.alloc);
            for argument in into {
                args.push(argument);
            }
            call.arguments = args;
            return;
        };
        match value.without_parentheses() {
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                let mut args = ArenaVec::new_in(&self.alloc);
                args.push(Argument::from(value));
                for argument in into {
                    args.push(argument);
                }
                call.arguments = args;
                return;
            }
            _ => {}
        }
        if suspends(&value) {
            let mut args = ArenaVec::new_in(&self.alloc);
            args.push(Argument::from(value));
            for argument in into {
                args.push(argument);
            }
            call.arguments = args;
            return;
        }
        let span = value.span();
        let text = &self.source[span.start as usize..span.end as usize];
        let body = if text.starts_with('{') {
            let builder = AstBuilder::new(self.alloc);
            Expression::new_parenthesized_expression(span, value, &builder)
        } else {
            value
        };
        let mut args = ArenaVec::new_in(&self.alloc);
        args.push(Argument::from(arrow_expr(self.alloc, SPAN, SPAN, body)));
        for argument in into {
            args.push(argument);
        }
        call.arguments = args;
        self.changed = true;
    }

    fn normalize_read(&mut self, it: &mut Expression<'a>) -> bool {
        let Expression::Identifier(id) = it else { return false };
        let Some(reference) = id.reference_id.get() else { return false };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else { return false };
        if self.decl_plan(symbol).is_none() && !self.imported_getters.bindings.contains(&symbol) {
            return false;
        }
        if self.scoping.get_reference(reference).flags().is_write() {
            return false;
        }
        let span = id.span;
        let text: &'a str = self.alloc.alloc_str(id.name.as_str());
        *it = getter_call(self.alloc, span, text);
        self.changed = true;
        true
    }

    fn normalize_namespace_read(&mut self, it: &mut Expression<'a>) -> bool {
        let Expression::StaticMemberExpression(member) = it else { return false };
        if member.optional {
            return false;
        }
        let Expression::Identifier(object) = &member.object else { return false };
        let Some(reference) = object.reference_id.get() else { return false };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else { return false };
        let getters = self.imported_getters;
        let Some(names) = getters.members.get(&symbol) else { return false };
        if !names.contains(member.property.name.as_str()) {
            return false;
        }
        let span = member.span;
        let read = it.take_in(&self.alloc);
        *it = call_expr(self.alloc, span, read, ArenaVec::new_in(&self.alloc));
        self.changed = true;
        true
    }

    fn normalize_assign(&mut self, it: &mut Expression<'a>) -> bool {
        let Expression::AssignmentExpression(assignment) = it else { return false };
        let AssignmentTarget::AssignmentTargetIdentifier(id) = &assignment.left else {
            return false;
        };
        let Some(reference) = id.reference_id.get() else { return false };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else { return false };
        let Some(plan) = self.decl_plan(symbol) else { return false };
        if plan.primitive != Syntax::Signal {
            return false;
        }
        let Some(setter) = plan.setter.clone() else { return false };
        let operator = assignment.operator;
        let span = assignment.span;
        enum Shape {
            Set,
            Logical(LogicalOperator),
            Compound(BinaryOperator),
        }
        let shape = match operator {
            AssignmentOperator::Assign => Shape::Set,
            AssignmentOperator::LogicalOr => Shape::Logical(LogicalOperator::Or),
            AssignmentOperator::LogicalAnd => Shape::Logical(LogicalOperator::And),
            AssignmentOperator::LogicalNullish => Shape::Logical(LogicalOperator::Coalesce),
            _ => match operator.as_str() {
                "+=" => Shape::Compound(BinaryOperator::Addition),
                "-=" => Shape::Compound(BinaryOperator::Subtraction),
                "*=" => Shape::Compound(BinaryOperator::Multiplication),
                "/=" => Shape::Compound(BinaryOperator::Division),
                "%=" => Shape::Compound(BinaryOperator::Remainder),
                "**=" => Shape::Compound(BinaryOperator::Exponential),
                "<<=" => Shape::Compound(BinaryOperator::ShiftLeft),
                ">>=" => Shape::Compound(BinaryOperator::ShiftRight),
                ">>>=" => Shape::Compound(BinaryOperator::ShiftRightZeroFill),
                "|=" => Shape::Compound(BinaryOperator::BitwiseOR),
                "^=" => Shape::Compound(BinaryOperator::BitwiseXOR),
                "&=" => Shape::Compound(BinaryOperator::BitwiseAnd),
                _ => return false,
            },
        };
        let taken = take(self.alloc, it);
        let Expression::AssignmentExpression(boxed) = taken else {
            *it = taken;
            return false;
        };
        let assignment = boxed.unbox();
        let builder = AstBuilder::new(self.alloc);
        let setter_text: &'a str = self.alloc.alloc_str(&setter);
        let getter_name: &'a str = self.alloc.alloc_str(self.scoping.symbol_name(symbol));
        let right = assignment.right;
        let rebuilt = match shape {
            Shape::Set => {
                let value = if !is_never_function(&right) && !suspends(&right) {
                    arrow_expr(self.alloc, SPAN, SPAN, right)
                } else {
                    right
                };
                let mut args = ArenaVec::new_in(&builder);
                args.push(Argument::from(value));
                call_expr(self.alloc, span, ident_expr(self.alloc, SPAN, setter_text), args)
            }
            Shape::Logical(op) => {
                let value = if !is_never_function(&right) && !suspends(&right) {
                    arrow_expr(self.alloc, SPAN, SPAN, right)
                } else {
                    right
                };
                let mut args = ArenaVec::new_in(&builder);
                args.push(Argument::from(value));
                let set =
                    call_expr(self.alloc, SPAN, ident_expr(self.alloc, SPAN, setter_text), args);
                Expression::new_logical_expression(
                    span,
                    getter_call(self.alloc, SPAN, getter_name),
                    op,
                    set,
                    &builder,
                )
            }
            Shape::Compound(binary) => {
                let right = if is_tight(&right) {
                    right
                } else {
                    let right_span = right.span();
                    Expression::new_parenthesized_expression(right_span, right, &builder)
                };
                let mut args = ArenaVec::new_in(&builder);
                args.push(Argument::from(Expression::new_binary_expression(
                    SPAN,
                    getter_call(self.alloc, SPAN, getter_name),
                    binary,
                    right,
                    &builder,
                )));
                call_expr(self.alloc, span, ident_expr(self.alloc, SPAN, setter_text), args)
            }
        };
        *it = rebuilt;
        self.changed = true;
        walk_mut::walk_expression(self, it);
        true
    }

    fn normalize_update(&mut self, it: &mut Expression<'a>) -> bool {
        let Expression::UpdateExpression(update) = it else { return false };
        if !self.pre.discarded.contains(&update.span.start) {
            return false;
        }
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &update.argument else {
            return false;
        };
        let Some(reference) = id.reference_id.get() else { return false };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else { return false };
        let Some(plan) = self.decl_plan(symbol) else { return false };
        if plan.primitive != Syntax::Signal {
            return false;
        }
        let Some(setter) = plan.setter.clone() else { return false };
        let span = update.span;
        let operator = update.operator;
        take(self.alloc, it);
        let builder = AstBuilder::new(self.alloc);
        let setter_text: &'a str = self.alloc.alloc_str(&setter);
        let getter_name: &'a str = self.alloc.alloc_str(self.scoping.symbol_name(symbol));
        let one = Expression::new_numeric_literal(SPAN, 1.0, None, NumberBase::Decimal, &builder);
        let binary_operator = if operator == UpdateOperator::Increment {
            BinaryOperator::Addition
        } else {
            BinaryOperator::Subtraction
        };
        let mut args = ArenaVec::new_in(&builder);
        args.push(Argument::from(Expression::new_binary_expression(
            SPAN,
            getter_call(self.alloc, SPAN, getter_name),
            binary_operator,
            one,
            &builder,
        )));
        *it = call_expr(self.alloc, span, ident_expr(self.alloc, SPAN, setter_text), args);
        self.changed = true;
        walk_mut::walk_expression(self, it);
        true
    }

    fn normalize_action(&mut self, call: &mut CallExpression<'a>) {
        let callee_span = call.callee.without_parentheses().span();
        if !self.pre.action_callees.contains(&(callee_span.start, callee_span.end)) {
            return;
        }
        let runtime = self.runtime_local(&call.callee);
        if let Some(runtime) = runtime {
            call.callee = ident_expr(self.alloc, callee_span, runtime);
            self.changed = true;
        }
        let taken = std::mem::replace(&mut call.arguments, ArenaVec::new_in(&self.alloc));
        let mut into = taken.into_iter();
        let Some(first) = into.next() else { return };
        let mut rest = ArenaVec::new_in(&self.alloc);
        for argument in into {
            rest.push(argument);
        }
        let body = first.as_expression().map(Expression::without_parentheses);
        match body {
            Some(Expression::ArrowFunctionExpression(_)) => {
                let Argument::ArrowFunctionExpression(arrow) = first else { return };
                self.action_arrow(call, arrow, rest);
            }
            Some(Expression::FunctionExpression(_)) => {
                let Argument::FunctionExpression(function) = first else { return };
                if function.generator {
                    let mut args = ArenaVec::new_in(&self.alloc);
                    args.push(Argument::FunctionExpression(function));
                    for argument in rest.into_iter() {
                        args.push(argument);
                    }
                    call.arguments = args;
                    return;
                }
                self.action_function(call, function, rest);
            }
            _ => {
                let mut args = ArenaVec::new_in(&self.alloc);
                args.push(first);
                for mut argument in rest.into_iter() {
                    self.visit_argument(&mut argument);
                    args.push(argument);
                }
                call.arguments = args;
            }
        }
    }

    fn action_arrow(
        &mut self,
        call: &mut CallExpression<'a>,
        mut arrow: ArenaBox<'a, ArrowFunctionExpression<'a>>,
        rest: ArenaVec<'a, Argument<'a>>,
    ) {
        let outer = self.action_depth.replace(0);
        let run_text: &'a str = self.alloc.alloc_str(&self.pre.run);
        let items = std::mem::replace(&mut arrow.params.items, ArenaVec::new_in(&self.alloc));
        let mut params = ArenaVec::new_in(&self.alloc);
        params.push(run_param(self.alloc, SPAN, run_text));
        for item in items {
            params.push(item);
        }
        arrow.params.items = params;
        self.visit_formal_parameters(&mut arrow.params);
        let params_span = arrow.params.span;
        match &mut arrow.body {
            ArrowFunctionBody::FunctionBody(block) => {
                self.action_block(block, run_text);
            }
            _ => {
                let builder = AstBuilder::new(self.alloc);
                let taken = std::mem::replace(
                    &mut arrow.body,
                    ArrowFunctionBody::FunctionBody(FunctionBody::boxed(
                        params_span,
                        ArenaVec::new_in(&builder),
                        ArenaVec::new_in(&builder),
                        &builder,
                    )),
                );
                let expression =
                    Expression::try_from(taken).unwrap_or_else(|_| dummy(self.alloc, params_span));
                let span = expression.span();
                let mut statements = ArenaVec::new_in(&builder);
                statements.push(Statement::new_return_statement(span, Some(expression), &builder));
                let mut block = FunctionBody::new(
                    params_span,
                    ArenaVec::new_in(&builder),
                    statements,
                    &builder,
                );
                self.action_block(&mut block, run_text);
                arrow.body = ArrowFunctionBody::FunctionBody(ArenaBox::new_in(block, &builder));
            }
        }
        let mut args = ArenaVec::new_in(&self.alloc);
        args.push(Argument::ArrowFunctionExpression(arrow));
        for mut argument in rest.into_iter() {
            self.visit_argument(&mut argument);
            args.push(argument);
        }
        call.arguments = args;
        self.action_depth = outer;
        self.changed = true;
    }

    fn action_function(
        &mut self,
        call: &mut CallExpression<'a>,
        mut function: ArenaBox<'a, Function<'a>>,
        rest: ArenaVec<'a, Argument<'a>>,
    ) {
        let outer = self.action_depth.replace(0);
        let run_text: &'a str = self.alloc.alloc_str(&self.pre.run);
        let items = std::mem::replace(&mut function.params.items, ArenaVec::new_in(&self.alloc));
        let mut params = ArenaVec::new_in(&self.alloc);
        params.push(run_param(self.alloc, SPAN, run_text));
        for item in items {
            params.push(item);
        }
        function.params.items = params;
        self.visit_formal_parameters(&mut function.params);
        if let Some(body) = function.body.as_mut() {
            self.action_block(body, run_text);
        }
        let mut args = ArenaVec::new_in(&self.alloc);
        args.push(Argument::FunctionExpression(function));
        for mut argument in rest.into_iter() {
            self.visit_argument(&mut argument);
            args.push(argument);
        }
        call.arguments = args;
        self.action_depth = outer;
        self.changed = true;
    }

    fn action_block(&mut self, body: &mut FunctionBody<'a>, run: &'a str) {
        let builder = AstBuilder::new(self.alloc);
        let taken = std::mem::replace(&mut body.statements, ArenaVec::new_in(&builder));
        let mut walked = ArenaVec::new_in(&builder);
        for mut statement in taken.into_iter() {
            self.visit_statement(&mut statement);
            walked.push(statement);
        }
        let span = body.span;
        let try_block = BlockStatement::boxed(span, walked, &builder);
        let end_call = call_expr(
            self.alloc,
            span,
            member_expr(self.alloc, span, ident_expr(self.alloc, SPAN, run), "end", SPAN),
            ArenaVec::new_in(&builder),
        );
        let finalizer = BlockStatement::boxed(
            span,
            ArenaVec::from_iter_in([expr_statement(self.alloc, span, end_call)], &builder),
            &builder,
        );
        let mut statements = ArenaVec::new_in(&builder);
        statements.push(Statement::new_try_statement(
            span,
            try_block,
            None,
            Some(finalizer),
            &builder,
        ));
        body.statements = statements;
    }

    fn prepend_resume(&mut self, body: &mut BlockStatement<'a>, run: &'a str) {
        let span = body.span;
        let resume = expr_statement(
            self.alloc,
            span,
            call_expr(
                self.alloc,
                span,
                member_expr(self.alloc, span, ident_expr(self.alloc, SPAN, run), "resume", SPAN),
                ArenaVec::new_in(&self.alloc),
            ),
        );
        let taken = std::mem::replace(&mut body.body, ArenaVec::new_in(&self.alloc));
        let mut statements = ArenaVec::new_in(&self.alloc);
        statements.push(resume);
        for statement in taken.into_iter() {
            statements.push(statement);
        }
        body.body = statements;
        self.changed = true;
    }

    fn normalize_await(&mut self, it: &mut Expression<'a>) -> bool {
        if self.action_depth != Some(0) {
            return false;
        }
        let Expression::AwaitExpression(_) = it else { return false };
        let taken = take(self.alloc, it);
        let Expression::AwaitExpression(boxed) = taken else {
            *it = taken;
            return false;
        };
        let mut awaited = boxed.unbox();
        let await_span = awaited.span;
        let arg_span = awaited.argument.span();
        self.visit_expression(&mut awaited.argument);
        let run: &'a str = self.alloc.alloc_str(&self.pre.run);
        let builder = AstBuilder::new(self.alloc);
        let mut suspend_args = ArenaVec::new_in(&self.alloc);
        suspend_args.push(Argument::from(awaited.argument));
        let suspended = call_expr(
            self.alloc,
            arg_span,
            member_expr(self.alloc, arg_span, ident_expr(self.alloc, SPAN, run), "suspend", SPAN),
            suspend_args,
        );
        let awaited = Expression::new_await_expression(await_span, suspended, &builder);
        let mut resume_args = ArenaVec::new_in(&self.alloc);
        resume_args.push(Argument::from(awaited));
        *it = call_expr(
            self.alloc,
            await_span,
            member_expr(self.alloc, await_span, ident_expr(self.alloc, SPAN, run), "resume", SPAN),
            resume_args,
        );
        self.changed = true;
        true
    }
}

impl<'a> VisitMut<'a> for Normalizer<'_, '_, '_, 'a> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        if matches!(it, Expression::Identifier(_)) {
            if self.normalize_read(it) {
                return;
            }
        } else if matches!(it, Expression::AssignmentExpression(_)) {
            if self.normalize_assign(it) {
                return;
            }
        } else if matches!(it, Expression::UpdateExpression(_)) {
            if self.normalize_update(it) {
                return;
            }
        } else if matches!(it, Expression::AwaitExpression(_)) {
            if self.normalize_await(it) {
                return;
            }
        } else if matches!(it, Expression::StaticMemberExpression(_)) {
            if self.normalize_namespace_read(it) {
                return;
            }
        }
        walk_mut::walk_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &mut CallExpression<'a>) {
        let callee_span = it.callee.without_parentheses().span();
        if self.pre.action_callees.contains(&(callee_span.start, callee_span.end)) {
            self.normalize_action(it);
            return;
        }
        walk_mut::walk_call_expression(self, it);
    }

    fn visit_variable_declaration(&mut self, it: &mut VariableDeclaration<'a>) {
        let make_const = it.declarations.first().is_some_and(|first| {
            self.pre
                .decls
                .get(&first.span.start)
                .is_some_and(|plan| plan.primitive == Syntax::Signal && plan.keyword.is_some())
        });
        if make_const {
            it.kind = VariableDeclarationKind::Const;
            self.changed = true;
        }
        walk_mut::walk_variable_declaration(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &mut VariableDeclarator<'a>) {
        if self.pre.decls.contains_key(&it.span.start) {
            self.norm_declarator(it);
            return;
        }
        walk_mut::walk_variable_declarator(self, it);
    }

    fn visit_try_statement(&mut self, it: &mut TryStatement<'a>) {
        if self.action_depth == Some(0) {
            let run_text: &'a str = self.alloc.alloc_str(&self.pre.run);
            let try_awaits = block_awaits(&it.block);
            if try_awaits && let Some(handler) = it.handler.as_mut() {
                self.prepend_resume(&mut handler.body, run_text);
            }
            let catch_awaits =
                it.handler.as_ref().is_some_and(|handler| block_awaits(&handler.body));
            if (try_awaits || catch_awaits)
                && let Some(finalizer) = it.finalizer.as_mut()
            {
                self.prepend_resume(finalizer, run_text);
            }
        }
        walk_mut::walk_try_statement(self, it);
    }

    fn visit_function(&mut self, it: &mut Function<'a>, flags: ScopeFlags) {
        if self.action_depth.is_some() {
            if let Some(depth) = self.action_depth.as_mut() {
                *depth += 1;
            }
            walk_mut::walk_function(self, it, flags);
            if let Some(depth) = self.action_depth.as_mut() {
                *depth -= 1;
            }
        } else {
            walk_mut::walk_function(self, it, flags);
        }
    }

    fn visit_arrow_function_expression(&mut self, it: &mut ArrowFunctionExpression<'a>) {
        if self.action_depth.is_some() {
            if let Some(depth) = self.action_depth.as_mut() {
                *depth += 1;
            }
            walk_mut::walk_arrow_function_expression(self, it);
            if let Some(depth) = self.action_depth.as_mut() {
                *depth -= 1;
            }
        } else {
            walk_mut::walk_arrow_function_expression(self, it);
        }
    }

    fn visit_object_property(&mut self, it: &mut ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some(reference) = id.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
            && self.decl_plan(symbol).is_some()
            && !self.scoping.get_reference(reference).flags().is_write()
        {
            let (span, name) = (id.span, id.name.as_str());
            let text: &'a str = self.alloc.alloc_str(name);
            it.shorthand = false;
            it.value = getter_call(self.alloc, span, text);
            self.changed = true;
        }
        walk_mut::walk_object_property(self, it);
    }

    fn visit_jsx_element_name(&mut self, it: &mut JSXElementName<'a>) {
        if !matches!(it, JSXElementName::IdentifierReference(_)) {
            walk_mut::walk_jsx_element_name(self, it);
        }
    }

    fn visit_ts_type(&mut self, _: &mut TSType<'a>) {}

    fn visit_import_declaration(&mut self, _: &mut ImportDeclaration<'a>) {}
}
