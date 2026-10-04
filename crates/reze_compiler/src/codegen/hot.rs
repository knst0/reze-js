use std::collections::HashMap;

use oxc_allocator::{Allocator, ArenaVec};
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, SPAN};

use crate::ast::Ast;
use crate::frontend::Namer;
use crate::frontend::pure::{has_jsx, is_component_name};
use crate::imports::HelperImports;
use crate::{CompileTarget, Options, RUNTIME_MODULE};

/// Top-level components of a module and whether it accepts its own updates.
///
/// Collect with [`collect`] while the original JSX remains, then hand the plan
/// to [`apply`] after ordinary emission but before helper imports install.
/// The plan keys statements by source span and owns every component name, so it
/// survives the frontend semantic rebuild and async normalization without
/// holding any semantic identity.
#[derive(Debug, Default)]
pub struct HotPlan {
    declarations: HashMap<u32, String>,
    inits: HashMap<u32, String>,
    accept: bool,
}

impl HotPlan {
    /// Whether the module needs no hot-swap output at all.
    pub fn is_empty(&self) -> bool {
        !self.accept && self.declarations.is_empty() && self.inits.is_empty()
    }
}

/// Whether hot-swap output applies: client target with `options.hot` only,
/// never `html` or `hydrate`.
pub fn should_apply(options: &Options) -> bool {
    options.hot && options.target == CompileTarget::Client
}

/// Scans the top-level statements for components: capitalized function
/// declarations containing JSX, and single-declarator `const` bindings of an
/// arrow or function expression containing JSX. Only top-level statements are
/// considered, so nested bindings shadowing a component name are never
/// collected. `accept` holds when every export is an eligible component or
/// type-only and at least one component exists.
pub fn collect(program: &Program<'_>) -> HotPlan {
    let mut plan = HotPlan::default();
    let mut exports_only_components = true;
    for statement in &program.body {
        let start = statement.span().start;
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(name) = hot_function(function) {
                    plan.declarations.insert(start, name.to_owned());
                }
            }
            Statement::VariableDeclaration(variables) => {
                hot_const(variables, &mut plan);
            }
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => match hot_function(function) {
                    Some(name) => {
                        plan.declarations.insert(start, name.to_owned());
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
                            plan.declarations.insert(start, name.to_owned());
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
    let names: Vec<&str> =
        plan.declarations.values().chain(plan.inits.values()).map(String::as_str).collect();
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

/// Registers the collected components in the emitted program: each matching
/// function declaration keeps its place and hoisting while an assignment
/// registration follows it, each matching `const` initializer is wrapped in
/// place around its already emitted value, and the module tail accepts updates
/// when the plan allows it. Only top-level statements match, each guarded by
/// both span and binding name so synthesized statements can never collide.
/// Requires the `hotComponent` helper from `reze-js` on first use and returns
/// whether the program changed.
pub fn apply<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    plan: &HotPlan,
    filename: &str,
    namer: &mut Namer<'a>,
    helpers: &mut HelperImports<'a>,
) -> bool {
    if plan.is_empty() {
        return false;
    }
    let ast = Ast::new(allocator);
    let mut helper: Option<&'a str> = None;
    let mut after: Vec<(usize, String)> = Vec::new();
    let mut changed = false;
    for (index, statement) in program.body.iter_mut().enumerate() {
        if let Some(name) = declared_name(statement)
            && let Some(wanted) = plan.declarations.get(&statement.span().start)
            && *wanted == name
        {
            after.push((index, wanted.clone()));
        }
        if let Some(variables) = top_const(statement) {
            if variables.kind != VariableDeclarationKind::Const
                || variables.declarations.len() != 1
            {
                continue;
            }
            let declarator = &mut variables.declarations[0];
            let (binding, start) = match (&declarator.id, &declarator.init) {
                (BindingPattern::BindingIdentifier(id), Some(init)) => {
                    (id.name.as_str(), init.span().start)
                }
                _ => continue,
            };
            if plan.inits.get(&start).is_some_and(|wanted| *wanted == binding) {
                let alias = helper.get_or_insert_with(|| {
                    helpers.require(allocator, namer, RUNTIME_MODULE, "hotComponent")
                });
                let init = declarator.init.take().expect("hot component has an initializer");
                let id = format!("{filename}#{binding}");
                declarator.init =
                    Some(ast.call(ast.ident(*alias), [import_meta_hot(allocator), ast.string(&id), init]));
                changed = true;
            }
        }
    }
    if after.is_empty() && !changed && !plan.accept {
        return false;
    }
    if !after.is_empty() && helper.is_none() {
        helper = Some(helpers.require(allocator, namer, RUNTIME_MODULE, "hotComponent"));
    }
    let extra = after.len() + usize::from(plan.accept);
    let mut pending = after.into_iter().peekable();
    let old = std::mem::replace(&mut program.body, ArenaVec::new_in(&ast.builder));
    let mut body = ArenaVec::with_capacity_in(old.len() + extra, &ast.builder);
    for (index, statement) in old.into_iter().enumerate() {
        body.push(statement);
        while pending.peek().is_some_and(|(at, _)| *at == index) {
            let (_, name) = pending.next().expect("peeked hot registration exists");
            let binding = allocator.alloc_str(&name);
            let id = format!("{filename}#{name}");
            let call = ast.call(
                ast.ident(helper.expect("hot registration needs its helper")),
                [import_meta_hot(allocator), ast.string(&id), ast.ident(binding)],
            );
            body.push(ast.stmt(ast.assign(ast.ident(binding), call)));
            changed = true;
        }
    }
    if plan.accept {
        body.push(accept_tail(allocator));
        changed = true;
    }
    program.body = body;
    changed
}

/// Fresh `import.meta.hot` over synthetic spans.
fn import_meta_hot(allocator: &Allocator) -> Expression<'_> {
    let ast = Ast::new(allocator);
    ast.member(Expression::new_import_meta(SPAN, &ast.builder), allocator.alloc_str("hot"))
}

/// `if (import.meta.hot) import.meta.hot.accept();` over synthetic spans.
fn accept_tail(allocator: &Allocator) -> Statement<'_> {
    let ast = Ast::new(allocator);
    let accept = ast.member(import_meta_hot(allocator), allocator.alloc_str("accept"));
    ast.if_stmt(import_meta_hot(allocator), ast.stmt(ast.call(accept, [])))
}

/// The component name a top-level statement declares, if it declares one.
fn declared_name<'a>(statement: &Statement<'a>) -> Option<&'a str> {
    match statement {
        Statement::FunctionDeclaration(function) => {
            function.id.as_ref().map(|id| id.name.as_str())
        }
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.as_str())
            }
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.as_str())
            }
            _ => None,
        },
        _ => None,
    }
}

/// The variable declaration a top-level statement binds, unwrapping `export`.
fn top_const<'b, 'a>(statement: &'b mut Statement<'a>) -> Option<&'b mut VariableDeclaration<'a>> {
    match statement {
        Statement::VariableDeclaration(variables) => Some(variables),
        Statement::ExportDeclaration(export) => match &mut export.declaration {
            Declaration::VariableDeclaration(variables) => Some(variables),
            _ => None,
        },
        _ => None,
    }
}

fn hot_function<'a>(function: &Function<'a>) -> Option<&'a str> {
    let name = function.id.as_ref()?.name.as_str();
    let body = function.body.as_ref()?;
    (is_component_name(name) && has_jsx(|check| check.visit_function_body(body))).then_some(name)
}

/// Registers a single-declarator `const N = <arrow | function>` component; `false` when the
/// declaration binds anything else.
fn hot_const(variables: &VariableDeclaration<'_>, plan: &mut HotPlan) -> bool {
    let [declarator] = variables.declarations.as_slice() else {
        return false;
    };
    if variables.kind != VariableDeclarationKind::Const {
        return false;
    }
    let BindingPattern::BindingIdentifier(id) = &declarator.id else {
        return false;
    };
    let Some(init) = &declarator.init else {
        return false;
    };
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
        plan.inits.insert(init.span().start, name.to_owned());
    }
    is_component
}
