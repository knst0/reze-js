use oxc_ast::ast::{Expression, Statement, VariableDeclarationKind};

use crate::ast::Ast;
use crate::ir::view::{
    Anchor, Attr, AttrTarget, ElementView, EventHandler, InsertOp, LateProp, LinkProp, Namespace,
    RefTarget, SpreadSegment, StaticNodeKind, View,
};

use super::native::{Bindings, NativeTarget, bindings, schedule};
use super::{EmitContext, client};

pub const HTML_SOURCE: &str = "reze-js/internal/html";

struct Target<'a> {
    bindings: Bindings<'a>,
    site: &'a str,
}

fn namespace_name(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Html => "",
        Namespace::Svg => "svg",
        Namespace::MathMl => "math",
    }
}

impl<'a> NativeTarget<'a> for Target<'a> {
    const CLIENT_WORK: bool = false;

    fn attr(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        attr: &Attr,
        value: &'a str,
        previous: Option<&'a str>,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = ast.ident(self.bindings.name(attr.node));
        let site = ast.ident(self.site);
        let data = ast.ident(value);
        match &attr.target {
            AttrTarget::Attr(name) => {
                ctx.call(HTML_SOURCE, "hSetAttr", [node, site, ast.string(name), data])
            }
            AttrTarget::AttrNs(namespace, name) => ctx.call(
                HTML_SOURCE,
                "hSetAttrNS",
                [node, site, ast.string(namespace), ast.string(name), data],
            ),
            AttrTarget::Bool(name) => {
                ctx.call(HTML_SOURCE, "hSetBool", [node, site, ast.string(name), data])
            }
            AttrTarget::Prop(name) if name == "innerHTML" => {
                ctx.call(HTML_SOURCE, "hSetInnerHTML", [node, site, data])
            }
            AttrTarget::Prop(name) => {
                ctx.call(HTML_SOURCE, "hSetProp", [node, site, ast.string(name), data])
            }
            AttrTarget::Class => ctx.call(HTML_SOURCE, "hSetClass", [node, site, data]),
            AttrTarget::ClassToggle(token) => {
                let mut args = vec![node, site, ast.string(token), data];
                args.extend(previous.map(|name| ast.ident(name)));
                ctx.call(HTML_SOURCE, "hSetToggle", args)
            }
            AttrTarget::Text => ctx.call(HTML_SOURCE, "hSetText", [node, site, data]),
            AttrTarget::Style => {
                let mut args = vec![node, site, data];
                args.extend(previous.map(|name| ast.ident(name)));
                ctx.call(HTML_SOURCE, "hSetStyle", args)
            }
        }
    }

    fn event(
        &mut self,
        _ctx: &mut EmitContext<'a, '_>,
        _event: &EventHandler,
        _handler: &'a str,
        _data: Option<&'a str>,
    ) -> Vec<Statement<'a>> {
        unreachable!("HTML schedules omit native events")
    }

    fn reference(
        &mut self,
        _ctx: &mut EmitContext<'a, '_>,
        _target: &RefTarget,
    ) -> Vec<Statement<'a>> {
        unreachable!("HTML schedules omit native references")
    }

    fn spread(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        spread: &SpreadSegment,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        ctx.call(
            HTML_SOURCE,
            "hSpread",
            [
                ast.ident(self.bindings.name(spread.node)),
                ast.ident(self.site),
                ast.ident(value),
                ast.boolean(spread.is_svg),
                ast.boolean(spread.has_children),
            ],
        )
    }

    fn insert(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        insert: &InsertOp,
        value: Expression<'a>,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let parent = ast.ident(self.bindings.name(insert.parent));
        let child = value;
        let site = ast.ident(self.site);
        let slot = ast.number(insert.slot as f64);
        match insert.anchor {
            Anchor::Only => ctx.call(HTML_SOURCE, "hInsert", [parent, child, site, slot]),
            Anchor::End => ctx.call(HTML_SOURCE, "hAppend", [parent, child, site, slot]),
            Anchor::Before(node) => ctx.call(
                HTML_SOURCE,
                "hInsert",
                [parent, child, site, slot, ast.ident(self.bindings.name(node))],
            ),
        }
    }

    fn late_value(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        late: &LateProp,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        ctx.call(
            HTML_SOURCE,
            "hSetProp",
            [
                ast.ident(self.bindings.name(late.node)),
                ast.ident(self.site),
                ast.string("value"),
                ast.ident(value),
            ],
        )
    }

    fn link(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        link: &LinkProp,
        href: Option<Expression<'a>>,
    ) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        client::write_link(ctx, ast.ident(self.bindings.name(link.node)), href)
    }

    fn effect(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        callback: Expression<'a>,
        _fixed: bool,
    ) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        ast.stmt(ctx.call(HTML_SOURCE, "hRenderEffect", [callback]))
    }
}

pub fn emit<'a>(
    ctx: &mut EmitContext<'a, '_>,
    view: &View,
    element: &ElementView,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let site = ctx.site_name(view);
    let mut target = Target { bindings: bindings(ctx, element, true), site };
    let root = target.bindings.name(0);
    let mut statements = Vec::new();
    for (index, node) in element.statics.nodes.iter().enumerate() {
        let index = index as u32;
        let name = target.bindings.name(index);
        let record = match node.kind {
            StaticNodeKind::Element if index == 0 => ctx.call(
                HTML_SOURCE,
                "hRoot",
                [ast.string(&node.tag), ast.string(namespace_name(node.ns)), ast.ident(site)],
            ),
            StaticNodeKind::Element => ctx.call(
                HTML_SOURCE,
                "hElement",
                [
                    ast.ident(root),
                    ast.number(index as f64),
                    ast.string(&node.tag),
                    ast.string(namespace_name(node.ns)),
                ],
            ),
            StaticNodeKind::Text => ctx.call(
                HTML_SOURCE,
                "hText",
                [ast.ident(root), ast.number(index as f64), ast.string(&node.text)],
            ),
            StaticNodeKind::Marker => {
                ctx.call(HTML_SOURCE, "hMarker", [ast.ident(root), ast.number(index as f64)])
            }
        };
        statements.push(ast.declaration(VariableDeclarationKind::Const, name, Some(record)));
        for attr in &node.attrs {
            let value =
                attr.value.as_deref().map_or_else(|| ast.string(""), |value| ast.string(value));
            let write = ctx.call(
                HTML_SOURCE,
                "hSetAttr",
                [ast.ident(name), ast.ident(site), ast.string(&attr.name), value],
            );
            statements.push(ast.stmt(write));
        }
    }
    for (parent, node) in element.statics.nodes.iter().enumerate() {
        for child in &node.children {
            let attach = ctx.call(
                HTML_SOURCE,
                "hAttach",
                [
                    ast.ident(target.bindings.name(parent as u32)),
                    ast.ident(target.bindings.name(*child)),
                ],
            );
            statements.push(ast.stmt(attach));
        }
    }
    schedule(ctx, element, &mut target, &mut statements);
    statements.push(ast.return_stmt(ast.ident(root)));
    ast.call(ast.block_arrow([], statements), [])
}
