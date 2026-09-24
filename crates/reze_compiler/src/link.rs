//! `link`: resolves every use across the program, finds open modules and escaping bindings, and
//! takes the cross-module decisions of SPEC §15.4–§15.11.

use std::collections::{BTreeSet, HashMap, HashSet};

use oxc_span::Span;

use crate::diagnostic::{Code, Related};
use crate::facts::{
    self, ComponentFact, FoldedImport, FoldedSignal, ImportRef, IslandFact, LeafNames, ModuleFacts,
    Primitive, PrimitiveImport, Reason, RootFact, RootIsland, StoreExport, StoreImport, StoreRole,
};
use crate::features::{self, Features};
use crate::summary::{
    DeclarationKind, Dep, DepKind, Export, ImportName, ModuleSummary, Ref, UseClass, Violation,
};

pub struct ModuleInput {
    pub id: String,
    pub summary: ModuleSummary,
    /// Program module each of `summary.specifiers` resolves to; `None` outside the program.
    pub resolved: Vec<Option<String>>,
    pub is_entry: bool,
}

pub struct LinkOptions {
    pub optimize: bool,
    pub islands: bool,
    /// Directory island ids are relative to (`config.root`); ids outside it hash as given.
    pub root: String,
}

pub struct Linked {
    /// Facts for every module of the program, even those without decisions.
    pub facts: HashMap<String, ModuleFacts>,
    pub features: Features,
    /// Modules with an export that a cross-module rewrite depends on (§15.3).
    pub closed: Vec<String>,
}

type ModuleIndex = usize;

/// How one binding uses a store: its role, and the leaves it reads and writes.
type LeafUses = (StoreRole, BTreeSet<Vec<String>>, BTreeSet<Vec<String>>);

/// What a name used in a module is, program-wide.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Binding(ModuleIndex, u32),
    Namespace(ModuleIndex),
    Runtime(String),
    RuntimeNamespace,
    External,
    Unresolved,
}

/// A use, resolved: where it is, what it is, what it uses.
struct ResolvedUse {
    module: ModuleIndex,
    index: usize,
    target: Target,
    /// Through a namespace member (`ns.x`).
    via_namespace: bool,
}

struct Program<'m> {
    modules: &'m [ModuleInput],
    resolved: Vec<Vec<Option<ModuleIndex>>>,
    runtime: Vec<HashSet<u32>>,
    imports: Vec<HashMap<u32, usize>>,
}

pub fn link(modules: &[ModuleInput], options: &LinkOptions) -> Linked {
    let by_id: HashMap<&str, ModuleIndex> =
        modules.iter().enumerate().map(|(i, m)| (m.id.as_str(), i)).collect();
    let program = Program {
        modules,
        resolved: modules
            .iter()
            .map(|m| {
                m.resolved
                    .iter()
                    .map(|id| id.as_deref().and_then(|id| by_id.get(id).copied()))
                    .collect()
            })
            .collect(),
        runtime: modules
            .iter()
            .map(|m| m.summary.runtime_specifiers.iter().copied().collect())
            .collect(),
        imports: modules
            .iter()
            .map(|m| {
                m.summary
                    .imports
                    .iter()
                    .enumerate()
                    .map(|(i, import)| (import.binding, i))
                    .collect()
            })
            .collect(),
    };
    let mut linker = Linker::new(program, options);
    linker.run();
    linker.finish()
}

struct Linker<'m, 'o> {
    program: Program<'m>,
    options: &'o LinkOptions,
    uses: Vec<ResolvedUse>,
    /// Uses of each program binding.
    uses_of: HashMap<(ModuleIndex, u32), Vec<usize>>,
    /// Index in `uses` of each module's first use.
    use_offsets: Vec<usize>,
    /// Modules exporting each program binding, under any name.
    exporters_of: HashMap<(ModuleIndex, u32), Vec<ModuleIndex>>,
    /// Named or default imports resolving to each program binding.
    importers_of: HashMap<(ModuleIndex, u32), Vec<(ModuleIndex, u32)>>,
    escaping: HashSet<(ModuleIndex, u32)>,
    facts: Vec<ModuleFacts>,
    closed: BTreeSet<ModuleIndex>,
    folded: HashMap<(ModuleIndex, u32), Option<String>>,
    static_components: HashSet<(ModuleIndex, u32)>,
    inert_constants: HashSet<(ModuleIndex, u32)>,
}

impl<'m, 'o> Linker<'m, 'o> {
    fn new(program: Program<'m>, options: &'o LinkOptions) -> Self {
        let facts = program
            .modules
            .iter()
            .map(|m| ModuleFacts {
                version: facts::VERSION.to_string(),
                source_hash: m.summary.source_hash.clone(),
                ..ModuleFacts::default()
            })
            .collect();
        Self {
            program,
            options,
            uses: Vec::new(),
            uses_of: HashMap::new(),
            use_offsets: Vec::new(),
            exporters_of: HashMap::new(),
            importers_of: HashMap::new(),
            escaping: HashSet::new(),
            facts,
            closed: BTreeSet::new(),
            folded: HashMap::new(),
            static_components: HashSet::new(),
            inert_constants: HashSet::new(),
        }
    }

    fn summary(&self, module: ModuleIndex) -> &'m ModuleSummary {
        &self.program.modules[module].summary
    }

    fn id(&self, module: ModuleIndex) -> &'m str {
        &self.program.modules[module].id
    }

    fn run(&mut self) {
        self.resolve_uses();
        self.escape();
        self.primitives();
        if self.options.optimize {
            self.signals();
            self.stores();
        }
        self.module_folds();
        self.classify_components();
        self.component_facts();
        if self.options.islands {
            self.islands();
            self.roots();
        }
    }

    fn finish(self) -> Linked {
        let mut features = Features::new();
        for flag in features::program_flags() {
            let is_used = self
                .uses
                .iter()
                .any(|u| matches!(&u.target, Target::Runtime(name) if flag.exports.contains(&name.as_str())));
            features.insert(flag.name.to_string(), is_used);
        }
        Linked {
            facts: self
                .program
                .modules
                .iter()
                .zip(self.facts)
                .map(|(m, facts)| (m.id.clone(), facts))
                .collect(),
            features,
            closed: self.closed.iter().map(|&m| self.program.modules[m].id.clone()).collect(),
        }
    }

    fn resolve_specifier(&self, module: ModuleIndex, specifier: u32) -> Target {
        if self.program.runtime[module].contains(&specifier) {
            return Target::RuntimeNamespace;
        }
        match self.program.resolved[module].get(specifier as usize).copied().flatten() {
            Some(target) => Target::Namespace(target),
            None => Target::External,
        }
    }

    /// What `binding` (a top-level binding or an import of `module`) is.
    fn resolve_binding(&self, module: ModuleIndex, binding: u32) -> Target {
        let Some(&import) = self.program.imports[module].get(&binding) else {
            return Target::Binding(module, binding);
        };
        let import = &self.summary(module).imports[import];
        let namespace = self.resolve_specifier(module, import.specifier);
        match &import.name {
            ImportName::Namespace => namespace,
            ImportName::Named(name) => self.member_of(namespace, name),
            ImportName::Default => self.member_of(namespace, "default"),
        }
    }

    fn member_of(&self, namespace: Target, name: &str) -> Target {
        match namespace {
            Target::RuntimeNamespace => Target::Runtime(name.to_string()),
            Target::Namespace(module) => self.resolve_export(module, name, &mut HashSet::new()),
            _ => Target::External,
        }
    }

    fn resolve_ref(&self, module: ModuleIndex, target: &Ref) -> Target {
        let binding = self.resolve_binding(module, target.binding);
        match &target.member {
            Some(member) => self.member_of(binding, member),
            None => binding,
        }
    }

    /// ES export resolution: explicit exports first, `default` never through `export *`, a name
    /// two stars provide differently is not exported.
    fn resolve_export(
        &self,
        module: ModuleIndex,
        name: &str,
        visiting: &mut HashSet<(ModuleIndex, String)>,
    ) -> Target {
        if !visiting.insert((module, name.to_string())) {
            return Target::Unresolved;
        }
        let summary = self.summary(module);
        for export in &summary.exports {
            match export {
                Export::Local { name: exported, binding } if exported == name => {
                    return self.resolve_binding(module, *binding);
                }
                Export::Reexport { name: exported, specifier, imported } if exported == name => {
                    return match self.resolve_specifier(module, *specifier) {
                        Target::RuntimeNamespace => Target::Runtime(imported.clone()),
                        Target::Namespace(target) => {
                            self.resolve_export(target, imported, visiting)
                        }
                        _ => Target::External,
                    };
                }
                Export::StarAs { name: exported, specifier } if exported == name => {
                    return self.resolve_specifier(module, *specifier);
                }
                _ => {}
            }
        }
        if name == "default" {
            return Target::Unresolved;
        }
        let mut found: Option<Target> = None;
        let mut has_unknown_star = false;
        for export in &summary.exports {
            let Export::Star { specifier } = export else { continue };
            let candidate = match self.resolve_specifier(module, *specifier) {
                Target::RuntimeNamespace => match Primitive::from_export(name) {
                    Some(_) => Target::Runtime(name.to_string()),
                    None => continue,
                },
                Target::Namespace(target) => self.resolve_export(target, name, visiting),
                _ => {
                    has_unknown_star = true;
                    continue;
                }
            };
            if candidate == Target::Unresolved {
                continue;
            }
            match &found {
                None => found = Some(candidate),
                Some(existing) if *existing == candidate => {}
                Some(_) => return Target::Unresolved,
            }
        }
        match found {
            Some(target) if !has_unknown_star => target,
            Some(_) => Target::External,
            None if has_unknown_star => Target::External,
            None => Target::Unresolved,
        }
    }

    /// Every name `module` exports, stars included.
    fn export_names(
        &self,
        module: ModuleIndex,
        visiting: &mut HashSet<ModuleIndex>,
    ) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        if !visiting.insert(module) {
            return names;
        }
        for export in &self.summary(module).exports {
            match export {
                Export::Local { name, .. }
                | Export::Reexport { name, .. }
                | Export::StarAs { name, .. } => {
                    names.insert(name.clone());
                }
                Export::Star { specifier } => {
                    if let Target::Namespace(target) = self.resolve_specifier(module, *specifier) {
                        names.extend(
                            self.export_names(target, visiting)
                                .into_iter()
                                .filter(|n| n != "default"),
                        );
                    }
                }
                Export::DefaultExpression => {
                    names.insert("default".to_string());
                }
            }
        }
        names
    }

    fn resolve_uses(&mut self) {
        for module in 0..self.program.modules.len() {
            for name in self.export_names(module, &mut HashSet::new()) {
                if let Target::Binding(owner, binding) =
                    self.resolve_export(module, &name, &mut HashSet::new())
                {
                    let exporters = self.exporters_of.entry((owner, binding)).or_default();
                    if !exporters.contains(&module) {
                        exporters.push(module);
                    }
                }
            }
            for import in &self.summary(module).imports {
                if import.name == ImportName::Namespace {
                    continue;
                }
                if let Target::Binding(owner, binding) =
                    self.resolve_binding(module, import.binding)
                {
                    self.importers_of
                        .entry((owner, binding))
                        .or_default()
                        .push((module, import.binding));
                }
            }
            self.use_offsets.push(self.uses.len());
            for (index, used) in self.summary(module).uses.iter().enumerate() {
                let target = self.resolve_ref(module, &used.target);
                let via_namespace = used.target.member.is_some();
                if let Target::Binding(owner, binding) = target {
                    self.uses_of.entry((owner, binding)).or_default().push(self.uses.len());
                }
                self.uses.push(ResolvedUse { module, index, target, via_namespace });
            }
        }
    }

    fn escape(&mut self) {
        let count = self.program.modules.len();
        let mut open = vec![false; count];
        for module in 0..count {
            let summary = self.summary(module);
            if summary.dynamic.opens_all {
                open.iter_mut().for_each(|o| *o = true);
            }
            for &specifier in &summary.dynamic.imports {
                if let Target::Namespace(target) = self.resolve_specifier(module, specifier) {
                    open[target] = true;
                }
            }
            for pattern in &summary.dynamic.globs {
                for (target, input) in self.program.modules.iter().enumerate() {
                    if glob_matches(
                        &self.program.modules[module].id,
                        &self.options.root,
                        pattern,
                        &input.id,
                    ) {
                        open[target] = true;
                    }
                }
            }
            for export in &summary.exports {
                if let Export::StarAs { specifier, .. } = export
                    && let Target::Namespace(target) = self.resolve_specifier(module, *specifier)
                {
                    open[target] = true;
                }
                if let Export::Local { binding, .. } = export
                    && let Target::Namespace(target) = self.resolve_binding(module, *binding)
                {
                    open[target] = true;
                }
            }
        }
        for used in &self.uses {
            let summary_use = &self.summary(used.module).uses[used.index];
            if summary_use.target.member.is_none()
                && let Target::Namespace(target) = used.target
            {
                open[target] = true;
            }
        }
        for (module, is_open) in open.into_iter().enumerate() {
            if !(is_open || self.program.modules[module].is_entry) {
                continue;
            }
            for name in self.export_names(module, &mut HashSet::new()) {
                if let Target::Binding(owner, binding) =
                    self.resolve_export(module, &name, &mut HashSet::new())
                {
                    self.escaping.insert((owner, binding));
                }
            }
        }
    }

    fn primitives(&mut self) {
        for module in 0..self.program.modules.len() {
            let summary = self.summary(module);
            let mut found: Vec<PrimitiveImport> = Vec::new();
            for import in &summary.imports {
                if self.program.runtime[module].contains(&import.specifier)
                    || import.name == ImportName::Namespace
                {
                    continue;
                }
                if let Target::Runtime(name) = self.resolve_binding(module, import.binding)
                    && let Some(primitive) = Primitive::from_export(&name)
                {
                    found.push(PrimitiveImport {
                        import: ImportRef { binding: import.binding, member: None },
                        primitive,
                    });
                }
            }
            for used in summary.uses.iter() {
                let Some(member) = &used.target.member else { continue };
                let Some(&import) = self.program.imports[module].get(&used.target.binding) else {
                    continue;
                };
                if self.program.runtime[module].contains(&summary.imports[import].specifier) {
                    continue;
                }
                if let Target::Runtime(name) = self.resolve_ref(module, &used.target)
                    && let Some(primitive) = Primitive::from_export(&name)
                {
                    let import =
                        ImportRef { binding: used.target.binding, member: Some(member.clone()) };
                    if !found.iter().any(|p| p.import == import) {
                        found.push(PrimitiveImport { import, primitive });
                    }
                }
            }
            self.facts[module].primitives = found;
        }
        for module in 0..self.program.modules.len() {
            let getters: Vec<ImportRef> = self
                .summary(module)
                .uses
                .iter()
                .filter(|u| self.program.imports[module].contains_key(&u.target.binding))
                .filter(|u| match self.resolve_ref(module, &u.target) {
                    Target::Binding(owner, binding) => self.is_getter(owner, binding),
                    _ => false,
                })
                .map(|u| u.target.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            self.facts[module].getter_imports = getters;
        }
    }

    fn primitive_of(&self, module: ModuleIndex, callee: &Ref) -> Option<Primitive> {
        match self.resolve_ref(module, callee) {
            Target::Runtime(name) => Primitive::from_export(&name),
            _ => None,
        }
    }

    fn is_getter(&self, module: ModuleIndex, binding: u32) -> bool {
        self.summary(module).declarations.iter().any(|d| {
            match (&d.kind, self.primitive_of(module, &d.callee)) {
                (DeclarationKind::Pair { first, .. }, Some(Primitive::Signal)) => *first == binding,
                (DeclarationKind::Single { binding: b }, Some(Primitive::Computed)) => {
                    *b == binding
                }
                _ => false,
            }
        })
    }

    fn uses_of(&self, module: ModuleIndex, binding: u32) -> &[usize] {
        self.uses_of.get(&(module, binding)).map_or(&[], Vec::as_slice)
    }

    fn class_of(&self, used: usize) -> &'m UseClass {
        let used = &self.uses[used];
        &self.program.modules[used.module].summary.uses[used.index].class
    }

    /// Modules that export `binding` of `module`, itself included, under any name.
    fn exporters(&self, module: ModuleIndex, binding: u32) -> Vec<ModuleIndex> {
        self.exporters_of.get(&(module, binding)).cloned().unwrap_or_default()
    }

    /// Program modules with a named or default import resolving to `binding`.
    fn importers(&self, module: ModuleIndex, binding: u32) -> &[(ModuleIndex, u32)] {
        self.importers_of.get(&(module, binding)).map_or(&[], Vec::as_slice)
    }

    fn related(&self, uses: &[usize], owner: ModuleIndex, message: &str) -> Vec<Related> {
        let mut related: Vec<Related> = uses
            .iter()
            .map(|&u| &self.uses[u])
            .filter(|u| u.module != owner)
            .map(|u| {
                let summary_use = &self.summary(u.module).uses[u.index];
                Related {
                    file: self.id(u.module).to_string(),
                    start: summary_use.start,
                    end: summary_use.end,
                    message: message.to_string(),
                }
            })
            .collect();
        related.sort_by(|a, b| (&a.file, a.start).cmp(&(&b.file, b.start)));
        related
    }

    fn signals(&mut self) {
        for module in 0..self.program.modules.len() {
            for declaration in &self.summary(module).declarations {
                let DeclarationKind::Pair {
                    first: getter,
                    second: setter,
                    is_foldable: true,
                    literal,
                    ..
                } = &declaration.kind
                else {
                    continue;
                };
                if self.primitive_of(module, &declaration.callee) != Some(Primitive::Signal) {
                    continue;
                }
                let getter_exporters = self.exporters(module, *getter);
                let setter_exporters =
                    setter.map(|s| self.exporters(module, s)).unwrap_or_default();
                if getter_exporters.is_empty() && setter_exporters.is_empty() {
                    continue;
                }
                let getter_uses = self.uses_of(module, *getter).to_vec();
                let is_foldable = !self.escaping.contains(&(module, *getter))
                    && getter_uses.iter().all(|&u| *self.class_of(u) == UseClass::Call0)
                    && setter.is_none_or(|s| {
                        !self.escaping.contains(&(module, s))
                            && self.uses_of(module, s).is_empty()
                            && self.importers(module, s).is_empty()
                            && setter_exporters.iter().all(|&e| e == module)
                    });
                if !is_foldable {
                    continue;
                }
                let related = self.related(&getter_uses, module, "read here");
                self.facts[module].folded_signals.push(FoldedSignal { getter: *getter, related });
                self.folded.insert((module, *getter), literal.clone());
                self.closed.extend(getter_exporters.iter().chain(&setter_exporters));
                for u in getter_uses {
                    let used_module = self.uses[u].module;
                    if used_module == module {
                        continue;
                    }
                    let import = self.summary(used_module).uses[self.uses[u].index].target.clone();
                    let facts = &mut self.facts[used_module];
                    if !facts.folded_imports.iter().any(|f| f.import == import) {
                        facts
                            .folded_imports
                            .push(FoldedImport { import, literal: literal.clone() });
                    }
                }
            }
        }
    }

    fn stores(&mut self) {
        for module in 0..self.program.modules.len() {
            for declaration in &self.summary(module).declarations {
                let DeclarationKind::Pair {
                    first: state,
                    second: setter,
                    store_leaves: Some(leaves),
                    ..
                } = &declaration.kind
                else {
                    continue;
                };
                if self.primitive_of(module, &declaration.callee) != Some(Primitive::Store) {
                    continue;
                }
                let bindings: Vec<u32> = std::iter::once(*state).chain(*setter).collect();
                let exporters: Vec<Vec<ModuleIndex>> =
                    bindings.iter().map(|&b| self.exporters(module, b)).collect();
                if exporters.iter().all(Vec::is_empty) {
                    continue;
                }
                if exporters.iter().flatten().any(|&e| e != module)
                    || bindings.iter().any(|&b| self.escaping.contains(&(module, b)))
                {
                    continue;
                }
                let is_leaf = |path: &Vec<String>| leaves.contains(path);
                let mut plan: HashMap<(ModuleIndex, u32), LeafUses> = HashMap::new();
                let mut is_valid = true;
                for (&binding, role) in bindings.iter().zip([StoreRole::State, StoreRole::Setter]) {
                    for &u in self.uses_of(module, binding) {
                        let used = &self.uses[u];
                        let summary_use = &self.summary(used.module).uses[used.index];
                        let is_direct = used.module == module
                            || (!used.via_namespace
                                && self.is_direct_import(
                                    used.module,
                                    summary_use.target.binding,
                                    module,
                                ));
                        if !is_direct {
                            is_valid = false;
                            break;
                        }
                        let entry = plan
                            .entry((used.module, summary_use.target.binding))
                            .or_insert_with(|| (role, BTreeSet::new(), BTreeSet::new()));
                        match (&summary_use.class, role) {
                            (UseClass::Path(path), StoreRole::State) if is_leaf(path) => {
                                entry.1.insert(path.clone());
                            }
                            (UseClass::StoreSet(accesses), StoreRole::Setter)
                                if accesses.iter().all(|a| is_leaf(&a.path)) =>
                            {
                                for access in accesses {
                                    if access.is_write {
                                        entry.2.insert(access.path.clone());
                                    } else {
                                        entry.1.insert(access.path.clone());
                                    }
                                }
                            }
                            _ => is_valid = false,
                        }
                    }
                }
                if !is_valid {
                    continue;
                }
                let state_name = self.export_name_of(module, *state);
                let setter_name = setter.map(|s| self.export_name_of(module, s));
                let mut taken: HashSet<String> =
                    self.export_names(module, &mut HashSet::new()).into_iter().collect();
                let mut exported: Vec<LeafNames> = Vec::new();
                let mut leaf_names =
                    |path: &Vec<String>, is_write: bool, exported: &mut Vec<LeafNames>| -> String {
                        let index = match exported.iter().position(|l| &l.path == path) {
                            Some(index) => index,
                            None => {
                                exported.push(LeafNames {
                                    path: path.clone(),
                                    getter: None,
                                    setter: None,
                                });
                                exported.len() - 1
                            }
                        };
                        let base = if is_write {
                            setter_name.as_deref().unwrap_or("setState")
                        } else {
                            state_name.as_str()
                        };
                        let slot = if is_write {
                            &mut exported[index].setter
                        } else {
                            &mut exported[index].getter
                        };
                        if let Some(name) = slot {
                            return name.clone();
                        }
                        let name = unique(&format!("{base}${}", leaf_suffix(path)), &mut taken);
                        *slot = Some(name.clone());
                        name
                    };
                let mut imports: Vec<(ModuleIndex, StoreImport)> = Vec::new();
                let mut importer_plans: Vec<_> =
                    plan.into_iter().filter(|((m, _), _)| *m != module).collect();
                importer_plans.sort_by_key(|((m, b), _)| (*m, *b));
                for ((importer, binding), (role, reads, writes)) in importer_plans {
                    let mut used_leaves: Vec<LeafNames> = Vec::new();
                    for path in leaves.iter() {
                        let getter =
                            reads.contains(path).then(|| leaf_names(path, false, &mut exported));
                        let setter =
                            writes.contains(path).then(|| leaf_names(path, true, &mut exported));
                        if getter.is_some() || setter.is_some() {
                            used_leaves.push(LeafNames { path: path.clone(), getter, setter });
                        }
                    }
                    imports.push((importer, StoreImport { binding, role, leaves: used_leaves }));
                }
                exported.sort_by_key(|l| leaves.iter().position(|p| *p == l.path));
                let all_uses: Vec<usize> = bindings
                    .iter()
                    .flat_map(|&b| self.uses_of(module, b).iter().copied())
                    .collect();
                let related = self.related(&all_uses, module, "used here");
                self.facts[module].stores.push(StoreExport {
                    state: *state,
                    leaves: exported,
                    related,
                });
                for (importer, import) in imports {
                    self.facts[importer].store_imports.push(import);
                }
                self.closed.insert(module);
            }
        }
    }

    /// The import `binding` of `importer` names an export of `module` directly (no re-export).
    fn is_direct_import(&self, importer: ModuleIndex, binding: u32, module: ModuleIndex) -> bool {
        let Some(&import) = self.program.imports[importer].get(&binding) else { return false };
        let import = &self.summary(importer).imports[import];
        let ImportName::Named(name) = &import.name else { return false };
        self.resolve_specifier(importer, import.specifier) == Target::Namespace(module)
            && self
                .summary(module)
                .exports
                .iter()
                .any(|e| matches!(e, Export::Local { name: n, .. } if n == name))
    }

    fn export_name_of(&self, module: ModuleIndex, binding: u32) -> String {
        self.summary(module)
            .exports
            .iter()
            .find_map(|e| match e {
                Export::Local { name, binding: b } if *b == binding => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "state".to_string())
    }

    /// O3 folds each module performs on its own, so components can rely on them (§15.8).
    fn module_folds(&mut self) {
        if !self.options.optimize {
            return;
        }
        for module in 0..self.program.modules.len() {
            for declaration in &self.summary(module).declarations {
                let DeclarationKind::Pair {
                    first: getter,
                    second: setter,
                    is_foldable: true,
                    literal,
                    ..
                } = &declaration.kind
                else {
                    continue;
                };
                if self.primitive_of(module, &declaration.callee) != Some(Primitive::Signal)
                    || self.folded.contains_key(&(module, *getter))
                {
                    continue;
                }
                let is_exported = !self.exporters(module, *getter).is_empty()
                    || setter.is_some_and(|s| !self.exporters(module, s).is_empty());
                let getter_uses = self.uses_of(module, *getter);
                if !is_exported
                    && getter_uses.iter().all(|&u| *self.class_of(u) == UseClass::Call0)
                    && setter.is_none_or(|s| self.uses_of(module, s).is_empty())
                {
                    self.folded.insert((module, *getter), literal.clone());
                }
            }
        }
    }

    fn component_at(
        &self,
        module: ModuleIndex,
        binding: u32,
    ) -> Option<&'m crate::summary::ComponentSummary> {
        self.summary(module).components.iter().find(|c| c.binding == binding)
    }

    fn constant_at(
        &self,
        module: ModuleIndex,
        binding: u32,
    ) -> Option<&'m crate::summary::ConstantSummary> {
        self.summary(module).constants.iter().find(|c| c.binding == binding)
    }

    fn classify_components(&mut self) {
        for module in 0..self.program.modules.len() {
            for component in &self.summary(module).components {
                if component.violation.is_none() {
                    self.static_components.insert((module, component.binding));
                }
            }
            for constant in &self.summary(module).constants {
                if constant.violation.is_none() {
                    self.inert_constants.insert((module, constant.binding));
                }
            }
        }
        loop {
            let mut changed = false;
            let constants: Vec<_> = self.inert_constants.iter().copied().collect();
            for (module, binding) in constants {
                let constant =
                    self.constant_at(module, binding).expect("inert constants are constants");
                let deps_hold = constant.deps.iter().all(|d| self.dep_failure(module, d).is_none());
                let uses_inert = self.uses_of(module, binding).iter().all(|&u| {
                    let used = &self.uses[u];
                    match self.summary(used.module).uses[used.index].site {
                        Some(owner) => {
                            self.static_components.contains(&(used.module, owner))
                                || self.inert_constants.contains(&(used.module, owner))
                        }
                        None => false,
                    }
                });
                if !(deps_hold && uses_inert && !self.escaping.contains(&(module, binding))) {
                    self.inert_constants.remove(&(module, binding));
                    changed = true;
                }
            }
            let components: Vec<_> = self.static_components.iter().copied().collect();
            for (module, binding) in components {
                let component =
                    self.component_at(module, binding).expect("static components are components");
                if component.deps.iter().any(|d| self.dep_failure(module, d).is_some()) {
                    self.static_components.remove(&(module, binding));
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Why `dep` does not hold under the current classification; `None` when it holds.
    fn dep_failure(&self, module: ModuleIndex, dep: &Dep) -> Option<String> {
        let target = |r: &Ref| self.resolve_ref(module, r);
        match &dep.kind {
            DepKind::Constant(r) => match target(r) {
                Target::Binding(owner, binding)
                    if self.inert_constants.contains(&(owner, binding)) =>
                {
                    None
                }
                _ => Some("reads a value that is not an inert constant".to_string()),
            },
            DepKind::ArrayConstant(r) => match target(r) {
                Target::Binding(owner, binding)
                    if self.inert_constants.contains(&(owner, binding))
                        && self.constant_at(owner, binding).is_some_and(|c| c.is_array_literal) =>
                {
                    None
                }
                _ => Some("maps over a value that is not a constant array literal".to_string()),
            },
            DepKind::Folded(r) => match target(r) {
                Target::Binding(owner, binding) if self.folded.contains_key(&(owner, binding)) => {
                    None
                }
                _ => Some("calls a function that is not a folded signal".to_string()),
            },
            DepKind::Element { callee, boundary, .. } => {
                let (owner, binding) = match target(callee) {
                    Target::Binding(owner, binding)
                        if self.component_at(owner, binding).is_some() =>
                    {
                        (owner, binding)
                    }
                    Target::Runtime(_) => {
                        return Some(
                            "is a runtime component, which runs on the client".to_string(),
                        );
                    }
                    _ => return Some("is not a component of the program".to_string()),
                };
                if self.static_components.contains(&(owner, binding)) {
                    return None;
                }
                let why = self.boundary_failure(module, owner, binding, boundary)?;
                Some(format!("is a client component in a position that cannot be an island: {why}"))
            }
        }
    }

    /// Why a client component at this position is not an island; `None` when it is one.
    fn boundary_failure(
        &self,
        module: ModuleIndex,
        owner: ModuleIndex,
        binding: u32,
        boundary: &Result<Vec<Dep>, Violation>,
    ) -> Option<String> {
        if !self.options.islands {
            return Some("islands are off".to_string());
        }
        let deps = match boundary {
            Ok(deps) => deps,
            Err(violation) => return Some(violation.message.clone()),
        };
        if let Some(dep) = deps.iter().find(|d| self.dep_failure(module, d).is_some()) {
            return Some(format!("`{}` is not an inert constant", dep.label));
        }
        if self.island_export(owner, binding).is_none() {
            return Some("the component is not exported from its module".to_string());
        }
        None
    }

    /// The name the declaring module exports the component under (§15.9: no re-exports).
    fn island_export(&self, module: ModuleIndex, binding: u32) -> Option<&'m str> {
        self.summary(module).exports.iter().find_map(|e| match e {
            Export::Local { name, binding: b } if *b == binding => Some(name.as_str()),
            _ => None,
        })
    }

    /// The first rule a client component breaks, in source order, continued into the client
    /// child that caused it (§15.10).
    fn client_reason(
        &self,
        module: ModuleIndex,
        binding: u32,
        visiting: &mut HashSet<(ModuleIndex, u32)>,
    ) -> Option<Reason> {
        if self.static_components.contains(&(module, binding))
            || !visiting.insert((module, binding))
        {
            return None;
        }
        let component = self.component_at(module, binding)?;
        let failing_dep = component
            .deps
            .iter()
            .find_map(|dep| self.dep_failure(module, dep).map(|why| (dep, why)))
            .filter(|(dep, _)| component.violation.as_ref().is_none_or(|v| dep.start < v.start));
        let reason = match (failing_dep, &component.violation) {
            (Some((dep, why)), _) => {
                let cause = match &dep.kind {
                    DepKind::Element { callee, .. } => match self.resolve_ref(module, callee) {
                        Target::Binding(owner, child) => self.client_reason(owner, child, visiting),
                        _ => None,
                    },
                    _ => None,
                };
                Reason {
                    code: Code::ClientComponent,
                    module: self.id(module).to_string(),
                    span: Span::new(dep.start, dep.end),
                    message: format!("`{}` {why}", dep.label),
                    cause: cause.map(Box::new),
                }
            }
            (None, Some(violation)) => Reason {
                code: Code::ClientComponent,
                module: self.id(module).to_string(),
                span: Span::new(violation.start, violation.end),
                message: violation.message.clone(),
                cause: None,
            },
            (None, None) => return None,
        };
        Some(reason)
    }

    fn component_facts(&mut self) {
        for module in 0..self.program.modules.len() {
            let components: Vec<ComponentFact> = self
                .summary(module)
                .components
                .iter()
                .map(|c| ComponentFact {
                    binding: c.binding,
                    client: self.client_reason(module, c.binding, &mut HashSet::new()),
                })
                .collect();
            self.facts[module].components = components;
        }
    }

    fn island_id(&self, module: ModuleIndex, export: &str) -> String {
        let id = self.id(module).replace('\\', "/");
        let root = self.options.root.replace('\\', "/");
        let relative = id
            .strip_prefix(root.trim_end_matches('/'))
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(&id);
        base36(facts::fnv1a64(format!("{relative}#{export}").as_bytes()))
    }

    /// For an element of a static component: the island it is, if its callee is client.
    fn island_at(&self, module: ModuleIndex, dep: &Dep) -> Option<(ModuleIndex, u32, &'m str)> {
        let DepKind::Element { callee, boundary, .. } = &dep.kind else { return None };
        let Target::Binding(owner, binding) = self.resolve_ref(module, callee) else { return None };
        if self.component_at(owner, binding).is_none()
            || self.static_components.contains(&(owner, binding))
        {
            return None;
        }
        if self.boundary_failure(module, owner, binding, boundary).is_some() {
            return None;
        }
        Some((owner, binding, self.island_export(owner, binding)?))
    }

    fn component_features(
        &self,
        module: ModuleIndex,
        binding: u32,
        features: &mut BTreeSet<String>,
        seen: &mut HashSet<(ModuleIndex, u32)>,
    ) {
        if !seen.insert((module, binding)) {
            return;
        }
        let Some(component) = self.component_at(module, binding) else { return };
        features.extend(component.helpers.iter().cloned());
        for (index, used) in self.summary(module).uses.iter().enumerate() {
            if used.component != Some(binding) {
                continue;
            }
            let resolved = self.uses.get(self.use_offsets[module] + index).map(|u| &u.target);
            match resolved {
                Some(Target::Runtime(name)) => {
                    features.insert(name.clone());
                }
                Some(Target::Binding(owner, callee)) if used.class == UseClass::Tag => {
                    self.component_features(*owner, *callee, features, seen);
                }
                _ => {}
            }
        }
    }

    fn islands(&mut self) {
        for module in 0..self.program.modules.len() {
            let mut islands = Vec::new();
            for component in &self.summary(module).components {
                if !self.static_components.contains(&(module, component.binding)) {
                    continue;
                }
                for dep in &component.deps {
                    let DepKind::Element { element, .. } = &dep.kind else { continue };
                    let Some((owner, binding, export)) = self.island_at(module, dep) else {
                        continue;
                    };
                    let mut features = BTreeSet::new();
                    self.component_features(owner, binding, &mut features, &mut HashSet::new());
                    islands.push(IslandFact {
                        element: *element,
                        id: self.island_id(owner, export),
                        features: features.into_iter().collect(),
                    });
                }
            }
            self.facts[module].islands = islands;
        }
    }

    fn roots(&mut self) {
        for module in 0..self.program.modules.len() {
            let mut roots = Vec::new();
            for root in &self.summary(module).roots {
                let is_root_call = match self.primitive_of(module, &root.callee) {
                    Some(Primitive::RenderToString) => root.argument_count == 1,
                    Some(Primitive::Hydrate) => root.argument_count == 2,
                    _ => false,
                };
                let Target::Binding(owner, binding) = self.resolve_ref(module, &root.component)
                else {
                    continue;
                };
                if !(is_root_call
                    && root.has_json_attributes
                    && self.static_components.contains(&(owner, binding)))
                {
                    continue;
                }
                let mut islands: Vec<RootIsland> = Vec::new();
                self.collect_islands(module, owner, binding, &mut islands, &mut HashSet::new());
                roots.push(RootFact { call: root.call, islands });
            }
            self.facts[module].roots = roots;
        }
    }

    fn collect_islands(
        &self,
        root_module: ModuleIndex,
        module: ModuleIndex,
        binding: u32,
        islands: &mut Vec<RootIsland>,
        seen: &mut HashSet<(ModuleIndex, u32)>,
    ) {
        if !seen.insert((module, binding)) {
            return;
        }
        let Some(component) = self.component_at(module, binding) else { return };
        for dep in &component.deps {
            let DepKind::Element { callee, .. } = &dep.kind else { continue };
            if let Some((owner, _, export)) = self.island_at(module, dep) {
                let id = self.island_id(owner, export);
                if !islands.iter().any(|i| i.id == id) {
                    islands.push(RootIsland {
                        id,
                        specifier: relative_specifier(self.id(root_module), self.id(owner)),
                        export: export.to_string(),
                    });
                }
                continue;
            }
            if let Target::Binding(owner, child) = self.resolve_ref(module, callee)
                && self.static_components.contains(&(owner, child))
            {
                self.collect_islands(root_module, owner, child, islands, seen);
            }
        }
    }
}

fn leaf_suffix(path: &[String]) -> String {
    path.iter()
        .map(|key| {
            key.chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '$' { c } else { '_' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("$")
}

fn unique(base: &str, taken: &mut HashSet<String>) -> String {
    let mut name = base.to_string();
    let mut suffix = 2;
    while taken.contains(&name) {
        name = format!("{base}{suffix}");
        suffix += 1;
    }
    taken.insert(name.clone());
    name
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII digits")
}

fn directory_of(id: &str) -> &str {
    id.rfind('/').map_or("", |i| &id[..i])
}

/// `to` as an import specifier from the module `from`: `./x.tsx`, `../lib/x.tsx`.
pub fn relative_specifier(from: &str, to: &str) -> String {
    let from = from.replace('\\', "/");
    let to = to.replace('\\', "/");
    let from_dir: Vec<&str> = directory_of(&from).split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let (to_dir, file) = to_parts.split_at(to_parts.len().saturating_sub(1));
    let common = from_dir.iter().zip(to_dir).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend(&to_dir[common..]);
    parts.extend(file);
    let joined = parts.join("/");
    if joined.starts_with("..") { joined } else { format!("./{joined}") }
}

/// Whether the `import.meta.glob` `pattern` of `importer` matches module `id`.
fn glob_matches(importer: &str, root: &str, pattern: &str, id: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    if pattern.starts_with('!') {
        return false;
    }
    let base = if let Some(rooted) = pattern.strip_prefix('/') {
        format!("{}/{rooted}", root.replace('\\', "/").trim_end_matches('/'))
    } else {
        format!("{}/{pattern}", directory_of(&importer.replace('\\', "/")))
    };
    let absolute = normalize_path(&base);
    expand_braces(&absolute).iter().any(|p| glob(p.as_bytes(), id.replace('\\', "/").as_bytes()))
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else { return vec![pattern.to_string()] };
    let Some(close) = pattern[open..].find('}').map(|i| open + i) else {
        return vec![pattern.to_string()];
    };
    pattern[open + 1..close]
        .split(',')
        .flat_map(|choice| {
            expand_braces(&format!("{}{choice}{}", &pattern[..open], &pattern[close + 1..]))
        })
        .collect()
}

fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern {
        [] => path.is_empty(),
        [b'*', b'*', b'/', rest @ ..] => {
            glob(rest, path)
                || (0..path.len()).any(|i| path[i] == b'/' && glob(rest, &path[i + 1..]))
        }
        [b'*', b'*', rest @ ..] => (0..=path.len()).any(|i| glob(rest, &path[i..])),
        [b'*', rest @ ..] => (0..=path.len())
            .take_while(|&i| i == 0 || path[i - 1] != b'/')
            .any(|i| glob(rest, &path[i..])),
        [b'?', rest @ ..] => path.first().is_some_and(|&c| c != b'/') && glob(rest, &path[1..]),
        [c, rest @ ..] => path.first() == Some(c) && glob(rest, &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_specifiers_walk_up_and_down() {
        assert_eq!(relative_specifier("/a/src/main.tsx", "/a/src/Counter.tsx"), "./Counter.tsx");
        assert_eq!(relative_specifier("/a/src/pages/x.tsx", "/a/src/lib/y.ts"), "../lib/y.ts");
    }

    #[test]
    fn globs_match_relative_to_the_importer() {
        assert!(glob_matches("/r/src/main.ts", "/r", "./pages/*.tsx", "/r/src/pages/Home.tsx"));
        assert!(!glob_matches("/r/src/main.ts", "/r", "./pages/*.tsx", "/r/src/pages/a/Home.tsx"));
        assert!(glob_matches(
            "/r/src/main.ts",
            "/r",
            "./pages/**/*.{ts,tsx}",
            "/r/src/pages/a/Home.tsx"
        ));
        assert!(glob_matches("/r/src/a/main.ts", "/r", "/src/*.ts", "/r/src/x.ts"));
    }
}
