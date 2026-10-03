//! First pass of `$signal`, `$computed`, `$action` and `$props.merge`: rewrites the syntax into
//! the `signal` tuple, the `computed` getter, the `action` body and the merged object literal a
//! person would write. The second pass is the ordinary compiler, run on the rewritten text.

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
use crate::lower::{
    constant::{is_defined, static_property},
    is_component_name,
};
use crate::namer::Namer;

/// Compiler syntax and the runtime function it compiles to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Primitive {
    Signal,
    Computed,
    Action,
}

impl Primitive {
    const ALL: [Primitive; 3] = [Primitive::Signal, Primitive::Computed, Primitive::Action];

    fn dollar(self) -> &'static str {
        match self {
            Primitive::Signal => "$signal",
            Primitive::Computed => "$computed",
            Primitive::Action => "$action",
        }
    }

    fn runtime(self) -> &'static str {
        match self {
            Primitive::Signal => "signal",
            Primitive::Computed => "computed",
            Primitive::Action => "action",
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
    Primitive::ALL.into_iter().any(|p| source.contains(p.dollar())) || source.contains("$props")
}

#[cfg(test)]
mod t01_mentions_tests {
    use super::mentions_syntax;

    #[test]
    fn markers_preserve_substring_and_unicode_detection() {
        for marker in ["$signal", "$computed", "$action", "$props"] {
            for source in [
                marker.to_owned(),
                format!("Привет 🌍{marker}世界"),
                format!("// {marker}Suffix"),
                format!("const text = '{marker}';"),
                format!("$$invalid${marker}"),
            ] {
                assert!(mentions_syntax(&source), "{source:?}");
            }
            for end in 0..marker.len() {
                assert!(!mentions_syntax(&marker[..end]), "{:?}", &marker[..end]);
            }
        }
        for source in ["", "$", "Привет 🌍世界", "$Signal", "$prop", "$$invalid $si $act"]
        {
            assert!(!mentions_syntax(source), "{source:?}");
        }
    }

    #[test]
    fn generated_sources_match_original_predicate() {
        let fragments = [
            "",
            "Привет",
            "🌍",
            "$",
            "$$",
            "$si",
            "$prop",
            "$Signal",
            "$signal",
            "$computed",
            "$action",
            "$props",
            "$propsExtra",
            "/*",
            "'",
            "世界",
        ];
        for left in fragments {
            for middle in fragments {
                for right in fragments {
                    let source = format!("{left}{middle}{right}");
                    let expected = ["$signal", "$computed", "$action", "$props"]
                        .into_iter()
                        .any(|marker| source.contains(marker));
                    assert_eq!(mentions_syntax(&source), expected, "{source:?}");
                }
            }
        }
    }
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

/// `Ok(None)` when the file does not use the syntax, or does not parse: the
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
    if imports.dollar.is_empty() && imports.namespaces.is_empty() && imports.props.is_empty() {
        return Ok(None);
    }
    let PropsScanned { patches: props_patches, pending, needed, misuse } =
        scan_props_calls(source, program, &scoping, &imports);
    let has_props = !props_patches.is_empty() || !pending.is_empty();
    let mut namer = Namer::new(&scoping);
    let mut patches = Vec::new();
    let callees = imports.patch(source, &mut namer, &mut patches, &needed, &pending);
    patches.extend(props_patches);

    let mut declarations = Declarations {
        scoping: &scoping,
        imports: &imports,
        namer: &mut namer,
        variables: HashMap::new(),
        declarators: HashMap::new(),
        consumed: HashSet::new(),
        reports: Vec::new(),
        in_for_init: false,
        has_action: false,
    };
    declarations.visit_program(program);
    let Declarations { variables, declarators, mut reports, has_action, .. } = declarations;
    reports.extend(misuse);
    if variables.is_empty() && !has_action && reports.is_empty() && !has_props {
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
        run: if has_action { namer.fresh("_a$") } else { String::new() },
        frame: None,
        later: HashMap::new(),
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
    props: Vec<PropsImport>,
    /// The local name of a value import of each `$props` runtime function, by method index.
    props_runtime: [Option<String>; 3],
    /// The local name of a value import of each runtime function, by `Primitive`.
    runtime: [Option<String>; Primitive::ALL.len()],
    declarations: Vec<ImportRange>,
}

/// A `$props` named import: the compiler dissolves `$props.merge` of object literals, the rest
/// runs.
struct PropsImport {
    specifier: Span,
    symbol: SymbolId,
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
            props: Vec::new(),
            props_runtime: [None, None, None],
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
            let mut has_props = false;
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
                        } else if imported == "$props" {
                            has_props = true;
                            imports.props.push(PropsImport {
                                specifier: named.span,
                                symbol: named.local.symbol_id(),
                            });
                        } else if let Some(index) =
                            PROPS_RUNTIME.iter().position(|name| *name == imported)
                        {
                            imports.props_runtime[index]
                                .get_or_insert_with(|| named.local.name.to_string());
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
            if let (Some(mut range), true) = (range, has_dollar || has_props) {
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
    /// unless the file already imports it, the others are dropped. Every `$props` call is
    /// rewritten, so its specifiers go: the first becomes the runtime functions `needed`
    /// residue calls use, the rest is dropped. Returns the name each syntax local stands for
    fn patch(
        &self,
        source: &str,
        namer: &mut Namer<'_>,
        patches: &mut Vec<Patch>,
        needed: &[bool; 3],
        pending: &[(Span, usize)],
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
        let mut locals: [Option<String>; 3] = [None, None, None];
        if !self.props.is_empty() {
            let mut targets = self.props_runtime.clone();
            let mut texts = Vec::new();
            for (index, want) in needed.iter().enumerate() {
                if !want {
                    continue;
                }
                match &targets[index] {
                    Some(name) => locals[index] = Some(name.clone()),
                    None => {
                        let runtime = PROPS_RUNTIME[index];
                        let name = namer.fresh(runtime);
                        texts.push(if name == runtime {
                            name.clone()
                        } else {
                            format!("{runtime} as {name}")
                        });
                        targets[index] = Some(name.clone());
                        locals[index] = Some(name);
                    }
                }
            }
            let mut first = true;
            for import in &self.props {
                if first {
                    first = false;
                    replaced.insert(
                        import.specifier.start,
                        (!texts.is_empty()).then(|| texts.join(", ")),
                    );
                } else {
                    replaced.insert(import.specifier.start, None);
                }
            }
            for (span, index) in pending {
                if let Some(name) = &locals[*index] {
                    patches.push(Patch { start: span.start, end: span.end, text: name.clone() });
                }
            }
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

/// `$props` calls of one file: dissolve patches, residue rewrites into runtime calls, and uses
/// no call explains.
struct PropsCalls<'s> {
    source: &'s str,
    scoping: &'s Scoping,
    props: &'s [PropsImport],
    patches: Vec<Patch>,
    pending: Vec<(Span, usize)>,
    needed: [bool; 3],
    used: Vec<Span>,
    misuse: Vec<Report>,
}

/// Runtime functions `$props` calls compile to, by method index: `merge`, `splitByGroups`, `omit`.
const PROPS_RUNTIME: [&str; 3] = ["mergeProps", "splitProps", "omitProps"];

fn merge_property_is_static(property: &ObjectPropertyKind<'_>) -> bool {
    let ObjectPropertyKind::ObjectProperty(property) = property else { return false };
    if property.kind != PropertyKind::Init || property.method || !is_defined(&property.value) {
        return false;
    }
    match &property.key {
        PropertyKey::StaticIdentifier(key) => !property.computed && key.name != "__proto__",
        PropertyKey::StringLiteral(key) => key.value != "__proto__",
        PropertyKey::NumericLiteral(_) => true,
        _ => false,
    }
}

struct PropsScanned {
    patches: Vec<Patch>,
    pending: Vec<(Span, usize)>,
    needed: [bool; 3],
    misuse: Vec<Report>,
}

fn scan_props_calls(
    source: &str,
    program: &Program<'_>,
    scoping: &Scoping,
    imports: &Imports,
) -> PropsScanned {
    if imports.props.is_empty() {
        return PropsScanned {
            patches: Vec::new(),
            pending: Vec::new(),
            needed: [false; 3],
            misuse: Vec::new(),
        };
    }
    let mut calls = PropsCalls {
        source,
        scoping,
        props: &imports.props,
        patches: Vec::new(),
        pending: Vec::new(),
        needed: [false; 3],
        used: Vec::new(),
        misuse: Vec::new(),
    };
    calls.visit_program(program);
    let PropsCalls { patches, pending, needed, misuse, .. } = calls;
    PropsScanned { patches, pending, needed, misuse }
}

impl PropsCalls<'_> {
    /// The runtime function index of a `$props` call: `merge`, `splitByGroups`, `omit`.
    fn method(&self, callee: &Expression<'_>) -> Option<usize> {
        let Expression::StaticMemberExpression(member) = callee.without_parentheses() else {
            return None;
        };
        if member.optional {
            return None;
        }
        let Expression::Identifier(object) = &member.object else { return None };
        let symbol = symbol_of(self.scoping, object)?;
        if !self.props.iter().any(|p| p.symbol == symbol) {
            return None;
        }
        match member.property.name.as_str() {
            "merge" => Some(0),
            "splitByGroups" => Some(1),
            "omit" => Some(2),
            _ => None,
        }
    }

    /// Rewrites `$props.merge` of object literals into the literal a person would write, keeping
    /// every inner span: the call wrapper becomes braces, object boundaries become commas.
    fn dissolve(&mut self, call: &CallExpression<'_>) -> bool {
        if call.optional || call.type_arguments.is_some() {
            return false;
        }
        let mut objects = Vec::with_capacity(call.arguments.len());
        for arg in &call.arguments {
            let Some(e) = arg.as_expression() else { return false };
            let Expression::ObjectExpression(object) = e.without_parentheses() else {
                return false;
            };
            if !object.properties.iter().all(merge_property_is_static) {
                return false;
            }
            if let (Some(first), Some(last)) = (object.properties.first(), object.properties.last())
            {
                objects.push((first.span().start, last.span().end));
            }
        }
        if objects.is_empty() {
            self.patches.push(Patch {
                start: call.span.start,
                end: call.span.end,
                text: String::from("{}"),
            });
        } else {
            let inner = objects;
            self.patches.push(Patch {
                start: call.span.start,
                end: inner[0].0,
                text: String::from("{ "),
            });
            for pair in inner.windows(2) {
                self.patches.push(Patch {
                    start: pair[0].1,
                    end: pair[1].0,
                    text: String::from(", "),
                });
            }
            let last = inner[inner.len() - 1];
            self.patches.push(Patch {
                start: last.1,
                end: call.span.end,
                text: String::from(" }"),
            });
        }
        true
    }

    /// Rewrites `$props.splitByGroups` over an object literal into the views a person would
    /// write: one literal per group plus the rest, the first group claiming a repeated key.
    /// Values move verbatim, so only static keys matter — one source, no cross-part shadowing.
    fn dissolve_split(&mut self, call: &CallExpression<'_>) -> bool {
        if call.optional || call.type_arguments.is_some() || call.arguments.len() < 2 {
            return false;
        }
        let mut args = call.arguments.iter();
        let Some(props) = args.next().and_then(|arg| arg.as_expression()) else { return false };
        let Expression::ObjectExpression(object) = props.without_parentheses() else {
            return false;
        };
        let mut entries = Vec::with_capacity(object.properties.len());
        for property in &object.properties {
            let Some((key, _)) = static_property(property) else { return false };
            if key == "__proto__" {
                return false;
            }
            entries.push((key, property.span()));
        }
        let mut groups = Vec::with_capacity(call.arguments.len() - 1);
        for arg in args {
            let Some(list) = arg.as_expression() else { return false };
            let Expression::ArrayExpression(array) = list.without_parentheses() else {
                return false;
            };
            let mut keys = Vec::with_capacity(array.elements.len());
            for element in &array.elements {
                let Some(e) = element.as_expression() else { return false };
                let Expression::StringLiteral(key) = e.without_parentheses() else {
                    return false;
                };
                keys.push(key.value.as_str());
            }
            groups.push(keys);
        }
        let mut views: Vec<Vec<Span>> = groups.iter().map(|_| Vec::new()).collect();
        views.push(Vec::new());
        for (key, span) in entries {
            let mut placed = views.len() - 1;
            for (index, group) in groups.iter().enumerate() {
                if group.contains(&key) {
                    placed = index;
                    break;
                }
            }
            views[placed].push(span);
        }
        let mut text = String::from("[");
        for (index, view) in views.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            push_entries(&mut text, self.source, view);
        }
        text.push(']');
        self.patches.push(Patch { start: call.span.start, end: call.span.end, text });
        true
    }

    /// Rewrites `$props.omit` over an object literal into the rest a person would write.
    fn dissolve_omit(&mut self, call: &CallExpression<'_>) -> bool {
        if call.optional || call.type_arguments.is_some() || call.arguments.len() < 2 {
            return false;
        }
        let mut args = call.arguments.iter();
        let Some(props) = args.next().and_then(|arg| arg.as_expression()) else { return false };
        let Expression::ObjectExpression(object) = props.without_parentheses() else {
            return false;
        };
        let mut entries = Vec::with_capacity(object.properties.len());
        for property in &object.properties {
            let Some((key, _)) = static_property(property) else { return false };
            if key == "__proto__" {
                return false;
            }
            entries.push((key, property.span()));
        }
        let mut keys = Vec::with_capacity(call.arguments.len() - 1);
        for arg in args {
            let Some(key) = arg.as_expression() else { return false };
            let Expression::StringLiteral(literal) = key.without_parentheses() else {
                return false;
            };
            keys.push(literal.value.as_str());
        }
        let rest: Vec<Span> = entries
            .into_iter()
            .filter(|(key, _)| !keys.contains(key))
            .map(|(_, span)| span)
            .collect();
        let mut text = String::new();
        push_entries(&mut text, self.source, &rest);
        self.patches.push(Patch { start: call.span.start, end: call.span.end, text });
        true
    }
}

fn push_entries(text: &mut String, source: &str, props: &[Span]) {
    if props.is_empty() {
        text.push_str("{}");
        return;
    }
    text.push_str("{ ");
    for (index, span) in props.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&source[span.start as usize..span.end as usize]);
    }
    text.push_str(" }");
}

impl<'a> Visit<'a> for PropsCalls<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(index) = self.method(&it.callee) {
            let callee = it.callee.without_parentheses().span();
            self.used.push(callee);
            let dissolved = match index {
                0 => self.dissolve(it),
                1 => self.dissolve_split(it),
                _ => self.dissolve_omit(it),
            };
            if !dissolved {
                self.pending.push((callee, index));
                self.needed[index] = true;
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let Some(symbol) = symbol_of(self.scoping, it) else { return };
        if self.props.iter().any(|p| p.symbol == symbol)
            && !self
                .used
                .iter()
                .any(|callee| callee.start <= it.span.start && it.span.end <= callee.end)
        {
            self.misuse.push(Report::new(diagnostic::Code::PropsAsValue, it.span));
        }
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
    /// Start offsets of syntax callees that initialize a declaration or call `$action`.
    consumed: HashSet<u32>,
    reports: Vec<Report>,
    in_for_init: bool,
    has_action: bool,
}

/// The primitive `callee` names: an imported syntax function, or `ns.$name` of a runtime namespace.
fn callee_primitive(
    scoping: &Scoping,
    imports: &Imports,
    callee: &Expression<'_>,
) -> Option<Primitive> {
    match callee.without_parentheses() {
        Expression::Identifier(id) => symbol_of(scoping, id).and_then(|s| imports.primitive_of(s)),
        Expression::StaticMemberExpression(member) => namespace_member(scoping, imports, member),
        _ => None,
    }
}

/// The report for syntax used anywhere but where it is valid.
fn misused(primitive: Primitive, span: Span) -> Report {
    match primitive {
        Primitive::Action => Report::new(diagnostic::Code::ActionNotCalled, span),
        _ => Report::new(diagnostic::Code::SignalNotDeclared, span)
            .arg("primitive", primitive.dollar()),
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
        let Some(call) = dollar_call(init) else { return };
        let Some(primitive) = callee_primitive(self.scoping, self.imports, &call.callee) else {
            return;
        };
        if primitive == Primitive::Action {
            return;
        }
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

/// The primitive `member` names when it is `ns.$signal`, `ns.$computed` or `ns.$action` of a
/// runtime namespace.
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

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if callee_primitive(self.scoping, self.imports, &it.callee) == Some(Primitive::Action) {
            self.consumed.insert(it.callee.without_parentheses().span().start);
            self.has_action = true;
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(primitive) =
            symbol_of(self.scoping, it).and_then(|s| self.imports.primitive_of(s))
            && !self.consumed.contains(&it.span.start)
        {
            self.reports.push(misused(primitive, it.span));
        }
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some(primitive) = namespace_member(self.scoping, self.imports, it) {
            if !self.consumed.contains(&it.span.start) {
                self.reports.push(misused(primitive, it.span));
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
    /// The name of the run parameter every `$action` body gets.
    run: String,
    frame: Option<ActionFrame>,
    /// Start offsets of functions passed where they run later, with how to name them.
    later: HashMap<u32, &'static str>,
}

/// Where the rewriter is inside the body of the innermost `$action`.
#[derive(Default)]
struct ActionFrame {
    /// Functions entered inside the body; their `await`s are their own.
    nested: u32,
    /// The outermost entered function that likely runs after the action moved on.
    later: Option<&'static str>,
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
            Primitive::Action => unreachable!("`$action` declares no variable"),
        }
        if let Some(keyword) = plan.keyword {
            self.patch(keyword.start, keyword.end, "const");
        }
        self.rename_callee(&call.callee, primitive);
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

    /// Points the syntax callee at its runtime function.
    fn rename_callee(&mut self, callee: &Expression<'_>, primitive: Primitive) {
        match callee.without_parentheses() {
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
    }

    /// Rewrites `$action(fn, …)` into `action(fn', …)`, where `fn'` takes the run first and
    /// resumes it after each `await`.
    fn action(&mut self, call: &CallExpression<'_>) {
        self.rename_callee(&call.callee, Primitive::Action);
        let mut arguments = call.arguments.iter();
        let body = arguments.next();
        match body.and_then(Argument::as_expression).map(Expression::without_parentheses) {
            Some(Expression::ArrowFunctionExpression(arrow)) => {
                let outer = self.frame.replace(ActionFrame::default());
                self.run_parameter(&arrow.params, None);
                self.visit_formal_parameters(&arrow.params);
                match &arrow.body {
                    ArrowFunctionBody::FunctionBody(body) => self.action_block(body),
                    body => {
                        if let Some(expression) = body.as_expression() {
                            self.action_expression(expression);
                        }
                    }
                }
                self.frame = outer;
            }
            Some(Expression::FunctionExpression(function)) if function.generator => {
                self.reports.push(
                    Report::new(diagnostic::Code::ActionUnsupported, function.span)
                        .arg("construct", "generator"),
                );
            }
            Some(Expression::FunctionExpression(function)) => {
                let outer = self.frame.replace(ActionFrame::default());
                self.run_parameter(&function.params, function.this_param.as_deref());
                self.visit_formal_parameters(&function.params);
                if let Some(body) = &function.body {
                    self.action_block(body);
                }
                self.frame = outer;
            }
            _ => {
                let span = body.map_or(call.span, GetSpan::span);
                self.reports.push(Report::new(diagnostic::Code::ActionArgument, span));
                if let Some(body) = body {
                    self.visit_argument(body);
                }
            }
        }
        for argument in arguments {
            self.visit_argument(argument);
        }
    }

    fn run_parameter(&mut self, params: &FormalParameters<'_>, this: Option<&TSThisParameter<'_>>) {
        let run = self.run.clone();
        if let Some(this) = this {
            self.patch(this.span.end, this.span.end, format!(", {run}"));
        } else if self.text(params.span).starts_with('(') {
            let at = params.span.start + 1;
            let is_empty = params.items.is_empty() && params.rest.is_none();
            self.patch(at, at, if is_empty { run } else { format!("{run}, ") });
        } else {
            self.patch(params.span.start, params.span.start, format!("({run}, "));
            self.patch(params.span.end, params.span.end, ")");
        }
    }

    /// `=> expression` becomes `=> { try { return expression; } finally { run.end(); } }`.
    fn action_expression(&mut self, expression: &Expression<'_>) {
        let span = expression.span();
        self.patch(span.start, span.start, "{ try { return ");
        self.visit_expression(expression);
        let end = format!("; }} finally {{ {}.end(); }} }}", self.run);
        self.patch(span.end, span.end, end);
    }

    /// `{ body }` becomes `{ try { body } finally { run.end(); } }`, directives kept first.
    fn action_block(&mut self, body: &FunctionBody<'_>) {
        let at = body.directives.last().map_or(body.span.start + 1, |d| d.span.end);
        self.patch(at, at, " try {");
        for statement in &body.statements {
            self.visit_statement(statement);
        }
        let end = body.span.end - 1;
        self.patch(end, end, format!("}} finally {{ {}.end(); }} ", self.run));
    }

    /// Enters a function nested in an action body; the result restores the frame on `leave`.
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

    /// The body level of an action, where `await` suspends the action itself.
    fn in_action_body(&self) -> bool {
        self.frame.as_ref().is_some_and(|frame| frame.nested == 0)
    }

    fn check_nested_write(&mut self, span: Span, target_is_member: bool) {
        if let (true, Some(via)) = (target_is_member, self.frame.as_ref().and_then(|f| f.later)) {
            self.reports
                .push(Report::new(diagnostic::Code::ActionNestedWrite, span).arg("via", via));
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
        match it.operator {
            UnaryOperator::Void => self.mark_discarded(&it.argument),
            UnaryOperator::Delete => {
                self.check_nested_write(it.span, it.argument.is_member_expression())
            }
            _ => {}
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if callee_primitive(self.scoping, self.imports, &it.callee) == Some(Primitive::Action) {
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

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        if !self.in_action_body() {
            walk::walk_await_expression(self, it);
            return;
        }
        let argument = it.argument.span();
        let (resume, suspend) = (format!("{}.resume(", self.run), format!("{}.suspend(", self.run));
        self.patch(it.span.start, it.span.start, resume);
        self.patch(argument.start, argument.start, suspend);
        self.visit_expression(&it.argument);
        self.patch(argument.end, argument.end, "))");
    }

    fn visit_try_statement(&mut self, it: &TryStatement<'a>) {
        if self.in_action_body() {
            let try_awaits = block_awaits(&it.block);
            let resume = format!(" {}.resume();", self.run);
            if let (true, Some(handler)) = (try_awaits, &it.handler) {
                let at = handler.body.span.start + 1;
                self.patch(at, at, resume.clone());
            }
            let catch_awaits =
                it.handler.as_ref().is_some_and(|handler| block_awaits(&handler.body));
            if let (true, Some(finalizer)) = (try_awaits || catch_awaits, &it.finalizer) {
                let at = finalizer.span.start + 1;
                self.patch(at, at, resume);
            }
        }
        walk::walk_try_statement(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if it.r#await && self.in_action_body() {
            let span = Span::sized(it.span.start, 9);
            self.reports.push(
                Report::new(diagnostic::Code::ActionUnsupported, span)
                    .arg("construct", "for await"),
            );
        }
        walk::walk_for_of_statement(self, it);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if it.kind == VariableDeclarationKind::AwaitUsing && self.in_action_body() {
            let span = Span::sized(it.span.start, 11);
            self.reports.push(
                Report::new(diagnostic::Code::ActionUnsupported, span)
                    .arg("construct", "await using"),
            );
        }
        walk::walk_variable_declaration(self, it);
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
        if let AssignmentTarget::AssignmentTargetIdentifier(id) = &it.left
            && let Some((symbol, variable)) = self.reactive_of(id)
        {
            match variable.primitive {
                Primitive::Signal => self.assign(it, symbol),
                Primitive::Computed => self.computed_written(it.span, symbol),
                Primitive::Action => unreachable!("`$action` declares no variable"),
            }
            return;
        }
        self.check_nested_write(it.span, it.left.is_member_expression());
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &it.argument
            && let Some((symbol, variable)) = self.reactive_of(id)
        {
            match variable.primitive {
                Primitive::Signal => self.update(it, symbol),
                Primitive::Computed => self.computed_written(it.span, symbol),
                Primitive::Action => unreachable!("`$action` declares no variable"),
            }
            return;
        }
        self.check_nested_write(it.span, it.argument.is_member_expression());
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
            (true, Primitive::Action) => unreachable!("`$action` declares no variable"),
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

/// Finds an `await` or `yield` outside nested functions.
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

/// Whether `e` awaits or yields outside a nested function, so an arrow around it would not parse.
fn suspends(e: &Expression<'_>) -> bool {
    let mut check = Suspends::default();
    check.visit_expression(e);
    check.0
}

/// Whether `block` awaits outside a nested function, so code after it may run without the action.
fn block_awaits(block: &BlockStatement<'_>) -> bool {
    let mut check = Suspends::default();
    check.visit_block_statement(block);
    check.0
}

/// How to name a function passed to `callee` when `callee` likely calls it after the caller moved on.
fn runs_later(callee: &Expression<'_>) -> Option<&'static str> {
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

#[cfg(test)]
mod t02_offset_tests {
    use super::{Patch, Rewritten, apply, rewrite};
    use oxc_span::SourceType;

    fn linear(rewritten: &Rewritten, offset: u32, is_end: bool) -> u32 {
        let mut source_cursor = 0;
        let mut text_cursor = 0;
        for span in &rewritten.spans {
            let passed = if is_end { span.text_end < offset } else { span.text_end <= offset };
            if passed {
                source_cursor = span.source_end;
                text_cursor = span.text_end;
                continue;
            }
            let inside = if is_end { span.text_start < offset } else { span.text_start <= offset };
            return if inside {
                if is_end { span.source_end } else { span.source_start }
            } else {
                offset - text_cursor + source_cursor
            };
        }
        offset - text_cursor + source_cursor
    }

    fn check(source: &str, rewritten: &Rewritten) {
        for offset in 0..=rewritten.code.text.len() as u32 {
            assert_eq!(rewritten.start(offset), linear(rewritten, offset, false));
            assert_eq!(rewritten.end(offset), linear(rewritten, offset, true));
            assert!(rewritten.start(offset) <= source.len() as u32);
            assert!(rewritten.end(offset) <= source.len() as u32);
        }
        let mut source_cursor = 0;
        let mut text_cursor = 0;
        for span in &rewritten.spans {
            assert_eq!(
                &source[source_cursor as usize..span.source_start as usize],
                &rewritten.code.text[text_cursor as usize..span.text_start as usize]
            );
            for offset in text_cursor + 1..span.text_start {
                let original = source_cursor + offset - text_cursor;
                assert_eq!(rewritten.start(offset), original);
                assert_eq!(rewritten.end(offset), original);
                assert_eq!(offset, text_cursor + original - source_cursor);
            }
            source_cursor = span.source_end;
            text_cursor = span.text_end;
        }
        assert_eq!(&source[source_cursor as usize..], &rewritten.code.text[text_cursor as usize..]);
        for offset in text_cursor + 1..=rewritten.code.text.len() as u32 {
            let original = source_cursor + offset - text_cursor;
            assert_eq!(rewritten.start(offset), original);
            assert_eq!(rewritten.end(offset), original);
        }
    }

    #[test]
    fn replacement_interiors_and_shared_boundaries_are_directional() {
        let rewritten = apply(
            "abcdef",
            &mut [
                Patch { start: 0, end: 2, text: "XYZ".into() },
                Patch { start: 2, end: 4, text: "Q".into() },
            ],
        );
        assert_eq!(rewritten.code.text, "XYZQef");
        assert_eq!((rewritten.start(0), rewritten.end(0)), (0, 0));
        assert_eq!((rewritten.start(1), rewritten.end(1)), (0, 2));
        assert_eq!((rewritten.start(3), rewritten.end(3)), (2, 2));
        assert_eq!((rewritten.start(4), rewritten.end(4)), (4, 4));
        assert_eq!((rewritten.start(6), rewritten.end(6)), (6, 6));
        check("abcdef", &rewritten);
    }

    #[test]
    fn collapsed_deletions_keep_start_and_end_boundary_bias() {
        let rewritten = apply(
            "abc",
            &mut [
                Patch { start: 0, end: 1, text: String::new() },
                Patch { start: 1, end: 1, text: String::new() },
                Patch { start: 1, end: 2, text: String::new() },
            ],
        );
        assert_eq!(rewritten.code.text, "c");
        assert_eq!((rewritten.start(0), rewritten.end(0)), (2, 0));
        assert_eq!((rewritten.start(1), rewritten.end(1)), (3, 3));
        check("abc", &rewritten);
    }

    #[test]
    fn utf8_empty_inserted_deleted_and_adjacent_patches_match_linear_mapping() {
        let source = "α🌍世界z";
        let boundaries: Vec<u32> = source
            .char_indices()
            .map(|(i, _)| i as u32)
            .chain(std::iter::once(source.len() as u32))
            .collect();
        for &a in &boundaries {
            for &b in boundaries.iter().filter(|&&b| b >= a) {
                for &c in boundaries.iter().filter(|&&c| c >= b) {
                    for left in ["", "x", "λ🌍"] {
                        for right in ["", "Q", "世界"] {
                            let rewritten = apply(
                                source,
                                &mut [
                                    Patch { start: a, end: b, text: left.into() },
                                    Patch { start: b, end: c, text: right.into() },
                                    Patch { start: c, end: c, text: String::new() },
                                ],
                            );
                            check(source, &rewritten);
                        }
                    }
                }
            }
        }
        check(source, &apply(source, &mut []));
    }

    #[test]
    fn deterministic_adjacent_dsl_fuzz_preserves_copied_intervals() {
        let mut state = 0x9e37_79b9u32;
        for case in 0..128 {
            let mut source = String::from("import { $signal, $computed, $action } from 'reze-js';");
            for index in 0..8 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let gap = ["", " ", "\n", "/* 世界🌍 */"][state as usize % 4];
                source.push_str(&format!("{gap}let s{index}=$signal({});const c{index}=$computed(s{index}+1);$action(()=>{{s{index}++;}});", state % 100));
            }
            let result = rewrite(&source, SourceType::mjs());
            let rewritten = match result {
                Ok(Some((rewritten, _))) => rewritten,
                _ => panic!("DSL fixture did not rewrite: {case}"),
            };
            assert!(!rewritten.spans.is_empty());
            check(&source, &rewritten);
        }
    }
}
