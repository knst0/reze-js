pub mod analysis;
pub mod async_component;
pub mod dsl;
pub mod dynamic;
pub mod imports;
pub mod keyed;
pub mod props;
pub mod props_shape;
pub mod pure;
pub mod selector;

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_semantic::{Scoping, SemanticBuilder};

pub use analysis::SharedFacts;

use crate::diagnostic::Report;
use crate::imports::HelperImports;
use crate::module_facts::{self, ModuleFacts};

pub struct Namer<'a> {
    taken: HashSet<&'a str>,
    generated: HashSet<String>,
    next_suffix: HashMap<&'a str, u32>,
    alloc: &'a Allocator,
}

impl<'a> Namer<'a> {
    pub fn seeded(scoping: &Scoping, allocator: &'a Allocator) -> Self {
        let mut taken = HashSet::new();
        for name in scoping.symbol_names() {
            let text: &'a str = allocator.alloc_str(name);
            taken.insert(text);
        }
        for name in scoping.root_unresolved_references().keys() {
            let text: &'a str = allocator.alloc_str(name.as_str());
            taken.insert(text);
        }
        Self { taken, generated: HashSet::new(), next_suffix: HashMap::new(), alloc: allocator }
    }

    pub fn fresh(&mut self, base: &str) -> String {
        let mut suffix = self.next_suffix.get(base).copied().unwrap_or(1);
        let name = loop {
            let candidate = if suffix == 1 { base.to_owned() } else { format!("{base}{suffix}") };
            suffix += 1;
            if !self.taken.contains(candidate.as_str()) && !self.generated.contains(&candidate) {
                break candidate;
            }
        };
        match self.next_suffix.get_mut(base) {
            Some(slot) => *slot = suffix,
            None => {
                let key: &'a str = self.alloc.alloc_str(base);
                self.next_suffix.insert(key, suffix);
            }
        }
        self.generated.insert(name.clone());
        name
    }
}

pub struct FrontendOutput<'a> {
    pub program: &'a mut Program<'a>,
    pub scoping: Scoping,
    pub facts: SharedFacts,
    pub reports: Vec<Report>,
    pub namer: Namer<'a>,
    pub helpers: HelperImports<'a>,
    pub content_changed: bool,
}

pub fn normalize<'a>(
    allocator: &'a Allocator,
    program: &'a mut Program<'a>,
    source: &str,
    imported: &HashMap<String, ModuleFacts>,
) -> FrontendOutput<'a> {
    let mut reports = Vec::new();
    imports::refuse_internal_reactivity(&*program, &mut reports);
    let syntax;
    let mut helpers = HelperImports::default();
    let mut namer;
    let pre;
    let props_plan;
    let mut async_plan;
    let selector_plan;
    let mut async_reports = Vec::new();
    let first_scoping;
    let imported_getters;
    {
        let first = SemanticBuilder::new().with_build_nodes(true).build(&*program);
        let (scoping, nodes) = first.semantic.into_scoping_and_nodes();
        imported_getters = module_facts::imported_getters(program, imported);
        syntax = imports::SyntaxImports::collect(program);
        namer = Namer::seeded(&scoping, allocator);
        pre = dsl::prescan(program, &scoping, &syntax, &mut namer, &mut reports);
        props_plan = props::collect(program, &scoping, &nodes, &mut reports);
        async_plan = async_component::collect(program, &scoping, &nodes, &mut async_reports);
        selector_plan = selector::collect(program, &scoping, &nodes, &pre, &mut reports);
        if syntax.declared.is_empty()
            && syntax.namespaces.is_empty()
            && !imports::has_lowerable(program)
            && !imports::has_lowerable_reexport(program)
            && props_plan.is_empty()
            && async_plan.is_empty()
            && selector_plan.is_empty()
            && imported_getters.is_empty()
        {
            reports.extend(async_reports);
            dsl::scan(program, &scoping, &pre, source, &mut reports);
            let facts = analysis::collect(program, &scoping, &nodes, &mut reports);
            drop(nodes);
            return FrontendOutput {
                program,
                scoping,
                facts,
                reports,
                namer,
                helpers,
                content_changed: false,
            };
        }
        first_scoping = scoping;
    }
    let mut content_changed = imports::lower_namespace_members(
        allocator,
        program,
        &first_scoping,
        &syntax,
        &mut namer,
        &mut helpers,
    );
    let outcome = imports::apply(allocator, program, &syntax, &mut namer);
    let reexports_changed = imports::lower_reexports(allocator, program);
    content_changed |= outcome.changed | reexports_changed;
    let props_changed = props::apply(allocator, program, props_plan, &mut namer, &mut helpers);
    content_changed |= props_changed;
    let normalized = dsl::normalize_ast(
        allocator,
        program,
        &first_scoping,
        &syntax,
        &pre,
        &outcome,
        &imported_getters,
        source,
    );
    content_changed |= normalized;
    if (props_changed || normalized) && (!async_plan.is_empty() || !async_reports.is_empty()) {
        content_changed |= helpers.install(allocator, program);
        let semantic = SemanticBuilder::new().with_build_nodes(true).build(&*program);
        let (scoping, nodes) = semantic.semantic.into_scoping_and_nodes();
        async_reports.clear();
        async_plan = async_component::collect(program, &scoping, &nodes, &mut async_reports);
    }
    reports.extend(async_reports);
    content_changed |=
        async_component::apply(allocator, program, async_plan, &mut namer, &mut helpers);
    content_changed |=
        selector::apply(allocator, program, selector_plan, &mut namer, &mut helpers, &mut reports);
    content_changed |= helpers.install(allocator, program);
    let second = SemanticBuilder::new().with_build_nodes(true).build(&*program);
    let (scoping, nodes) = second.semantic.into_scoping_and_nodes();
    dsl::scan(program, &scoping, &pre, source, &mut reports);
    let facts = analysis::collect(program, &scoping, &nodes, &mut reports);
    FrontendOutput { program, scoping, facts, reports, namer, helpers, content_changed }
}
