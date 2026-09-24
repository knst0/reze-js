//! O4 (SPEC §8): `const d = computed(() => expr)` read once, as `d()` inside a reactive JSX
//! expression of the function that declares it, is inlined into that read. Across modules
//! (§16.5), `link` decides and each side applies its part of the decision.

use std::collections::HashSet;

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeId;
use oxc_syntax::symbol::SymbolId;

use super::Facts;
use crate::diagnostic::{Code, Report};
use crate::facts::{ComputedRead, InlinedComputed, ModuleFacts};
use crate::lower::constant::literal_truthy;
use crate::lower::is_native_name;
use crate::usage::{Context, classify};

/// `expr` of `computed(() => expr)` without annotations, type arguments or options.
pub fn body<'b, 'a>(declarator: &'b VariableDeclarator<'a>) -> Option<&'b Expression<'a>> {
    if declarator.type_annotation.is_some() {
        return None;
    }
    let Some(Expression::CallExpression(call)) =
        declarator.init.as_ref().map(Expression::without_parentheses)
    else {
        return None;
    };
    if call.optional || call.type_arguments.is_some() || call.arguments.len() != 1 {
        return None;
    }
    let Some(Expression::ArrowFunctionExpression(arrow)) =
        call.arguments[0].as_expression().map(Expression::without_parentheses)
    else {
        return None;
    };
    let is_plain = !arrow.r#async
        && arrow.type_parameters.is_none()
        && arrow.return_type.is_none()
        && arrow.params.items.is_empty()
        && arrow.params.rest.is_none();
    if !is_plain {
        return None;
    }
    arrow.get_expression().map(Expression::without_parentheses)
}

pub fn inline(
    facts: &mut Facts,
    declarators: &[NodeId],
    exported: &HashSet<SymbolId>,
    source: &str,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) {
    for &declarator_node in declarators {
        let AstKind::VariableDeclarator(declarator) = nodes.kind(declarator_node) else { continue };
        let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
        let computed = id.symbol_id();
        if exported.contains(&computed) {
            continue;
        }
        let Some(expr) = body(declarator) else { continue };
        let declaration_node = nodes.parent_id(declarator_node);
        let AstKind::VariableDeclaration(declaration) = nodes.kind(declaration_node) else {
            continue;
        };
        if declaration.kind != VariableDeclarationKind::Const
            || declaration.declare
            || declaration.declarations.len() != 1
        {
            continue;
        }
        let is_top_level = match nodes.parent_kind(declaration_node) {
            AstKind::Program(_) => true,
            AstKind::FunctionBody(_) | AstKind::BlockStatement(_) => false,
            _ => continue,
        };
        let &[reference] = scoping.get_resolved_reference_ids(computed) else { continue };
        let Some(access) = classify(reference, scoping, nodes) else { continue };
        if access.context != (Context::Call { argument_count: 0 })
            || !access.keys.is_empty()
            || !access.tail.is_empty()
        {
            continue;
        }
        let call_node = nodes.parent_id(access.node);
        let AstKind::CallExpression(call) = nodes.kind(call_node) else { continue };
        if !matches!(call.callee, Expression::Identifier(_))
            || call.span.start < declaration.span.end
        {
            continue;
        }
        let Some(read_boundary) = reactive_jsx_boundary(call_node, nodes, facts) else { continue };
        if read_boundary != boundary(declaration_node, nodes)
            || !is_plain_boundary(nodes.kind(read_boundary))
        {
            continue;
        }
        let read_scope = nodes.get_node(call_node).scope_id();
        if !resolves_alike(expr, declarator.span, read_scope, scoping) {
            continue;
        }

        facts.inlined_reads.insert(reference, declarator_node);
        facts
            .removed_declarations
            .insert(declaration.span.start, removal(source, declaration.span, is_top_level));
        let name = scoping.symbol_name(computed);
        reports.push(
            Report::new(
                Code::ComputedInlined,
                declarator.span,
                format!(
                    "`{name}` is read only once, by `{name}()` in a reactive JSX expression of the \
                     same function, so the declaration was removed and its expression inlined there."
                ),
            )
            .label(call.span, "inlined here")
            .data("computed", name)
            .data("scope", "module"),
        );
    }
}

/// A read of a computed another module declares, inlined here (§16.5).
pub struct InlinedRead {
    pub body: String,
    pub references: Vec<ReadReference>,
}

/// A reference of an inlined body, by offsets into it, and the name it reads here.
pub struct ReadReference {
    pub start: u32,
    pub end: u32,
    pub name: ReadName,
}

pub enum ReadName {
    /// An import already in scope at the read.
    Local(String),
    /// An import added for the body.
    Imported(ImportedName),
}

/// The export `export` of the module imported from `source`, imported under a fresh name based on
/// `base`.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportedName {
    pub source: String,
    pub export: String,
    pub base: String,
}

/// Whether `call` stands in a reactive JSX expression of a function lowering keeps in place, as
/// O4 requires of a read.
pub fn is_reactive_read(call: NodeId, nodes: &AstNodes<'_>) -> bool {
    reactive_jsx_boundary(call, nodes, &Facts::default())
        .is_some_and(|boundary| is_plain_boundary(nodes.kind(boundary)))
}

/// The identifiers of `expr` with their symbols when `expr` can move to another module as text:
/// plain expression syntax (no functions, JSX, types, `this` or object literals) whose every
/// identifier names a binding of the program scope.
pub fn portable_references(
    expr: &Expression<'_>,
    scoping: &Scoping,
) -> Option<Vec<(Span, SymbolId)>> {
    let mut references = Vec::new();
    portable(expr, scoping, &mut references)?;
    Some(references)
}

fn portable(
    e: &Expression<'_>,
    scoping: &Scoping,
    references: &mut Vec<(Span, SymbolId)>,
) -> Option<()> {
    match e {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::StringLiteral(_) => Some(()),
        Expression::TemplateLiteral(template) => {
            template.expressions.iter().try_for_each(|e| portable(e, scoping, references))
        }
        Expression::Identifier(id) => {
            let symbol = scoping.get_reference(id.reference_id.get()?).symbol_id()?;
            if scoping.symbol_scope_id(symbol) != scoping.root_scope_id() {
                return None;
            }
            references.push((id.span, symbol));
            Some(())
        }
        Expression::StaticMemberExpression(member) => portable(&member.object, scoping, references),
        Expression::ComputedMemberExpression(member) => {
            portable(&member.object, scoping, references)?;
            portable(&member.expression, scoping, references)
        }
        Expression::CallExpression(call) => portable_call(call, scoping, references),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => portable_call(call, scoping, references),
            ChainElement::StaticMemberExpression(member) => {
                portable(&member.object, scoping, references)
            }
            ChainElement::ComputedMemberExpression(member) => {
                portable(&member.object, scoping, references)?;
                portable(&member.expression, scoping, references)
            }
            _ => None,
        },
        Expression::UnaryExpression(unary) if unary.operator != UnaryOperator::Delete => {
            portable(&unary.argument, scoping, references)
        }
        Expression::BinaryExpression(binary) => {
            portable(&binary.left, scoping, references)?;
            portable(&binary.right, scoping, references)
        }
        Expression::LogicalExpression(logical) => {
            portable(&logical.left, scoping, references)?;
            portable(&logical.right, scoping, references)
        }
        Expression::ConditionalExpression(conditional) => {
            portable(&conditional.test, scoping, references)?;
            portable(&conditional.consequent, scoping, references)?;
            portable(&conditional.alternate, scoping, references)
        }
        Expression::ParenthesizedExpression(parenthesized) => {
            portable(&parenthesized.expression, scoping, references)
        }
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.iter().try_for_each(|e| portable(e, scoping, references))
        }
        _ => None,
    }
}

fn portable_call(
    call: &CallExpression<'_>,
    scoping: &Scoping,
    references: &mut Vec<(Span, SymbolId)>,
) -> Option<()> {
    if call.type_arguments.is_some() {
        return None;
    }
    portable(&call.callee, scoping, references)?;
    call.arguments.iter().try_for_each(|argument| match argument {
        Argument::SpreadElement(spread) => portable(&spread.argument, scoping, references),
        argument => portable(argument.as_expression()?, scoping, references),
    })
}

/// Applies the §16.5 decisions of `module_facts`: computeds of this module inlined elsewhere are
/// removed with their export specifiers, reads of other modules' computeds are inlined here.
pub fn apply_program(
    facts: &mut Facts,
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    module_facts: &ModuleFacts,
    reports: &mut Vec<Report>,
) {
    for inlined in &module_facts.inlined_computeds {
        remove_inlined(facts, program, scoping, nodes, inlined, reports);
    }
    for read in &module_facts.computed_reads {
        inline_read(facts, program, scoping, nodes, read);
    }
    for statement in &program.body {
        let Statement::ExportNamedDeclaration(export) = statement else { continue };
        let is_emptied = !export.specifiers.is_empty()
            && export
                .specifiers
                .iter()
                .all(|s| facts.program.removed_specifiers.contains(&s.span.start));
        if is_emptied {
            facts
                .removed_declarations
                .insert(export.span.start, removal(program.source_text, export.span, true));
        }
    }
}

fn remove_inlined(
    facts: &mut Facts,
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    inlined: &InlinedComputed,
    reports: &mut Vec<Report>,
) -> Option<()> {
    let (statement, declaration) = program.body.iter().find_map(|statement| {
        let declaration = match statement {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => declaration,
                _ => return None,
            },
            _ => return None,
        };
        let [declarator] = declaration.declarations.as_slice() else { return None };
        matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if id.span.start == inlined.binding)
            .then_some((statement.span(), &**declaration))
    })?;
    let declarator = &declaration.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &declarator.id else { return None };
    facts
        .removed_declarations
        .insert(declaration.span.start, removal(program.source_text, statement, true));
    for &reference in scoping.get_resolved_reference_ids(id.symbol_id()) {
        if let AstKind::ExportSpecifier(specifier) =
            nodes.parent_kind(scoping.get_reference(reference).node_id())
        {
            facts.program.removed_specifiers.insert(specifier.span.start);
        }
    }
    let name = id.name.as_str();
    let mut report = Report::new(
        Code::ComputedInlined,
        declarator.span,
        format!(
            "`{name}` is read only once, by `{name}()` in a reactive JSX expression of another \
             module, so the declaration and its exports were removed and its expression inlined \
             there."
        ),
    )
    .data("computed", name)
    .data("scope", "program");
    for related in &inlined.related {
        report = report.related(related.clone());
    }
    reports.push(report);
    Some(())
}

fn inline_read(
    facts: &mut Facts,
    program: &Program<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    read: &ComputedRead,
) -> Option<()> {
    let (computed, source) = import_binding(program, read.import)?;
    let scope = scoping
        .get_resolved_reference_ids(computed)
        .iter()
        .map(|&reference| scoping.get_reference(reference).node_id())
        .find(|&node| nodes.kind(node).span().start == read.callee)
        .map(|node| nodes.get_node(node).scope_id())?;
    let mut imported: Vec<ImportedName> = Vec::new();
    let mut references = Vec::with_capacity(read.references.len());
    for reference in &read.references {
        let in_scope = reference
            .import
            .and_then(|binding| import_binding(program, binding))
            .map(|(symbol, _)| symbol)
            .filter(|&symbol| {
                scoping.find_binding(scope, scoping.symbol_name(symbol).into()) == Some(symbol)
            });
        let name = match in_scope {
            Some(symbol) => ReadName::Local(scoping.symbol_name(symbol).to_string()),
            None => {
                let base = read.body.get(reference.start as usize..reference.end as usize)?;
                let name = ImportedName {
                    source: source.to_string(),
                    export: reference.export.clone(),
                    base: base.to_string(),
                };
                if !imported.contains(&name) {
                    imported.push(name.clone());
                }
                ReadName::Imported(name)
            }
        };
        references.push(ReadReference { start: reference.start, end: reference.end, name });
    }
    facts
        .program
        .computed_reads
        .insert(read.callee, InlinedRead { body: read.body.clone(), references });
    facts.program.computed_imports.insert(read.import, imported);
    Some(())
}

/// The symbol of the import binding whose local identifier starts at `binding`, with the module
/// it is imported from.
fn import_binding<'p>(program: &'p Program<'_>, binding: u32) -> Option<(SymbolId, &'p str)> {
    program.body.iter().find_map(|statement| {
        let Statement::ImportDeclaration(import) = statement else { return None };
        import.specifiers.iter().flatten().find_map(|specifier| {
            let local = specifier.local();
            (local.span.start == binding).then(|| (local.symbol_id(), import.source.value.as_str()))
        })
    })
}

/// The node whose body runs `node`: the nearest function, class member or the program.
fn boundary(node: NodeId, nodes: &AstNodes<'_>) -> NodeId {
    nodes.ancestor_ids(node).find(|&id| is_boundary(nodes.kind(id))).unwrap_or(NodeId::ROOT)
}

fn is_boundary(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::Program(_)
            | AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::PropertyDefinition(_)
            | AstKind::AccessorProperty(_)
            | AstKind::StaticBlock(_)
            | AstKind::TSModuleBlock(_)
    )
}

/// Async and generator bodies are restructured by lowering, so their statements are not kept
/// in place.
fn is_plain_boundary(kind: AstKind<'_>) -> bool {
    match kind {
        AstKind::Function(function) => !function.r#async && !function.generator,
        AstKind::ArrowFunctionExpression(arrow) => !arrow.r#async,
        _ => true,
    }
}

/// The boundary of a call inside a JSX expression that lowering compiles as reactive (a bind,
/// an insert or a getter prop) and keeps: no spread, event, `ref`, `children`, overridden
/// attribute or dead branch on the way up.
fn reactive_jsx_boundary(call: NodeId, nodes: &AstNodes<'_>, facts: &Facts) -> Option<NodeId> {
    let mut in_jsx = false;
    for id in nodes.ancestor_ids(call) {
        match nodes.kind(id) {
            kind if is_boundary(kind) => return in_jsx.then_some(id),
            AstKind::JSXExpressionContainer(container) => {
                match nodes.parent_kind(id) {
                    AstKind::JSXElement(_) | AstKind::JSXFragment(_) => {
                        if has_literal_condition(container.expression.as_expression()?, facts) {
                            return None;
                        }
                    }
                    AstKind::JSXAttribute(_) => {}
                    _ => return None,
                }
                in_jsx = true;
            }
            AstKind::JSXAttribute(attribute) => {
                let opening = nodes.ancestor_ids(id).find_map(|a| match nodes.kind(a) {
                    AstKind::JSXOpeningElement(opening) => Some(opening),
                    _ => None,
                })?;
                if !is_reactive_attribute(attribute, opening) {
                    return None;
                }
            }
            AstKind::JSXSpreadAttribute(_) | AstKind::JSXSpreadChild(_) => return None,
            _ => {}
        }
    }
    None
}

/// A child `a && b` or `a ? b : c` whose condition O5 decides statically.
fn has_literal_condition(e: &Expression<'_>, facts: &Facts) -> bool {
    match e.without_parentheses() {
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            literal_truthy(&logical.left, facts).is_some()
        }
        Expression::ConditionalExpression(conditional) => {
            literal_truthy(&conditional.test, facts).is_some()
        }
        _ => false,
    }
}

fn is_reactive_attribute(attribute: &JSXAttribute<'_>, opening: &JSXOpeningElement<'_>) -> bool {
    let is_native = match &opening.name {
        JSXElementName::Identifier(id) => is_native_name(id.name.as_str()),
        JSXElementName::IdentifierReference(id) => is_native_name(id.name.as_str()),
        JSXElementName::NamespacedName(_) => true,
        JSXElementName::MemberExpression(_) | JSXElementName::ThisExpression(_) => false,
    };
    let is_plain_name = match &attribute.name {
        JSXAttributeName::Identifier(id) => {
            let name = id.name.as_str();
            name != "ref" && name != "children" && !(is_native && name.starts_with("on"))
        }
        JSXAttributeName::NamespacedName(name) => {
            !is_native || matches!(name.namespace.name.as_str(), "prop" | "attr" | "bool")
        }
    };
    let has_spread =
        opening.attributes.iter().any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)));
    let same_name = opening
        .attributes
        .iter()
        .filter(|a| match a {
            JSXAttributeItem::Attribute(other) => same_attribute_name(&other.name, &attribute.name),
            JSXAttributeItem::SpreadAttribute(_) => false,
        })
        .count();
    is_plain_name && !(is_native && has_spread) && same_name == 1
}

fn same_attribute_name(a: &JSXAttributeName<'_>, b: &JSXAttributeName<'_>) -> bool {
    match (a, b) {
        (JSXAttributeName::Identifier(a), JSXAttributeName::Identifier(b)) => a.name == b.name,
        (JSXAttributeName::NamespacedName(a), JSXAttributeName::NamespacedName(b)) => {
            a.namespace.name == b.namespace.name && a.name.name == b.name.name
        }
        _ => false,
    }
}

/// Whether every free identifier of `expr` (declared outside `declarator`) names the same symbol
/// from `scope`.
fn resolves_alike(
    expr: &Expression<'_>,
    declarator: Span,
    scope: ScopeId,
    scoping: &Scoping,
) -> bool {
    let mut check = SameResolution { declarator, scope, scoping, is_alike: true };
    check.visit_expression(expr);
    check.is_alike
}

struct SameResolution<'s> {
    declarator: Span,
    scope: ScopeId,
    scoping: &'s Scoping,
    is_alike: bool,
}

impl<'a> Visit<'a> for SameResolution<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let symbol = it.reference_id.get().and_then(|r| self.scoping.get_reference(r).symbol_id());
        if symbol.is_some_and(|s| self.declarator.contains_inclusive(self.scoping.symbol_span(s))) {
            return;
        }
        self.is_alike &= self.scoping.find_binding(self.scope, it.name) == symbol;
    }
}

/// The declaration together with its line when it stands alone on it.
fn removal(source: &str, span: Span, is_top_level: bool) -> Span {
    let bytes = source.as_bytes();
    let is_blank = |b: u8| b == b' ' || b == b'\t';
    let mut start = span.start as usize;
    while start > 0 && is_blank(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = span.end as usize;
    while end < bytes.len() && is_blank(bytes[end]) {
        end += 1;
    }
    let starts_line = start == 0 || bytes[start - 1] == b'\n';
    let rest = &source[end..];
    let line_break = if rest.starts_with("\r\n") {
        2
    } else if rest.starts_with('\n') {
        1
    } else {
        0
    };
    if starts_line && line_break > 0 {
        let start = if is_top_level { span.start as usize } else { start };
        return Span::new(start as u32, (end + line_break) as u32);
    }
    Span::new(span.start, end as u32)
}
