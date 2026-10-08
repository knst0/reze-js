use std::collections::{HashMap, HashSet};

use oxc_allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_semantic::Scoping;
use oxc_span::Span;
use oxc_str::{Ident, Str};
use oxc_syntax::symbol::SymbolId;

use super::Namer;
use super::analysis::Primitive;
use crate::diagnostic::{Code, Report};
use crate::exports::{self, REACTIVITY, Role};
use crate::imports::HelperImports;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Syntax {
    Signal,
    Computed,
    Action,
}

impl Syntax {
    pub fn name(self) -> &'static str {
        match self {
            Syntax::Signal => "signal",
            Syntax::Computed => "computed",
            Syntax::Action => "action",
        }
    }

    pub const fn primitive(self) -> Primitive {
        match self {
            Syntax::Signal => Primitive::Signal,
            Syntax::Computed => Primitive::Computed,
            Syntax::Action => Primitive::Action,
        }
    }
}

pub struct SyntaxImport {
    pub specifier: Span,
    pub symbol: SymbolId,
    pub syntax: Syntax,
    pub is_unaliased: bool,
}

pub struct SyntaxImports {
    pub declared: Vec<SyntaxImport>,
    pub namespaces: HashMap<SymbolId, &'static str>,
}

impl SyntaxImports {
    pub fn collect(program: &Program<'_>) -> Self {
        let mut imports = SyntaxImports { declared: Vec::new(), namespaces: HashMap::new() };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.import_kind.is_type() {
                continue;
            }
            let source = import.source.value.as_str();
            let Some(module) = exports::module(source) else {
                continue;
            };
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        if named.import_kind.is_type() {
                            continue;
                        }
                        let imported = named.imported.name();
                        if let Some(syntax) = exports::syntax_named(source, imported.as_str()) {
                            imports.declared.push(SyntaxImport {
                                specifier: named.span,
                                symbol: named.local.symbol_id(),
                                syntax,
                                is_unaliased: named.local.name.as_str() == imported.as_str(),
                            });
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                        imports.namespaces.insert(namespace.local.symbol_id(), module);
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                }
            }
        }
        imports
    }

    pub fn syntax_of(&self, symbol: SymbolId) -> Option<Syntax> {
        self.declared.iter().find(|import| import.symbol == symbol).map(|import| import.syntax)
    }
}

pub(crate) struct ImportResult<'a> {
    pub changed: bool,
    pub targets: HashMap<SymbolId, &'a str>,
    pub locals: [Option<&'a str>; 3],
}

struct Relocation {
    local: String,
    home: &'static str,
}

type Moved<'a> = Vec<ArenaBox<'a, ImportSpecifier<'a>>>;

pub(crate) fn refuse_internal_reactivity(program: &Program<'_>, reports: &mut Vec<Report>) {
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() || import.source.value.as_str() != REACTIVITY {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else { continue };
            let name = named.imported.name();
            if named.import_kind.is_type() || exports::primitive_named(REACTIVITY, name.as_str()).is_none() {
                continue;
            }
            reports.push(Report::new(Code::InternalReactivityImport, named.span).arg("name", name.as_str()));
        }
    }
}

pub(crate) fn has_lowerable(program: &Program<'_>) -> bool {
    program.body.iter().any(|statement| {
        let Statement::ImportDeclaration(import) = statement else { return false };
        if import.import_kind.is_type() {
            return false;
        }
        let source = import.source.value.as_str();
        import.specifiers.iter().flatten().any(|specifier| {
            let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else {
                return false;
            };
            !named.import_kind.is_type()
                && exports::lowered_home(source, named.imported.name().as_str()).is_some()
        })
    })
}

pub(crate) fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    syntax: &SyntaxImports,
    namer: &mut Namer,
) -> ImportResult<'a> {
    let builder = AstBuilder::new(allocator);
    let mut kept: [Option<(u32, String)>; 3] = [None, None, None];
    for import in &syntax.declared {
        let index = import.syntax as usize;
        if kept[index].is_some() {
            continue;
        }
        let local = if import.is_unaliased {
            import.syntax.name().to_string()
        } else {
            namer.fresh(import.syntax.name())
        };
        kept[index] = Some((import.specifier.start, local));
    }
    let mut plan: HashMap<u32, Option<Relocation>> = HashMap::new();
    for import in &syntax.declared {
        let relocation = match &kept[import.syntax as usize] {
            Some((start, local)) if *start == import.specifier.start => {
                Some(Relocation { local: local.clone(), home: REACTIVITY })
            }
            _ => None,
        };
        plan.insert(import.specifier.start, relocation);
    }
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let source = import.source.value.as_str();
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else { continue };
            if named.import_kind.is_type() || plan.contains_key(&named.span.start) {
                continue;
            }
            if let Some(home) = exports::lowered_home(source, named.imported.name().as_str()) {
                plan.insert(
                    named.span.start,
                    Some(Relocation { local: named.local.name.to_string(), home }),
                );
            }
        }
    }
    if plan.is_empty() {
        return ImportResult { changed: false, targets: HashMap::new(), locals: [None; 3] };
    }
    let mut changed = false;
    let mut buckets: HashMap<&'static str, Moved<'a>> = HashMap::new();
    let mut home_decls: HashMap<&'static str, usize> = HashMap::new();
    let mut emptied: Vec<usize> = Vec::new();
    let mut insert_at: Option<usize> = None;
    for (index, statement) in program.body.iter_mut().enumerate() {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let source_home = exports::module(import.source.value.as_str());
        let is_type = import.import_kind.is_type();
        let Some(specs) = import.specifiers.as_mut() else { continue };
        if let Some(home) = source_home
            && !is_type
            && !specs
                .iter()
                .any(|spec| matches!(spec, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)))
        {
            home_decls.entry(home).or_insert(index);
        }
        if !specs.iter().any(|spec| {
            matches!(spec, ImportDeclarationSpecifier::ImportSpecifier(named) if plan.contains_key(&named.span.start))
        }) {
            continue;
        }
        if insert_at.is_none() {
            insert_at = Some(index);
        }
        let old = std::mem::replace(specs, ArenaVec::new_in(&builder));
        for spec in old {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(mut named)
                    if plan.contains_key(&named.span.start) =>
                {
                    if let Some(Some(relocation)) = plan.remove(&named.span.start) {
                        if named.local.name.as_str() != relocation.local.as_str() {
                            let text: &str = allocator.alloc_str(&relocation.local);
                            named.local.name = Ident::from(text);
                        }
                        buckets.entry(relocation.home).or_default().push(named);
                    }
                    changed = true;
                }
                other => specs.push(other),
            }
        }
        if specs.is_empty() {
            emptied.push(index);
        }
    }
    let mut ordered: Vec<(&'static str, Moved<'a>)> = buckets.into_iter().collect();
    ordered.sort_by(|a, b| a.0.cmp(b.0));
    let mut brand_new: Vec<(&'static str, Moved<'a>)> = Vec::new();
    let mut appends: HashMap<usize, Moved<'a>> = HashMap::new();
    let mut repurposed: HashMap<usize, (&'static str, Moved<'a>)> = HashMap::new();
    for (home, nodes) in ordered {
        if let Some(&decl) = home_decls.get(home) {
            appends.entry(decl).or_default().extend(nodes);
        } else if let Some(rep) = emptied.pop() {
            repurposed.insert(rep, (home, nodes));
        } else {
            brand_new.push((home, nodes));
        }
    }
    if !brand_new.is_empty()
        && let Some(at) = insert_at
        && emptied.contains(&at)
    {
        let (home, nodes) = brand_new.remove(0);
        emptied.retain(|&index| index != at);
        repurposed.insert(at, (home, nodes));
    }
    for (index, statement) in program.body.iter_mut().enumerate() {
        if let Some(nodes) = appends.remove(&index)
            && let Statement::ImportDeclaration(import) = statement
            && let Some(specs) = import.specifiers.as_mut()
        {
            for node in nodes {
                specs.push(ImportDeclarationSpecifier::ImportSpecifier(node));
            }
        }
        if let Some((home, nodes)) = repurposed.remove(&index)
            && let Statement::ImportDeclaration(import) = statement
        {
            import.source.value = Str::from(home);
            import.source.raw = None;
            import.source.lone_surrogates = false;
            let mut specs = ArenaVec::new_in(&builder);
            for node in nodes {
                specs.push(ImportDeclarationSpecifier::ImportSpecifier(node));
            }
            import.specifiers = Some(specs);
        }
        if appends.is_empty() && repurposed.is_empty() {
            break;
        }
    }
    let remove: HashSet<usize> = emptied.into_iter().collect();
    if !remove.is_empty() || !brand_new.is_empty() {
        let mut kept_body = ArenaVec::new_in(&builder);
        let mut made: Vec<Statement<'a>> = Vec::new();
        for (home, nodes) in std::mem::take(&mut brand_new) {
            let span = nodes.first().map(|node| node.span).unwrap_or(Span::empty(0));
            made.push(new_home_decl(allocator, span, home, nodes));
        }
        let old = std::mem::replace(&mut program.body, ArenaVec::new_in(&builder));
        for (index, statement) in old.into_iter().enumerate() {
            if remove.contains(&index) {
                continue;
            }
            kept_body.push(statement);
            if Some(index) == insert_at {
                for new in made.drain(..) {
                    kept_body.push(new);
                }
            }
        }
        for new in made.drain(..) {
            kept_body.push(new);
        }
        program.body = kept_body;
    }
    let mut targets = HashMap::new();
    let mut locals: [Option<&'a str>; 3] = [None; 3];
    for (index, slot) in kept.iter().enumerate() {
        if let Some((_, local)) = slot {
            locals[index] = Some(allocator.alloc_str(local));
        }
    }
    for import in &syntax.declared {
        if let Some(local) = locals[import.syntax as usize] {
            targets.insert(import.symbol, local);
        }
    }
    ImportResult { changed, targets, locals }
}

fn new_home_decl<'a>(
    allocator: &'a Allocator,
    span: Span,
    home: &'static str,
    nodes: Moved<'a>,
) -> Statement<'a> {
    let builder = AstBuilder::new(allocator);
    let mut specs = ArenaVec::new_in(&builder);
    for node in nodes {
        specs.push(ImportDeclarationSpecifier::ImportSpecifier(node));
    }
    let source = StringLiteral::new(span, Str::from(home), None, &builder);
    Statement::ImportDeclaration(ImportDeclaration::boxed(
        span,
        Some(specs),
        source,
        None,
        None,
        ImportOrExportKind::Value,
        &builder,
    ))
}

struct NamespaceMembers<'x, 'a> {
    allocator: &'a Allocator,
    scoping: &'x Scoping,
    namespaces: &'x HashMap<SymbolId, &'static str>,
    namer: &'x mut Namer<'a>,
    helpers: &'x mut HelperImports<'a>,
    changed: bool,
}

impl<'a> VisitMut<'a> for NamespaceMembers<'_, 'a> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        if let Expression::StaticMemberExpression(member) = it
            && !member.optional
            && let Expression::Identifier(object) = &member.object
            && let Some(reference) = object.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(reference).symbol_id()
            && self.namespaces.get(&symbol) == Some(&exports::PUBLIC)
            && let Some(entry) = exports::lookup(exports::PUBLIC, member.property.name.as_str())
            && matches!(entry.role, Role::Plain | Role::Call(_) | Role::Props(_))
        {
            let home = entry.lower_to.unwrap_or(exports::RUNTIME);
            let alias = self.helpers.require(self.allocator, self.namer, home, entry.name);
            let builder = AstBuilder::new(self.allocator);
            *it = Expression::new_identifier(member.span, Ident::from(alias), &builder);
            self.changed = true;
            return;
        }
        walk_mut::walk_expression(self, it);
    }
}

pub(crate) fn lower_namespace_members<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    scoping: &Scoping,
    syntax: &SyntaxImports,
    namer: &mut Namer<'a>,
    helpers: &mut HelperImports<'a>,
) -> bool {
    if !syntax.namespaces.values().any(|module| *module == exports::PUBLIC) {
        return false;
    }
    let mut members = NamespaceMembers {
        allocator,
        scoping,
        namespaces: &syntax.namespaces,
        namer,
        helpers,
        changed: false,
    };
    members.visit_program(program);
    members.changed
}

fn reexport_home(source: &str, specifier: &ExportSpecifier<'_>) -> Option<&'static str> {
    if source != exports::PUBLIC || specifier.export_kind.is_type() {
        return None;
    }
    let entry = exports::lookup(exports::PUBLIC, specifier.local.name().as_str())?;
    matches!(entry.role, Role::Plain | Role::Call(_) | Role::Props(_))
        .then_some(entry.lower_to.unwrap_or(exports::RUNTIME))
}

pub(crate) fn has_lowerable_reexport(program: &Program<'_>) -> bool {
    program.body.iter().any(|statement| {
        let Statement::ExportFromDeclaration(export) = statement else { return false };
        !export.export_kind.is_type()
            && export
                .specifiers
                .iter()
                .any(|specifier| reexport_home(export.source.value.as_str(), specifier).is_some())
    })
}

pub(crate) fn lower_reexports<'a>(allocator: &'a Allocator, program: &mut Program<'a>) -> bool {
    let builder = AstBuilder::new(allocator);
    let mut changed = false;
    let mut body = ArenaVec::with_capacity_in(program.body.len(), &builder);
    for statement in std::mem::replace(&mut program.body, ArenaVec::new_in(&builder)) {
        let Statement::ExportFromDeclaration(mut export) = statement else {
            body.push(statement);
            continue;
        };
        let source = export.source.value.as_str();
        if export.export_kind.is_type()
            || !export.specifiers.iter().any(|s| reexport_home(source, s).is_some())
        {
            body.push(Statement::ExportFromDeclaration(export));
            continue;
        }
        changed = true;
        let (moved, kept): (Vec<_>, Vec<_>) =
            std::mem::replace(&mut export.specifiers, ArenaVec::new_in(&builder))
                .into_iter()
                .partition(|s| reexport_home(source, s).is_some());
        let span = export.span;
        let kind = export.export_kind;
        let mut moved_specifiers = ArenaVec::with_capacity_in(moved.len(), &builder);
        moved_specifiers.extend(moved);
        let target = StringLiteral::new(span, Str::from(exports::RUNTIME), None, &builder);
        if kept.is_empty() {
            export.specifiers = moved_specifiers;
            export.source = target;
            body.push(Statement::ExportFromDeclaration(export));
        } else {
            export.specifiers.extend(kept);
            body.push(Statement::ExportFromDeclaration(export));
            body.push(Statement::ExportFromDeclaration(ExportFromDeclaration::boxed(
                span,
                moved_specifiers,
                target,
                kind,
                None,
                &builder,
            )));
        }
    }
    program.body = body;
    changed
}
