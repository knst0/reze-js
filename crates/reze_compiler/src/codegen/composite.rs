use oxc_ast::ast::{
    AwaitExpression, Expression, IdentifierReference, NewTarget, ObjectPropertyKind,
    PrivateIdentifier, PropertyKind, Statement, Super, ThisExpression, VariableDeclarationKind,
    YieldExpression,
};
use oxc_ast_visit::Visit;
use oxc_span::SPAN;
use oxc_str::Ident;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};

use super::EmitContext;
use crate::CompileTarget;
use crate::RUNTIME_MODULE;
use crate::ast::Ast;
use crate::html::is_identifier_name;
use crate::ir::view::{
    AssignTarget, Child, ComponentProp, ComponentSegment, ComponentValue, ComponentView, ExprRef,
    FlowKeyValue, FlowRender, FlowView, IslandLoader, IslandView, MemberKey, View, ViewKind,
};

const HTML: &str = "reze-js/internal/html";
const HYDRATE: &str = "reze-js/internal/hydrate";

pub fn emit<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, view: &'m View) -> Option<Expression<'a>> {
    match &view.kind {
        ViewKind::Element(_) => None,
        ViewKind::Component(value) => Some(component(ctx, view, value)),
        ViewKind::Fragment(children) => Some(fragment(ctx, view, children)),
        ViewKind::Flow(value) => Some(flow(ctx, view, value)),
    }
}

fn component<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    view: &'m View,
    component: &'m ComponentView,
) -> Expression<'a> {
    let target = ctx.options.target;
    let props = component_props(ctx, &component.props);
    match &component.island {
        Some(island) => island_call(ctx, view, component, island, props, target),
        None => match target {
            CompileTarget::Client => {
                let mut args = vec![ctx.expr(component.callee), props];
                if ctx.options.debug_names {
                    let span = component.callee.span;
                    let tag = &ctx.source[span.start as usize..span.end as usize];
                    let ast = Ast::new(ctx.allocator);
                    args.push(ast.string(&format!("{}#{tag}", ctx.filename)));
                }
                ctx.call(RUNTIME_MODULE, "createComponent", args)
            }
            CompileTarget::Html => {
                let site = ctx.site(view);
                ctx.call(HTML, "hComponent", vec![ctx.expr(component.callee), props, site])
            }
            CompileTarget::Hydrate => {
                let site = ctx.site(view);
                ctx.call(
                    HYDRATE,
                    "prepareComponent",
                    vec![site, ctx.expr(component.callee), props],
                )
            }
        },
    }
}

fn component_props<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    segments: &'m [ComponentSegment],
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    match segments {
        [] => ast.object(Vec::new()),
        [ComponentSegment::Object(entries)] => object_props(ctx, entries),
        [ComponentSegment::Spread { expr, is_dynamic: false }] => ctx.expr(*expr),
        _ => {
            let mut args = Vec::with_capacity(segments.len());
            for segment in segments {
                match segment {
                    ComponentSegment::Object(entries) => args.push(object_props(ctx, entries)),
                    ComponentSegment::Spread { expr, is_dynamic: false } => {
                        args.push(ctx.expr(*expr));
                    }
                    ComponentSegment::Spread { expr, .. } => {
                        let ast = Ast::new(ctx.allocator);
                        args.push(ast.arrow(Vec::new(), ctx.expr(*expr)));
                    }
                }
            }
            ctx.call(RUNTIME_MODULE, "mergeProps", args)
        }
    }
}

fn object_props<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    entries: &'m [ComponentProp],
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let mut props = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            ComponentProp::Value { key, value } => {
                props.push(ast.prop(key, prop_value(ctx, value)));
            }
            ComponentProp::Getter { key, value } => {
                props.push(ast.getter(key, prop_value(ctx, value)));
            }
            ComponentProp::ForwardRef(target) => {
                props.push(ast.prop("ref", ref_arrow(ctx, target)));
            }
        }
    }
    object(ctx, props)
}

fn prop_value<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, value: &'m ComponentValue) -> Expression<'a> {
    match value {
        ComponentValue::True => Ast::new(ctx.allocator).boolean(true),
        ComponentValue::Str(text) => Ast::new(ctx.allocator).string(text),
        ComponentValue::Dynamic(dynamic) => ctx.expr(dynamic.expr),
        ComponentValue::Nested(id) => ctx.view(*id),
        ComponentValue::Children(children) => {
            let ast = Ast::new(ctx.allocator);
            ast.array(children.iter().map(|child| ctx.child(child)))
        }
    }
}

fn ref_arrow<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, target: &'m AssignTarget) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let element = ctx.fresh("r$");
    let mut body = Vec::new();
    match target {
        AssignTarget::Identifier(name) => {
            let slot = ctx.intern(name);
            let check = is_function(&ast, ast.ident(slot));
            let then = ast.call(ast.ident(slot), [ast.ident(element)]);
            let write =
                ctx.assign_ref(target, ast.ident(element));
            body.push(ast.stmt(ast.conditional(check, then, write)));
        }
        AssignTarget::Member { object, key } => {
            let object_value = ctx.expr(*object);
            let holder = if matches!(object_value, Expression::Super(_)) {
                None
            } else {
                let holder = ctx.fresh("_o$");
                body.push(ast.declaration(VariableDeclarationKind::Const, holder, Some(object_value)));
                Some(holder)
            };
            let receiver = || match holder {
                Some(holder) => ast.ident(holder),
                None => Expression::new_super(SPAN, &ast.builder),
            };
            let (read, write) = match key {
                MemberKey::Static(name) => match name.strip_prefix('#') {
                    Some(field) => {
                        let field = ctx.intern(field);
                        let read = Expression::new_private_field_expression(
                            SPAN,
                            receiver(),
                            PrivateIdentifier::new(SPAN, Ident::from(field), &ast.builder),
                            false,
                            &ast.builder,
                        );
                        let write = Expression::new_private_field_expression(
                            SPAN,
                            receiver(),
                            PrivateIdentifier::new(SPAN, Ident::from(field), &ast.builder),
                            false,
                            &ast.builder,
                        );
                        (read, ast.assign(write, ast.ident(element)))
                    }
                    None => {
                        let field = ctx.intern(name);
                        (
                            ast.member(receiver(), field),
                            ast.assign(ast.member(receiver(), field), ast.ident(element)),
                        )
                    }
                },
                MemberKey::Computed(key) => {
                    let index = ctx.fresh("_k$");
                    body.push(ast.declaration(
                        VariableDeclarationKind::Const,
                        index,
                        Some(ctx.expr(*key)),
                    ));
                    (
                        ast.index(receiver(), ast.ident(index)),
                        ast.assign(
                            ast.index(receiver(), ast.ident(index)),
                            ast.ident(element),
                        ),
                    )
                }
            };
            let slot = ctx.fresh("_r$");
            body.push(ast.declaration(VariableDeclarationKind::Const, slot, Some(read)));
            let check = is_function(&ast, ast.ident(slot));
            let then = ast.call(ast.ident(slot), [ast.ident(element)]);
            body.push(ast.stmt(ast.conditional(check, then, write)));
        }
    }
    ast.block_arrow([element], body)
}

fn is_function<'a>(ast: &Ast<'a>, value: Expression<'a>) -> Expression<'a> {
    ast.binary(
        ast.unary(UnaryOperator::Typeof, value),
        BinaryOperator::StrictEquality,
        ast.string("function"),
    )
}

pub fn object<'a>(
    ctx: &mut EmitContext<'a, '_>,
    properties: impl IntoIterator<Item = ObjectPropertyKind<'a>>,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let mut props: Vec<ObjectPropertyKind<'a>> = properties.into_iter().collect();
    let mut prelude: Vec<Statement<'a>> = Vec::new();
    for prop in &mut props {
        let ObjectPropertyKind::ObjectProperty(property) = prop else {
            continue;
        };
        if property.kind != PropertyKind::Get {
            continue;
        }
        if !getter_return(&property.value).is_some_and(lexical) {
            continue;
        }
        let Some(taken) = take_getter_return(&mut property.value) else {
            continue;
        };
        let read = ctx.fresh("_r$");
        prelude.push(ast.declaration(
            VariableDeclarationKind::Const,
            read,
            Some(ast.arrow(Vec::new(), taken)),
        ));
        if let Expression::FunctionExpression(function) = &mut property.value
            && let Some(body) = function.body.as_mut()
            && body.statements.len() == 1
            && let Statement::ReturnStatement(statement) = &mut body.statements[0]
        {
            statement.argument = Some(ast.call(ast.ident(read), Vec::new()));
        }
    }
    if prelude.is_empty() {
        ast.object(props)
    } else {
        prelude.push(ast.return_stmt(ast.object(props)));
        ast.call(ast.block_arrow(Vec::<&'a str>::new(), prelude), Vec::new())
    }
}

fn getter_return<'e, 'a>(value: &'e Expression<'a>) -> Option<&'e Expression<'a>> {
    let Expression::FunctionExpression(function) = value else {
        return None;
    };
    let body = function.body.as_ref()?;
    if body.statements.len() != 1 {
        return None;
    }
    let Statement::ReturnStatement(statement) = &body.statements[0] else {
        return None;
    };
    statement.argument.as_ref()
}

fn take_getter_return<'a>(value: &mut Expression<'a>) -> Option<Expression<'a>> {
    let Expression::FunctionExpression(function) = value else {
        return None;
    };
    let body = function.body.as_mut()?;
    if body.statements.len() != 1 {
        return None;
    }
    let Statement::ReturnStatement(statement) = &mut body.statements[0] else {
        return None;
    };
    statement.argument.take()
}

fn lexical(value: &Expression<'_>) -> bool {
    let mut check = LexCheck { lexical: false, veto: false };
    check.visit_expression(value);
    check.lexical && !check.veto
}

struct LexCheck {
    lexical: bool,
    veto: bool,
}

impl<'a> Visit<'a> for LexCheck {
    fn visit_this_expression(&mut self, _: &ThisExpression) {
        self.lexical = true;
    }

    fn visit_super(&mut self, _: &Super) {
        self.lexical = true;
    }

    fn visit_new_target(&mut self, _: &NewTarget) {
        self.lexical = true;
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name.as_str() == "arguments" {
            self.lexical = true;
        }
    }

    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.veto = true;
    }

    fn visit_yield_expression(&mut self, _: &YieldExpression<'a>) {
        self.veto = true;
    }
}

fn fragment<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    view: &'m View,
    children: &'m [Child],
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let value = match children {
        [] => ast.array(Vec::new()),
        [single] => ctx.child(single),
        _ => ast.array(children.iter().map(|child| ctx.child(child))),
    };
    match ctx.options.target {
        CompileTarget::Client => value,
        CompileTarget::Html => {
            let site = ctx.site(view);
            ctx.call(HTML, "hFragment", [site, ast.arrow([], value)])
        }
        CompileTarget::Hydrate => {
            let site = ctx.site(view);
            ctx.call(HYDRATE, "prepareFragment", [site, ast.arrow([], value)])
        }
    }
}

fn flow<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, view: &'m View, flow: &'m FlowView) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    match flow {
        FlowView::Show(branch) => {
            let when = ctx.flow_getter(&branch.when);
            let child = render_flow(ctx, &branch.child);
            let fallback = branch.fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![when, child];
                    args.extend(fallback);
                    ctx.call(RUNTIME_MODULE, "branch", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, when, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HTML, "hShow", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, when, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HYDRATE, "prepareShow", args)
                }
            }
        }
        FlowView::Switch { whens, children, fallback } => {
            let whens: Vec<Expression<'a>> =
                whens.iter().map(|when| ctx.flow_getter(when)).collect();
            let children: Vec<Expression<'a>> =
                children.iter().map(|child| render_flow(ctx, child)).collect();
            let fallback = fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![ast.array(whens), ast.array(children)];
                    args.extend(fallback);
                    ctx.call(RUNTIME_MODULE, "choose", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, ast.array(whens), ast.array(children)];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HTML, "hChoose", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, ast.array(whens), ast.array(children)];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HYDRATE, "prepareChoose", args)
                }
            }
        }
        FlowView::For { each, map, fallback, key } => {
            let each = ctx.flow_getter(each);
            let map = ctx.expr(*map);
            let fallback = fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            let key = key.as_ref().map(|key| flow_key(ctx, key));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![each, map];
                    match fallback {
                        Some(fallback) => args.push(fallback),
                        None if key.is_some() => args.push(ast.undefined()),
                        None => {}
                    }
                    args.extend(key);
                    ctx.call(RUNTIME_MODULE, "list", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, each, map];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    args.extend(key);
                    ctx.call(HTML, "hList", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, each, map];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    args.extend(key);
                    ctx.call(HYDRATE, "prepareList", args)
                }
            }
        }
        FlowView::Repeat { count, map, fallback } => {
            let count = ctx.flow_getter(count);
            let map = ctx.expr(*map);
            let fallback = fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![count, map];
                    args.extend(fallback);
                    ctx.call(RUNTIME_MODULE, "repeat", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, count, map];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HTML, "hRepeat", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, count, map];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HYDRATE, "prepareRepeat", args)
                }
            }
        }
        FlowView::Rows { times, map } => {
            match ctx.options.target {
                CompileTarget::Client => rows(ctx, *times, *map),
                CompileTarget::Html | CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let map = ctx.expr(*map);
                    let (source, helper) = if ctx.options.target == CompileTarget::Html {
                        (HTML, "hRows")
                    } else {
                        (HYDRATE, "prepareRows")
                    };
                    ctx.call(source, helper, [site, ast.number(f64::from(*times)), map])
                }
            }
        }
        FlowView::Loading { child, fallback } => {
            let child = render_flow(ctx, child);
            let fallback = fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![child];
                    args.extend(fallback);
                    ctx.call(RUNTIME_MODULE, "loading", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HTML, "hLoading", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HYDRATE, "prepareLoading", args)
                }
            }
        }
        FlowView::Errored { child, fallback } => {
            let child = render_flow(ctx, child);
            let fallback = fallback.as_ref().map(|fallback| render_flow(ctx, fallback));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![child];
                    args.extend(fallback);
                    ctx.call(RUNTIME_MODULE, "errored", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HTML, "hErrored", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.push(fallback.unwrap_or_else(|| ast.undefined()));
                    ctx.call(HYDRATE, "prepareErrored", args)
                }
            }
        }
        FlowView::Portal { child, mount } => {
            let child = render_flow(ctx, child);
            let mount = mount.as_ref().map(|mount| ctx.flow_getter(mount));
            match ctx.options.target {
                CompileTarget::Client => {
                    let mut args = vec![child];
                    args.extend(mount);
                    ctx.call(RUNTIME_MODULE, "portal", args)
                }
                CompileTarget::Html => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.extend(mount);
                    ctx.call(HTML, "hPortal", args)
                }
                CompileTarget::Hydrate => {
                    let site = ctx.site(view);
                    let mut args = vec![site, child];
                    args.extend(mount);
                    ctx.call(HYDRATE, "claimPortal", args)
                }
            }
        }
    }
}

fn render_flow<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, render: &'m FlowRender) -> Expression<'a> {
    match render {
        FlowRender::Function(expression) => ctx.expr(*expression),
        FlowRender::Child(child) => {
            let ast = Ast::new(ctx.allocator);
            ast.arrow(Vec::new(), ctx.child(child))
        }
    }
}

fn flow_key<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, key: &'m FlowKeyValue) -> Expression<'a> {
    match key {
        FlowKeyValue::Expr(expression) => ctx.expr(*expression),
        FlowKeyValue::Str(text) => Ast::new(ctx.allocator).string(text),
        FlowKeyValue::View(id) => ctx.view(*id),
    }
}

fn rows<'a, 'm>(ctx: &mut EmitContext<'a, 'm>, times: u32, map: ExprRef) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let array = ctx.intern("Array");
    let count = ast.call(ast.ident(array), [ast.number(f64::from(times))]);
    let keys = ast.call(ast.member(count, ctx.intern("keys")), Vec::new());
    ast.call(ast.member(ast.ident(array), ctx.intern("from")), [keys, ctx.expr(map)])
}

fn island_call<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    view: &'m View,
    component: &'m ComponentView,
    island: &'m IslandView,
    props: Expression<'a>,
    target: CompileTarget,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let load = match &island.loader {
        IslandLoader::Direct => ast.arrow(Vec::new(), ctx.expr(component.callee)),
        IslandLoader::Split { specifier, path } => {
            let module = ctx.fresh("_m$");
            let mut body = ast.ident(module);
            for segment in path {
                body = if is_identifier_name(segment) {
                    ast.member(body, ctx.intern(segment))
                } else {
                    ast.index(body, ast.string(segment))
                };
            }
            let source = Expression::new_import_expression(
                SPAN,
                ast.string(specifier),
                None,
                None,
                &ast.builder,
            );
            let then = ast.call(ast.member(source, ctx.intern("then")), [ast.arrow([module], body)]);
            ast.arrow(Vec::new(), then)
        }
    };
    let fallback = match &island.fallback {
        Some(fallback) => render_flow(ctx, fallback),
        None => Ast::new(ctx.allocator).undefined(),
    };
    let mut args = vec![ast.string(island.trigger.name()), load, props, fallback];
    if island.media.is_some() || island.root_margin.is_some() {
        let ast = Ast::new(ctx.allocator);
        let mut options = Vec::new();
        if let Some(media) = &island.media {
            options.push(ast.prop("media", ast.string(media)));
        }
        if let Some(root_margin) = &island.root_margin {
            options.push(ast.prop("rootMargin", ast.string(root_margin)));
        }
        args.push(ast.object(options));
    }
    match target {
        CompileTarget::Client => ctx.call(RUNTIME_MODULE, "island", args),
        CompileTarget::Html => {
            let site = ctx.site(view);
            let mut full = vec![site];
            full.extend(args);
            ctx.call(HTML, "hIsland", full)
        }
        CompileTarget::Hydrate => {
            let site = ctx.site(view);
            let mut full = vec![site];
            full.extend(args);
            ctx.call(HYDRATE, "prepareIsland", full)
        }
    }
}
