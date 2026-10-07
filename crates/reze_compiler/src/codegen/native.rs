use oxc_ast::ast::{Expression, ObjectPropertyKind, Statement, VariableDeclarationKind};
use oxc_syntax::operator::BinaryOperator;

use crate::ast::Ast;
use crate::ir::schedule::{MemberKind, SchedMember};
use crate::ir::view::{
    Attr, AttrTarget, AttrValue, ElementProp, ElementView, EventHandler, InsertOp, LateProp,
    LinkProp, RefTarget, SpreadPart, SpreadSegment,
};

use super::{EmitContext, composite};

pub struct Bindings<'a> {
    pub names: Vec<Option<&'a str>>,
}

impl<'a> Bindings<'a> {
    pub fn name(&self, node: u32) -> &'a str {
        self.names[node as usize].expect("scheduled native node has a binding")
    }
}

pub fn bindings<'a>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    all_nodes: bool,
) -> Bindings<'a> {
    let names = element
        .statics
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            (index == 0 || all_nodes || node.referenced).then(|| ctx.fresh("_el$"))
        })
        .collect();
    Bindings { names }
}

pub trait NativeTarget<'a> {
    const CLIENT_WORK: bool;
    const CAPTURE_INSERT: bool = false;
    fn attr(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        attr: &Attr,
        value: &'a str,
        previous: Option<&'a str>,
    ) -> Expression<'a>;
    fn event(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        event: &EventHandler,
        handler: &'a str,
        data: Option<&'a str>,
    ) -> Vec<Statement<'a>>;
    fn reference(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        target: &RefTarget,
    ) -> Vec<Statement<'a>>;
    fn spread(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        spread: &SpreadSegment,
        value: &'a str,
    ) -> Expression<'a>;
    fn insert(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        insert: &InsertOp,
        value: Expression<'a>,
    ) -> Expression<'a>;
    fn late_value(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        late: &LateProp,
        value: &'a str,
    ) -> Expression<'a>;
    fn link(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        link: &LinkProp,
        href: Option<Expression<'a>>,
    ) -> Statement<'a>;
    fn effect(
        &mut self,
        ctx: &mut EmitContext<'a, '_>,
        callback: Expression<'a>,
        fixed: bool,
    ) -> Statement<'a>;
}

pub fn schedule<'a>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    target: &mut impl NativeTarget<'a>,
    out: &mut Vec<Statement<'a>>,
) {
    for member in &element.schedule.immediate {
        immediate(ctx, element, target, *member, out);
    }
    for member in &element.schedule.children {
        immediate(ctx, element, target, *member, out);
    }
    for member in &element.schedule.post_children {
        immediate(ctx, element, target, *member, out);
    }
    if ctx.cold && element.schedule.effects.len() > 1 {
        effect(
            ctx,
            element,
            target,
            element.schedule.effects.iter().flat_map(|group| group.members.iter().copied()),
            element.schedule.effects.iter().all(|group| group.fixed),
            out,
        );
    } else {
        for group in &element.schedule.effects {
            effect(ctx, element, target, group.members.iter().copied(), group.fixed, out);
        }
    }
}

fn immediate<'a, T: NativeTarget<'a>>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    target: &mut T,
    member: SchedMember,
    out: &mut Vec<Statement<'a>>,
) {
    if member.kind == MemberKind::Insert && !T::CAPTURE_INSERT {
        let insert = &element.inserts[member.index];
        let value = ctx.child(&insert.value);
        let expression = target.insert(ctx, insert, value);
        out.push(Ast::new(ctx.allocator).stmt(expression));
        return;
    }
    if member.kind == MemberKind::Prop
        && let ElementProp::Spread(spread) = &element.props[member.index]
        && spread.closed
    {
        let mut body = Vec::new();
        closed_spread_body(ctx, target, spread, out, &mut body);
        if !body.is_empty() {
            let callback = Ast::new(ctx.allocator).block_arrow([], body);
            out.push(target.effect(ctx, callback, false));
        }
        return;
    }
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    member_statements(ctx, element, target, member, false, out, &mut reads, &mut writes);
    out.extend(reads);
    out.extend(writes);
}

fn effect<'a>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    target: &mut impl NativeTarget<'a>,
    members: impl IntoIterator<Item = SchedMember>,
    fixed: bool,
    out: &mut Vec<Statement<'a>>,
) {
    let ast = Ast::new(ctx.allocator);
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for member in members {
        member_statements(ctx, element, target, member, true, out, &mut reads, &mut writes);
    }
    if writes.is_empty() {
        return;
    }
    reads.extend(writes);
    let callback = ast.block_arrow([], reads);
    out.push(target.effect(ctx, callback, fixed));
}

fn closed_spread_body<'a>(
    ctx: &mut EmitContext<'a, '_>,
    target: &mut impl NativeTarget<'a>,
    spread: &SpreadSegment,
    outer: &mut Vec<Statement<'a>>,
    body: &mut Vec<Statement<'a>>,
) {
    for part in &spread.parts {
        let SpreadPart::Entries(entries) = part else {
            unreachable!("closed spread contains only entries")
        };
        for attr in entries {
            let value = ctx.value(&attr.value);
            let deferred = matches!(&attr.value, AttrValue::Dynamic(dynamic) if dynamic.mode == crate::ir::schedule::ValueMode::Tracked);
            let name = read_value(ctx, value, if deferred { body } else { outer });
            attr_write(ctx, target, attr, name, true, true, outer, body);
        }
    }
}

fn read_value<'a>(
    ctx: &mut EmitContext<'a, '_>,
    value: Expression<'a>,
    reads: &mut Vec<Statement<'a>>,
) -> &'a str {
    let ast = Ast::new(ctx.allocator);
    let name = ctx.fresh("_v$");
    reads.push(ast.declaration(VariableDeclarationKind::Const, name, Some(value)));
    name
}

fn attr_write<'a>(
    ctx: &mut EmitContext<'a, '_>,
    target: &mut impl NativeTarget<'a>,
    attr: &Attr,
    name: &'a str,
    tracked: bool,
    guarded: bool,
    outer: &mut Vec<Statement<'a>>,
    writes: &mut Vec<Statement<'a>>,
) {
    let ast = Ast::new(ctx.allocator);
    let stateful = matches!(attr.target, AttrTarget::Style | AttrTarget::ClassToggle(_));
    if tracked && (stateful || guarded) {
        let previous = ctx.fresh("_p$");
        outer.push(ast.declaration(VariableDeclarationKind::Let, previous, None));
        let write = target.attr(ctx, attr, name, stateful.then_some(previous));
        if stateful {
            writes.push(ast.stmt(ast.assign(ast.ident(previous), write)));
        } else {
            let body = ast.block([
                ast.stmt(write),
                ast.stmt(ast.assign(ast.ident(previous), ast.ident(name))),
            ]);
            writes.push(ast.if_stmt(
                ast.binary(ast.ident(name), BinaryOperator::StrictInequality, ast.ident(previous)),
                body,
            ));
        }
    } else {
        writes.push(ast.stmt(target.attr(ctx, attr, name, None)));
    }
}

fn member_statements<'a, T: NativeTarget<'a>>(
    ctx: &mut EmitContext<'a, '_>,
    element: &ElementView,
    target: &mut T,
    member: SchedMember,
    tracked: bool,
    outer: &mut Vec<Statement<'a>>,
    reads: &mut Vec<Statement<'a>>,
    writes: &mut Vec<Statement<'a>>,
) {
    let ast = Ast::new(ctx.allocator);
    match member.kind {
        MemberKind::Prop => match &element.props[member.index] {
            ElementProp::Attr(attr) => {
                if embedded(element, attr) {
                    return;
                }
                let value = ctx.value(&attr.value);
                let name = read_value(ctx, value, reads);
                attr_write(ctx, target, attr, name, tracked, false, outer, writes);
            }
            ElementProp::Event(event) if T::CLIENT_WORK => {
                let handler = ctx.expr(event.handler);
                let handler = read_value(ctx, handler, reads);
                let data = event.data.map(|expr| {
                    let value = ctx.expr(expr);
                    read_value(ctx, value, reads)
                });
                writes.extend(target.event(ctx, event, handler, data));
            }
            ElementProp::Ref(reference) if T::CLIENT_WORK => {
                writes.extend(target.reference(ctx, reference));
            }
            ElementProp::Event(_) | ElementProp::Ref(_) => {}
            ElementProp::Spread(spread) => {
                let value = spread_value(ctx, spread);
                let name = read_value(ctx, value, reads);
                writes.push(ast.stmt(target.spread(ctx, spread, name)));
            }
        },
        MemberKind::Insert => {
            let insert = &element.inserts[member.index];
            let value = ctx.child(&insert.value);
            let name = read_value(ctx, value, reads);
            writes.push(ast.stmt(target.insert(ctx, insert, ast.ident(name))));
        }
        MemberKind::Late => {
            let late = &element.late_values[member.index];
            let value = ctx.value(&late.value);
            let name = read_value(ctx, value, reads);
            writes.push(ast.stmt(target.late_value(ctx, late, name)));
        }
        MemberKind::Link => {
            let link = &element.links[member.index];
            let href = link.href.as_ref().map(|href| ctx.getter(href));
            writes.push(target.link(ctx, link, href));
        }
    }
}

fn embedded(element: &ElementView, attr: &Attr) -> bool {
    let name = match &attr.target {
        AttrTarget::Attr(name) => name.as_str(),
        AttrTarget::Style => "style",
        _ => return false,
    };
    element.statics.nodes[attr.node as usize].attrs.iter().any(|static_attr| {
        static_attr.name == name
            && match (&attr.value, &static_attr.value) {
                (AttrValue::True, None) => true,
                (AttrValue::Str(value), Some(initial)) => value == initial,
                _ => false,
            }
    })
}

fn spread_value<'a>(ctx: &mut EmitContext<'a, '_>, spread: &SpreadSegment) -> Expression<'a> {
    let ast = Ast::new(ctx.allocator);
    let mut values = Vec::with_capacity(spread.parts.len());
    let mut has_dynamic_source = false;
    for part in &spread.parts {
        match part {
            SpreadPart::Entries(entries) => {
                let mut properties: Vec<ObjectPropertyKind<'a>> = Vec::with_capacity(entries.len());
                for attr in entries {
                    let AttrTarget::Attr(key) = &attr.target else {
                        unreachable!("generic props retain their original key")
                    };
                    let value = ctx.value(&attr.value);
                    properties.push(if matches!(attr.value, AttrValue::Dynamic(_)) {
                        ast.getter(key, value)
                    } else {
                        ast.prop(key, value)
                    });
                }
                values.push(composite::object(ctx, properties));
            }
            SpreadPart::Generic { expr, dynamic } => {
                let value = ctx.expr(*expr);
                if *dynamic {
                    has_dynamic_source = true;
                    values.push(ast.arrow([], value));
                } else {
                    values.push(value);
                }
            }
        }
    }
    if values.len() == 1 && !has_dynamic_source {
        values.pop().expect("one props segment")
    } else {
        ctx.call(crate::RUNTIME_MODULE, "mergeProps", values)
    }
}
