use std::collections::{HashMap, HashSet};

use oxc_allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::Span;
use oxc_str::{Ident, Str};
use oxc_syntax::symbol::SymbolId;

use super::Namer;

pub(crate) fn allows(source: &str, imported: &str) -> bool {
    match imported {
        "$signal" | "$computed" | "$action" => {
            matches!(source, "reze-js" | "@rezejs/signals")
        }
        "$props" => matches!(source, "reze-js" | "@rezejs/dom"),
        "signal" | "computed" | "action" => {
            matches!(source, "@rezejs/signals" | "reze-js/internal/reactivity")
        }
        "mergeProps" | "splitProps" | "omitProps" => {
            matches!(source, "reze-js" | "@rezejs/dom")
        }
        "asyncComputed" | "createUniqueId" => matches!(source, "reze-js" | "@rezejs/signals"),
        "asyncComponent" | "dynamic" | "dynamicElement" | "island" | "Show" | "For" | "Repeat"
        | "Switch" | "Match" | "Loading" | "Errored" | "Portal" => {
            matches!(source, "reze-js" | "@rezejs/dom")
        }
        _ => false,
    }
}
pub(crate) fn runtime_home(runtime: &str) -> Option<&'static str> {
    match runtime {
        "signal" | "computed" | "action" => Some("reze-js/internal/reactivity"),
        "effect" => Some("@rezejs/signals"),
        "renderEffect" => Some("@rezejs/signals/render"),
        "mergeProps" | "splitProps" | "omitProps" => Some("@rezejs/dom"),
        _ => None,
    }
}

pub(crate) fn home_of(source: &str) -> Option<&'static str> {
    match source {
        "reze-js" => Some("reze-js"),
        "reze-js/internal/reactivity" => Some("reze-js/internal/reactivity"),
        "@rezejs/dom" => Some("@rezejs/dom"),
        "@rezejs/signals" => Some("@rezejs/signals"),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Syntax {
    Signal,
    Computed,
    Action,
}

impl Syntax {
    const ALL: [Syntax; 3] = [Syntax::Signal, Syntax::Computed, Syntax::Action];

    pub fn dollar(self) -> &'static str {
        match self {
            Syntax::Signal => "$signal",
            Syntax::Computed => "$computed",
            Syntax::Action => "$action",
        }
    }

    pub fn runtime(self) -> &'static str {
        match self {
            Syntax::Signal => "signal",
            Syntax::Computed => "computed",
            Syntax::Action => "action",
        }
    }

    pub fn from_dollar(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.dollar() == name)
    }
}

pub struct DollarImport {
    pub specifier: Span,
    pub symbol: SymbolId,
    pub syntax: Syntax,
}

pub struct SyntaxImports {
    pub dollar: Vec<DollarImport>,
    pub namespaces: HashMap<SymbolId, &'static str>,
}

impl SyntaxImports {
    pub fn collect(program: &Program<'_>) -> Self {
        let mut imports = SyntaxImports { dollar: Vec::new(), namespaces: HashMap::new() };
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
                        let imported = named.imported.name();
                        if !allows(home, imported.as_str()) {
                            continue;
                        }
                        if let Some(syntax) = Syntax::from_dollar(imported.as_str()) {
                            imports.dollar.push(DollarImport {
                                specifier: named.span,
                                symbol: named.local.symbol_id(),
                                syntax,
                            });
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                        imports.namespaces.insert(namespace.local.symbol_id(), home);
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                }
            }
        }
        imports
    }

    pub fn syntax_of(&self, symbol: SymbolId) -> Option<Syntax> {
        self.dollar.iter().find(|import| import.symbol == symbol).map(|import| import.syntax)
    }
}

pub(crate) struct ImportResult<'a> {
    pub changed: bool,
    pub targets: HashMap<SymbolId, &'a str>,
    pub locals: [Option<&'a str>; 3],
}

pub(crate) fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    syntax: &SyntaxImports,
    namer: &mut Namer,
) -> ImportResult<'a> {
    let builder = AstBuilder::new(allocator);
    let empty =
        ImportResult { changed: false, targets: HashMap::new(), locals: [None, None, None] };
    if syntax.dollar.is_empty() {
        return empty;
    }
    let mut kept_spec: [Option<u32>; 3] = [None; 3];
    let mut kept_local: [Option<String>; 3] = [None, None, None];
    for import in &syntax.dollar {
        let index = import.syntax as usize;
        if kept_spec[index].is_some() {
            continue;
        }
        let local = namer.fresh(import.syntax.runtime());
        kept_spec[index] = Some(import.specifier.start);
        kept_local[index] = Some(local);
    }
    let dollar_starts: HashSet<u32> = syntax.dollar.iter().map(|i| i.specifier.start).collect();
    let mut plan: HashMap<u32, (usize, String, String)> = HashMap::new();
    for import in &syntax.dollar {
        let index = import.syntax as usize;
        if kept_spec[index] == Some(import.specifier.start) {
            plan.insert(
                import.specifier.start,
                (
                    index,
                    import.syntax.runtime().to_string(),
                    kept_local[index]
                        .clone()
                        .unwrap_or_else(|| import.syntax.runtime().to_string()),
                ),
            );
        }
    }
    let mut changed = false;
    let mut buckets: HashMap<&'static str, Vec<ArenaBox<'a, ImportSpecifier<'a>>>> = HashMap::new();
    let mut home_decls: HashMap<&'static str, usize> = HashMap::new();
    let mut emptied: Vec<usize> = Vec::new();
    let mut insert_at: Option<usize> = None;
    for (index, statement) in program.body.iter_mut().enumerate() {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let source_home = home_of(import.source.value.as_str());
        if let Some(home) = source_home {
            home_decls.entry(home).or_insert(index);
        }
        let Some(specs) = import.specifiers.as_mut() else { continue };
        if !specs.iter().any(|spec| {
            matches!(spec, ImportDeclarationSpecifier::ImportSpecifier(named) if dollar_starts.contains(&named.span.start))
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
                    if dollar_starts.contains(&named.span.start) =>
                {
                    match plan.remove(&named.span.start) {
                        Some((_, runtime, local)) => {
                            let home = runtime_home(&runtime).unwrap_or("@rezejs/signals");
                            set_imported(allocator, named.as_mut(), &runtime);
                            if named.local.name.as_str() != local.as_str() {
                                let text: &str = allocator.alloc_str(&local);
                                named.local.name = Ident::from(text);
                            }
                            if source_home == Some(home) {
                                specs.push(ImportDeclarationSpecifier::ImportSpecifier(named));
                            } else {
                                buckets.entry(home).or_default().push(named);
                            }
                            changed = true;
                        }
                        None => {
                            changed = true;
                        }
                    }
                }
                other => specs.push(other),
            }
        }
        if specs.is_empty() {
            emptied.push(index);
        }
    }
    let mut ordered: Vec<(&'static str, Vec<ArenaBox<'a, ImportSpecifier<'a>>>)> =
        buckets.into_iter().collect();
    ordered.sort_by(|a, b| a.0.cmp(b.0));
    let mut brand_new: Vec<(&'static str, Vec<ArenaBox<'a, ImportSpecifier<'a>>>)> = Vec::new();
    let mut appends: HashMap<usize, Vec<ArenaBox<'a, ImportSpecifier<'a>>>> = HashMap::new();
    let mut repurposed: HashMap<usize, (&'static str, Vec<ArenaBox<'a, ImportSpecifier<'a>>>)> =
        HashMap::new();
    for (home, nodes) in ordered {
        if let Some(&decl) = home_decls.get(home) {
            appends.entry(decl).or_default().extend(nodes);
        } else if let Some(rep) = emptied.pop() {
            repurposed.insert(rep, (home, nodes));
        } else {
            brand_new.push((home, nodes));
        }
    }
    if !brand_new.is_empty() {
        if let Some(at) = insert_at
            && emptied.contains(&at)
        {
            let (home, nodes) = brand_new.remove(0);
            emptied.retain(|&index| index != at);
            repurposed.insert(at, (home, nodes));
        }
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
        let mut kept = ArenaVec::new_in(&builder);
        let mut made: Vec<Statement<'a>> = Vec::new();
        for (home, nodes) in std::mem::replace(&mut brand_new, Vec::new()) {
            let span = nodes.first().map(|node| node.span).unwrap_or(Span::empty(0));
            made.push(new_home_decl(allocator, span, home, nodes));
        }
        let old = std::mem::replace(&mut program.body, ArenaVec::new_in(&builder));
        for (index, statement) in old.into_iter().enumerate() {
            if remove.contains(&index) {
                continue;
            }
            kept.push(statement);
            if Some(index) == insert_at {
                for new in made.drain(..) {
                    kept.push(new);
                }
            }
        }
        for new in made.drain(..) {
            kept.push(new);
        }
        program.body = kept;
    }
    let mut arena_local: HashMap<usize, &'a str> = HashMap::new();
    let mut targets = HashMap::new();
    for import in &syntax.dollar {
        let index = import.syntax as usize;
        let local = *arena_local.entry(index).or_insert_with(|| {
            let text: &'a str = allocator
                .alloc_str(kept_local[index].as_deref().unwrap_or(import.syntax.runtime()));
            text
        });
        targets.insert(import.symbol, local);
    }
    let locals: [Option<&'a str>; 3] = [0, 1, 2].map(|i| arena_local.get(&i).copied());
    ImportResult { changed, targets, locals }
}

fn set_imported<'a>(allocator: &'a Allocator, named: &mut ImportSpecifier<'a>, runtime: &str) {
    let builder = AstBuilder::new(allocator);
    let text: &'a str = allocator.alloc_str(runtime);
    match &mut named.imported {
        ModuleExportName::IdentifierName(name) => {
            name.name = Ident::from(text);
        }
        _ => {
            named.imported =
                ModuleExportName::new_identifier_name(named.span, Ident::from(text), &builder);
        }
    }
}

fn new_home_decl<'a>(
    allocator: &'a Allocator,
    span: Span,
    home: &'static str,
    nodes: Vec<ArenaBox<'a, ImportSpecifier<'a>>>,
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
