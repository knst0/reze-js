use std::collections::{HashMap, HashSet};

use oxc_allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::{AstNodes, Scoping};
use oxc_span::{GetSpan, SPAN, Span};
use oxc_str::{Ident, Str};
use oxc_syntax::identifier::is_identifier_name;
use oxc_syntax::number::{BigintBase, NumberBase};
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use super::imports::{allows, home_of};
use super::props_shape::{has_unnameable_type_read, is_literal_default, props_plan};
use super::pure::{
    has_jsx, is_component_name, is_declared_component, merge_property_is_static, static_property,
};
use super::Namer;
use crate::diagnostic::{Code, Report};
use crate::imports::HelperImports;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PropsMethod {
    Merge,
    Split,
    Omit,
}

#[derive(Default)]
pub struct Plan {
    components: Vec<ComponentPlan>,
    calls: Vec<CallPlan>,
    reads: HashMap<ReferenceId, ReadPlan>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.calls.is_empty()
    }
}

#[derive(Clone)]
struct ComponentPlan {
    param: u32,
    defaults: Vec<DefaultEntry>,
    rest: Option<RestPlan>,
}

impl ComponentPlan {
    fn needs_entry(&self) -> bool {
        self.rest.is_some()
            || self.defaults.iter().any(|default| {
                matches!(default.kind, DefaultKind::Hoisted { .. })
            })
    }
}

#[derive(Clone)]
struct DefaultEntry {
    span: Span,
    kind: DefaultKind,
}

#[derive(Clone)]
enum DefaultKind {
    Literal(LitShape),
    Hoisted { base: String },
}

#[derive(Clone)]
enum LitShape {
    Bool(bool),
    Null,
    Num(f64),
    BigInt { digits: String, base: BigintBase },
    Text(String),
    NegNum(f64),
    Template { raw: String, cooked: Option<String>, lone: bool, quasi: Span },
    Add(Box<LitShape>, Box<LitShape>),
}

#[derive(Clone)]
struct RestPlan {
    keys: Vec<String>,
}

struct ReadPlan {
    param: u32,
    path: Vec<String>,
    default: ReadDefault,
    in_value: bool,
}

#[derive(Clone, Copy)]
enum ReadDefault {
    None,
    Index(usize),
}

struct CallPlan {
    call: (u32, u32),
    callee: (u32, u32),
    method: PropsMethod,
    dissolved: bool,
}

pub fn collect(
    program: &Program<'_>,
    scoping: &Scoping,
    _nodes: &AstNodes<'_>,
    reports: &mut Vec<Report>,
) -> Plan {
    let mut symbols = Vec::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        let Some(home) = home_of(import.source.value.as_str()) else { continue };
        if !allows(home, "$props") {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else { continue };
            if named.import_kind.is_type() {
                continue;
            }
            if named.imported.name().as_str() != "$props" {
                continue;
            }
            symbols.push(named.local.symbol_id());
        }
    }
    let mut plan = Plan::default();
    let mut collector = Collector {
        scoping,
        symbols: &symbols,
        used: Vec::new(),
        misused: HashSet::new(),
        reports,
        plan: &mut plan,
    };
    collector.visit_program(program);
    plan
}

pub fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    plan: Plan,
    namer: &mut Namer<'a>,
    helpers: &mut HelperImports<'a>,
) -> bool {
    if plan.is_empty() {
        return false;
    }
    let mut rewrite = Rewrite {
        alloc: allocator,
        plan: &plan,
        namer,
        helpers,
        aliases: HashMap::new(),
        temps: HashMap::new(),
        done: HashSet::new(),
        changed: false,
    };
    rewrite.visit_program(program);
    rewrite.changed
}

struct Collector<'s, 'r, 'p> {
    scoping: &'s Scoping,
    symbols: &'s [SymbolId],
    used: Vec<Span>,
    misused: HashSet<SymbolId>,
    reports: &'r mut Vec<Report>,
    plan: &'p mut Plan,
}

impl Collector<'_, '_, '_> {
    fn component(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
        generator: bool,
    ) {
        let Some(first) = params.items.first() else { return };
        let pattern = first.pattern.span();
        match props_plan(params, body, generator, self.scoping) {
            Ok(None) => {}
            Err(reason) => {
                self.reports.push(
                    Report::new(Code::PropsDestructured, pattern)
                        .arg("reason", reason)
                        .arg("component", name),
                );
            }
            Ok(Some(resolved)) => {
                if has_unnameable_type_read(self.scoping, &resolved.bindings) {
                    self.reports.push(
                        Report::new(Code::PropsDestructured, pattern)
                            .arg("reason", "type")
                            .arg("component", name),
                    );
                    return;
                }
                self.reports
                    .push(Report::new(Code::PropsRewritten, pattern).arg("component", name));
                let param = pattern.start;
                let mut defaults = Vec::new();
                for binding in &resolved.bindings {
                    let default = match binding.default {
                        None => ReadDefault::None,
                        Some(value) => {
                            let index = defaults.len();
                            let base = self.scoping.symbol_name(binding.symbol).to_string();
                            let kind = if is_literal_default(value) {
                                match lit_shape(value) {
                                    Some(shape) => DefaultKind::Literal(shape),
                                    None => DefaultKind::Hoisted { base },
                                }
                            } else {
                                DefaultKind::Hoisted { base }
                            };
                            defaults.push(DefaultEntry {
                                span: value.span(),
                                kind,
                            });
                            ReadDefault::Index(index)
                        }
                    };
                    for &reference in self.scoping.get_resolved_reference_ids(binding.symbol) {
                        let in_value = self.scoping.get_reference(reference).flags().is_value();
                        self.plan.reads.insert(
                            reference,
                            ReadPlan {
                                param,
                                path: binding.path.clone(),
                                default,
                                in_value,
                            },
                        );
                    }
                }
                self.plan.components.push(ComponentPlan {
                    param,
                    defaults,
                    rest: resolved.rest.map(|rest| RestPlan { keys: rest.keys }),
                });
            }
        }
    }
}

impl<'a> Visit<'a> for Collector<'_, '_, '_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if let (Some(id), Some(body)) = (&it.id, &it.body)
            && is_component_name(id.name.as_str())
            && has_jsx(|check| check.visit_function_body(body))
        {
            self.component(id.name.as_str(), &it.params, Some(body), it.generator);
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id
            && is_component_name(id.name.as_str())
        {
            match it.init.as_ref().map(Expression::without_parentheses) {
                Some(Expression::ArrowFunctionExpression(arrow))
                    if has_jsx(|check| check.visit_arrow_function_body(&arrow.body)) =>
                {
                    self.component(id.name.as_str(), &arrow.params, None, false);
                }
                Some(Expression::FunctionExpression(function))
                    if is_declared_component(function)
                        && let Some(body) = &function.body
                        && has_jsx(|check| check.visit_function_body(body)) =>
                {
                    self.component(id.name.as_str(), &function.params, Some(body), function.generator);
                }
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(method) = method_of(self.scoping, self.symbols, &it.callee) {
            let callee = it.callee.without_parentheses().span();
            self.used.push(callee);
            let dissolved = match method {
                PropsMethod::Merge => merge_ok(it),
                PropsMethod::Split => split_shape(it).is_some(),
                PropsMethod::Omit => omit_shape(it).is_some(),
            };
            self.plan.calls.push(CallPlan {
                call: (it.span.start, it.span.end),
                callee: (callee.start, callee.end),
                method,
                dissolved,
            });
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let Some(reference) = it.reference_id.get() else { return };
        let Some(symbol) = self.scoping.get_reference(reference).symbol_id() else { return };
        if self.symbols.contains(&symbol)
            && !self
                .used
                .iter()
                .any(|callee| callee.start <= it.span.start && it.span.end <= callee.end)
        {
            self.misused.insert(symbol);
            self.reports.push(Report::new(Code::PropsAsValue, it.span));
        }
    }
}

fn method_of(scoping: &Scoping, symbols: &[SymbolId], callee: &Expression<'_>) -> Option<PropsMethod> {
    let Expression::StaticMemberExpression(member) = callee.without_parentheses() else {
        return None;
    };
    if member.optional {
        return None;
    }
    let Expression::Identifier(object) = &member.object else { return None };
    let symbol = object.reference_id.get().and_then(|id| scoping.get_reference(id).symbol_id())?;
    if !symbols.contains(&symbol) {
        return None;
    }
    match member.property.name.as_str() {
        "merge" => Some(PropsMethod::Merge),
        "splitByGroups" => Some(PropsMethod::Split),
        "omit" => Some(PropsMethod::Omit),
        _ => None,
    }
}

fn merge_ok(call: &CallExpression<'_>) -> bool {
    if call.optional || call.type_arguments.is_some() {
        return false;
    }
    call.arguments.iter().all(|arg| {
        let Some(expr) = arg.as_expression() else { return false };
        let Expression::ObjectExpression(object) = expr.without_parentheses() else { return false };
        object.properties.iter().all(merge_property_is_static)
    })
}

fn split_shape(call: &CallExpression<'_>) -> Option<Vec<Vec<String>>> {
    if call.optional || call.type_arguments.is_some() || call.arguments.len() < 2 {
        return None;
    }
    let mut args = call.arguments.iter();
    let first = args.next()?.as_expression()?;
    let Expression::ObjectExpression(object) = first.without_parentheses() else { return None };
    for property in &object.properties {
        let (key, _) = static_property(property)?;
        if key == "__proto__" {
            return None;
        }
    }
    let mut groups = Vec::with_capacity(call.arguments.len() - 1);
    for arg in args {
        let list = arg.as_expression()?;
        let Expression::ArrayExpression(array) = list.without_parentheses() else { return None };
        let mut keys = Vec::with_capacity(array.elements.len());
        for element in &array.elements {
            let item = element.as_expression()?;
            let Expression::StringLiteral(key) = item.without_parentheses() else { return None };
            keys.push(key.value.as_str().to_string());
        }
        groups.push(keys);
    }
    Some(groups)
}

fn omit_shape(call: &CallExpression<'_>) -> Option<Vec<String>> {
    if call.optional || call.type_arguments.is_some() || call.arguments.len() < 2 {
        return None;
    }
    let mut args = call.arguments.iter();
    let first = args.next()?.as_expression()?;
    let Expression::ObjectExpression(object) = first.without_parentheses() else { return None };
    for property in &object.properties {
        let (key, _) = static_property(property)?;
        if key == "__proto__" {
            return None;
        }
    }
    let mut keys = Vec::with_capacity(call.arguments.len() - 1);
    for arg in args {
        let item = arg.as_expression()?;
        let Expression::StringLiteral(key) = item.without_parentheses() else { return None };
        keys.push(key.value.as_str().to_string());
    }
    Some(keys)
}


fn lit_shape(value: &Expression<'_>) -> Option<LitShape> {
    match value.without_parentheses() {
        Expression::BooleanLiteral(literal) => Some(LitShape::Bool(literal.value)),
        Expression::NullLiteral(_) => Some(LitShape::Null),
        Expression::NumericLiteral(literal) => Some(LitShape::Num(literal.value)),
        Expression::BigIntLiteral(literal) => {
            Some(LitShape::BigInt { digits: literal.value.as_str().to_string(), base: literal.base })
        }
        Expression::StringLiteral(literal) => {
            Some(LitShape::Text(literal.value.as_str().to_string()))
        }
        Expression::UnaryExpression(unary) => {
            if unary.operator != UnaryOperator::UnaryNegation {
                return None;
            }
            let Expression::NumericLiteral(inner) = unary.argument.without_parentheses() else {
                return None;
            };
            Some(LitShape::NegNum(inner.value))
        }
        Expression::TemplateLiteral(template) => {
            if !template.expressions.is_empty() || template.quasis.len() != 1 {
                return None;
            }
            let quasi = &template.quasis[0];
            Some(LitShape::Template {
                raw: quasi.value.raw.as_str().to_string(),
                cooked: quasi.value.cooked.as_ref().map(|text| text.as_str().to_string()),
                lone: quasi.lone_surrogates,
                quasi: quasi.span,
            })
        }
        Expression::BinaryExpression(binary) => {
            if binary.operator != BinaryOperator::Addition {
                return None;
            }
            Some(LitShape::Add(Box::new(lit_shape(&binary.left)?), Box::new(lit_shape(&binary.right)?)))
        }
        _ => None,
    }
}

enum Slot<'b, 'a> {
    Block(Option<&'b mut FunctionBody<'a>>),
    Arrow(&'b mut ArrowFunctionBody<'a>),
}

impl Slot<'_, '_> {
    fn has_body(&self) -> bool {
        match self {
            Slot::Block(body) => body.is_some(),
            Slot::Arrow(_) => true,
        }
    }
}

struct Rewrite<'x, 'p, 'a> {
    alloc: &'a Allocator,
    plan: &'p Plan,
    namer: &'x mut Namer<'a>,
    helpers: &'x mut HelperImports<'a>,
    aliases: HashMap<u32, &'a str>,
    temps: HashMap<u32, Vec<Option<&'a str>>>,
    done: HashSet<u32>,
    changed: bool,
}

impl<'a> Rewrite<'_, '_, 'a> {
    fn rewrite_component(&mut self, params: &mut FormalParameters<'a>, slot: Slot<'_, 'a>) {
        let Some(first) = params.items.first() else { return };
        let key = first.pattern.span().start;
        let plan: &Plan = self.plan;
        let Some(component) = plan.components.iter().find(|component| component.param == key).cloned()
        else {
            return;
        };
        if !self.done.insert(key) {
            return;
        }
        if component.needs_entry() && !slot.has_body() {
            return;
        }
        let Some(entries) = self.rewrite_params(params, &component) else { return };
        match slot {
            Slot::Block(Some(body)) => prepend(self.alloc, body, entries),
            Slot::Block(None) => {}
            Slot::Arrow(body) => {
                if !entries.is_empty() {
                    blockify(self.alloc, body, entries);
                }
            }
        }
    }

    fn rewrite_params(
        &mut self,
        params: &mut FormalParameters<'a>,
        component: &ComponentPlan,
    ) -> Option<ArenaVec<'a, Statement<'a>>> {
        let alloc = self.alloc;
        let first = params.items.iter_mut().next()?;
        let pattern_span = first.pattern.span();
        let BindingPattern::ObjectPattern(object) = &mut first.pattern else { return None };
        let builder = AstBuilder::new(alloc);
        let mut rest = match object.rest.take() {
            None => None,
            Some(rest) => {
                let span = rest.span;
                match rest.unbox().argument {
                    BindingPattern::BindingIdentifier(id) => Some(id),
                    argument => {
                        object.rest =
                            Some(BindingRestElement::boxed(span, argument, &builder));
                        return None;
                    }
                }
            }
        };
        if rest.is_some() && component.rest.is_none() {
            if let Some(id) = rest.take() {
                let span = id.span;
                object.rest = Some(BindingRestElement::boxed(
                    span,
                    BindingPattern::BindingIdentifier(id),
                    &builder,
                ));
            }
            return None;
        }
        let mut hoisted = HashSet::new();
        for default in &component.defaults {
            if matches!(default.kind, DefaultKind::Hoisted { .. }) {
                hoisted.insert((default.span.start, default.span.end));
            }
        }
        let mut taken = HashMap::new();
        take_defaults(object, &hoisted, &mut taken, alloc);
        let key = pattern_span.start;
        let props: &'a str = alloc.alloc_str(&self.namer.fresh("_props$"));
        self.aliases.insert(key, props);
        first.pattern =
            BindingPattern::new_binding_identifier(pattern_span, Ident::from(props), &builder);
        self.temps.insert(key, vec![None; component.defaults.len()]);
        let mut entries = ArenaVec::new_in(&alloc);
        if let Some(id) = rest
            && let Some(keys) = component.rest.as_ref().map(|plan| &plan.keys)
        {
            let split = self.helpers.require(alloc, self.namer, "reze-js", "splitProps");
            entries.push(rest_decl(alloc, id, props, split, keys));
        }
        for (index, default) in component.defaults.iter().enumerate() {
            let base = match &default.kind {
                DefaultKind::Literal(_) => continue,
                DefaultKind::Hoisted { base } => base,
            };
            let temp: &'a str = alloc.alloc_str(&self.namer.fresh(&format!("_{base}$default")));
            let Some(init) = taken.remove(&(default.span.start, default.span.end)) else {
                continue;
            };
            if let Some(temps) = self.temps.get_mut(&key)
                && let Some(slot) = temps.get_mut(index)
            {
                *slot = Some(temp);
            }
            entries.push(const_decl(alloc, temp, init));
        }
        self.changed = true;
        Some(entries)
    }

    fn plan_read(&self, span: Span, reference: Option<ReferenceId>) -> Option<Expression<'a>> {
        let reference = reference?;
        let plan: &Plan = self.plan;
        let read = plan.reads.get(&reference)?;
        let props = *self.aliases.get(&read.param)?;
        if !read.in_value {
            return Some(chain(self.alloc, span, props, &read.path));
        }
        let component = plan.components.iter().find(|component| component.param == read.param)?;
        match read.default {
            ReadDefault::None => Some(chain(self.alloc, span, props, &read.path)),
            ReadDefault::Index(index) => {
                let default = component.defaults.get(index)?;
                let fallback = match &default.kind {
                    DefaultKind::Literal(shape) => build_literal(self.alloc, default.span, shape),
                    DefaultKind::Hoisted { .. } => {
                        let temps = self.temps.get(&read.param)?;
                        let temp = temps.get(index)?.as_ref()?;
                        reference_expr(self.alloc, SPAN, temp)
                    }
                };
                Some(conditional(self.alloc, span, props, &read.path, fallback))
            }
        }
    }

    fn plan_call(&mut self, it: &mut Expression<'a>) {
        let Expression::CallExpression(call) = it else { return };
        let key = (call.span.start, call.span.end);
        let plan: &Plan = self.plan;
        let Some(found) = plan.calls.iter().find(|call| call.call == key) else { return };
        let (method, dissolved, callee) = (found.method, found.dissolved, found.callee);
        if dissolved
            && let Some(replacement) = dissolve(self.alloc, call, method)
        {
            *it = replacement;
            self.changed = true;
            return;
        }
        let export = match method {
            PropsMethod::Merge => "mergeProps",
            PropsMethod::Split => "splitProps",
            PropsMethod::Omit => "omitProps",
        };
        let alias = self.helpers.require(self.alloc, self.namer, "reze-js", export);
        call.callee = reference_expr(self.alloc, Span::new(callee.0, callee.1), alias);
        self.changed = true;
    }
}

impl<'a> VisitMut<'a> for Rewrite<'_, '_, 'a> {
    fn visit_function(&mut self, it: &mut Function<'a>, flags: ScopeFlags) {
        let key = it.params.items.first().map(|first| first.pattern.span().start);
        if let Some(key) = key
            && self.plan.components.iter().any(|component| component.param == key)
        {
            let body = it.body.as_mut().map(|body| body.as_mut());
            self.rewrite_component(&mut it.params, Slot::Block(body));
        }
        walk_mut::walk_function(self, it, flags);
    }

    fn visit_variable_declarator(&mut self, it: &mut VariableDeclarator<'a>) {
        if let Some(init) = it.init.as_mut() {
            if let Some(arrow) = arrow_of(init) {
                let key = arrow.params.items.first().map(|first| first.pattern.span().start);
                if let Some(key) = key
                    && self.plan.components.iter().any(|component| component.param == key)
                {
                    let body = std::mem::replace(&mut arrow.body, empty_body(self.alloc));
                    match body {
                        ArrowFunctionBody::FunctionBody(mut taken) => {
                            self.rewrite_component(
                                &mut arrow.params,
                                Slot::Block(Some(taken.as_mut())),
                            );
                            arrow.body = ArrowFunctionBody::FunctionBody(taken);
                        }
                        mut other => {
                            self.rewrite_component(&mut arrow.params, Slot::Arrow(&mut other));
                            arrow.body = other;
                        }
                    }
                }
            } else if let Some(function) = function_of(init) {
                let key = function.params.items.first().map(|first| first.pattern.span().start);
                if let Some(key) = key
                    && self.plan.components.iter().any(|component| component.param == key)
                {
                    let mut taken = function.body.take();
                    let slot = taken.as_mut().map(|body| body.as_mut());
                    self.rewrite_component(&mut function.params, Slot::Block(slot));
                    function.body = taken;
                }
            }
        }
        walk_mut::walk_variable_declarator(self, it);
    }

    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        if matches!(it, Expression::CallExpression(_)) {
            self.plan_call(it);
        } else if let Expression::Identifier(id) = it {
            let span = id.span;
            let reference = id.reference_id.get();
            if let Some(replacement) = self.plan_read(span, reference) {
                *it = replacement;
                self.changed = true;
                return;
            }
        }
        walk_mut::walk_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &mut ObjectProperty<'a>) {
        if it.shorthand
            && let Expression::Identifier(id) = &it.value
            && let Some(replacement) = self.plan_read(id.span, id.reference_id.get())
        {
            it.shorthand = false;
            it.value = replacement;
            self.changed = true;
        }
        walk_mut::walk_object_property(self, it);
    }

    fn visit_ts_type_name(&mut self, it: &mut TSTypeName<'a>) {
        let hit = match it {
            TSTypeName::IdentifierReference(id) => id.reference_id.get(),
            _ => None,
        };
        if let Some(reference) = hit {
            let plan: &Plan = self.plan;
            if let Some(read) = plan.reads.get(&reference)
                && let Some(props) = self.aliases.get(&read.param)
                && let Some(name) = qualified(self.alloc, props, &read.path)
            {
                *it = name;
                self.changed = true;
                return;
            }
        }
        walk_mut::walk_ts_type_name(self, it);
    }
}

fn arrow_of<'a, 'b>(expr: &'a mut Expression<'b>) -> Option<&'a mut ArenaBox<'b, ArrowFunctionExpression<'b>>> {
    match expr {
        Expression::ParenthesizedExpression(inner) => arrow_of(&mut inner.expression),
        Expression::ArrowFunctionExpression(arrow) => Some(arrow),
        _ => None,
    }
}

fn function_of<'a, 'b>(expr: &'a mut Expression<'b>) -> Option<&'a mut ArenaBox<'b, Function<'b>>> {
    match expr {
        Expression::ParenthesizedExpression(inner) => function_of(&mut inner.expression),
        Expression::FunctionExpression(function) => Some(function),
        _ => None,
    }
}

fn object_mut<'a, 'b>(
    expr: &'a mut Expression<'b>,
) -> Option<&'a mut ArenaBox<'b, ObjectExpression<'b>>> {
    match expr {
        Expression::ParenthesizedExpression(inner) => object_mut(&mut inner.expression),
        Expression::ObjectExpression(object) => Some(object),
        _ => None,
    }
}

fn take_defaults<'a>(
    pattern: &mut ObjectPattern<'a>,
    hoisted: &HashSet<(u32, u32)>,
    taken: &mut HashMap<(u32, u32), Expression<'a>>,
    alloc: &'a Allocator,
) {
    for property in pattern.properties.iter_mut() {
        match &mut property.value {
            BindingPattern::ObjectPattern(nested) => take_defaults(nested, hoisted, taken, alloc),
            BindingPattern::AssignmentPattern(assignment) => {
                let span = assignment.right.span();
                if hoisted.contains(&(span.start, span.end)) {
                    taken.insert((span.start, span.end), take_expr(alloc, &mut assignment.right));
                }
            }
            _ => {}
        }
    }
}

fn dissolve<'a>(alloc: &'a Allocator, call: &mut CallExpression<'a>, method: PropsMethod) -> Option<Expression<'a>> {
    match method {
        PropsMethod::Merge => dissolve_merge(alloc, call),
        PropsMethod::Split => dissolve_split(alloc, call),
        PropsMethod::Omit => dissolve_omit(alloc, call),
    }
}

fn dissolve_merge<'a>(alloc: &'a Allocator, call: &mut CallExpression<'a>) -> Option<Expression<'a>> {
    if !merge_ok(call) {
        return None;
    }
    let span = call.span;
    let mut props = ArenaVec::new_in(&alloc);
    for arg in call.arguments.iter_mut() {
        let Some(expr) = arg.as_expression_mut() else { continue };
        let Some(object) = object_mut(expr) else { continue };
        let taken = std::mem::replace(&mut object.properties, ArenaVec::new_in(&alloc));
        for property in taken {
            props.push(property);
        }
    }
    Some(object_expr(alloc, span, props))
}

fn dissolve_split<'a>(alloc: &'a Allocator, call: &mut CallExpression<'a>) -> Option<Expression<'a>> {
    let groups = split_shape(call)?;
    let span = call.span;
    let first = call.arguments.iter_mut().next()?.as_expression_mut()?;
    let object = object_mut(first)?;
    let taken = std::mem::replace(&mut object.properties, ArenaVec::new_in(&alloc));
    let mut views: Vec<ArenaVec<ObjectPropertyKind>> = Vec::new();
    for _ in 0..groups.len() + 1 {
        views.push(ArenaVec::new_in(&alloc));
    }
    for property in taken {
        let key = static_property(&property).map(|(key, _)| key).unwrap_or("");
        let mut placed = views.len() - 1;
        for (index, group) in groups.iter().enumerate() {
            if group.iter().any(|member| member.as_str() == key) {
                placed = index;
                break;
            }
        }
        views[placed].push(property);
    }
    let mut elements = ArenaVec::new_in(&alloc);
    for view in views {
        elements.push(ArrayExpressionElement::from(object_expr(alloc, SPAN, view)));
    }
    Some(array_expr(alloc, span, elements))
}

fn dissolve_omit<'a>(alloc: &'a Allocator, call: &mut CallExpression<'a>) -> Option<Expression<'a>> {
    let omitted = omit_shape(call)?;
    let span = call.span;
    let first = call.arguments.iter_mut().next()?.as_expression_mut()?;
    let object = object_mut(first)?;
    let taken = std::mem::replace(&mut object.properties, ArenaVec::new_in(&alloc));
    let mut kept = ArenaVec::new_in(&alloc);
    for property in taken {
        let drop = static_property(&property)
            .map(|(key, _)| omitted.iter().any(|one| one.as_str() == key))
            .unwrap_or(false);
        if !drop {
            kept.push(property);
        }
    }
    Some(object_expr(alloc, span, kept))
}

fn prepend<'a>(alloc: &'a Allocator, body: &mut FunctionBody<'a>, entries: ArenaVec<'a, Statement<'a>>) {
    if entries.is_empty() {
        return;
    }
    let old = std::mem::replace(&mut body.statements, ArenaVec::new_in(&alloc));
    let mut statements = ArenaVec::new_in(&alloc);
    for entry in entries {
        statements.push(entry);
    }
    for statement in old {
        statements.push(statement);
    }
    body.statements = statements;
}

fn blockify<'a>(
    alloc: &'a Allocator,
    body: &mut ArrowFunctionBody<'a>,
    entries: ArenaVec<'a, Statement<'a>>,
) {
    let builder = AstBuilder::new(alloc);
    let old = std::mem::replace(body, empty_body(alloc));
    if !old.is_expression() {
        *body = old;
        return;
    }
    let expr = Expression::try_from(old).unwrap();
    let span = expr.span();
    let mut statements = entries;
    statements.push(Statement::new_return_statement(SPAN, Some(expr), &builder));
    let taken = ArrowFunctionBody::new_function_body(
        span,
        ArenaVec::new_in(&builder),
        statements,
        &builder,
    );
    *body = taken;
}

fn empty_body<'a>(alloc: &'a Allocator) -> ArrowFunctionBody<'a> {
    let builder = AstBuilder::new(alloc);
    ArrowFunctionBody::new_function_body(
        SPAN,
        ArenaVec::new_in(&builder),
        ArenaVec::new_in(&builder),
        &builder,
    )
}

fn rest_decl<'a>(
    alloc: &'a Allocator,
    id: ArenaBox<'a, BindingIdentifier<'a>>,
    props: &'a str,
    split: &'a str,
    keys: &[String],
) -> Statement<'a> {
    let builder = AstBuilder::new(alloc);
    let mut key_list = ArenaVec::new_in(&builder);
    for key in keys {
        key_list.push(ArrayExpressionElement::from(string_expr(alloc, SPAN, key)));
    }
    let mut arguments = ArenaVec::new_in(&builder);
    arguments.push(Argument::from(reference_expr(alloc, SPAN, props)));
    arguments.push(Argument::from(Expression::new_array_expression(SPAN, key_list, &builder)));
    let split_call = Expression::new_call_expression(
        SPAN,
        reference_expr(alloc, SPAN, split),
        None,
        arguments,
        false,
        &builder,
    );
    let one = Expression::new_numeric_literal(SPAN, 1.0, None, NumberBase::Decimal, &builder);
    let init = Expression::new_computed_member_expression(SPAN, split_call, one, false, &builder);
    const_decl_pattern(alloc, BindingPattern::BindingIdentifier(id), init)
}

fn const_decl<'a>(alloc: &'a Allocator, name: &'a str, init: Expression<'a>) -> Statement<'a> {
    let builder = AstBuilder::new(alloc);
    let pattern = BindingPattern::new_binding_identifier(SPAN, Ident::from(name), &builder);
    const_decl_pattern(alloc, pattern, init)
}

fn const_decl_pattern<'a>(
    alloc: &'a Allocator,
    id: BindingPattern<'a>,
    init: Expression<'a>,
) -> Statement<'a> {
    let builder = AstBuilder::new(alloc);
    let mut declarations = ArenaVec::new_in(&builder);
    declarations.push(VariableDeclarator::new(SPAN, id, None, Some(init), false, &builder));
    Statement::new_variable_declaration(
        SPAN,
        VariableDeclarationKind::Const,
        declarations,
        false,
        &builder,
    )
}

fn chain<'a>(alloc: &'a Allocator, outer: Span, props: &str, path: &[String]) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    let mut expr = reference_expr(alloc, SPAN, alloc.alloc_str(props));
    let last = path.len().saturating_sub(1);
    for (index, key) in path.iter().enumerate() {
        let span = if index == last { outer } else { SPAN };
        if is_identifier_name(key) {
            let text: &'a str = alloc.alloc_str(key);
            let property = IdentifierName::new(SPAN, Ident::from(text), &builder);
            expr = Expression::new_static_member_expression(span, expr, property, false, &builder);
        } else {
            expr = Expression::new_computed_member_expression(
                span,
                expr,
                string_expr(alloc, SPAN, key),
                false,
                &builder,
            );
        }
    }
    expr
}

fn conditional<'a>(
    alloc: &'a Allocator,
    span: Span,
    props: &'a str,
    path: &[String],
    fallback: Expression<'a>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    let undefined: &'a str = alloc.alloc_str("undefined");
    let test = Expression::new_binary_expression(
        SPAN,
        chain(alloc, SPAN, props, path),
        BinaryOperator::StrictEquality,
        reference_expr(alloc, SPAN, undefined),
        &builder,
    );
    Expression::new_conditional_expression(
        span,
        test,
        fallback,
        chain(alloc, span, props, path),
        &builder,
    )
}

fn qualified<'a>(alloc: &'a Allocator, props: &str, path: &[String]) -> Option<TSTypeName<'a>> {
    let builder = AstBuilder::new(alloc);
    let props_text: &'a str = alloc.alloc_str(props);
    let mut name =
        TSTypeName::IdentifierReference(IdentifierReference::boxed(SPAN, Ident::from(props_text), &builder));
    for key in path {
        if !is_identifier_name(key) {
            return None;
        }
        let text: &'a str = alloc.alloc_str(key);
        let right = IdentifierName::new(SPAN, Ident::from(text), &builder);
        name = TSTypeName::QualifiedName(TSQualifiedName::boxed(SPAN, name, right, &builder));
    }
    Some(name)
}

fn build_literal<'a>(alloc: &'a Allocator, span: Span, shape: &LitShape) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    match shape {
        LitShape::Bool(value) => Expression::new_boolean_literal(span, *value, &builder),
        LitShape::Null => Expression::new_null_literal(span, &builder),
        LitShape::Num(value) => {
            Expression::new_numeric_literal(span, *value, None, NumberBase::Decimal, &builder)
        }
        LitShape::BigInt { digits, base } => Expression::new_big_int_literal(
            span,
            Str::from_str_in(digits, &alloc),
            None,
            *base,
            &builder,
        ),
        LitShape::Text(value) => string_expr(alloc, span, value),
        LitShape::NegNum(value) => {
            let argument =
                Expression::new_numeric_literal(span, *value, None, NumberBase::Decimal, &builder);
            Expression::new_unary_expression(span, UnaryOperator::UnaryNegation, argument, &builder)
        }
        LitShape::Template { raw, cooked, lone, quasi } => {
            let value = TemplateElementValue {
                raw: Str::from_str_in(raw, &alloc),
                cooked: cooked.as_deref().map(|text| Str::from_str_in(text, &alloc)),
            };
            let mut quasis = ArenaVec::new_in(&builder);
            quasis.push(TemplateElement::new_with_lone_surrogates(
                *quasi, value, true, *lone, &builder,
            ));
            Expression::new_template_literal(
                span,
                quasis,
                ArenaVec::new_in(&builder),
                &builder,
            )
        }
        LitShape::Add(left, right) => Expression::new_binary_expression(
            span,
            build_literal(alloc, span, left),
            BinaryOperator::Addition,
            build_literal(alloc, span, right),
            &builder,
        ),
    }
}

fn reference_expr<'a>(alloc: &'a Allocator, span: Span, name: &'a str) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_identifier(span, Ident::from(name), &builder)
}

fn string_expr<'a>(alloc: &'a Allocator, span: Span, value: &str) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_string_literal(span, Str::from_str_in(value, &alloc), None, &builder)
}

fn object_expr<'a>(
    alloc: &'a Allocator,
    span: Span,
    props: ArenaVec<'a, ObjectPropertyKind<'a>>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_object_expression(span, props, &builder)
}

fn array_expr<'a>(
    alloc: &'a Allocator,
    span: Span,
    elements: ArenaVec<'a, ArrayExpressionElement<'a>>,
) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_array_expression(span, elements, &builder)
}

fn dummy_expr<'a>(alloc: &'a Allocator, span: Span) -> Expression<'a> {
    let builder = AstBuilder::new(alloc);
    Expression::new_null_literal(span, &builder)
}

fn take_expr<'a>(alloc: &'a Allocator, it: &mut Expression<'a>) -> Expression<'a> {
    let span = it.span();
    std::mem::replace(it, dummy_expr(alloc, span))
}
