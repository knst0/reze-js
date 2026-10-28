use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    ArrowFunctionExpression, Expression, Function, JSXElement, JSXFragment, Program, PropertyKey,
    SimpleAssignmentTarget, Statement, UpdateExpression, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};
use oxc_syntax::symbol::SymbolId;

use crate::diagnostic::{Code, Report};
use crate::frontend::analysis::SharedFacts;

/// Names of request-time route callbacks: them writing module state leaks between requests.
const REQUEST_CALLBACKS: &[&str] = &["preload", "beforeLoad", "redirect", "loader", "action"];

/// Module bindings the module-state lowering recreates per request, so writing them is safe.
const REQUEST_LOCAL_FACTORIES: &[&str] =
    &["signal", "computed", "action", "selector", "store", "createContext"];

struct Binding {
    name: String,
}

struct TopLevel<'a> {
    facts: &'a SharedFacts,
    scoping: &'a Scoping,
    mutable: Vec<(SymbolId, Binding)>,
}

fn is_request_local(facts: &SharedFacts, scoping: &Scoping, init: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = init.without_parentheses() else {
        return false;
    };
    if !matches!(call.callee.without_parentheses(), Expression::Identifier(_)) {
        return false;
    }
    facts
        .imported(scoping, &call.callee)
        .is_some_and(|name| REQUEST_LOCAL_FACTORIES.contains(&name))
}

impl<'a> TopLevel<'a> {
    fn statement(&mut self, statement: &Statement<'_>) {
        let declaration = match statement {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportDeclaration(export) => {
                let oxc_ast::ast::Declaration::VariableDeclaration(declaration) = &export.declaration
                else {
                    return;
                };
                declaration
            }
            _ => return,
        };
        let kind = declaration.kind;
        if !matches!(kind, VariableDeclarationKind::Let | VariableDeclarationKind::Var) {
            return;
        }
        for declarator in &declaration.declarations {
            if declarator
                .init
                .as_ref()
                .is_some_and(|init| is_request_local(self.facts, self.scoping, init))
            {
                continue;
            }
            for id in declarator.id.get_binding_identifiers() {
                self.mutable.push((
                    id.symbol_id(),
                    Binding { name: id.name.to_string() },
                ));
            }
        }
    }
}

struct Functions {
    names: HashMap<u32, String>,
    jsx: HashSet<u32>,
    stack: Vec<u32>,
}

fn callback_name(name: &str) -> bool {
    REQUEST_CALLBACKS.contains(&name)
}

fn static_key<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

impl Functions {
    fn expression_name(&mut self, expression: &Expression<'_>, name: Option<&str>) {
        let start = expression.span().start;
        match expression {
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => {
                if let Some(name) = name.filter(|name| callback_name(name)) {
                    self.names.insert(start, name.to_string());
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let Some(key) = static_key(&property.key) else {
                        continue;
                    };
                    self.expression_name(&property.value, Some(key));
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for Functions {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        let start = function.span.start;
        if let Some(id) = &function.id
            && callback_name(id.name.as_str())
        {
            self.names.insert(start, id.name.to_string());
        }
        self.stack.push(start);
        walk::walk_function(self, function, flags);
        self.stack.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.stack.push(arrow.span.start);
        walk::walk_arrow_function_expression(self, arrow);
        self.stack.pop();
    }

    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        if let Some(&start) = self.stack.last() {
            self.jsx.insert(start);
        }
        walk::walk_jsx_element(self, element);
    }

    fn visit_jsx_fragment(&mut self, fragment: &JSXFragment<'a>) {
        if let Some(&start) = self.stack.last() {
            self.jsx.insert(start);
        }
        walk::walk_jsx_fragment(self, fragment);
    }

    fn visit_variable_declarator(&mut self, declarator: &oxc_ast::ast::VariableDeclarator<'a>) {
        if let Some(init) = &declarator.init {
            let name = match &declarator.id {
                oxc_ast::ast::BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                _ => None,
            };
            self.expression_name(init, name);
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_object_property(&mut self, property: &oxc_ast::ast::ObjectProperty<'a>) {
        if let Some(key) = static_key(&property.key) {
            self.expression_name(&property.value, Some(key));
        }
        walk::walk_object_property(self, property);
    }
}

enum Owner {
    Component,
    Callback,
    Other,
}

struct Writes<'a> {
    scoping: &'a Scoping,
    mutable: HashMap<SymbolId, Binding>,
    names: &'a HashMap<u32, String>,
    jsx: &'a HashSet<u32>,
    stack: Vec<Owner>,
    reported: HashSet<SymbolId>,
    reports: Vec<Report>,
}

fn root_symbol(scoping: &Scoping, target: &SimpleAssignmentTarget<'_>) -> Option<SymbolId> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
            scoping.get_reference(id.reference_id.get()?).symbol_id()
        }
        SimpleAssignmentTarget::StaticMemberExpression(member) => {
            let Expression::Identifier(object) = &member.object else {
                return None;
            };
            scoping.get_reference(object.reference_id.get()?).symbol_id()
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            let Expression::Identifier(object) = &member.object else {
                return None;
            };
            scoping.get_reference(object.reference_id.get()?).symbol_id()
        }
        _ => None,
    }
}

impl<'a> Writes<'a> {
    fn owner_at(&self, start: u32) -> Owner {
        if self.names.contains_key(&start) {
            return Owner::Callback;
        }
        if self.jsx.contains(&start) {
            return Owner::Component;
        }
        Owner::Other
    }

    fn write(&mut self, symbol: SymbolId, span: Span) {
        if !matches!(self.stack.last(), Some(Owner::Component | Owner::Callback)) {
            return;
        }
        let Some(binding) = self.mutable.get(&symbol) else {
            return;
        };
        if !self.reported.insert(symbol) {
            return;
        }
        self.reports.push(
            Report::new(Code::ModuleStateShared, span).arg("name", binding.name.clone()),
        );
    }
}

impl<'a> Visit<'a> for Writes<'a> {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.stack.push(self.owner_at(function.span.start));
        walk::walk_function(self, function, flags);
        self.stack.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.stack.push(self.owner_at(arrow.span.start));
        walk::walk_arrow_function_expression(self, arrow);
        self.stack.pop();
    }

    fn visit_assignment_expression(&mut self, assignment: &oxc_ast::ast::AssignmentExpression<'a>) {
        if let Some(target) = assignment.left.as_simple_assignment_target()
            && let Some(symbol) = root_symbol(self.scoping, target)
        {
            self.write(symbol, assignment.left.span());
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        if let Some(symbol) = root_symbol(self.scoping, &update.argument) {
            self.write(symbol, update.span);
        }
        walk::walk_update_expression(self, update);
    }
}

/// Reports module bindings that survive a request and are written from a component or a route
/// callback. Only server rendering shares a process across requests, so callers gate on the target.
pub fn check(
    program: &Program<'_>,
    scoping: &Scoping,
    facts: &SharedFacts,
) -> Vec<Report> {
    let mut top = TopLevel { facts, scoping, mutable: Vec::new() };
    for statement in &program.body {
        top.statement(statement);
    }
    if top.mutable.is_empty() {
        return Vec::new();
    }
    let mut functions = Functions { names: HashMap::new(), jsx: HashSet::new(), stack: Vec::new() };
    functions.visit_program(program);
    let mut writes = Writes {
        scoping,
        mutable: top.mutable.drain(..).collect(),
        names: &functions.names,
        jsx: &functions.jsx,
        stack: Vec::new(),
        reported: HashSet::new(),
        reports: Vec::new(),
    };
    writes.visit_program(program);
    writes.reports
}
