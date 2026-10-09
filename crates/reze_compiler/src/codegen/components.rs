use oxc_allocator::TakeIn;
use oxc_ast::ast::Program;

use super::EmitContext;
use super::html::HTML_SOURCE;
use super::module_scope::parse;
use crate::ast::Ast;
use crate::frontend::analysis::{ComponentExport, components_of};

pub fn register<'a>(ctx: &mut EmitContext<'a, '_>, program: &mut Program<'a>) {
    let ast = Ast::new(ctx.allocator);
    let old = program.body.take_in(&ctx.allocator);
    for statement in old {
        let components: Vec<(String, ComponentExport)> = components_of(&statement)
            .into_iter()
            .map(|(name, export)| (name.to_owned(), export))
            .collect();
        program.body.push(statement);
        for (name, export) in components {
            ctx.changed = true;
            if ctx.options.target == crate::CompileTarget::Html {
                let client_work = ctx.facts.client_work.get(&name).copied().unwrap_or(false);
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
