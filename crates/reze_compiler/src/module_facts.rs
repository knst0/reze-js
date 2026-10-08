use std::collections::{BTreeMap, HashMap, HashSet};

use oxc_ast::ast::{
    ArrowFunctionBody, BindingIdentifier, BindingPattern, Declaration, Expression, Function,
    FunctionBody, ImportDeclarationSpecifier, ImportSpecifier, ModuleExportName, Program,
    Statement,
};
use oxc_ast_visit::Visit;
use oxc_semantic::{Scoping, SymbolFlags, SymbolId};
use serde::{Deserialize, Serialize};

use crate::frontend::analysis::{self, Primitive, SharedFacts};
use crate::kind::Kind;

pub const VERSION: u32 = 1;
const COMPILER: &str = env!("CARGO_PKG_VERSION");

/// Reactive facts about one module's exports. The facts are a function of the module's own source
/// only; `hash` is the FNV-1a64 of that source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleFacts {
    pub v: u32,
    pub compiler: String,
    pub hash: String,
    pub exports: BTreeMap<String, ExportFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum ExportFacts {
    /// `written`: `never` when no code writes the setter; `local` when every write is in this
    /// module; `external` when the setter is exported and other modules may write it. `kind` is
    /// present only when every write is in this module and proves the kind.
    Signal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<FactKind>,
        written: Written,
    },
    Computed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<FactKind>,
    },
    Action,
    /// `reads`: `none` reads no reactive state; `fixed` reads only stable getters on every call;
    /// `dynamic` otherwise.
    Function {
        reads: Reads,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        returns: Option<FactKind>,
    },
    Value,
    Reexport {
        from: String,
        name: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactKind {
    Number,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Written {
    Never,
    Local,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reads {
    None,
    Fixed,
    Dynamic,
}

impl From<Kind> for FactKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Numeric => Self::Number,
            Kind::String => Self::String,
        }
    }
}

impl From<FactKind> for Kind {
    fn from(kind: FactKind) -> Self {
        match kind {
            FactKind::Number => Self::Numeric,
            FactKind::String => Self::String,
        }
    }
}

/// Facts for the exports of `program`. Depends only on `program` and `facts` computed from it, so
/// callers that apply imported facts afterwards keep the result import-independent.
pub fn collect(
    program: &Program<'_>,
    scoping: &Scoping,
    facts: &SharedFacts,
    hash: String,
) -> ModuleFacts {
    let exported = analysis::exported_symbols(program, scoping);
    let mut declared = HashMap::new();
    let mut imports = HashMap::new();
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) if !import.import_kind.is_type() => {
                for specifier in import.specifiers.iter().flatten() {
                    let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                        continue;
                    };
                    if !specifier.import_kind.is_type() {
                        imports.insert(
                            specifier.local.symbol_id(),
                            (
                                import.source.value.to_string(),
                                specifier.imported.name().to_string(),
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for statement in &program.body {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => Some(&export.declaration),
            other => other.as_declaration(),
        };
        match declaration {
            Some(Declaration::VariableDeclaration(variables)) => {
                for declarator in &variables.declarations {
                    let init = match &declarator.id {
                        BindingPattern::BindingIdentifier(_) => declarator.init.as_ref(),
                        _ => None,
                    };
                    for id in declarator.id.get_binding_identifiers() {
                        declared.insert(id.symbol_id(), Declared::Init(init));
                    }
                }
            }
            Some(Declaration::FunctionDeclaration(function)) => {
                if let Some(id) = &function.id {
                    declared.insert(id.symbol_id(), Declared::Function(function));
                }
            }
            _ => {}
        }
    }
    let describe = |symbol: SymbolId| -> ExportFacts {
        if let Some(&setter) = facts.signal_setters.get(&symbol) {
            return ExportFacts::Signal {
                kind: facts.getter_kinds.get(&symbol).copied().map(FactKind::from),
                written: match setter {
                    None => Written::Never,
                    Some(setter) if exported.contains(&setter) => Written::External,
                    Some(setter) if scoping.get_resolved_reference_ids(setter).is_empty() => {
                        Written::Never
                    }
                    Some(_) => Written::Local,
                },
            };
        }
        if facts.computed_getters.contains(&symbol) {
            let kind = match declared.get(&symbol) {
                Some(Declared::Init(Some(init))) => match init.get_inner_expression() {
                    Expression::CallExpression(call) => call
                        .arguments
                        .first()
                        .and_then(|argument| argument.as_expression())
                        .and_then(|expression| facts.kind_of(scoping, expression)),
                    _ => None,
                },
                _ => None,
            };
            return ExportFacts::Computed { kind: kind.map(FactKind::from) };
        }
        match declared.get(&symbol) {
            Some(Declared::Function(function)) => {
                function_facts(facts, scoping, function.body.as_deref())
            }
            Some(Declared::Init(Some(init))) => match init.get_inner_expression() {
                Expression::ArrowFunctionExpression(arrow) => {
                    arrow_facts(facts, scoping, &arrow.body)
                }
                Expression::FunctionExpression(function) => {
                    function_facts(facts, scoping, function.body.as_deref())
                }
                Expression::CallExpression(call)
                    if facts.primitive(scoping, &call.callee) == Some(Primitive::Action) =>
                {
                    ExportFacts::Action
                }
                _ => ExportFacts::Value,
            },
            _ => match imports.get(&symbol) {
                Some((from, name)) => {
                    ExportFacts::Reexport { from: from.clone(), name: name.clone() }
                }
                None => ExportFacts::Value,
            },
        }
    };

    let mut exports = BTreeMap::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => {
                for (name, symbol) in declared_names(&export.declaration) {
                    exports.insert(name, describe(symbol));
                }
            }
            Statement::ExportNamedDeclaration(export) if !export.export_kind.is_type() => {
                for specifier in &export.specifiers {
                    if specifier.export_kind.is_type() {
                        continue;
                    }
                    let fact = match &specifier.local {
                        ModuleExportName::IdentifierReference(local) => match local
                            .reference_id
                            .get()
                            .and_then(|r| scoping.get_reference(r).symbol_id())
                        {
                            Some(symbol) => match imports.get(&symbol) {
                                Some((from, imported)) => ExportFacts::Reexport {
                                    from: from.clone(),
                                    name: imported.clone(),
                                },
                                None => describe(symbol),
                            },
                            None => ExportFacts::Value,
                        },
                        _ => ExportFacts::Value,
                    };
                    exports.insert(specifier.exported.name().to_string(), fact);
                }
            }
            Statement::ExportFromDeclaration(export) if !export.export_kind.is_type() => {
                for specifier in &export.specifiers {
                    if specifier.export_kind.is_type() {
                        continue;
                    }
                    exports.insert(
                        specifier.exported.name().to_string(),
                        ExportFacts::Reexport {
                            from: export.source.value.to_string(),
                            name: module_export_name(&specifier.local),
                        },
                    );
                }
            }
            _ => {}
        }
    }
    ModuleFacts { v: VERSION, compiler: COMPILER.to_owned(), hash, exports }
}

/// Adds the reactive facts of imported bindings to `facts`, so the module's own code sees imported
/// signals and computeds as stable getters and imported functions as typed calls. Facts whose
/// version or compiler differs from this build are ignored; the hash check is the caller's
/// responsibility since only the caller holds the imported source.
pub fn apply_imports(
    program: &Program<'_>,
    scoping: &Scoping,
    imported: &HashMap<String, ModuleFacts>,
    facts: &mut SharedFacts,
) {
    for (specifier, export) in imported_exports(program, imported) {
        let symbol = specifier.local.symbol_id();
        match export {
            ExportFacts::Signal { kind, .. } | ExportFacts::Computed { kind } => {
                facts.getter_refs.extend(scoping.get_resolved_reference_ids(symbol));
                if let Some(kind) = kind {
                    facts.getter_kinds.insert(symbol, Kind::from(*kind));
                }
            }
            ExportFacts::Function { returns: Some(kind), .. } => {
                facts.callee_kinds.insert(symbol, Kind::from(*kind));
            }
            _ => {}
        }
    }
}

/// Imported bindings that read as signal or computed getters: named imports by local symbol, and
/// namespace imports with their getter export names. The normalizer lowers their reads to getter
/// calls before the module's own facts are collected.
#[derive(Default)]
pub(crate) struct ImportedGetters {
    pub(crate) bindings: HashSet<SymbolId>,
    pub(crate) members: HashMap<SymbolId, HashSet<String>>,
}

impl ImportedGetters {
    pub(crate) fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.members.is_empty()
    }
}

pub(crate) fn imported_getters(
    program: &Program<'_>,
    imported: &HashMap<String, ModuleFacts>,
) -> ImportedGetters {
    let mut getters = ImportedGetters::default();
    for (specifier, export) in imported_exports(program, imported) {
        if is_getter(export) {
            getters.bindings.insert(specifier.local.symbol_id());
        }
    }
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let Some(module) = compatible_module(import.source.value.as_str(), imported) else {
            continue;
        };
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) = specifier else {
                continue;
            };
            let names: HashSet<String> = module
                .exports
                .iter()
                .filter(|(_, export)| is_getter(export))
                .map(|(name, _)| name.clone())
                .collect();
            if !names.is_empty() {
                getters.members.insert(namespace.local.symbol_id(), names);
            }
        }
    }
    getters
}

fn is_getter(export: &ExportFacts) -> bool {
    matches!(export, ExportFacts::Signal { .. } | ExportFacts::Computed { .. })
}

fn compatible_module<'b>(
    source: &str,
    imported: &'b HashMap<String, ModuleFacts>,
) -> Option<&'b ModuleFacts> {
    imported.get(source).filter(|module| module.v == VERSION && module.compiler == COMPILER)
}

fn imported_exports<'b, 'a>(
    program: &'b Program<'a>,
    imported: &'b HashMap<String, ModuleFacts>,
) -> Vec<(&'b ImportSpecifier<'a>, &'b ExportFacts)> {
    let mut exports = Vec::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let Some(module) = compatible_module(import.source.value.as_str(), imported) else {
            continue;
        };
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                continue;
            };
            if specifier.import_kind.is_type() {
                continue;
            }
            if let Some(export) = module.exports.get(specifier.imported.name().as_str()) {
                exports.push((&**specifier, export));
            }
        }
    }
    exports
}

enum Declared<'b, 'a> {
    Init(Option<&'b Expression<'a>>),
    Function(&'b Function<'a>),
}

fn declared_names<'a>(declaration: &Declaration<'a>) -> Vec<(String, SymbolId)> {
    match declaration {
        Declaration::VariableDeclaration(variables) => variables
            .declarations
            .iter()
            .flat_map(|declarator| declarator.id.get_binding_identifiers())
            .map(|id: &BindingIdentifier<'a>| (id.name.to_string(), id.symbol_id()))
            .collect(),
        Declaration::FunctionDeclaration(function) => {
            function.id.iter().map(|id| (id.name.to_string(), id.symbol_id())).collect()
        }
        _ => Vec::new(),
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> String {
    name.name().to_string()
}

fn arrow_returned_expression<'b, 'a>(
    body: &'b ArrowFunctionBody<'a>,
) -> Option<&'b Expression<'a>> {
    match body {
        ArrowFunctionBody::FunctionBody(block) => returned_in_block(block.statements.as_slice()),
        expression => expression.as_expression(),
    }
}

fn returned_in_block<'b, 'a>(statements: &'b [Statement<'a>]) -> Option<&'b Expression<'a>> {
    let [statement] = statements else { return None };
    match statement {
        Statement::ReturnStatement(ret) => ret.argument.as_ref(),
        _ => None,
    }
}

fn function_facts(
    facts: &SharedFacts,
    scoping: &Scoping,
    body: Option<&FunctionBody<'_>>,
) -> ExportFacts {
    let Some(body) = body else {
        return ExportFacts::Function { reads: Reads::None, returns: None };
    };
    let mut finder = GetterReads { facts, scoping, found: false, imports: false };
    finder.visit_function_body(body);
    summarize_body(facts, scoping, &finder, returned_in_block(body.statements.as_slice()))
}

fn arrow_facts(
    facts: &SharedFacts,
    scoping: &Scoping,
    body: &ArrowFunctionBody<'_>,
) -> ExportFacts {
    let mut finder = GetterReads { facts, scoping, found: false, imports: false };
    finder.visit_arrow_function_body(body);
    summarize_body(facts, scoping, &finder, arrow_returned_expression(body))
}

fn summarize_body(
    facts: &SharedFacts,
    scoping: &Scoping,
    finder: &GetterReads<'_, '_>,
    returned: Option<&Expression<'_>>,
) -> ExportFacts {
    let reads = if finder.imports {
        Reads::Dynamic
    } else if !finder.found {
        Reads::None
    } else if returned.is_some_and(|e| facts.reads_unconditionally(scoping, e)) {
        Reads::Fixed
    } else {
        Reads::Dynamic
    };
    let returns = returned.and_then(|e| facts.kind_of(scoping, e)).map(FactKind::from);
    ExportFacts::Function { reads, returns }
}

struct GetterReads<'f, 's> {
    facts: &'f SharedFacts,
    scoping: &'s Scoping,
    found: bool,
    imports: bool,
}

impl<'a> Visit<'a> for GetterReads<'_, '_> {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        let Some(reference) = it.reference_id.get() else { return };
        self.found |= self.facts.getter_refs.contains(&reference);
        self.imports |=
            self.scoping.get_reference(reference).symbol_id().is_some_and(|symbol| {
                self.scoping.symbol_flags(symbol).contains(SymbolFlags::Import)
            });
    }
}
