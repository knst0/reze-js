//! First pass of `$signal` and `$computed`: rewrites the syntax into the `signal` tuple and the
//! `computed` getter a person would write. The second pass is the ordinary compiler, run on the
//! rewritten text.

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::operator::{AssignmentOperator, UnaryOperator, UpdateOperator};
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use crate::analyze::{RUNTIME_MODULES, exported_symbols};
use crate::code::Code;
use crate::diagnostic::{self, Report};
use crate::lower::is_component_name;
use crate::namer::Namer;

/// Compiler syntax and the runtime function it compiles to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Primitive {
    Signal,
    Computed,
}

impl Primitive {
    const ALL: [Primitive; 2] = [Primitive::Signal, Primitive::Computed];

    fn dollar(self) -> &'static str {
        match self {
            Primitive::Signal => "$signal",
            Primitive::Computed => "$computed",
        }
    }

    fn runtime(self) -> &'static str {
        match self {
            Primitive::Signal => "signal",
            Primitive::Computed => "computed",
        }
    }

    fn from_dollar(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.dollar() == name)
    }

    fn from_runtime(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.runtime() == name)
    }
}

/// Whether `source` may use the syntax; files without it skip the first pass.
pub fn mentions_syntax(source: &str) -> bool {
    Primitive::ALL.into_iter().any(|p| source.contains(p.dollar()))
}

struct Patch {
    start: u32,
    end: u32,
    text: String,
}

/// The source after the first pass, and how to move positions of the rewritten text back.
pub struct Rewritten {
    /// The rewritten text, marked with offsets of the original source.
    pub code: Code,
    spans: Vec<Replaced>,
}

/// One patch in both coordinate systems.
struct Replaced {
    source_start: u32,
    source_end: u32,
    text_start: u32,
    text_end: u32,
}

impl Rewritten {
    /// The original offset a span starting at `offset` of the rewritten text started at.
    pub fn start(&self, offset: u32) -> u32 {
        let before = self.spans.partition_point(|r| r.text_end <= offset);
        match self.spans.get(before) {
            Some(r) if r.text_start <= offset => r.source_start,
            _ => self.copied(before, offset),
        }
    }

    /// The original offset a span ending at `offset` of the rewritten text ended at.
    pub fn end(&self, offset: u32) -> u32 {
        let before = self.spans.partition_point(|r| r.text_end < offset);
        match self.spans.get(before) {
            Some(r) if r.text_start < offset => r.source_end,
            _ => self.copied(before, offset),
        }
    }

    fn copied(&self, patches_before: usize, offset: u32) -> u32 {
        match patches_before.checked_sub(1).map(|i| &self.spans[i]) {
            Some(r) => offset - r.text_end + r.source_end,
            None => offset,
        }
    }
}

/// `Ok(None)` when the file does not use `$signal` or `$computed`, or does not parse: the
/// ordinary pass reports syntax errors. `Err` holds the reports of the first pass when one is an
/// error; `Ok` carries its warnings.
pub fn rewrite(
    source: &str,
    source_type: SourceType,
) -> Result<Option<(Rewritten, Vec<Report>)>, Vec<Report>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Ok(None);
    }
    let program = allocator.alloc(parsed.program);
    let scoping = SemanticBuilder::new().build(program).semantic.into_scoping();
    let imports = Imports::collect(program);
    if imports.dollar.is_empty() && imports.namespaces.is_empty() {
        return Ok(None);
    }
    let mut namer = Namer::new(&scoping);
    let mut patches = Vec::new();
    let callees = imports.patch(source, &mut namer, &mut patches);

    let mut declarations = Declarations {
        scoping: &scoping,
        imports: &imports,
        namer: &mut namer,
        variables: HashMap::new(),
        declarators: HashMap::new(),
        consumed: HashSet::new(),
        reports: Vec::new(),
        in_for_init: false,
    };
    declarations.visit_program(program);
    let Declarations { variables, declarators, reports, .. } = declarations;
    if variables.is_empty() && reports.is_empty() {
        return Ok(None);
    }

    let mut rewriter = Rewriter {
        source,
        scoping: &scoping,
        imports: &imports,
        callees: &callees,
        variables: &variables,
        declarators: &declarators,
        patches,
        reports,
        top: HashSet::new(),
        discarded: HashSet::new(),
        component_inits: HashSet::new(),
    };
    rewriter.visit_program(program);
    let Rewriter { mut patches, mut reports, .. } = rewriter;

    let exported = exported_symbols(program, &scoping);
    for (symbol, variable) in &variables {
        if exported.contains(symbol) {
            reports.push(
                Report::new(diagnostic::Code::SignalExported, variable.declarator)
                    .arg("signal", scoping.symbol_name(*symbol))
                    .arg("primitive", variable.primitive.dollar()),
            );
        }
    }
    if reports.iter().any(|report| report.code.severity() == diagnostic::Severity::Error) {
        return Err(reports);
    }
    Ok(Some((apply(source, &mut patches), reports)))
}

fn apply(source: &str, patches: &mut [Patch]) -> Rewritten {
    patches.sort_by_key(|patch| (patch.start, patch.end));
    let mut code = Code::default();
    let mut spans = Vec::with_capacity(patches.len());
    let mut cursor = 0;
    for patch in patches.iter() {
        debug_assert!(patch.start >= cursor, "overlapping patches at {}", patch.start);
        code.src(source, cursor, patch.start);
        code.mark(patch.start);
        let text_start = code.text.len() as u32;
        code.push(&patch.text);
        spans.push(Replaced {
            source_start: patch.start,
            source_end: patch.end,
            text_start,
            text_end: code.text.len() as u32,
        });
        cursor = patch.end;
    }
    code.src(source, cursor, source.len() as u32);
    Rewritten { code, spans }
}

struct DollarImport {
    specifier: Span,
    symbol: SymbolId,
    primitive: Primitive,
    local: String,
    is_aliased: bool,
}

struct Imports {
    dollar: Vec<DollarImport>,
    namespaces: HashSet<SymbolId>,
    /// The local name of a value import of each runtime function, by `Primitive`.
    runtime: [Option<String>; Primitive::ALL.len()],
    declarations: Vec<ImportRange>,
}

/// The named specifiers of an import declaration that mentions compiler syntax.
struct ImportRange {
    span: Span,
    named: Span,
    has_other_bindings: bool,
    specifiers: Vec<Span>,
}

impl Imports {
    fn collect(program: &Program<'_>) -> Self {
        let mut imports = Imports {
            dollar: Vec::new(),
            namespaces: HashSet::new(),
            runtime: Default::default(),
            declarations: Vec::new(),
        };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.import_kind.is_type()
                || !RUNTIME_MODULES.contains(&import.source.value.as_str())
            {
                continue;
            }
            let mut range: Option<ImportRange> = None;
            let mut has_other_bindings = false;
            let mut has_dollar = false;
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        let range = range.get_or_insert_with(|| ImportRange {
                            span: import.span,
                            named: named.span,
                            has_other_bindings: false,
                            specifiers: Vec::new(),
                        });
                        range.named.end = named.span.end;
                        range.specifiers.push(named.span);
                        if named.import_kind.is_type() {
                            continue;
                        }
                        let imported = named.imported.name();
                        if let Some(primitive) = Primitive::from_dollar(&imported) {
                            has_dollar = true;
                            imports.dollar.push(DollarImport {
                                specifier: named.span,
                                symbol: named.local.symbol_id(),
                                primitive,
                                local: named.local.name.to_string(),
                                is_aliased: named.local.name != imported,
                            });
                        } else if let Some(primitive) = Primitive::from_runtime(&imported) {
                            imports.runtime[primitive as usize]
                                .get_or_insert_with(|| named.local.name.to_string());
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                        has_other_bindings = true;
                        imports.namespaces.insert(namespace.local.symbol_id());
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {
                        has_other_bindings = true;
                    }
                }
            }
            if let (Some(mut range), true) = (range, has_dollar) {
                range.has_other_bindings = has_other_bindings;
                imports.declarations.push(range);
            }
        }
        imports
    }

    fn primitive_of(&self, symbol: SymbolId) -> Option<Primitive> {
        self.dollar.iter().find(|import| import.symbol == symbol).map(|import| import.primitive)
    }

    /// Rewrites the syntax specifiers: per primitive, the first becomes its runtime function
    /// unless the file already imports it, the others are dropped. Returns the name each syntax
    /// local stands for.
    fn patch(
        &self,
        source: &str,
        namer: &mut Namer<'_>,
        patches: &mut Vec<Patch>,
    ) -> HashMap<SymbolId, String> {
        let mut callees = HashMap::new();
        let mut targets = self.runtime.clone();
        let mut replaced: HashMap<u32, Option<String>> = HashMap::new();
        for import in &self.dollar {
            let runtime = import.primitive.runtime();
            let target = &mut targets[import.primitive as usize];
            let text = match target {
                Some(name) => {
                    callees.insert(import.symbol, name.clone());
                    None
                }
                None if import.is_aliased => {
                    callees.insert(import.symbol, import.local.clone());
                    *target = Some(import.local.clone());
                    Some(format!("{runtime} as {}", import.local))
                }
                None => {
                    let name = namer.fresh(runtime);
                    let text =
                        if name == runtime { name.clone() } else { format!("{runtime} as {name}") };
                    callees.insert(import.symbol, name.clone());
                    *target = Some(name);
                    Some(text)
                }
            };
            replaced.insert(import.specifier.start, text);
        }
        for declaration in &self.declarations {
            let kept: Vec<&str> = declaration
                .specifiers
                .iter()
                .filter_map(|span| match replaced.get(&span.start) {
                    Some(Some(text)) => Some(text.as_str()),
                    Some(None) => None,
                    None => Some(&source[span.start as usize..span.end as usize]),
                })
                .collect();
            if kept.is_empty() && !declaration.has_other_bindings {
                let mut end = declaration.span.end as usize;
                if source[end..].starts_with("\r\n") {
                    end += 2;
                } else if source[end..].starts_with('\n') {
                    end += 1;
                }
                patches.push(Patch {
                    start: declaration.span.start,
                    end: end as u32,
                    text: String::new(),
                });
            } else {
                patches.push(Patch {
                    start: declaration.named.start,
                    end: declaration.named.end,
                    text: kept.join(", "),
                });
            }
        }
        callees
    }
}

/// What the first pass knows about one `$signal` or `$computed` variable.
struct Reactive {
    declarator: Span,
    primitive: Primitive,
    /// Only a `$signal` that is written has one.
    setter: Option<String>,
}

/// How a declarator is rewritten.
struct Declarator {
    symbol: SymbolId,
    keyword: Option<Span>,
}

fn dollar_call<'e, 'a>(init: &'e Expression<'a>) -> Option<&'e CallExpression<'a>> {
    match init.without_parentheses() {
        Expression::CallExpression(call) => Some(call),
        _ => None,
    }
}

struct Declarations<'p, 's, 'n> {
    scoping: &'s Scoping,
    imports: &'p Imports,
    namer: &'p mut Namer<'n>,
    variables: HashMap<SymbolId, Reactive>,
    declarators: HashMap<u32, Declarator>,
    /// Start offsets of syntax callees that initialize a declaration.
    consumed: HashSet<u32>,
    reports: Vec<Report>,
    in_for_init: bool,
}

impl Declarations<'_, '_, '_> {
    fn callee_primitive(&self, callee: &Expression<'_>) -> Option<Primitive> {
        match callee.without_parentheses() {
            Expression::Identifier(id) => {
                symbol_of(self.scoping, id).and_then(|s| self.imports.primitive_of(s))
            }
            Expression::StaticMemberExpression(member) => {
                namespace_member(self.scoping, self.imports, member)
            }
            _ => None,
        }
    }

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
        let Some(call) = dollar_call(init) else { return };
        let Some(primitive) = self.callee_primitive(&call.callee) else { return };
        self.consumed.insert(call.callee.without_parentheses().span().start);
        if !matches!(
            declaration.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Const
        ) {
            self.reports.push(
                Report::new(diagnostic::Code::SignalNotDeclared, call.span)
                    .arg("primitive", primitive.dollar()),
            );
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            self.reports.push(
                Report::new(diagnostic::Code::SignalPattern, declarator.id.span())
                    .arg("primitive", primitive.dollar()),
            );
            return;
        };
        let symbol = id.symbol_id();
        let name = self.scoping.symbol_name(symbol);
        let setter = (primitive == Primitive::Signal && self.is_written(symbol)).then(|| {
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
        self.variables.insert(symbol, Reactive { declarator: declarator.span, primitive, setter });
        self.declarators.insert(declarator.span.start, Declarator { symbol, keyword });
    }
}

fn symbol_of(scoping: &Scoping, id: &IdentifierReference<'_>) -> Option<SymbolId> {
    scoping.get_reference(id.reference_id.get()?).symbol_id()
}

/// The primitive `member` names when it is `ns.$signal` or `ns.$computed` of a runtime namespace.
fn namespace_member(
    scoping: &Scoping,
    imports: &Imports,
    member: &StaticMemberExpression<'_>,
) -> Option<Primitive> {
    if member.optional {
        return None;
    }
    let primitive = Primitive::from_dollar(&member.property.name)?;
    let Expression::Identifier(object) = &member.object else { return None };
    symbol_of(scoping, object).is_some_and(|s| imports.namespaces.contains(&s)).then_some(primitive)
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

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(primitive) =
            symbol_of(self.scoping, it).and_then(|s| self.imports.primitive_of(s))
            && !self.consumed.contains(&it.span.start)
        {
            self.reports.push(
                Report::new(diagnostic::Code::SignalNotDeclared, it.span)
                    .arg("primitive", primitive.dollar()),
            );
        }
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some(primitive) = namespace_member(self.scoping, self.imports, it) {
            if !self.consumed.contains(&it.span.start) {
                self.reports.push(
                    Report::new(diagnostic::Code::SignalNotDeclared, it.span)
                        .arg("primitive", primitive.dollar()),
                );
            }
            return;
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_ts_type(&mut self, _: &TSType<'a>) {}
}

struct Rewriter<'p, 's> {
    source: &'p str,
    scoping: &'s Scoping,
    imports: &'p Imports,
    callees: &'p HashMap<SymbolId, String>,
    variables: &'p HashMap<SymbolId, Reactive>,
    declarators: &'p HashMap<u32, Declarator>,
    patches: Vec<Patch>,
    reports: Vec<Report>,
    /// Expressions that are a whole statement, `for` clause or element of such a sequence.
    top: HashSet<u32>,
    /// Expressions whose value nothing reads.
    discarded: HashSet<u32>,
    component_inits: HashSet<u32>,
}

impl Rewriter<'_, '_> {
    fn reactive_of(&self, id: &IdentifierReference<'_>) -> Option<(SymbolId, &Reactive)> {
        let symbol = symbol_of(self.scoping, id)?;
        self.variables.get(&symbol).map(|variable| (symbol, variable))
    }

    fn name(&self, symbol: SymbolId) -> &str {
        self.scoping.symbol_name(symbol)
    }

    fn text(&self, span: Span) -> &str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn patch(&mut self, start: u32, end: u32, text: impl Into<String>) {
        self.patches.push(Patch { start, end, text: text.into() });
    }

    fn mark_top(&mut self, e: &Expression<'_>) {
        self.top.insert(e.span().start);
        self.mark_discarded(e);
        if let Expression::SequenceExpression(sequence) = e {
            for element in &sequence.expressions {
                self.mark_top(element);
            }
        }
    }

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

    fn setter(&self, symbol: SymbolId) -> &str {
        self.variables[&symbol].setter.as_deref().unwrap_or_default()
    }

    fn computed_written(&mut self, span: Span, symbol: SymbolId) {
        self.reports.push(
            Report::new(diagnostic::Code::ComputedWritten, span).arg("computed", self.name(symbol)),
        );
    }

    fn assign(&mut self, it: &AssignmentExpression<'_>, symbol: SymbolId) {
        let right = it.right.span();
        let setter = self.setter(symbol).to_string();
        let getter = self.name(symbol).to_string();
        let is_top = self.top.contains(&it.span.start);
        let mut before = String::new();
        let mut after = String::new();
        match it.operator {
            AssignmentOperator::Assign => {
                before.push_str(&setter);
                before.push('(');
                if !is_never_function(&it.right) && !suspends(&it.right) {
                    before.push_str("() => ");
                }
                after.push(')');
            }
            AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalNullish => {
                let operator = match it.operator {
                    AssignmentOperator::LogicalOr => "||",
                    AssignmentOperator::LogicalAnd => "&&",
                    _ => "??",
                };
                if !is_top {
                    before.push('(');
                    after.push(')');
                }
                before.push_str(&format!("{getter}() {operator} {setter}("));
                if !is_never_function(&it.right) && !suspends(&it.right) {
                    before.push_str("() => ");
                }
                after.insert(0, ')');
            }
            operator => {
                let operator = operator.as_str().trim_end_matches('=');
                before.push_str(&format!("{setter}({getter}() {operator} "));
                if !is_tight(&it.right) {
                    before.push('(');
                    after.push(')');
                }
                after.push(')');
            }
        }
        self.patch(it.span.start, right.start, before);
        self.visit_expression(&it.right);
        self.patch(right.end, right.end, after);
    }

    fn update(&mut self, it: &UpdateExpression<'_>, symbol: SymbolId) {
        if !self.discarded.contains(&it.span.start) {
            self.reports.push(
                Report::new(diagnostic::Code::SignalUpdateInExpression, it.span)
                    .arg("signal", self.name(symbol)),
            );
            return;
        }
        let operator = if it.operator == UpdateOperator::Increment { '+' } else { '-' };
        let (getter, setter) = (self.name(symbol), self.setter(symbol));
        let text = format!("{setter}({getter}() {operator} 1)");
        self.patch(it.span.start, it.span.end, text);
    }

    fn declarator(&mut self, it: &VariableDeclarator<'_>, plan: &Declarator) {
        let (Some(init), Some(call)) = (&it.init, it.init.as_ref().and_then(dollar_call)) else {
            return;
        };
        let variable = &self.variables[&plan.symbol];
        let primitive = variable.primitive;
        match primitive {
            Primitive::Signal => {
                let mut target = format!("[{}", self.name(plan.symbol));
                if let Some(setter) = &variable.setter {
                    target.push_str(", ");
                    target.push_str(setter);
                }
                target.push_str("] = ");
                self.patch(it.id.span().start, init.span().start, target);
            }
            Primitive::Computed if it.type_annotation.is_some() => {
                self.patch(it.id.span().end, init.span().start, " = ");
            }
            Primitive::Computed => {}
        }
        if let Some(keyword) = plan.keyword {
            self.patch(keyword.start, keyword.end, "const");
        }
        match call.callee.without_parentheses() {
            Expression::Identifier(id) => {
                if let Some(name) = symbol_of(self.scoping, id).and_then(|s| self.callees.get(&s))
                    && name.as_str() != id.name.as_str()
                {
                    self.patch(id.span.start, id.span.end, name.clone());
                }
            }
            Expression::StaticMemberExpression(member) => {
                let property = member.property.span;
                self.patch(property.start, property.end, primitive.runtime());
            }
            _ => {}
        }
        if let (Some(annotation), None) = (&it.type_annotation, &call.type_arguments) {
            let text = format!("<{}>", self.text(annotation.type_annotation.span()));
            let at = call.callee.span().end;
            self.patch(at, at, text);
        }
        let mut arguments = call.arguments.iter();
        if primitive == Primitive::Computed
            && let Some(value) = arguments.next()
        {
            match value.as_expression() {
                Some(value) => self.computed_value(value),
                None => self.visit_argument(value),
            }
        }
        for argument in arguments {
            self.visit_argument(argument);
        }
    }

    /// Wraps the value of a `$computed` into the getter `computed` runs.
    fn computed_value(&mut self, value: &Expression<'_>) {
        match value.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                let mut report = Report::new(diagnostic::Code::ComputedFunction, arrow.span);
                if let (false, true, Some(body)) =
                    (arrow.r#async, arrow.params.is_empty(), arrow.get_expression())
                {
                    let edit = diagnostic::Edit {
                        start: arrow.span.start,
                        end: body.span().start,
                        text: String::new(),
                    };
                    report = report.fix(vec![edit]);
                }
                self.reports.push(report);
                return;
            }
            Expression::FunctionExpression(function) => {
                self.reports.push(Report::new(diagnostic::Code::ComputedFunction, function.span));
                return;
            }
            _ => {}
        }
        if suspends(value) {
            self.reports.push(Report::new(diagnostic::Code::ComputedAwait, value.span()));
            return;
        }
        let span = value.span();
        let is_object_first = self.text(span).starts_with('{');
        self.patch(span.start, span.start, if is_object_first { "() => (" } else { "() => " });
        self.visit_expression(value);
        if is_object_first {
            self.patch(span.end, span.end, ")");
        }
    }

    fn check_read_once(&mut self, body: &FunctionBody<'_>) {
        for statement in &body.statements {
            let Statement::VariableDeclaration(declaration) = statement else { continue };
            for declarator in &declaration.declarations {
                let Some(init) = &declarator.init else { continue };
                if self.declarators.contains_key(&declarator.span.start) {
                    continue;
                }
                let mut first = FirstRead { rewriter: self, found: None };
                first.visit_expression(init);
                let Some((span, symbol)) = first.found else { continue };
                let variable = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                    pattern => self.text(pattern.span()).to_string(),
                };
                let report = Report::new(diagnostic::Code::SignalReadOnce, span)
                    .arg("variable", variable)
                    .arg("signal", self.name(symbol));
                self.reports.push(report);
            }
        }
    }
}

/// The first read of a `$signal` or `$computed` variable that runs when the initializer does.
struct FirstRead<'r, 'p, 's> {
    rewriter: &'r Rewriter<'p, 's>,
    found: Option<(Span, SymbolId)>,
}

impl<'a> Visit<'a> for FirstRead<'_, '_, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if self.found.is_none()
            && let Some((symbol, _)) = self.rewriter.reactive_of(it)
            && let Some(reference) = it.reference_id.get()
            && !self.rewriter.scoping.get_reference(reference).flags().is_write()
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

impl<'a> Visit<'a> for Rewriter<'_, '_> {
    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        self.mark_top(&it.expression);
        walk::walk_expression_statement(self, it);
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        if let Some(init) = it.init.as_ref().and_then(ForStatementInit::as_expression) {
            self.mark_top(init);
        }
        if let Some(update) = &it.update {
            self.mark_top(update);
        }
        walk::walk_for_statement(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        if it.operator == UnaryOperator::Void {
            self.mark_discarded(&it.argument);
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(plan) = self.declarators.get(&it.span.start) {
            self.declarator(it, plan);
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
        walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let ArrowFunctionBody::FunctionBody(body) = &it.body
            && self.component_inits.contains(&it.span.start)
        {
            self.check_read_once(body);
        }
        walk::walk_arrow_function_expression(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(id) = &it.left
            && let Some((symbol, variable)) = self.reactive_of(id)
        {
            match variable.primitive {
                Primitive::Signal => self.assign(it, symbol),
                Primitive::Computed => self.computed_written(it.span, symbol),
            }
            return;
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &it.argument
            && let Some((symbol, variable)) = self.reactive_of(id)
        {
            match variable.primitive {
                Primitive::Signal => self.update(it, symbol),
                Primitive::Computed => self.computed_written(it.span, symbol),
            }
            return;
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some((symbol, _)) = self.reactive_of(id)
            && id
                .reference_id
                .get()
                .is_some_and(|r| !self.scoping.get_reference(r).flags().is_write())
        {
            let text = format!(": {}()", self.name(symbol));
            self.patch(id.span.end, id.span.end, text);
            return;
        }
        walk::walk_object_property(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let Some((symbol, variable)) = self.reactive_of(it) else { return };
        let is_write =
            it.reference_id.get().is_some_and(|r| self.scoping.get_reference(r).flags().is_write());
        match (is_write, variable.primitive) {
            (false, _) => self.patch(it.span.end, it.span.end, "()"),
            (true, Primitive::Signal) => self.reports.push(
                Report::new(diagnostic::Code::SignalAssignPattern, it.span)
                    .arg("signal", self.name(symbol)),
            ),
            (true, Primitive::Computed) => self.computed_written(it.span, symbol),
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
        if namespace_member(self.scoping, self.imports, it).is_some() {
            return;
        }
        walk::walk_static_member_expression(self, it);
    }
}

/// Whether the syntax proves `e` never evaluates to a function.
fn is_never_function(e: &Expression<'_>) -> bool {
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

/// Whether `e` can follow a binary operator without parentheses.
fn is_tight(e: &Expression<'_>) -> bool {
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

/// Whether `e` awaits or yields outside a nested function, so an arrow around it would not parse.
fn suspends(e: &Expression<'_>) -> bool {
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
    let mut check = Suspends::default();
    check.visit_expression(e);
    check.0
}
