use oxc_allocator::{ArenaVec, TakeIn};
use oxc_ast::ast::{Argument, CallExpression, Expression};

use super::{EmitContext, continuation};
use crate::ast::Ast;
use crate::frontend::analysis::RuntimeCallKind;
use crate::frontend::dynamic::DynamicTag;
use crate::ir::view::Namespace;
use crate::RUNTIME_MODULE;

pub fn rewrite<'a>(
    ctx: &mut EmitContext<'a, '_>,
    call: &mut CallExpression<'a>,
    kind: RuntimeCallKind,
) {
    if kind == RuntimeCallKind::Resource {
        rewrite_resource(ctx, call);
        return;
    }
    if kind == RuntimeCallKind::AsyncComponent {
        continuation::prepare(ctx, call);
    }
    if ctx.options.target.runs_in_browser() {
        return;
    }
    let ast = Ast::new(ctx.allocator);
    let site = ctx.origin_site(call.span);
    let arguments = call.arguments.take_in(&ctx.allocator);
    let mut next = ArenaVec::with_capacity_in(arguments.len() + 3, &ast.builder);
    next.push(Argument::from(site));
    let callee = match kind {
        RuntimeCallKind::Resource => unreachable!("resource calls return before the managed path"),
        RuntimeCallKind::UniqueId => {
            let receiver = match call.callee.without_parentheses() {
                Expression::StaticMemberExpression(member) => match &member.object {
                    Expression::Identifier(namespace) => ast.ident(namespace.name.as_str()),
                    _ => unreachable!(
                        "statically resolved factory receiver is an imported namespace"
                    ),
                },
                _ => ast.undefined(),
            };
            next.push(Argument::from(call.callee.take_in(&ctx.allocator)));
            next.push(Argument::from(receiver));
            ctx.helper("reze-js/internal/reactivity", "withResourceSite")
        }
        RuntimeCallKind::AsyncComponent
        | RuntimeCallKind::AsyncViews
        | RuntimeCallKind::Dynamic
        | RuntimeCallKind::DynamicElement
        | RuntimeCallKind::Island => {
            let export = match kind {
                RuntimeCallKind::AsyncComponent => "hAsyncComponent",
                RuntimeCallKind::AsyncViews => "hAsyncViews",
                RuntimeCallKind::Dynamic => "hDynamic",
                RuntimeCallKind::DynamicElement => "hDynamicElement",
                RuntimeCallKind::Island => "hIsland",
                RuntimeCallKind::Resource | RuntimeCallKind::UniqueId => {
                    unreachable!("handled before the runtime helper lookup")
                }
            };
            ctx.helper("reze-js/internal/html", export)
        }
    };
    next.extend(arguments);
    call.callee = callee;
    call.arguments = next;
    ctx.changed = true;
}

fn rewrite_resource<'a>(ctx: &mut EmitContext<'a, '_>, call: &mut CallExpression<'a>) {
    let resource = ctx.helper("reze-js/internal/reactivity", "resource");
    if ctx.options.target.runs_in_browser() {
        call.callee = resource;
    } else {
        let ast = Ast::new(ctx.allocator);
        let site = ctx.origin_site(call.span);
        let arguments = call.arguments.take_in(&ctx.allocator);
        let mut next = ArenaVec::with_capacity_in(arguments.len() + 3, &ast.builder);
        next.push(Argument::from(site));
        next.push(Argument::from(resource));
        next.push(Argument::from(ast.undefined()));
        next.extend(arguments);
        call.callee = ctx.helper("reze-js/internal/reactivity", "withResourceSite");
        call.arguments = next;
    }
    ctx.changed = true;
}

pub fn native_type<'a>(
    ctx: &mut EmitContext<'a, '_>,
    tag: DynamicTag,
    value: Expression<'a>,
) -> Expression<'a> {
    let (client, namespace) = match tag.namespace {
        Namespace::Html => ("element", ""),
        Namespace::Svg => ("elementSVG", "svg"),
        Namespace::MathMl => ("elementMathML", "math"),
    };
    if ctx.options.target.runs_in_browser() {
        return ctx.call(RUNTIME_MODULE, client, [value]);
    }
    let ast = Ast::new(ctx.allocator);
    let site = ctx.origin_site(tag.source);
    ctx.call("reze-js/internal/html", "hElementType", [site, value, ast.string(namespace)])
}
