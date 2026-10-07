use oxc_allocator::CloneIn;
use oxc_ast::ast::{Expression, Statement, VariableDeclarationKind};
use oxc_syntax::operator::UnaryOperator;

use super::native::NativeTarget;
use super::{EmitContext, client, native};
use crate::ast::Ast;
use crate::ir::layout::logical_path;
use crate::ir::view::{
    Anchor, Attr, AttrTarget, ElementView, EventHandler, EventKind, InsertOp, LateProp, LinkProp,
    RefTarget, SpreadSegment, StaticNodeKind, View,
};

const HYDRATE: &str = "reze-js/internal/hydrate";

pub fn emit<'a, 'm>(
    ctx: &mut EmitContext<'a, 'm>,
    view: &'m View,
    element: &'m ElementView,
) -> Expression<'a> {
    let _ = ctx.site(view);
    let site = ctx.site_name(view);
    let bindings = native::bindings(ctx, element, false);
    if element.props.is_empty()
        && element.inserts.is_empty()
        && element.links.is_empty()
        && element.late_values.is_empty()
        && element.schedule.effects.is_empty()
        && bindings.names.iter().skip(1).all(|name| name.is_none())
    {
        let ast = Ast::new(ctx.allocator);
        let claim = ctx.call(HYDRATE, "claimRoot", [ast.ident(site)]);
        let template = client::template(ctx, element);
        let test = ctx.call(HYDRATE, "isPreparing", Vec::new());
        return ast.conditional(test, claim, template);
    }
    let preparing = ctx.fresh("preparing");
    let mut target = Target { preparing, site, bindings };
    let ast = Ast::new(ctx.allocator);
    let root = target.bindings.name(0);
    let mut out = Vec::new();
    out.push(ast.declaration(
        VariableDeclarationKind::Const,
        preparing,
        Some(ctx.call(HYDRATE, "isPreparing", Vec::new())),
    ));
    let claim = ctx.call(HYDRATE, "claimRoot", [ast.ident(target.site)]);
    let template = client::template(ctx, element);
    out.push(ast.declaration(
        VariableDeclarationKind::Const,
        root,
        Some(ast.conditional(ast.ident(preparing), claim, template)),
    ));
    let count = target.bindings.names.len();
    for index in 1..count {
        let Some(name) = target.bindings.names[index] else { continue };
        let kind = element.statics.nodes[index].kind;
        let path = logical_path(&element.statics, index as u32);
        let claim = match kind {
            StaticNodeKind::Element => ctx.call(
                HYDRATE,
                "claimElement",
                [ast.ident(root), ast.string(&path), ast.ident(target.site)],
            ),
            StaticNodeKind::Text => ctx.call(
                HYDRATE,
                "claimText",
                [ast.ident(root), ast.string(&path), ast.ident(target.site)],
            ),
            StaticNodeKind::Marker => ctx.call(
                HYDRATE,
                "claimMarker",
                [ast.ident(root), ast.string(&path), ast.ident(target.site)],
            ),
        };
        let access = client::accessor(ctx, element, &target.bindings, index as u32);
        out.push(ast.declaration(
            VariableDeclarationKind::Const,
            name,
            Some(ast.conditional(ast.ident(preparing), claim, access)),
        ));
    }
    native::schedule(ctx, element, &mut target, &mut out);
    out.push(ast.return_stmt(ast.ident(root)));
    let body = ast.block_arrow(Vec::<&'a str>::new(), out);
    ast.call(body, Vec::new())
}

struct Target<'a> {
    preparing: &'a str,
    site: &'a str,
    bindings: native::Bindings<'a>,
}

impl<'a> NativeTarget<'a> for Target<'a> {
    const CLIENT_WORK: bool = true;
    const CAPTURE_INSERT: bool = true;

    fn attr(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        attr: &Attr,
        value: &'a str,
        previous: Option<&'a str>,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = self.bindings.name(attr.node);
        let queued = match &attr.target {
            AttrTarget::Text => ctx.call(
                HYDRATE,
                "queueText",
                [ast.ident(node), ast.ident(self.site), ast.ident(value)],
            ),
            AttrTarget::Attr(key) => ctx.call(
                HYDRATE,
                "queueAttr",
                [ast.ident(node), ast.ident(self.site), ast.string(key), ast.ident(value)],
            ),
            AttrTarget::AttrNs(ns, key) => ctx.call(
                HYDRATE,
                "queueAttrNS",
                [
                    ast.ident(node),
                    ast.ident(self.site),
                    ast.string(ns),
                    ast.string(key),
                    ast.ident(value),
                ],
            ),
            AttrTarget::Bool(key) => ctx.call(
                HYDRATE,
                "queueBool",
                [ast.ident(node), ast.ident(self.site), ast.string(key), ast.ident(value)],
            ),
            AttrTarget::Prop(key) => ctx.call(
                HYDRATE,
                "queueProp",
                [ast.ident(node), ast.ident(self.site), ast.string(key), ast.ident(value)],
            ),
            AttrTarget::Class => ctx.call(
                HYDRATE,
                "queueProp",
                [ast.ident(node), ast.ident(self.site), ast.string("class"), ast.ident(value)],
            ),
            AttrTarget::ClassToggle(token) => {
                let mut args = vec![
                    ast.ident(node),
                    ast.ident(self.site),
                    ast.string(token),
                    ast.ident(value),
                ];
                if let Some(name) = previous {
                    args.push(ast.ident(name));
                }
                ctx.call(HYDRATE, "queueToggle", args)
            }
            AttrTarget::Style => {
                let mut args = vec![ast.ident(node), ast.ident(self.site), ast.ident(value)];
                if let Some(name) = previous {
                    args.push(ast.ident(name));
                }
                ctx.call(HYDRATE, "queueStyle", args)
            }
        };
        let direct = client::write_attr(
            ctx,
            attr,
            ast.ident(node),
            ast.ident(value),
            previous.map(|name| ast.ident(name)),
        );
        ast.conditional(ast.ident(self.preparing), queued, direct)
    }

    fn event(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        event: &EventHandler,
        handler: &'a str,
        data: Option<&'a str>,
    ) -> Vec<Statement<'a>> {
        let ast = Ast::new(ctx.allocator);
        let delegated = !matches!(event.kind, EventKind::Direct);
        let node = self.bindings.name(event.node);
        let mut args = Vec::with_capacity(6);
        args.push(ast.ident(node));
        args.push(ast.ident(self.site));
        args.push(ast.string(event.name.as_str()));
        args.push(ast.ident(handler));
        args.push(ast.boolean(delegated));
        if let Some(name) = data {
            args.push(ast.ident(name));
        }
        let stage = ctx.call(HYDRATE, "stageListener", args);
        let direct = client::write_event(ctx, event, node, handler, data);
        vec![
            ast.if_stmt(ast.ident(self.preparing), ast.block([ast.stmt(stage)])),
            ast.if_stmt(
                ast.unary(UnaryOperator::LogicalNot, ast.ident(self.preparing)),
                ast.block(direct),
            ),
        ]
    }

    fn reference(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        target: &RefTarget,
    ) -> Vec<Statement<'a>> {
        let ast = Ast::new(ctx.allocator);
        let node = self.bindings.name(target.node);
        let real = ctx.fresh("node");
        let mut body = Vec::new();
        body.push(ast.declaration(
            VariableDeclarationKind::Const,
            real,
            Some(ast.conditional(
                ast.ident(self.preparing),
                ctx.call(HYDRATE, "deref", [ast.ident(node)]),
                ast.ident(node),
            )),
        ));
        body.extend(client::write_ref(ctx, target, real));
        let callback = ast.block_arrow(Vec::<&'a str>::new(), body);
        vec![ast.stmt(ctx.call(HYDRATE, "stageRef", [ast.ident(self.site), callback]))]
    }

    fn spread(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        spread: &SpreadSegment,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = self.bindings.name(spread.node);
        let queued = ctx.call(
            HYDRATE,
            "queueSpread",
            [
                ast.ident(node),
                ast.ident(self.site),
                ast.ident(value),
                ast.boolean(spread.is_svg),
                ast.boolean(spread.has_children),
            ],
        );
        let direct = client::write_spread(ctx, spread, ast.ident(node), ast.ident(value));
        ast.conditional(ast.ident(self.preparing), queued, direct)
    }

    fn insert(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        insert: &InsertOp,
        value: Expression<'a>,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let parent = self.bindings.name(insert.parent);
        let slot = ast.number(insert.slot as f64);
        let prepared = match insert.anchor {
            Anchor::Only => ctx.call(
                HYDRATE,
                "prepareInsert",
                [ast.ident(parent), ast.ident(self.site), slot, value.clone_in(ctx.allocator)],
            ),
            Anchor::Before(anchor) => {
                let target = self.bindings.name(anchor);
                ctx.call(
                    HYDRATE,
                    "prepareInsert",
                    [
                        ast.ident(parent),
                        ast.ident(self.site),
                        slot,
                        value.clone_in(ctx.allocator),
                        ast.ident(target),
                    ],
                )
            }
            Anchor::End => ctx.call(
                HYDRATE,
                "prepareAppend",
                [ast.ident(parent), ast.ident(self.site), slot, value.clone_in(ctx.allocator)],
            ),
        };
        let direct = client::write_insert(ctx, insert, &self.bindings, value);
        ast.conditional(ast.ident(self.preparing), prepared, direct)
    }

    fn link(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        link: &LinkProp,
        href: Option<Expression<'a>>,
    ) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = self.bindings.name(link.node);
        client::write_link(ctx, ast.ident(node), href)
    }

    fn effect(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        callback: Expression<'a>,
        _fixed: bool,
    ) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        ast.stmt(ctx.call(HYDRATE, "prepareEffect", [callback, ast.ident(self.site)]))
    }

    fn late_value(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        late: &LateProp,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = self.bindings.name(late.node);
        let queued = ctx.call(
            HYDRATE,
            "queueProp",
            [ast.ident(node), ast.ident(self.site), ast.string("value"), ast.ident(value)],
        );
        let direct = ast.assign(ast.member(ast.ident(node), ctx.intern("value")), ast.ident(value));
        ast.conditional(ast.ident(self.preparing), queued, direct)
    }
}
