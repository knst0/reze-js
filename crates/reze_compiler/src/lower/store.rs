//! Store unproxying (SPEC §15.6): a `store` whose every use reads a leaf of its form or writes one
//! through a setter draft becomes one signal per leaf.

use super::Lowerer;
use crate::diagnostic::{Code, Report};
use crate::facts::{ModuleFacts, StoreExport, StoreRole};
use crate::ir::{
    ArrayWriteKind, Embed, FormSet, FormTemp, Hole, HoleKind, IndexOp, Specifier, StoreLeaf,
    StoreWriteKind,
};
use crate::summary::LeafShape;
use crate::usage::{self, Context};
use oxc_allocator::Vec as ArenaVec;
use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, Span};
use oxc_syntax::identifier::is_identifier_part;
use oxc_syntax::node::NodeId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;
use std::collections::{HashMap, HashSet};

/// Array methods a draft or a statement may call on an array leaf (§16.6): each rewrites to a
/// copy-on-write updater, so untouched elements keep their identity.
pub const ARRAY_METHODS: &[&str] =
    &["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "copyWithin", "fill"];
/// Leaf paths of the form `init`, in source order, with whether each holds an array; `None`
/// when `init` is not a form.
pub fn store_shape(init: &Expression<'_>) -> Option<Vec<LeafShape>> {
    Some(
        form_leaves(init)?
            .into_iter()
            .map(|(path, _, is_array)| LeafShape { path, is_array })
            .collect(),
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DraftWrite {
    Assign,
    Compound,
    Update,
}

/// A chain `d.k₁…kₙ` in a setter draft: `target` says what it touches, `write` is `None` for
/// an rvalue.
pub struct DraftAccess {
    pub path: Vec<String>,
    pub index: Option<DraftIndex>,
    pub method: Option<String>,
    pub form: Option<Vec<Vec<String>>>,
    pub write: Option<DraftWrite>,
    /// A static read in an array position (`each`, spread).
    pub array_site: bool,
}
/// `d.a[i].p…`: the tail keys after the index.
pub struct DraftIndex {
    pub tail: Vec<String>,
}

/// The draft chains of `setState((d) => …)`; `None` when the call is not a valid setter form.
pub fn draft_accesses(
    call: &CallExpression<'_>,
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
) -> Option<Vec<DraftAccess>> {
    let chains = draft_chains(call, scoping, nodes)?;
    Some(
        chains
            .into_iter()
            .map(|c| DraftAccess {
                path: c.path,
                index: c.index,
                method: c.method,
                form: c.form,
                write: c.write,
                array_site: c.array_site,
            })
            .collect(),
    )
}
type Leaves<'b, 'a> = Vec<(Vec<String>, &'b Expression<'a>, bool)>;

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
        let value = property.value.without_parentheses();
        if matches!(value, Expression::ObjectExpression(_)) {
            collect_leaves(&property.value, path, leaves)?;
        } else {
            leaves.push((
                path.clone(),
                &property.value,
                matches!(value, Expression::ArrayExpression(_)),
            ));
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

/// The tail keys of an index chain: exactly one index first, then static keys.
fn draft_index(tail: &[usage::Tail<'_>]) -> Option<DraftIndex> {
    let mut steps = tail.iter();
    if !matches!(steps.next(), Some(usage::Tail::Index { .. })) {
        return None;
    }
    let mut keys = Vec::new();
    for step in steps {
        match step {
            usage::Tail::Key(key) => keys.push(key.to_string()),
            usage::Tail::Index { .. } => return None,
        }
    }
    Some(DraftIndex { tail: keys })
}

/// Whether an index of the tail holds JSX, which `src` copies cannot reproduce.
fn index_has_jsx(tail: &[usage::Tail<'_>]) -> bool {
    tail.iter().any(|step| match step {
        usage::Tail::Index { index } => super::has_jsx(|check| check.visit_expression(index)),
        usage::Tail::Key(_) => false,
    })
}

/// The write of the assignment or update at `write_node`, when its value is discarded.
fn write_kind(write_node: NodeId, callback_span: Span, nodes: &AstNodes<'_>) -> Option<DraftWrite> {
    let write = match nodes.kind(write_node) {
        AstKind::AssignmentExpression(a) if a.operator.is_assign() => DraftWrite::Assign,
        AstKind::AssignmentExpression(_) => DraftWrite::Compound,
        AstKind::UpdateExpression(_) => DraftWrite::Update,
        _ => return None,
    };
    is_value_unused(write_node, callback_span, nodes).then_some(write)
}

/// Leaf-relative key paths of the object literal assigned at `write_node`, when it is a plain
/// literal without spread, methods or computed keys.
fn form_literal(write_node: NodeId, nodes: &AstNodes<'_>) -> Option<Vec<Vec<String>>> {
    let AstKind::AssignmentExpression(assignment) = nodes.kind(write_node) else { return None };
    let Expression::ObjectExpression(object) = assignment.right.without_parentheses() else {
        return None;
    };
    let mut out = Vec::new();
    literal_shape(object, &mut Vec::new(), &mut out)?;
    Some(out)
}

fn literal_shape(
    object: &ObjectExpression<'_>,
    prefix: &mut Vec<String>,
    out: &mut Vec<Vec<String>>,
) -> Option<()> {
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
        prefix.push(key);
        match property.value.without_parentheses() {
            Expression::ObjectExpression(nested) => literal_shape(nested, prefix, out)?,
            _ => out.push(prefix.clone()),
        }
        prefix.pop();
    }
    Some(())
}

struct DraftChain {
    path: Vec<String>,
    index: Option<DraftIndex>,
    method: Option<String>,
    form: Option<Vec<Vec<String>>>,
    write: Option<DraftWrite>,
    /// A static read in an array position (`each`, spread): whole array leaves need it.
    array_site: bool,
    /// The chain for an rvalue, the whole assignment, update or call for a write.
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
        let mut chain = DraftChain {
            path,
            index: None,
            method: None,
            form: None,
            write: None,
            array_site: false,
            span: chain_span(access.node, nodes),
        };
        if access.tail.is_empty() && access.context == Context::Read {
            chain.array_site = array_read_site(access.node, nodes);
        }
        if !access.tail.is_empty() {
            chain.index = Some(draft_index(&access.tail)?);
            if index_has_jsx(&access.tail) || !element_position_valid(access.node, nodes) {
                return None;
            }
        }
        match access.context {
            Context::Read => {}
            Context::Write if chain.index.is_some() => {
                let write_node = nodes.parent_id(access.node);
                chain.write = Some(write_kind(write_node, callback_span, nodes)?);
                chain.span = nodes.kind(write_node).span();
            }
            Context::Write => {
                let write_node = nodes.parent_id(access.node);
                let write = write_kind(write_node, callback_span, nodes)?;
                if write == DraftWrite::Assign
                    && matches!(nodes.parent_kind(write_node), AstKind::ExpressionStatement(_))
                {
                    chain.form = form_literal(write_node, nodes);
                }
                chain.write = Some(write);
                chain.span = nodes.kind(write_node).span();
            }
            Context::Call { .. } if chain.index.is_none() => {
                let call_node = nodes.parent_id(access.node);
                let AstKind::CallExpression(call) = nodes.kind(call_node) else { return None };
                if !is_value_unused(call_node, callback_span, nodes) {
                    return None;
                }
                let Some(method) = chain.path.pop() else { return None };
                chain.method = Some(method);
                chain.span = call.span;
            }
            _ => return None,
        }
        chains.push(chain);
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
    leaves: Vec<LeafShape>,
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
    /// Indexed reads (`state.a[i].p`, `state.a.length`), by chain span.
    array_reads: HashMap<Span, ArrayReadPlan>,
    /// Draft writes, by assignment or update span: the setter slot.
    writes: HashMap<Span, usize>,
    /// Indexed draft writes (`d.a[i].p = e`), by assignment or update span: the setter slot.
    index_writes: HashMap<Span, ArrayReadPlan>,
    /// Array method calls (`d.a.push(x)`, `state.a.push(x)`), by call span: the setter slot.
    method_calls: HashMap<Span, usize>,
    /// Literal writes to a form path, by assignment span: the setter slot and leaf-relative
    /// path of each written leaf.
    form_writes: HashMap<Span, Vec<(usize, Vec<String>)>>,
    /// Setter calls, by span.
    sets: HashSet<Span>,
}

/// An indexed read of an array leaf: the leaf and what follows the array (§16.6). Lowering
/// rebuilds the index expressions from the chain itself; the shape aligns them.
struct ArrayReadPlan {
    slot: usize,
    suffix: Vec<ArraySuffix>,
}

enum ArraySuffix {
    Key(String),
    Index,
}

impl Stores {
    fn slot(&mut self, base: String) -> usize {
        self.slots.push(base);
        self.slots.len() - 1
    }
}

/// An indexed read of an array leaf at plan time: the leaf index and what follows the array.
struct ArrayRead {
    slot: usize,
    suffix: Vec<ArraySuffix>,
}

struct Plan<'f> {
    reads: Vec<(Span, usize)>,
    array_reads: Vec<(Span, ArrayRead)>,
    writes: Vec<(Span, usize)>,
    index_writes: Vec<(Span, ArrayRead)>,
    method_calls: Vec<(Span, usize)>,
    form_writes: Vec<(Span, Vec<(usize, Vec<String>)>)>,
    sets: Vec<Span>,
    written: Vec<bool>,
    export_statement: Option<u32>,
    export_specifiers: Vec<Span>,
    program: Option<&'f StoreExport>,
}

/// The array leaf of `keys` when it ends in `.length` (§16.6).
fn length_leaf(keys: &[&str], leaves: &[LeafShape]) -> Option<usize> {
    let (last, prefix) = keys.split_last()?;
    if *last != "length" || prefix.is_empty() {
        return None;
    }
    leaves.iter().position(|leaf| leaf.is_array && same_path(&leaf.path, prefix))
}

/// The suffix of an index tail: one index, then static keys; `None` with JSX inside.
fn array_suffix(tail: &[usage::Tail<'_>]) -> Option<Vec<ArraySuffix>> {
    let mut steps = tail.iter();
    let mut suffix = Vec::new();
    match steps.next()? {
        usage::Tail::Index { index } => {
            if super::has_jsx(|check| check.visit_expression(index)) {
                return None;
            }
            suffix.push(ArraySuffix::Index);
        }
        usage::Tail::Key(_) => return None,
    }
    for step in steps {
        match step {
            usage::Tail::Key(key) => suffix.push(ArraySuffix::Key(key.to_string())),
            usage::Tail::Index { .. } => return None,
        }
    }
    Some(suffix)
}

/// Whether the chain at `node` is `each={…}` of a bare `<For>` or `[...…]` of an array
/// literal: the only positions a whole array leaf may be read in (§16.6).
fn array_read_site(node: NodeId, nodes: &AstNodes<'_>) -> bool {
    match nodes.kind(nodes.parent_id(node)) {
        AstKind::JSXExpressionContainer(_) => {
            let attribute_node = nodes.parent_id(nodes.parent_id(node));
            let AstKind::JSXAttribute(attribute) = nodes.kind(attribute_node) else {
                return false;
            };
            if !matches!(&attribute.name, JSXAttributeName::Identifier(id) if id.name == "each") {
                return false;
            }
            let opening_node = nodes.parent_id(attribute_node);
            let AstKind::JSXOpeningElement(opening) = nodes.kind(opening_node) else {
                return false;
            };
            matches!(&opening.name, JSXElementName::IdentifierReference(tag) if tag.name == "For")
        }
        AstKind::SpreadElement(_) => {
            matches!(nodes.parent_kind(nodes.parent_id(node)), AstKind::ArrayExpression(_))
        }
        _ => false,
    }
}
/// Whether an index read at `node` stands where §16.6 allows an element: not compared with
/// `==`, not passed as a call argument, and not spread into an object.
pub(crate) fn element_position_valid(node: NodeId, nodes: &AstNodes<'_>) -> bool {
    let parent = nodes.parent_id(node);
    match nodes.kind(parent) {
        AstKind::BinaryExpression(binary) => !matches!(
            binary.operator,
            BinaryOperator::Equality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
        ),
        AstKind::CallExpression(call) => call.callee.span() == nodes.kind(node).span(),
        AstKind::SpreadElement(_) => {
            !matches!(nodes.parent_kind(parent), AstKind::ObjectExpression(_))
        }
        _ => true,
    }
}

/// Whether the call of the chain at `node` discards its value as a statement.
fn is_statement_call(node: NodeId, nodes: &AstNodes<'_>) -> bool {
    let call = nodes.parent_id(node);
    matches!(nodes.kind(call), AstKind::CallExpression(_))
        && matches!(nodes.parent_kind(call), AstKind::ExpressionStatement(_))
}

/// The span of the call of the chain at `node`.
fn call_span(node: NodeId, nodes: &AstNodes<'_>) -> Option<Span> {
    let call = nodes.parent_id(node);
    match nodes.kind(call) {
        AstKind::CallExpression(call) => Some(call.span),
        _ => None,
    }
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
    let leaf = |keys: &[&str]| candidate.leaves.iter().position(|leaf| same_path(&leaf.path, keys));
    let mut plan = Plan {
        reads: Vec::new(),
        array_reads: Vec::new(),
        writes: Vec::new(),
        index_writes: Vec::new(),
        method_calls: Vec::new(),
        form_writes: Vec::new(),
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
        if is_ref_value(access.node, nodes) {
            return None;
        }
        let span = chain_span(access.node, nodes);
        match access.context {
            Context::Read if access.tail.is_empty() => {
                let keys: Vec<&str> = access.keys.iter().map(|k| *k).collect();
                match length_leaf(&keys, &candidate.leaves) {
                    Some(index) => plan.array_reads.push((
                        span,
                        ArrayRead {
                            slot: index,
                            suffix: vec![ArraySuffix::Key("length".to_string())],
                        },
                    )),
                    None => {
                        let index = leaf(&access.keys)?;
                        if candidate.leaves[index].is_array && !array_read_site(access.node, nodes)
                        {
                            return None;
                        }
                        plan.reads.push((span, index));
                    }
                }
            }
            Context::Read => {
                let index = leaf(&access.keys)?;
                if !candidate.leaves[index].is_array || !element_position_valid(access.node, nodes)
                {
                    return None;
                }
                plan.array_reads
                    .push((span, ArrayRead { slot: index, suffix: array_suffix(&access.tail)? }));
            }
            Context::Call { .. } if access.tail.is_empty() => {
                let (method, prefix) = access.keys.split_last()?;
                let index = leaf(prefix)?;
                if !candidate.leaves[index].is_array || !ARRAY_METHODS.contains(method) {
                    return None;
                }
                plan.method_calls.push((call_span(access.node, nodes)?, index));
                plan.written[index] = true;
            }
            _ => return None,
        }
    }
    for &reference in candidate.setter.map_or(&[][..], |s| scoping.get_resolved_reference_ids(s)) {
        let Some(access) = usage::classify(reference, scoping, nodes) else {
            plan.export_specifiers.push(export_specifier(reference)?);
            continue;
        };
        if !access.keys.is_empty()
            || !access.tail.is_empty()
            || access.context != (Context::Call { argument_count: 1 })
        {
            return None;
        }
        let AstKind::CallExpression(call) = nodes.parent_kind(access.node) else { return None };
        let chains = draft_chains(call, scoping, nodes)?;
        for chain in chains {
            plan_draft(&mut plan, candidate, chain)?;
        }
        plan.sets.push(call.span);
    }
    if let Some(program) = program {
        for names in &program.leaves {
            let index = candidate.leaves.iter().position(|leaf| leaf.path == names.path)?;
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

/// Records one draft chain of `candidate` into `plan`: a leaf read or write, an index or a
/// method of an array leaf, or a literal write to a form path (§16.6).
fn plan_draft(plan: &mut Plan<'_>, candidate: &Candidate, chain: DraftChain) -> Option<()> {
    let leaf = |keys: &[String]| candidate.leaves.iter().position(|leaf| leaf.path == *keys);
    if let Some(method) = &chain.method {
        let index = leaf(&chain.path)?;
        if !candidate.leaves[index].is_array || !ARRAY_METHODS.contains(&method.as_str()) {
            return None;
        }
        plan.method_calls.push((chain.span, index));
        plan.written[index] = true;
        return Some(());
    }
    if let Some(index_chain) = &chain.index {
        let index = leaf(&chain.path)?;
        if !candidate.leaves[index].is_array {
            return None;
        }
        if chain.write.is_some() {
            let mut suffix = vec![ArraySuffix::Index];
            suffix.extend(index_chain.tail.iter().cloned().map(ArraySuffix::Key));
            plan.index_writes.push((chain.span, ArrayRead { slot: index, suffix }));
            plan.written[index] = true;
        } else {
            let mut suffix = vec![ArraySuffix::Index];
            suffix.extend(index_chain.tail.iter().cloned().map(ArraySuffix::Key));
            plan.array_reads.push((chain.span, ArrayRead { slot: index, suffix }));
        }
        return Some(());
    }
    if let Some(shape) = &chain.form {
        if chain.write.is_none() {
            return None;
        }
        if let Some(index) = leaf(&chain.path) {
            plan.writes.push((chain.span, index));
            plan.written[index] = true;
            return Some(());
        }
        let mut writes = Vec::new();
        for relative in shape {
            let mut full = chain.path.clone();
            full.extend(relative.iter().cloned());
            let index = leaf(&full)?;
            writes.push((index, relative.clone()));
            plan.written[index] = true;
        }
        plan.form_writes.push((chain.span, writes));
        return Some(());
    }
    match leaf(&chain.path) {
        Some(index) => {
            if chain.write.is_none() && candidate.leaves[index].is_array && !chain.array_site {
                return None;
            }
            if chain.write.is_some() {
                plan.writes.push((chain.span, index));
                plan.written[index] = true;
            } else {
                plan.reads.push((chain.span, index));
            }
            Some(())
        }
        None if chain.write.is_none() => {
            let (last, prefix) = chain.path.split_last()?;
            if *last != "length" || prefix.is_empty() {
                return None;
            }
            let index = leaf(&prefix.to_vec())?;
            if !candidate.leaves[index].is_array {
                return None;
            }
            plan.array_reads.push((
                chain.span,
                ArrayRead { slot: index, suffix: vec![ArraySuffix::Key("length".to_string())] },
            ));
            Some(())
        }
        None => None,
    }
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
    for (leaf, &written) in candidate.leaves.iter().zip(&plan.written) {
        let getter = stores.slot(leaf_name(state, &leaf.path));
        let setter = setter.filter(|_| written).map(|s| stores.slot(leaf_name(s, &leaf.path)));
        leaves.push((getter, setter));
    }
    for (span, index) in plan.reads {
        stores.reads.insert(span, leaves[index].0);
    }
    for (span, read) in plan.array_reads {
        stores
            .array_reads
            .insert(span, ArrayReadPlan { slot: leaves[read.slot].0, suffix: read.suffix });
    }
    for (span, index) in plan.writes {
        if let Some(setter) = leaves[index].1 {
            stores.writes.insert(span, setter);
        }
    }
    for (span, write) in plan.index_writes {
        if let Some(setter) = leaves[write.slot].1 {
            stores.index_writes.insert(span, ArrayReadPlan { slot: setter, suffix: write.suffix });
        }
    }
    for (span, index) in plan.method_calls {
        if let Some(setter) = leaves[index].1 {
            stores.method_calls.insert(span, setter);
        }
    }
    for (span, writes) in plan.form_writes {
        let writes = writes
            .into_iter()
            .filter_map(|(index, relative)| leaves[index].1.map(|setter| (setter, relative)))
            .collect::<Vec<_>>();
        stores.form_writes.insert(span, writes);
    }
    stores.sets.extend(plan.sets);

    if let Some(program) = plan.program {
        let mut exports = Vec::new();
        for (leaf, (getter, setter)) in candidate.leaves.iter().zip(&leaves) {
            let Some(names) = program.leaves.iter().find(|l| l.path == leaf.path) else { continue };
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

/// Records one draft chain of an imported store: like `plan_draft`, with the link's export
/// names. Array-ness was validated by `link`; only shapes are rechecked here.
#[allow(clippy::too_many_arguments)]
fn import_draft<'i>(
    chain: DraftChain,
    import: &'i crate::facts::StoreImport,
    reads: &mut Vec<(Span, &'i str)>,
    array_reads: &mut Vec<(Span, &'i str, Vec<ArraySuffix>)>,
    writes: &mut Vec<(Span, &'i str)>,
    index_writes: &mut Vec<(Span, &'i str, Vec<ArraySuffix>)>,
    method_calls: &mut Vec<(Span, &'i str)>,
    form_writes: &mut Vec<(Span, Vec<(&'i str, Vec<String>)>)>,
) -> Option<()> {
    let names = |keys: &[&str]| import.leaves.iter().find(|l| same_path(&l.path, keys));
    if let Some(method) = &chain.method {
        if !ARRAY_METHODS.contains(&method.as_str()) {
            return None;
        }
        let leaf = names(&chain.path.iter().map(String::as_str).collect::<Vec<_>>())?;
        method_calls.push((chain.span, leaf.setter.as_deref()?));
        return Some(());
    }
    if let Some(index_chain) = &chain.index {
        let leaf = names(&chain.path.iter().map(String::as_str).collect::<Vec<_>>())?;
        let mut suffix = vec![ArraySuffix::Index];
        suffix.extend(index_chain.tail.iter().cloned().map(ArraySuffix::Key));
        if chain.write.is_some() {
            index_writes.push((chain.span, leaf.setter.as_deref()?, suffix));
        } else {
            array_reads.push((chain.span, leaf.getter.as_deref()?, suffix));
        }
        return Some(());
    }
    if let Some(shape) = &chain.form {
        if chain.write.is_none() {
            return None;
        }
        let keys: Vec<&str> = chain.path.iter().map(String::as_str).collect();
        if let Some(leaf) = names(&keys) {
            writes.push((chain.span, leaf.setter.as_deref()?));
            return Some(());
        }
        let mut form = Vec::new();
        for relative in shape {
            let mut full = chain.path.clone();
            full.extend(relative.iter().cloned());
            let keys: Vec<&str> = full.iter().map(String::as_str).collect();
            let leaf = names(&keys)?;
            form.push((leaf.setter.as_deref()?, relative.clone()));
        }
        form_writes.push((chain.span, form));
        return Some(());
    }
    match names(&chain.path.iter().map(String::as_str).collect::<Vec<_>>()) {
        Some(leaf) => {
            if chain.write.is_none() && leaf.is_array && !chain.array_site {
                return None;
            }
            if chain.write.is_some() {
                writes.push((chain.span, leaf.setter.as_deref()?));
            } else {
                reads.push((chain.span, leaf.getter.as_deref()?));
            }
            Some(())
        }
        None if chain.write.is_none() => {
            let (last, prefix) = chain.path.split_last()?;
            if *last != "length" || prefix.is_empty() {
                return None;
            }
            let keys: Vec<&str> = prefix.iter().map(String::as_str).collect();
            let leaf = names(&keys)?;
            if !leaf.is_array {
                return None;
            }
            array_reads.push((
                chain.span,
                leaf.getter.as_deref()?,
                vec![ArraySuffix::Key("length".to_string())],
            ));
            Some(())
        }
        None => None,
    }
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
    let mut array_reads: Vec<(Span, &str, Vec<ArraySuffix>)> = Vec::new();
    let mut writes: Vec<(Span, &str)> = Vec::new();
    let mut index_writes: Vec<(Span, &str, Vec<ArraySuffix>)> = Vec::new();
    let mut method_calls: Vec<(Span, &str)> = Vec::new();
    let mut form_writes: Vec<(Span, Vec<(&str, Vec<String>)>)> = Vec::new();
    let mut sets = Vec::new();
    for &reference in scoping.get_resolved_reference_ids(symbol) {
        let access = usage::classify(reference, scoping, nodes)?;
        match import.role {
            StoreRole::State => {
                if is_ref_value(access.node, nodes) {
                    return None;
                }
                let span = chain_span(access.node, nodes);
                match access.context {
                    Context::Read if access.tail.is_empty() => {
                        if let Some(leaf) = names(&access.keys) {
                            if leaf.is_array && !array_read_site(access.node, nodes) {
                                return None;
                            }
                            reads.push((span, leaf.getter.as_deref()?));
                        } else {
                            let (last, prefix) = access.keys.split_last()?;
                            if *last != "length" || prefix.is_empty() {
                                return None;
                            }
                            let leaf = names(prefix)?;
                            array_reads.push((
                                span,
                                leaf.getter.as_deref()?,
                                vec![ArraySuffix::Key("length".to_string())],
                            ));
                        }
                    }
                    Context::Read => {
                        if !element_position_valid(access.node, nodes) {
                            return None;
                        }
                        let leaf = names(&access.keys)?;
                        array_reads.push((
                            span,
                            leaf.getter.as_deref()?,
                            array_suffix(&access.tail)?,
                        ));
                    }
                    Context::Call { .. } if access.tail.is_empty() => {
                        let (method, prefix) = access.keys.split_last()?;
                        if !ARRAY_METHODS.contains(method) || !is_statement_call(access.node, nodes)
                        {
                            return None;
                        }
                        let leaf = names(prefix)?;
                        method_calls
                            .push((call_span(access.node, nodes)?, leaf.setter.as_deref()?));
                    }
                    _ => return None,
                }
            }
            StoreRole::Setter => {
                if !access.keys.is_empty()
                    || !access.tail.is_empty()
                    || access.context != (Context::Call { argument_count: 1 })
                {
                    return None;
                }
                let AstKind::CallExpression(call) = nodes.parent_kind(access.node) else {
                    return None;
                };
                for chain in draft_chains(call, scoping, nodes)? {
                    import_draft(
                        chain,
                        import,
                        &mut reads,
                        &mut array_reads,
                        &mut writes,
                        &mut index_writes,
                        &mut method_calls,
                        &mut form_writes,
                    )?;
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
    let array_reads: Vec<_> = array_reads
        .into_iter()
        .map(|(span, export, suffix)| (span, ArrayReadPlan { slot: slot(export), suffix }))
        .collect();
    let writes: Vec<_> = writes.into_iter().map(|(span, export)| (span, slot(export))).collect();
    let index_writes: Vec<_> = index_writes
        .into_iter()
        .map(|(span, export, suffix)| (span, ArrayReadPlan { slot: slot(export), suffix }))
        .collect();
    let method_calls: Vec<_> =
        method_calls.into_iter().map(|(span, export)| (span, slot(export))).collect();
    let form_writes: Vec<_> = form_writes
        .into_iter()
        .map(|(span, writes)| {
            let writes =
                writes.into_iter().map(|(export, relative)| (slot(export), relative)).collect();
            (span, writes)
        })
        .collect();
    stores.form_writes.extend(form_writes);
    stores.reads.extend(reads);
    stores.array_reads.extend(array_reads);
    stores.writes.extend(writes);
    stores.index_writes.extend(index_writes);
    stores.method_calls.extend(method_calls);
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
        for ((_, value, _), &(getter, setter)) in values.into_iter().zip(slots) {
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

    /// `import { state } from "./store"` → the leaf imports, each once per module; the import of
    /// a computed the program inlined → the bindings its body reads (SPEC §16.5).
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
        if !named.iter().any(|s| {
            stores.import_specifiers.contains_key(&s.local.span.start)
                || facts.program.computed_imports.contains_key(&s.local.span.start)
        }) {
            return None;
        }
        let mut specifiers = self.vec();
        for specifier in &named {
            let Some(imports) = stores.import_specifiers.get(&specifier.local.span.start) else {
                match self.computed_import(specifier) {
                    Some(replacement) => specifiers.extend(replacement),
                    None => specifiers.push(Specifier::Source(specifier.span)),
                }
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

    /// `state.a[i].p`, `state.a.length` → `s$a()[i].p`, `s$a().length` (§16.6).
    pub(super) fn array_read(
        &mut self,
        span: Span,
        steps: Vec<crate::ir::ArraySuffix<'a>>,
    ) -> Option<Hole<'a>> {
        let plan = self.facts.stores.array_reads.get(&span)?;
        let suffix = self.align_suffix(steps, &plan.suffix)?;
        Some(Hole { span, kind: HoleKind::ArrayRead { getter: self.slot_name(plan.slot), suffix } })
    }

    /// Member steps of a chain as IR suffix steps, root first; `None` on optional links or
    /// exotic roots.
    pub(super) fn member_steps(
        &mut self,
        e: &Expression<'a>,
    ) -> Option<Vec<crate::ir::ArraySuffix<'a>>> {
        match e.without_parentheses() {
            Expression::StaticMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                steps.push(crate::ir::ArraySuffix::Key(m.property.name.as_str()));
                Some(steps)
            }
            Expression::ComputedMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                match m.expression.without_parentheses() {
                    Expression::StringLiteral(key) => {
                        steps.push(crate::ir::ArraySuffix::Key(self.str(&key.value)))
                    }
                    _ => steps.push(crate::ir::ArraySuffix::Index(self.expr(&m.expression))),
                }
                Some(steps)
            }
            Expression::Identifier(_) => Some(Vec::new()),
            _ => None,
        }
    }

    /// Member steps of an assignment target or update argument, root first.
    fn target_steps(
        &mut self,
        target: &AssignmentTarget<'a>,
    ) -> Option<Vec<crate::ir::ArraySuffix<'a>>> {
        match target {
            AssignmentTarget::StaticMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                steps.push(crate::ir::ArraySuffix::Key(m.property.name.as_str()));
                Some(steps)
            }
            AssignmentTarget::ComputedMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                match m.expression.without_parentheses() {
                    Expression::StringLiteral(key) => {
                        steps.push(crate::ir::ArraySuffix::Key(self.str(&key.value)))
                    }
                    _ => steps.push(crate::ir::ArraySuffix::Index(self.expr(&m.expression))),
                }
                Some(steps)
            }
            _ => None,
        }
    }

    fn simple_steps(
        &mut self,
        target: &SimpleAssignmentTarget<'a>,
    ) -> Option<Vec<crate::ir::ArraySuffix<'a>>> {
        match target {
            SimpleAssignmentTarget::StaticMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                steps.push(crate::ir::ArraySuffix::Key(m.property.name.as_str()));
                Some(steps)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) if !m.optional => {
                let mut steps = self.member_steps(&m.object)?;
                match m.expression.without_parentheses() {
                    Expression::StringLiteral(key) => {
                        steps.push(crate::ir::ArraySuffix::Key(self.str(&key.value)))
                    }
                    _ => steps.push(crate::ir::ArraySuffix::Index(self.expr(&m.expression))),
                }
                Some(steps)
            }
            _ => None,
        }
    }

    /// The trailing steps as an IR suffix, aligned with the planned shape.
    fn align_suffix(
        &mut self,
        steps: Vec<crate::ir::ArraySuffix<'a>>,
        shape: &[ArraySuffix],
    ) -> Option<ArenaVec<'a, crate::ir::ArraySuffix<'a>>> {
        let start = steps.len().checked_sub(shape.len())?;
        let mut suffix = self.vec();
        for (step, expected) in steps.into_iter().skip(start).zip(shape) {
            match (step, expected) {
                (crate::ir::ArraySuffix::Key(key), ArraySuffix::Key(expected))
                    if key == expected =>
                {
                    suffix.push(crate::ir::ArraySuffix::Key(key));
                }
                (crate::ir::ArraySuffix::Index(index), ArraySuffix::Index) => {
                    suffix.push(crate::ir::ArraySuffix::Index(index));
                }
                _ => return None,
            }
        }
        Some(suffix)
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

    /// `d.a[i].p = e` → `set$a((v) => { const c = v.slice(); c[i].p = e; return c; })`.
    pub(super) fn array_assignment(
        &mut self,
        assignment: &AssignmentExpression<'a>,
    ) -> Option<Hole<'a>> {
        let plan = self.facts.stores.index_writes.get(&assignment.span)?;
        let setter = self.slot_name(plan.slot);
        let steps = self.target_steps(&assignment.left)?;
        let (index, tail) = self.split_index(steps, plan.suffix.len())?;
        let right = &assignment.right;
        let op = if assignment.operator.is_assign() {
            IndexOp::Assign { value: self.expr(right) }
        } else {
            let operator = match assignment.operator.to_binary_operator() {
                Some(binary) => binary.as_str(),
                None => assignment.operator.to_logical_operator()?.as_str(),
            };
            IndexOp::Compound { operator, value: self.expr(right) }
        };
        let temp = (!tail.is_empty()).then(|| self.fresh("i$"));
        let write = ArrayWriteKind::Index { index, tail, op, temp };
        Some(Hole { span: assignment.span, kind: HoleKind::ArrayWrite { setter, write } })
    }

    /// `d.a[i].p++` → `set$a((v) => { const c = v.slice(); ++c[i].p; return c; })`.
    pub(super) fn array_update(&mut self, update: &UpdateExpression<'a>) -> Option<Hole<'a>> {
        let plan = self.facts.stores.index_writes.get(&update.span)?;
        let setter = self.slot_name(plan.slot);
        let steps = self.simple_steps(&update.argument)?;
        let (index, tail) = self.split_index(steps, plan.suffix.len())?;
        let temp = (!tail.is_empty()).then(|| self.fresh("i$"));
        let write = ArrayWriteKind::Index {
            index,
            tail,
            op: IndexOp::Update { operator: update.operator.as_str() },
            temp,
        };
        Some(Hole { span: update.span, kind: HoleKind::ArrayWrite { setter, write } })
    }

    /// The index expression and tail keys of an indexed write: the trailing shape steps.
    fn split_index(
        &mut self,
        steps: Vec<crate::ir::ArraySuffix<'a>>,
        shape: usize,
    ) -> Option<(Embed<'a>, ArenaVec<'a, &'a str>)> {
        let start = steps.len().checked_sub(shape)?;
        if shape == 0 {
            return None;
        }
        let mut tail = self.vec();
        let mut index = None;
        for step in steps.into_iter().skip(start) {
            match step {
                crate::ir::ArraySuffix::Index(embed) if index.is_none() => index = Some(embed),
                crate::ir::ArraySuffix::Key(key) if index.is_some() => tail.push(key),
                _ => return None,
            }
        }
        Some((index?, tail))
    }
    /// `d.a.push(e)` and statement `state.a.push(e)` → copy-on-write updaters (§16.6).
    pub(super) fn store_method_call(&mut self, call: &CallExpression<'a>) -> Option<Hole<'a>> {
        let slot = *self.facts.stores.method_calls.get(&call.span)?;
        let setter = self.slot_name(slot);
        let Expression::StaticMemberExpression(callee) = call.callee.without_parentheses() else {
            return None;
        };
        let name = self.str(callee.property.name.as_str());
        let mut args = self.vec();
        for argument in &call.arguments {
            args.push(self.expr(argument.as_expression()?));
        }
        let write = ArrayWriteKind::Method { name, args };
        Some(Hole { span: call.span, kind: HoleKind::ArrayWrite { setter, write } })
    }

    /// `d.a.b = {…}` → `{ const _t0 = v0; …; set$l0(() => _t0); … }` (§16.6).
    pub(super) fn form_assignment(
        &mut self,
        assignment: &AssignmentExpression<'a>,
    ) -> Option<Hole<'a>> {
        let writes = self.facts.stores.form_writes.get(&assignment.span)?.clone();
        let Expression::ObjectExpression(object) = assignment.right.without_parentheses() else {
            return None;
        };
        let mut parts: std::vec::Vec<(Vec<String>, &Expression<'a>)> = std::vec::Vec::new();
        literal_parts(object, &mut std::vec::Vec::new(), &mut parts)?;
        let mut temps = self.vec();
        let mut temp_of: std::vec::Vec<(Vec<String>, &'a str)> = std::vec::Vec::new();
        for (relative, value) in parts {
            let name = self.fresh("t$");
            temps.push(FormTemp { name, value: self.expr(value) });
            temp_of.push((relative, name));
        }
        let mut ordered: std::vec::Vec<(usize, &'a str)> = writes
            .iter()
            .map(|(slot, relative)| {
                let temp = temp_of.iter().find(|(path, _)| path == relative)?.1;
                Some((*slot, temp))
            })
            .collect::<Option<_>>()?;
        ordered.sort_by_key(|(slot, _)| *slot);
        let mut sets = self.vec();
        for (slot, temp) in ordered {
            sets.push(FormSet { setter: self.slot_name(slot), temp });
        }
        Some(Hole {
            span: assignment.span,
            kind: HoleKind::ArrayWrite { setter: "", write: ArrayWriteKind::Form { temps, sets } },
        })
    }
}

/// Leaf-relative paths and values of a plain object literal, in source order.
fn literal_parts<'b, 'a>(
    object: &'b ObjectExpression<'a>,
    prefix: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, &'b Expression<'a>)>,
) -> Option<()> {
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
        prefix.push(key);
        match property.value.without_parentheses() {
            Expression::ObjectExpression(nested) => literal_parts(nested, prefix, out)?,
            value => out.push((prefix.clone(), value)),
        }
        prefix.pop();
    }
    Some(())
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
