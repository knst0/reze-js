pub mod analysis;
pub mod async_component;
pub mod dsl;
pub mod dynamic;
pub mod flow_params;
pub mod imports;
pub mod keyed;
pub mod props;
pub mod props_shape;
pub mod pure;
pub mod selector;

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, Function, Program, VariableDeclarator};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

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

#[derive(Default)]
struct ComponentSpans {
    spans: Vec<(Span, String)>,
}

impl<'a> Visit<'a> for ComponentSpans {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if let Some(id) = &it.id
            && pure::is_component_name(id.name.as_str())
        {
            self.spans.push((it.span, id.name.to_string()));
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && pure::is_component_name(id.name.as_str())
            && let Some(init) = it.init.as_ref().map(Expression::without_parentheses)
            && matches!(
                init,
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            )
        {
            self.spans.push((init.span(), id.name.to_string()));
        }
        walk::walk_variable_declarator(self, it);
    }
}

fn stamp_component_paths(program: &Program<'_>, reports: &mut [Report]) {
    let mut components = ComponentSpans::default();
    components.visit_program(program);
    for report in reports.iter_mut() {
        if !report.path.is_empty() {
            continue;
        }
        let Some((_, name)) = components
            .spans
            .iter()
            .filter(|(span, _)| span.start <= report.span.start && report.span.end <= span.end)
            .min_by_key(|(span, _)| span.end - span.start)
        else {
            continue;
        };
        report.path = vec![format!("<{name}>")];
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
    let flow_plan;
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
        flow_plan = flow_params::collect(program, &scoping, &mut reports);
        if syntax.declared.is_empty()
            && syntax.namespaces.is_empty()
            && !imports::has_lowerable(program)
            && !imports::has_lowerable_reexport(program)
            && props_plan.is_empty()
            && async_plan.is_empty()
            && selector_plan.is_empty()
            && flow_plan.is_empty()
            && imported_getters.is_empty()
        {
            reports.extend(async_reports);
            dsl::scan(program, &scoping, &pre, source, &mut reports);
            let facts = analysis::collect(program, &scoping, &nodes, &mut reports);
            drop(nodes);
            stamp_component_paths(program, &mut reports);
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
    let flow_changed = flow_params::apply(allocator, program, &first_scoping, flow_plan);
    content_changed |= flow_changed;
    if (props_changed || normalized || flow_changed)
        && (!async_plan.is_empty() || !async_reports.is_empty())
    {
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
    stamp_component_paths(program, &mut reports);
    FrontendOutput { program, scoping, facts, reports, namer, helpers, content_changed }
}
