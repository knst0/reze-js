//! Store unproxying (SPEC §15.6): a `store` whose every use reads a leaf of its form or writes one
//! through a setter draft becomes one signal per leaf.

use std::collections::{HashMap, HashSet};

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::identifier::is_identifier_part;
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::Lowerer;
use crate::diagnostic::{Code, Report};
use crate::facts::{LeafNames, ModuleFacts, StoreExport, StoreRole};
use crate::ir::{Hole, HoleKind, Specifier, StoreLeaf, StoreWriteKind};
use crate::usage::{self, Context};

/// Leaf paths of the form `init`, in source order; `None` when `init` is not a form.
pub fn store_shape(init: &Expression<'_>) -> Option<Vec<Vec<String>>> {
    Some(form_leaves(init)?.into_iter().map(|(path, _)| path).collect())
}

pub enum DraftWrite {
    Assign,
    Compound,
    Update,
}

/// A chain `d.k₁…kₙ` in a setter draft; `write` is `None` for an rvalue.
pub struct DraftAccess {
    pub path: Vec<String>,
    pub write: Option<DraftWrite>,
}

/// The draft chains of `setState((d) => …)`; `None` when the call is not a valid setter form.
pub fn draft_accesses(
    call: &CallExpression<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<Vec<DraftAccess>> {
    let chains = draft_chains(call, scoping, nodes)?;
    Some(chains.into_iter().map(|c| DraftAccess { path: c.path, write: c.write }).collect())
}

type Leaves<'b, 'a> = Vec<(Vec<String>, &'b Expression<'a>)>;

fn form_leaves<'b, 'a>(init: &'b Expression<'a>) -> Option<Leaves<'b, 'a>> {
    let mut leaves = Vec::new();
    collect_leaves(init, &mut Vec::new(), &mut leaves)?;
    Some(leaves)
}

fn collect_leaves<'b, 'a>(
    form: &'b Expression<'a>,
    path: &mut Vec<String>,
    leaves: &mut Leaves<'b, 'a>,
) -> Option<()> {
    let Expression::ObjectExpression(object) = form.without_parentheses() else { return None };
    let mut keys = HashSet::new();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
        if property.kind != PropertyKind::Init || property.method || property.computed {
            return None;
        }
        let key = form_key(&property.key)?;
        if key == "__proto__" || !keys.insert(key.clone()) {
            return None;
        }
        path.push(key);
        if matches!(property.value.without_parentheses(), Expression::ObjectExpression(_)) {
            collect_leaves(&property.value, path, leaves)?;
        } else {
            leaves.push((path.clone(), &property.value));
        }
        path.pop();
    }
    Some(())
}

/// The property name of a form key: identifiers, strings, and integers JS prints as written.
fn form_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n)
            if n.value.fract() == 0.0 && (0.0..9_007_199_254_740_992.0).contains(&n.value) =>
        {
            Some(format!("{}", n.value as u64))
        }
        _ => None,
    }
}

struct DraftChain {
    path: Vec<String>,
    write: Option<DraftWrite>,
    /// The chain for an rvalue, the whole assignment or update for a write.
    span: Span,
}

fn draft_chains(
    call: &CallExpression<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<Vec<DraftChain>> {
    if call.optional || call.type_arguments.is_some() || call.arguments.len() != 1 {
        return None;
    }
    let callback = call.arguments[0].as_expression()?.without_parentheses();
    let mut forbidden = ForbiddenInDraft { found: false, arrow_depth: 0 };
    let (params, callback_span) = match callback {
        Expression::ArrowFunctionExpression(arrow)
            if !arrow.r#async && arrow.type_parameters.is_none() =>
        {
            forbidden.visit_arrow_function_body(&arrow.body);
            (&arrow.params, arrow.span)
        }
        Expression::FunctionExpression(function)
            if !function.r#async
                && !function.generator
                && function.id.is_none()
                && function.type_parameters.is_none()
                && function.this_param.is_none() =>
        {
            forbidden.visit_function_body(function.body.as_ref()?);
            (&function.params, function.span)
        }
        _ => return None,
    };
    if forbidden.found || params.rest.is_some() || params.items.len() != 1 {
        return None;
    }
    let param = &params.items[0];
    let BindingPattern::BindingIdentifier(draft) = &param.pattern else { return None };
    if param.initializer.is_some() {
        return None;
    }

    let mut chains = Vec::new();
    for &reference in scoping.get_resolved_reference_ids(draft.symbol_id()) {
        let node = scoping.get_reference(reference).node_id();
        if !is_directly_in(node, callback_span, nodes) {
            return None;
        }
        let access = usage::classify(reference, scoping, nodes)?;
        if access.keys.is_empty() || is_ref_value(access.node, nodes) {
            return None;
        }
        let path = access.keys.iter().map(|k| k.to_string()).collect();
        let (write, span) = match access.context {
            Context::Read => (None, chain_span(access.node, nodes)),
            Context::Write => {
                let write_node = nodes.parent_id(access.node);
                let write = match nodes.kind(write_node) {
                    AstKind::AssignmentExpression(a) if a.operator.is_assign() => {
                        DraftWrite::Assign
                    }
                    AstKind::AssignmentExpression(_) => DraftWrite::Compound,
                    AstKind::UpdateExpression(_) => DraftWrite::Update,
                    _ => return None,
                };
                if !is_value_unused(write_node, callback_span, nodes) {
                    return None;
                }
                (Some(write), nodes.kind(write_node).span())
            }
            _ => return None,
        };
        chains.push(DraftChain { path, write, span });
    }
    chains.sort_by_key(|c| c.span.start);
    Some(chains)
}

/// Whether `node` is inside the function at `callback` and no function or class nested in it.
fn is_directly_in(node: NodeId, callback: Span, nodes: &AstNodes<'_>) -> bool {
    for kind in nodes.ancestor_kinds(node) {
        match kind {
            AstKind::ArrowFunctionExpression(f) => return f.span == callback,
            AstKind::Function(f) => return f.span == callback,
            AstKind::Class(_) => return false,
            _ => {}
        }
    }
    false
}

/// Whether the value of the expression at `node` is discarded: it is an expression statement,
/// the body of the arrow `callback`, or a sequence element in such a position.
fn is_value_unused(node: NodeId, callback: Span, nodes: &AstNodes<'_>) -> bool {
    for kind in nodes.ancestor_kinds(node) {
        match kind {
            AstKind::ParenthesizedExpression(_) | AstKind::SequenceExpression(_) => {}
            AstKind::ExpressionStatement(_) => return true,
            AstKind::ArrowFunctionExpression(arrow) => return arrow.span == callback,
            _ => return false,
        }
    }
    false
}

/// The span of the member chain at `node` without enclosing parentheses: the node lowering visits.
fn chain_span(node: NodeId, nodes: &AstNodes<'_>) -> Span {
    match nodes.kind(node) {
        AstKind::ParenthesizedExpression(p) => p.expression.without_parentheses().span(),
        kind => kind.span(),
    }
}

/// Whether the chain at `node` is the value of a JSX `ref` attribute, which lowering splits.
fn is_ref_value(node: NodeId, nodes: &AstNodes<'_>) -> bool {
    for kind in nodes.ancestor_kinds(node) {
        match kind {
            AstKind::ParenthesizedExpression(_) | AstKind::JSXExpressionContainer(_) => {}
            AstKind::JSXAttribute(attribute) => {
                return matches!(&attribute.name, JSXAttributeName::Identifier(id) if id.name == "ref");
            }
            _ => return false,
        }
    }
    false
}

/// `this`, `arguments`, `yield` and `await` of the draft function itself.
struct ForbiddenInDraft {
    found: bool,
    arrow_depth: u32,
}

impl<'a> Visit<'a> for ForbiddenInDraft {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_class(&mut self, _: &Class<'a>) {
        self.found = true;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.arrow_depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.arrow_depth -= 1;
    }

    fn visit_this_expression(&mut self, _: &ThisExpression) {
        self.found = true;
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name == "arguments" {
            self.found = true;
        }
    }

    fn visit_yield_expression(&mut self, it: &YieldExpression<'a>) {
        self.found = true;
        walk::walk_yield_expression(self, it);
    }

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        if self.arrow_depth == 0 {
            self.found = true;
        }
        walk::walk_await_expression(self, it);
    }
}

/// `const [state, setState] = store(init)` with a form for `init`.
pub struct Candidate {
    declarator: NodeId,
    span: Span,
    state: SymbolId,
    setter: Option<SymbolId>,
    leaves: Vec<Vec<String>>,
}

pub fn candidate(
    declarator: &VariableDeclarator<'_>,
    call: &CallExpression<'_>,
) -> Option<Candidate> {
    let BindingPattern::ArrayPattern(pattern) = &declarator.id else { return None };
    let binding = |index: usize| match pattern.elements.get(index) {
        Some(Some(BindingPattern::BindingIdentifier(id))) => Some(id.symbol_id()),
        _ => None,
    };
    let is_shape = pattern.rest.is_none()
        && (1..=2).contains(&pattern.elements.len())
        && (pattern.elements.len() == 1 || binding(1).is_some())
        && declarator.type_annotation.is_none()
        && call.type_arguments.is_none()
        && call.arguments.len() == 1;
    if !is_shape {
        return None;
    }
    let leaves = store_shape(call.arguments[0].as_expression()?)?;
    if leaves.is_empty() {
        return None;
    }
    Some(Candidate {
        declarator: declarator.node_id.get(),
        span: declarator.span,
        state: binding(0)?,
        setter: binding(1),
        leaves,
    })
}

/// The unproxying decisions of one module, keyed by source spans; generated names are slots that
/// lowering reserves on first use.
#[derive(Default)]
pub struct Stores {
    /// Base names of generated bindings.
    slots: Vec<String>,
    /// Leaf getter and setter slots of each declaration, by declarator start.
    declarations: HashMap<u32, Vec<(usize, Option<usize>)>>,
    /// `export const [state, setState] = store(…)` statements, by start: leaf exports.
    export_declarations: HashMap<u32, Vec<(usize, String)>>,
    /// `export { state }` specifiers, by start: the leaf exports that replace them.
    export_specifiers: HashMap<u32, Vec<(usize, String)>>,
    /// Import specifiers of unproxied stores, by local start: the leaf imports that replace them.
    import_specifiers: HashMap<u32, Vec<(String, usize)>>,
    /// Leaf reads (`state.a`, draft `d.a`), by chain span: the getter slot.
    reads: HashMap<Span, usize>,
    /// Draft writes, by assignment or update span: the setter slot.
    writes: HashMap<Span, usize>,
    /// Setter calls, by span.
    sets: HashSet<Span>,
}

impl Stores {
    fn slot(&mut self, base: String) -> usize {
        self.slots.push(base);
        self.slots.len() - 1
    }
}

struct Plan<'f> {
    reads: Vec<(Span, usize)>,
    writes: Vec<(Span, usize)>,
    sets: Vec<Span>,
    written: Vec<bool>,
    export_statement: Option<u32>,
    export_specifiers: Vec<Span>,
    program: Option<&'f StoreExport>,
}

fn same_path(path: &[String], keys: &[impl AsRef<str>]) -> bool {
    path.len() == keys.len() && path.iter().zip(keys).all(|(p, k)| p == k.as_ref())
}

/// Decides every candidate and every imported store of `module_facts`; each unproxied store
/// reports `STORE_UNPROXIED`.
pub fn unproxy(
    program: &Program<'_>,
    candidates: &[Candidate],
    exported: &HashSet<SymbolId>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    module_facts: Option<&ModuleFacts>,
    reports: &mut Vec<Report>,
) -> Stores {
    let mut stores = Stores::default();
    for candidate in candidates {
        let Some(plan) = plan(candidate, exported, scoping, nodes, module_facts) else { continue };
        reports.push(commit(&mut stores, candidate, plan, scoping));
    }
    if let Some(module_facts) = module_facts {
        let mut imported: HashMap<(String, String), usize> = HashMap::new();
        for import in &module_facts.store_imports {
            import_store(&mut stores, &mut imported, program, import, scoping, nodes);
        }
    }
    stores
}

fn plan<'f>(
    candidate: &Candidate,
    exported: &HashSet<SymbolId>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    module_facts: Option<&'f ModuleFacts>,
) -> Option<Plan<'f>> {
    let declaration_node = nodes.parent_id(candidate.declarator);
    let AstKind::VariableDeclaration(declaration) = nodes.kind(declaration_node) else {
        return None;
    };
    if declaration.kind != VariableDeclarationKind::Const {
        return None;
    }
    let export_statement = match nodes.parent_kind(declaration_node) {
        AstKind::ExportDeclaration(export) => Some(export.span.start),
        _ => None,
    };
    let state_start = scoping.symbol_span(candidate.state).start;
    let program = module_facts.and_then(|f| f.stores.iter().find(|s| s.state == state_start));
    let is_exported = exported.contains(&candidate.state)
        || candidate.setter.is_some_and(|s| exported.contains(&s));
    if is_exported && program.is_none()
        || export_statement.is_some() && declaration.declarations.len() != 1
    {
        return None;
    }
    let leaf = |keys: &[&str]| candidate.leaves.iter().position(|path| same_path(path, keys));
    let mut plan = Plan {
        reads: Vec::new(),
        writes: Vec::new(),
        sets: Vec::new(),
        written: vec![false; candidate.leaves.len()],
        export_statement,
        export_specifiers: Vec::new(),
        program,
    };
    let export_specifier =
        |reference| match nodes.parent_kind(scoping.get_reference(reference).node_id()) {
            AstKind::ExportSpecifier(specifier) if program.is_some() => Some(specifier.span),
            _ => None,
        };
    for &reference in scoping.get_resolved_reference_ids(candidate.state) {
        let Some(access) = usage::classify(reference, scoping, nodes) else {
            plan.export_specifiers.push(export_specifier(reference)?);
            continue;
        };
        if access.context != Context::Read || is_ref_value(access.node, nodes) {
            return None;
        }
        plan.reads.push((chain_span(access.node, nodes), leaf(&access.keys)?));
    }
    for &reference in candidate.setter.map_or(&[][..], |s| scoping.get_resolved_reference_ids(s)) {
        let Some(access) = usage::classify(reference, scoping, nodes) else {
            plan.export_specifiers.push(export_specifier(reference)?);
            continue;
        };
        if !access.keys.is_empty() || access.context != (Context::Call { argument_count: 1 }) {
            return None;
        }
        let AstKind::CallExpression(call) = nodes.parent_kind(access.node) else { return None };
        for chain in draft_chains(call, scoping, nodes)? {
            let keys: Vec<&str> = chain.path.iter().map(String::as_str).collect();
            let index = leaf(&keys)?;
            if chain.write.is_some() {
                plan.written[index] = true;
                plan.writes.push((chain.span, index));
            } else {
                plan.reads.push((chain.span, index));
            }
        }
        plan.sets.push(call.span);
    }
    if let Some(program) = program {
        for names in &program.leaves {
            let index = candidate.leaves.iter().position(|path| path == &names.path)?;
            if names.setter.is_some() {
                if candidate.setter.is_none() {
                    return None;
                }
                plan.written[index] = true;
            }
        }
    }
    Some(plan)
}

fn identifier_part(key: &str) -> String {
    key.chars().map(|c| if is_identifier_part(c) { c } else { '_' }).collect()
}

fn leaf_name(binding: &str, path: &[String]) -> String {
    let mut name = binding.to_string();
    for key in path {
        name.push('$');
        name.push_str(&identifier_part(key));
    }
    name
}

fn commit(stores: &mut Stores, candidate: &Candidate, plan: Plan<'_>, scoping: &Scoping) -> Report {
    let state = scoping.symbol_name(candidate.state);
    let setter = candidate.setter.map(|s| scoping.symbol_name(s));
    let mut leaves = Vec::with_capacity(candidate.leaves.len());
    for (path, &written) in candidate.leaves.iter().zip(&plan.written) {
        let getter = stores.slot(leaf_name(state, path));
        let setter = setter.filter(|_| written).map(|s| stores.slot(leaf_name(s, path)));
        leaves.push((getter, setter));
    }
    for (span, index) in plan.reads {
        stores.reads.insert(span, leaves[index].0);
    }
    for (span, index) in plan.writes {
        if let Some(setter) = leaves[index].1 {
            stores.writes.insert(span, setter);
        }
    }
    stores.sets.extend(plan.sets);

    if let Some(program) = plan.program {
        let mut exports = Vec::new();
        for (path, (getter, setter)) in candidate.leaves.iter().zip(&leaves) {
            let Some(names) = program.leaves.iter().find(|l| &l.path == path) else { continue };
            if let Some(name) = &names.getter {
                exports.push((*getter, name.clone()));
            }
            if let (Some(name), Some(setter)) = (&names.setter, setter) {
                exports.push((*setter, name.clone()));
            }
        }
        let mut specifiers = plan.export_specifiers;
        specifiers.sort_by_key(|s| s.start);
        match plan.export_statement {
            Some(statement) => {
                stores.export_declarations.insert(statement, exports);
                for specifier in specifiers {
                    stores.export_specifiers.insert(specifier.start, Vec::new());
                }
            }
            None => {
                let mut exports = Some(exports);
                for specifier in specifiers {
                    stores
                        .export_specifiers
                        .insert(specifier.start, exports.take().unwrap_or_default());
                }
            }
        }
    }
    let span = candidate.span;
    stores.declarations.insert(span.start, leaves);

    let message = match setter {
        Some(setter) => format!(
            "Every use of `{state}` reads one of its fields and every write goes through a \
             `{setter}` draft, so the store compiled to one signal per field."
        ),
        None => format!(
            "Every use of `{state}` reads one of its fields, so the store compiled to one signal \
             per field."
        ),
    };
    let mut report = Report::new(Code::StoreUnproxied, span, message)
        .data("store", state)
        .data("scope", if plan.program.is_some() { "program" } else { "module" });
    for related in plan.program.map_or(&[][..], |p| &p.related) {
        report = report.related(related.clone());
    }
    report
}

/// Rewrites the uses of one imported store binding the program unproxied; leaves it alone when a
/// use is not one the facts name.
fn import_store(
    stores: &mut Stores,
    imported: &mut HashMap<(String, String), usize>,
    program: &Program<'_>,
    import: &crate::facts::StoreImport,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<()> {
    let (symbol, source) = program.body.iter().find_map(|statement| {
        let Statement::ImportDeclaration(declaration) = statement else { return None };
        declaration.specifiers.iter().flatten().find_map(|specifier| match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(s)
                if s.local.span.start == import.binding && !s.import_kind.is_type() =>
            {
                Some((s.local.symbol_id(), declaration.source.value.as_str()))
            }
            _ => None,
        })
    })?;
    let names = |keys: &[&str]| import.leaves.iter().find(|l| same_path(&l.path, keys));
    let mut reads: Vec<(Span, &str)> = Vec::new();
    let mut writes: Vec<(Span, &str)> = Vec::new();
    let mut sets = Vec::new();
    for &reference in scoping.get_resolved_reference_ids(symbol) {
        let access = usage::classify(reference, scoping, nodes)?;
        match import.role {
            StoreRole::State => {
                if access.context != Context::Read || is_ref_value(access.node, nodes) {
                    return None;
                }
                reads.push((
                    chain_span(access.node, nodes),
                    names(&access.keys)?.getter.as_deref()?,
                ));
            }
            StoreRole::Setter => {
                if !access.keys.is_empty()
                    || access.context != (Context::Call { argument_count: 1 })
                {
                    return None;
                }
                let AstKind::CallExpression(call) = nodes.parent_kind(access.node) else {
                    return None;
                };
                for chain in draft_chains(call, scoping, nodes)? {
                    let keys: Vec<&str> = chain.path.iter().map(String::as_str).collect();
                    let leaf: &LeafNames = names(&keys)?;
                    if chain.write.is_some() {
                        writes.push((chain.span, leaf.setter.as_deref()?));
                    } else {
                        reads.push((chain.span, leaf.getter.as_deref()?));
                    }
                }
                sets.push(call.span);
            }
        }
    }
    let mut slot = |export: &str| {
        *imported
            .entry((source.to_string(), export.to_string()))
            .or_insert_with(|| stores.slot(format!("_{}", identifier_part(export))))
    };
    let mut specifiers = Vec::new();
    for leaf in &import.leaves {
        for export in [&leaf.getter, &leaf.setter].into_iter().flatten() {
            specifiers.push((export.clone(), slot(export)));
        }
    }
    let reads: Vec<_> = reads.into_iter().map(|(span, export)| (span, slot(export))).collect();
    let writes: Vec<_> = writes.into_iter().map(|(span, export)| (span, slot(export))).collect();
    stores.reads.extend(reads);
    stores.writes.extend(writes);
    stores.sets.extend(sets);
    stores.import_specifiers.insert(import.binding, specifiers);
    Some(())
}

/// Names lowering reserved for store slots, and the import specifiers already written.
#[derive(Default)]
pub struct StoreNames<'a> {
    slots: HashMap<usize, &'a str>,
    draft_value: Option<&'a str>,
    imported: HashSet<usize>,
}

impl<'a> Lowerer<'a, '_> {
    fn slot_name(&mut self, slot: usize) -> &'a str {
        if let Some(name) = self.store_names.slots.get(&slot) {
            return name;
        }
        let facts = self.facts;
        let name = self.fresh(&facts.stores.slots[slot]);
        self.store_names.slots.insert(slot, name);
        name
    }

    fn draft_value(&mut self) -> &'a str {
        if let Some(name) = self.store_names.draft_value {
            return name;
        }
        let name = self.fresh("v");
        self.store_names.draft_value = Some(name);
        name
    }

    /// `[state, setState] = store(init)` → `[s$a, set$a] = signal(<a>), …`.
    pub(super) fn store_declaration(
        &mut self,
        declarator: &VariableDeclarator<'a>,
    ) -> Option<Hole<'a>> {
        let facts = self.facts;
        let slots = facts.stores.declarations.get(&declarator.span.start)?;
        let Some(Expression::CallExpression(call)) =
            declarator.init.as_ref().map(Expression::without_parentheses)
        else {
            return None;
        };
        let values = form_leaves(call.arguments.first()?.as_expression()?)?;
        let mut leaves = self.vec();
        for ((_, value), &(getter, setter)) in values.into_iter().zip(slots) {
            let getter = self.slot_name(getter);
            let setter = setter.map(|s| self.slot_name(s));
            let value = self.expr(value);
            leaves.push(StoreLeaf { getter, setter, value });
        }
        Some(Hole { span: declarator.span, kind: HoleKind::StoreDecl { leaves } })
    }

    /// `export const [state, setState] = store(…);` → the declaration and its leaf exports.
    pub(super) fn store_export_declaration(
        &mut self,
        export: &ExportDeclaration<'a>,
    ) -> Option<Hole<'a>> {
        let facts = self.facts;
        let exports = facts.stores.export_declarations.get(&export.span.start)?;
        let Declaration::VariableDeclaration(declaration) = &export.declaration else {
            return None;
        };
        let mut specifiers = self.vec();
        for (slot, exported) in exports {
            let name = self.slot_name(*slot);
            specifiers.push(Specifier::Alias { name, alias: self.str(exported) });
        }
        let declaration =
            self.embed(declaration.span, |finder| finder.visit_variable_declaration(declaration));
        Some(Hole { span: export.span, kind: HoleKind::StoreExport { declaration, specifiers } })
    }

    /// `export { state, setState }` → the leaf exports; `export { setX }` of a setter whose
    /// signal the program folded → nothing (SPEC §15.5).
    pub(super) fn store_export_specifiers(
        &mut self,
        export: &ExportNamedDeclaration<'a>,
    ) -> Option<Hole<'a>> {
        let facts = self.facts;
        let stores = &facts.stores;
        let removed = &facts.program.removed_specifiers;
        if !export.specifiers.iter().any(|s| {
            stores.export_specifiers.contains_key(&s.span.start) || removed.contains(&s.span.start)
        }) {
            return None;
        }
        let first = export.specifiers.first()?;
        let last = export.specifiers.last()?;
        let mut specifiers = self.vec();
        for specifier in &export.specifiers {
            if removed.contains(&specifier.span.start) {
                continue;
            }
            let Some(exports) = stores.export_specifiers.get(&specifier.span.start) else {
                specifiers.push(Specifier::Source(specifier.span));
                continue;
            };
            for (slot, exported) in exports {
                let name = self.slot_name(*slot);
                specifiers.push(Specifier::Alias { name, alias: self.str(exported) });
            }
        }
        Some(Hole {
            span: Span::new(first.span.start, last.span.end),
            kind: HoleKind::Specifiers { specifiers },
        })
    }

    /// `import { state } from "./store"` → the leaf imports, each once per module.
    pub(super) fn store_import(&mut self, import: &ImportDeclaration<'a>) -> Option<Hole<'a>> {
        let named: Vec<&ImportSpecifier<'a>> = import
            .specifiers
            .iter()
            .flatten()
            .filter_map(|s| match s {
                ImportDeclarationSpecifier::ImportSpecifier(s) => Some(&**s),
                _ => None,
            })
            .collect();
        let facts = self.facts;
        let stores = &facts.stores;
        if !named.iter().any(|s| stores.import_specifiers.contains_key(&s.local.span.start)) {
            return None;
        }
        let mut specifiers = self.vec();
        for specifier in &named {
            let Some(imports) = stores.import_specifiers.get(&specifier.local.span.start) else {
                specifiers.push(Specifier::Source(specifier.span));
                continue;
            };
            for (export, slot) in imports {
                if !self.store_names.imported.insert(*slot) {
                    continue;
                }
                let alias = self.slot_name(*slot);
                specifiers.push(Specifier::Alias { name: self.str(export), alias });
            }
        }
        let span = Span::new(named.first()?.span.start, named.last()?.span.end);
        Some(Hole { span, kind: HoleKind::Specifiers { specifiers } })
    }

    /// `state.a.b` or draft `d.a.b` → `s$a$b()`.
    pub(super) fn store_read(&mut self, span: Span) -> Option<Hole<'a>> {
        let slot = *self.facts.stores.reads.get(&span)?;
        Some(Hole { span, kind: HoleKind::StoreRead { getter: self.slot_name(slot) } })
    }

    /// `setState((d) => E)` → `void untrack(() => E')`.
    pub(super) fn store_set(&mut self, call: &CallExpression<'a>) -> Option<Hole<'a>> {
        if !self.facts.stores.sets.contains(&call.span) {
            return None;
        }
        let body = match call.arguments.first()?.as_expression()?.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => {
                    self.embed(body.span, |finder| finder.visit_function_body(body))
                }
                body => self.expr(body.as_expression()?),
            },
            Expression::FunctionExpression(function) => {
                let body = function.body.as_ref()?;
                self.embed(body.span, |finder| finder.visit_function_body(body))
            }
            _ => return None,
        };
        Some(Hole { span: call.span, kind: HoleKind::StoreSet { body } })
    }

    /// Draft `d.p = e` → `set$p(() => e)`, `d.p op= e` → `set$p((v) => v op e)`.
    pub(super) fn store_assignment(
        &mut self,
        assignment: &AssignmentExpression<'a>,
    ) -> Option<Hole<'a>> {
        let slot = *self.facts.stores.writes.get(&assignment.span)?;
        let setter = self.slot_name(slot);
        let right = &assignment.right;
        let write = if assignment.operator.is_assign() {
            let parenthesize = self.source.as_bytes()[right.span().start as usize] == b'{';
            StoreWriteKind::Assign { value: self.expr(right), parenthesize }
        } else {
            let operator = match assignment.operator.to_binary_operator() {
                Some(binary) => binary.as_str(),
                None => assignment.operator.to_logical_operator()?.as_str(),
            };
            StoreWriteKind::Compound {
                parameter: self.draft_value(),
                operator,
                value: self.expr(right),
                parenthesize: !is_operand(right),
            }
        };
        Some(Hole { span: assignment.span, kind: HoleKind::StoreWrite { setter, write } })
    }

    /// Draft `d.p++` → `set$p((v) => ++v)`.
    pub(super) fn store_update(&mut self, update: &UpdateExpression<'a>) -> Option<Hole<'a>> {
        let slot = *self.facts.stores.writes.get(&update.span)?;
        let setter = self.slot_name(slot);
        let write = StoreWriteKind::Update {
            parameter: self.draft_value(),
            operator: update.operator.as_str(),
        };
        Some(Hole { span: update.span, kind: HoleKind::StoreWrite { setter, write } })
    }
}

/// Whether `e` binds tighter than any binary or logical operator.
fn is_operand(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::Identifier(_)
            | Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::ParenthesizedExpression(_)
            | Expression::CallExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::ArrayExpression(_)
    )
}
