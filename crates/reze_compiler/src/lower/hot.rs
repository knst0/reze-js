use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::{has_jsx, is_component_name};
use oxc_ast_visit::Visit;

/// Hot components of a module and whether it can accept its own updates.
#[derive(Default)]
pub struct HotPlan<'a> {
    /// Component names of function declarations, by the start of their statement.
    pub declarations: HashMap<u32, &'a str>,
    /// Component names of `const` initializers, by the start of the initializer.
    pub inits: HashMap<u32, &'a str>,
    /// Every export is a hot component binding, and there is one.
    pub accept: bool,
}

pub fn plan<'a>(program: &Program<'a>) -> HotPlan<'a> {
    let mut plan = HotPlan::default();
    let mut exports_only_components = true;
    for statement in &program.body {
        let start = statement.span().start;
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(name) = hot_function(function) {
                    plan.declarations.insert(start, name);
                }
            }
            Statement::VariableDeclaration(variables) => {
                hot_const(variables, &mut plan);
            }
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => match hot_function(function) {
                    Some(name) => {
                        plan.declarations.insert(start, name);
                    }
                    None => exports_only_components = false,
                },
                Declaration::VariableDeclaration(variables) => {
                    exports_only_components &= hot_const(variables, &mut plan);
                }
                declaration => exports_only_components &= declaration.is_type(),
            },
            Statement::ExportFromDeclaration(export) => {
                exports_only_components &= export.export_kind.is_type();
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    match hot_function(function) {
                        Some(name) => {
                            plan.declarations.insert(start, name);
                        }
                        None => exports_only_components = false,
                    }
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {}
                _ => exports_only_components = false,
            },
            Statement::ExportAllDeclaration(export) => {
                exports_only_components &= export.export_kind.is_type();
            }
            _ => {}
        }
    }
    let names: std::vec::Vec<&str> =
        plan.declarations.values().chain(plan.inits.values()).copied().collect();
    for statement in &program.body {
        if let Statement::ExportNamedDeclaration(export) = statement
            && !export.export_kind.is_type()
        {
            exports_only_components &= export.specifiers.iter().all(|specifier| {
                specifier.export_kind.is_type()
                    || matches!(&specifier.local, ModuleExportName::IdentifierReference(local) if names.contains(&local.name.as_str()))
            });
        }
    }
    plan.accept = exports_only_components && !names.is_empty();
    plan
}

fn hot_function<'a>(function: &Function<'a>) -> Option<&'a str> {
    let name = function.id.as_ref()?.name.as_str();
    let body = function.body.as_ref()?;
    (is_component_name(name) && has_jsx(|check| check.visit_function_body(body))).then_some(name)
}

/// Registers a single-declarator `const N = <arrow | function>` component; `false` when the
/// declaration binds anything else.
fn hot_const<'a>(variables: &VariableDeclaration<'a>, plan: &mut HotPlan<'a>) -> bool {
    let [declarator] = variables.declarations.as_slice() else { return false };
    if variables.kind != VariableDeclarationKind::Const {
        return false;
    }
    let BindingPattern::BindingIdentifier(id) = &declarator.id else { return false };
    let Some(init) = &declarator.init else { return false };
    let name = id.name.as_str();
    let is_component = is_component_name(name)
        && match init.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                has_jsx(|check| check.visit_arrow_function_body(&arrow.body))
            }
            Expression::FunctionExpression(function) => function
                .body
                .as_ref()
                .is_some_and(|body| has_jsx(|check| check.visit_function_body(body))),
            _ => false,
        };
    if is_component {
        plan.inits.insert(init.span().start, name);
    }
    is_component
}
