use oxc_ast::ast::{Expression, PrivateIdentifier, Statement, VariableDeclarationKind};
use oxc_span::SPAN;
use oxc_str::Ident;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

use super::EmitContext;
use super::native::{Bindings, NativeTarget, bindings, schedule};
use crate::RUNTIME_MODULE;
use crate::ast::Ast;
use crate::html::is_identifier_name;
use crate::ir::layout::{PathStep, needs_text_nodes, path_steps, serialize_client_static};
use crate::ir::view::{
    Anchor, AssignTarget, Attr, AttrTarget, ElementView, EventHandler, EventKind, ExprRef,
    InsertOp, LateProp, LinkProp, MemberKey, Namespace, RefOp, RefTarget, SpreadSegment,
    StaticNodeKind,
};

pub struct ClientTarget<'a> {
    bindings: Bindings<'a>,
}

impl<'a> NativeTarget<'a> for ClientTarget<'a> {
    const CLIENT_WORK: bool = true;

    fn attr(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        attr: &Attr,
        value: &'a str,
        previous: Option<&'a str>,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = ast.ident(self.bindings.name(attr.node));
        let value = ast.ident(value);
        let previous = previous.map(|name| ast.ident(name));
        write_attr(ctx, attr, node, value, previous)
    }

    fn event(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        event: &EventHandler,
        handler: &'a str,
        data: Option<&'a str>,
    ) -> Vec<Statement<'a>> {
        write_event(ctx, event, self.bindings.name(event.node), handler, data)
    }

    fn reference(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        target: &RefTarget,
    ) -> Vec<Statement<'a>> {
        write_ref(ctx, target, self.bindings.name(target.node))
    }

    fn spread(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        spread: &SpreadSegment,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = ast.ident(self.bindings.name(spread.node));
        write_spread(ctx, spread, node, ast.ident(value))
    }

    fn insert(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        insert: &InsertOp,
        value: Expression<'a>,
    ) -> Expression<'a> {
        write_insert(ctx, insert, &self.bindings, value)
    }

    fn link(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        link: &LinkProp,
        href: Option<Expression<'a>>,
    ) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = ast.ident(self.bindings.name(link.node));
        write_link(ctx, node, href)
    }
    fn late_value(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        late: &LateProp,
        value: &'a str,
    ) -> Expression<'a> {
        let ast = Ast::new(ctx.allocator);
        let node = ast.ident(self.bindings.name(late.node));
        ast.assign(ast.member(node, "value"), ast.ident(value))
    }

    fn effect(&mut self, ctx: &mut EmitContext<'a, '_>, callback: Expression<'a>) -> Statement<'a> {
        let ast = Ast::new(ctx.allocator);
        ast.stmt(ctx.call(RUNTIME_MODULE, "renderEffect", [callback]))
    }
}

pub fn emit<'a>(ctx: &mut EmitContext<'a, '_>, element: &ElementView) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let root = template(ctx, element);
    let mut out: Vec<Statement<'a>> = Vec::new();
    let mut target = ClientTarget { bindings: bindings(ctx, element, false) };
    schedule(ctx, element, &mut target, &mut out);
    if out.is_empty() {
        return root;
    }
    let root_name = target.bindings.name(0);
    let mut body = Vec::with_capacity(out.len() + 2);
    body.push(ast.declaration(VariableDeclarationKind::Const, root_name, Some(root)));
    for (index, slot) in target.bindings.names.iter().enumerate().skip(1) {
        let Some(name) = slot else { continue };
        body.push(ast.declaration(
            VariableDeclarationKind::Const,
            name,
            Some(accessor(ctx, element, &target.bindings, index as u32)),
        ));
    }
    body.extend(out);
    body.push(ast.return_stmt(ast.ident(root_name)));
    ast.call(ast.block_arrow([], body), [])
}

pub fn template<'a>(ctx: &mut EmitContext<'a, '_>, element: &ElementView) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let patches = element
        .statics
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| needs_text_nodes(&element.statics, node))
        .map(|(index, node)| {
            let path = ast.array(path_steps(&element.statics, 0, index as u32).iter().map(
                |step| match step {
                    PathStep::Index(index) => ast.number(f64::from(*index)),
                    PathStep::Content => ast.string("c"),
                },
            ));
            let children = ast.array(node.children.iter().map(|&id| {
                let child = &element.statics.nodes[id as usize];
                match child.kind {
                    StaticNodeKind::Text => ast.string(&child.text),
                    StaticNodeKind::Marker => ast.null(),
                    StaticNodeKind::Element => {
                        unreachable!("normalized raw text contains no elements")
                    }
                }
            }));
            ast.object([ast.prop("path", path), ast.prop("children", children)])
        })
        .collect::<Vec<_>>();
    let html = if patches.is_empty() {
        crate::ir::layout::serialize_static(&element.statics)
    } else {
        serialize_client_static(&element.statics)
    };
    let (helper, source) = match element.namespace {
        Namespace::Html => ("template", html),
        Namespace::Svg if element.tag == "svg" => ("template", html),
        Namespace::MathMl if element.tag == "math" => ("template", html),
        Namespace::Svg => ("templateSVG", format!("<svg>{html}")),
        Namespace::MathMl => ("templateMathML", html),
    };
    if patches.is_empty() {
        return ctx.call(RUNTIME_MODULE, helper, [ast.string(&source)]);
    }
    let name = ctx.fresh("_textTemplate$");
    let namespace = match helper {
        "templateSVG" => "svg",
        "templateMathML" => "math",
        _ => "",
    };
    let definition = ast.object([
        ast.prop("source", ast.string(&source)),
        ast.prop("namespace", ast.string(namespace)),
        ast.prop("patches", ast.array(patches)),
    ]);
    ctx.hoisted.push(ast.declaration(VariableDeclarationKind::Const, name, Some(definition)));
    ctx.call(RUNTIME_MODULE, "templateWithTextNodes", [ast.ident(name)])
}

pub fn accessor<'a>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    bindings: &Bindings<'a>,
    node: u32,
) -> Expression<'a> {
    use crate::ir::layout::{PathStep, path_steps};

    let ast = Ast::new(ctx.allocator);
    let mut current = node;
    let (origin, mut base) = 'origin: loop {
        if current != node
            && let Some(name) = bindings.names[current as usize]
        {
            break (current, ast.ident(name));
        }
        let parent = element.statics.nodes[current as usize]
            .parent
            .expect("every accessed node has a named ancestor");
        let siblings = &element.statics.nodes[parent as usize].children;
        let index = siblings
            .iter()
            .position(|&sibling| sibling == current)
            .expect("static children contain every node");
        for (distance, &sibling) in siblings[..index].iter().rev().take(2).enumerate() {
            if sibling < node
                && let Some(name) = bindings.names[sibling as usize]
            {
                let mut value = ast.ident(name);
                for _ in 0..=distance {
                    value = ast.member(value, "nextSibling");
                }
                break 'origin (current, value);
            }
        }
        current = parent;
    };
    for step in path_steps(&element.statics, origin, node) {
        match step {
            PathStep::Index(index) if index > 2 => {
                base = ast.index(ast.member(base, "childNodes"), ast.number(index as f64));
            }
            PathStep::Index(index) => {
                base = ast.member(base, "firstChild");
                for _ in 0..index {
                    base = ast.member(base, "nextSibling");
                }
            }
            PathStep::Content => {
                base = ast.member(base, "content");
            }
        }
    }
    base
}

pub fn write_attr<'a>(
    ctx: &mut EmitContext<'a, '_>,
    attr: &Attr,
    node: Expression<'a>,
    value: Expression<'a>,
    previous: Option<Expression<'a>>,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    match &attr.target {
        AttrTarget::Attr(name) => {
            if let Some(prop) = name.strip_prefix("prop:") {
                return prop_write(&ast, ctx, node, prop, value);
            }
            if let Some(name) = name.strip_prefix("attr:") {
                let name = ast.string(name);
                return ctx.call(RUNTIME_MODULE, "setAttribute", [node, name, value]);
            }
            if let Some(name) = name.strip_prefix("bool:") {
                let name = ast.string(name);
                return ctx.call(RUNTIME_MODULE, "setBoolAttribute", [node, name, value]);
            }
            if name == "style" {
                return style_write(ctx, node, value, previous);
            }
            let name = ast.string(name);
            ctx.call(RUNTIME_MODULE, "setAttribute", [node, name, value])
        }
        AttrTarget::AttrNs(ns, name) => {
            let ns = ast.string(ns);
            let name = ast.string(name);
            ctx.call(RUNTIME_MODULE, "setAttributeNS", [node, ns, name, value])
        }
        AttrTarget::Bool(name) => {
            let name = ast.string(name);
            ctx.call(RUNTIME_MODULE, "setBoolAttribute", [node, name, value])
        }
        AttrTarget::Prop(name) => prop_write(&ast, ctx, node, name, value),
        AttrTarget::Class => ctx.call(RUNTIME_MODULE, "className", [node, value]),
        AttrTarget::ClassToggle(token) => {
            let token = ast.string(token);
            match previous {
                Some(previous) => {
                    ctx.call(RUNTIME_MODULE, "toggleClass", [node, token, value, previous])
                }
                None => ctx.call(RUNTIME_MODULE, "toggleClass", [node, token, value]),
            }
        }
        AttrTarget::Text => ast.assign(ast.member(node, "data"), value),
        AttrTarget::Style => style_write(ctx, node, value, previous),
    }
}

fn prop_write<'a>(
    ast: &Ast<'a>,
    ctx: &mut EmitContext<'a, '_>,
    node: Expression<'a>,
    name: &str,
    value: Expression<'a>,
) -> Expression<'a> {
    if is_identifier_name(name) {
        ast.assign(ast.member(node, ctx.intern(name)), value)
    } else {
        ast.assign(ast.index(node, ast.string(name)), value)
    }
}

fn style_write<'a>(
    ctx: &mut EmitContext<'a, '_>,
    node: Expression<'a>,
    value: Expression<'a>,
    previous: Option<Expression<'a>>,
) -> Expression<'a> {
    match previous {
        Some(previous) => ctx.call(RUNTIME_MODULE, "style", [node, value, previous]),
        None => ctx.call(RUNTIME_MODULE, "style", [node, value]),
    }
}

pub fn write_event<'a>(
    ctx: &mut EmitContext<'a, '_>,
    event: &EventHandler,
    node_name: &'a str,
    handler: &'a str,
    data: Option<&'a str>,
) -> Vec<Statement<'a>> {
    let ast = Ast::new(ctx.allocator);
    let node = || ast.ident(node_name);
    match event.kind {
        EventKind::DelegatedStatic => {
            ctx.delegate(&event.name);
            let key = ctx.intern(&format!("$${}", event.name));
            let target = if is_identifier_name(key) {
                ast.member(node(), key)
            } else {
                ast.index(node(), ast.string(key))
            };
            let mut out = vec![ast.stmt(ast.assign(target, ast.ident(handler)))];
            if let Some(data) = data {
                let key = ctx.intern(&format!("$${}Data", event.name));
                let target = if is_identifier_name(key) {
                    ast.member(node(), key)
                } else {
                    ast.index(node(), ast.string(key))
                };
                out.push(ast.stmt(ast.assign(target, ast.ident(data))));
            }
            out
        }
        EventKind::DelegatedDynamic => {
            ctx.delegate(&event.name);
            let name = ast.string(&event.name);
            vec![ast.stmt(ctx.call(
                RUNTIME_MODULE,
                "addEventListener",
                [node(), name, ast.ident(handler), ast.boolean(true)],
            ))]
        }
        EventKind::Direct => {
            let name = ast.string(&event.name);
            vec![ast.stmt(ctx.call(
                RUNTIME_MODULE,
                "addEventListener",
                [node(), name, ast.ident(handler)],
            ))]
        }
    }
}

pub fn write_ref<'a>(
    ctx: &mut EmitContext<'a, '_>,
    target: &RefTarget,
    node_name: &'a str,
) -> Vec<Statement<'a>> {
    let ast = Ast::new(ctx.allocator);
    match &target.target {
        RefOp::Callback(callback) => {
            let callback = ctx.expr(*callback);
            vec![ast.stmt(ctx.call(RUNTIME_MODULE, "use", [callback, ast.ident(node_name)]))]
        }
        RefOp::Assign(AssignTarget::Identifier(name)) => {
            let slot = ctx.intern(name);
            let check = is_function(&ast, ast.ident(slot));
            let use_call = ctx.call(RUNTIME_MODULE, "use", [ast.ident(slot), ast.ident(node_name)]);
            let write =
                ctx.assign_ref(&AssignTarget::Identifier(name.clone()), ast.ident(node_name));
            vec![ast.stmt(ast.conditional(check, use_call, write))]
        }
        RefOp::Assign(AssignTarget::Member { object, key }) => {
            let mut out = Vec::new();
            let object_name = if is_super_object(ctx, object) {
                None
            } else {
                let name = ctx.fresh("_o$");
                out.push(ast.declaration(
                    VariableDeclarationKind::Const,
                    name,
                    Some(ctx.expr(*object)),
                ));
                Some(name)
            };
            let read_name = ctx.fresh("_r$");
            let (read, write) = match key {
                MemberKey::Static(name) => {
                    if let Some(field) = name.strip_prefix('#') {
                        let field = ctx.intern(field);
                        let read = Expression::new_private_field_expression(
                            SPAN,
                            member_receiver(&ast, object_name),
                            PrivateIdentifier::new(SPAN, Ident::from(field), &ast.builder),
                            false,
                            &ast.builder,
                        );
                        let write = Expression::new_private_field_expression(
                            SPAN,
                            member_receiver(&ast, object_name),
                            PrivateIdentifier::new(SPAN, Ident::from(field), &ast.builder),
                            false,
                            &ast.builder,
                        );
                        (read, ast.assign(write, ast.ident(node_name)))
                    } else {
                        let prop = ctx.intern(name);
                        let read = ast.member(member_receiver(&ast, object_name), prop);
                        let write = ast.assign(
                            ast.member(member_receiver(&ast, object_name), prop),
                            ast.ident(node_name),
                        );
                        (read, write)
                    }
                }
                MemberKey::Computed(key) => {
                    let key_name = ctx.fresh("_k$");
                    out.push(ast.declaration(
                        VariableDeclarationKind::Const,
                        key_name,
                        Some(ctx.expr(*key)),
                    ));
                    let read = ast.index(member_receiver(&ast, object_name), ast.ident(key_name));
                    let write = ast.assign(
                        ast.index(member_receiver(&ast, object_name), ast.ident(key_name)),
                        ast.ident(node_name),
                    );
                    (read, write)
                }
            };
            out.push(ast.declaration(VariableDeclarationKind::Const, read_name, Some(read)));
            let check = is_function(&ast, ast.ident(read_name));
            let use_call =
                ctx.call(RUNTIME_MODULE, "use", [ast.ident(read_name), ast.ident(node_name)]);
            out.push(ast.stmt(ast.conditional(check, use_call, write)));
            out
        }
        RefOp::Expr(value) => {
            let slot = ctx.fresh("_r$");
            let check = is_function(&ast, ast.ident(slot));
            let use_call = ctx.call(RUNTIME_MODULE, "use", [ast.ident(slot), ast.ident(node_name)]);
            vec![
                ast.declaration(VariableDeclarationKind::Const, slot, Some(ctx.expr(*value))),
                ast.stmt(Expression::new_logical_expression(
                    SPAN,
                    check,
                    LogicalOperator::And,
                    use_call,
                    &ast.builder,
                )),
            ]
        }
    }
}

fn is_function<'a>(ast: &Ast<'a>, value: Expression<'a>) -> Expression<'a> {
    ast.binary(
        ast.unary(UnaryOperator::Typeof, value),
        BinaryOperator::StrictEquality,
        ast.string("function"),
    )
}
fn is_super_object(ctx: &EmitContext<'_, '_>, object: &ExprRef) -> bool {
    ctx.source.get(object.span.start as usize..object.span.end as usize) == Some("super")
}

fn member_receiver<'a>(ast: &Ast<'a>, object_name: Option<&'a str>) -> Expression<'a> {
    match object_name {
        Some(name) => ast.ident(name),
        None => Expression::new_super(SPAN, &ast.builder),
    }
}

pub fn write_spread<'a>(
    ctx: &mut EmitContext<'a, '_>,
    spread: &SpreadSegment,
    node: Expression<'a>,
    value: Expression<'a>,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    ctx.call(
        RUNTIME_MODULE,
        "spread",
        [node, value, ast.boolean(spread.is_svg), ast.boolean(spread.has_children)],
    )
}

pub fn write_insert<'a>(
    ctx: &mut EmitContext<'a, '_>,
    insert: &InsertOp,
    bindings: &Bindings<'a>,
    value: Expression<'a>,
) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let parent = ast.ident(bindings.name(insert.parent));
    match insert.anchor {
        Anchor::Only => ctx.call(RUNTIME_MODULE, "insert", [parent, value]),
        Anchor::Before(node) => {
            let anchor = ast.ident(bindings.name(node));
            ctx.call(RUNTIME_MODULE, "insert", [parent, value, anchor])
        }
        Anchor::End => ctx.call(RUNTIME_MODULE, "append", [parent, value]),
    }
}

pub fn write_link<'a>(
    ctx: &mut EmitContext<'a, '_>,
    node: Expression<'a>,
    href: Option<Expression<'a>>,
) -> Statement<'a> {
    let ast = Ast::new(ctx.allocator);
    let source = ctx.options.links.as_deref().expect("link elements require the user links module");
    match href {
        Some(href) => ast.stmt(ctx.call(source, "link", [node, href])),
        None => ast.stmt(ctx.call(source, "link", [node])),
    }
}
