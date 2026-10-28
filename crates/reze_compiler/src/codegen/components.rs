use oxc_allocator::TakeIn;
use oxc_ast::ast::{Declaration, Function, Program, Statement};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};

use super::EmitContext;
use super::html::HTML_SOURCE;
use super::module_scope::parse;
use crate::ast::Ast;
use crate::frontend::analysis::{ComponentExport, components_of};
use crate::ir::view::{Child, ViewKind};
use crate::ir::view::ViewId;

pub fn register<'a>(ctx: &mut EmitContext<'a, '_>, program: &mut Program<'a>) {
    let ast = Ast::new(ctx.allocator);
    let old = program.body.take_in(&ctx.allocator);
    for statement in old {
        let components: Vec<(String, ComponentExport)> = components_of(&statement)
            .into_iter()
            .map(|(name, export)| (name.to_owned(), export))
            .collect();
        let spans: Vec<(String, Span)> = components
            .iter()
            .filter_map(|(name, _)| component_span(&statement, name).map(|span| (name.clone(), span)))
            .collect();
        let has_async = contains_async(&statement);
        program.body.push(statement);
        for (name, export) in components {
            ctx.changed = true;
            if ctx.options.target == crate::CompileTarget::Html {
                let client_work = ctx.facts.client_work.get(&name).copied().unwrap_or(false);
                let adoptable = spans
                    .iter()
                    .find(|(candidate, _)| candidate == &name)
                    .is_some_and(|(_, span)| !has_async && is_adoptable(ctx, *span));
                let export_name = match export {
                    ComponentExport::Local => format!("__rz${name}"),
                    ComponentExport::Named => name.clone(),
                    ComponentExport::Default => "default".to_owned(),
                };
                let module_id = ctx.options.module_id.as_deref().unwrap_or_default();
                let component = ast.ident(ctx.intern(&name));
                let args = [
                    component,
                    ast.string(module_id),
                    ast.string(&export_name),
                    ast.number(f64::from(u8::from(client_work))),
                    ast.number(f64::from(u8::from(adoptable))),
                ];
                let call = ctx.call(HTML_SOURCE, "hDefineComponent", args);
                program.body.push(ast.stmt(call));
            }
            if let ComponentExport::Local = export {
                let text = format!("export {{ {name} as __rz${name} }};");
                for statement in parse(ctx, &text) {
                    program.body.push(statement);
                }
            }
        }
    }
}

fn component_span(statement: &Statement<'_>, name: &str) -> Option<Span> {
    match statement {
        Statement::FunctionDeclaration(function) if function.id.as_ref().is_some_and(|id| id.name == name) => {
            Some(function.span)
        }
        Statement::VariableDeclaration(declaration) => declaration.declarations.iter().find_map(|declarator| {
            let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &declarator.id else { return None };
            (id.name == name).then(|| declarator.init.as_ref().map_or(declarator.span, |init| init.span()))
        }),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::FunctionDeclaration(function) if function.id.as_ref().is_some_and(|id| id.name == name) => {
                Some(function.span)
            }
            Declaration::VariableDeclaration(declaration) => declaration.declarations.iter().find_map(|declarator| {
                let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &declarator.id else { return None };
                (id.name == name).then(|| declarator.init.as_ref().map_or(declarator.span, |init| init.span()))
            }),
            _ => None,
        },
        _ => None,
    }
}

fn span_within(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

fn plain_root(ctx: &EmitContext<'_, '_>, id: ViewId) -> bool {
    match &ctx.ir.view(id).kind {
        ViewKind::Element(element) => {
            let roots: Vec<_> = element.statics.nodes.iter().filter(|node| node.parent.is_none()).collect();
            let [root] = roots.as_slice() else { return false };
            if root.kind != crate::ir::view::StaticNodeKind::Element {
                return false;
            }
            !element.props.iter().any(rewrites_structure)
                && element
                    .inserts
                    .iter()
                    .all(|insert| matches!(insert.value, Child::Dynamic(_) | Child::StaticText(_)))
        }
        ViewKind::Fragment(children) => children.iter().all(|child| match child {
            Child::StaticText(_) | Child::Dynamic(_) => true,
            Child::View(id) => plain_root(ctx, *id),
            Child::Conditional(_) => false,
        }),
        _ => false,
    }
}

/// A component whose view is a static element tree with value bindings only can reuse the DOM the
/// server rendered instead of creating a second copy and swapping it in.
fn is_adoptable(ctx: &EmitContext<'_, '_>, span: Span) -> bool {
    let inside: Vec<ViewId> = ctx
        .ir
        .views
        .iter()
        .enumerate()
        .filter(|(_, view)| span_within(span, view.origin))
        .map(|(index, _)| ViewId(index as u32))
        .collect();
    let roots: Vec<ViewId> = inside
        .iter()
        .copied()
        .filter(|id| {
            let origin = ctx.ir.view(*id).origin;
            !inside.iter().any(|other| {
                other != id && {
                    let other = ctx.ir.view(*other).origin;
                    other != origin && span_within(other, origin)
                }
            })
        })
        .collect();
    let [root] = roots.as_slice() else { return false };
    plain_root(ctx, *root)
}

struct AsyncFinder {
    found: bool,
}

impl<'a> Visit<'a> for AsyncFinder {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        if function.r#async {
            self.found = true;
            return;
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        if arrow.r#async {
            self.found = true;
            return;
        }
        walk::walk_arrow_function_expression(self, arrow);
    }
}

fn contains_async(statement: &Statement<'_>) -> bool {
    let mut finder = AsyncFinder { found: false };
    finder.visit_statement(statement);
    finder.found
}

fn rewrites_structure(prop: &crate::ir::view::ElementProp) -> bool {
    match prop {
        crate::ir::view::ElementProp::Attr(attr) => match &attr.target {
            crate::ir::view::AttrTarget::Prop(name) => name == "innerHTML" || name == "textContent",
            _ => false,
        },
        _ => false,
    }
}
