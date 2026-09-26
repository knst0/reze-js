use std::fmt::Write;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::{push_js_string, push_member};
use crate::ir::{
    Anchor, AssignTarget, Bind, BindTarget, Child, Conditional, Embed, ExprChild, From, Handler,
    MemberKey, Op, Placement, RefTarget, Template,
};

impl<'a> Emitter<'a, '_> {
    pub(super) fn template(&mut self, out: &mut Code, template: &Template<'a>) {
        if !template.has_work() {
            self.push_template(out, template.html, template.namespace);
            return;
        }
        let is_block = template.placement == Placement::Block;
        let mut names: std::vec::Vec<&'a str> = vec![""; template.node_count as usize];
        let root = self.fresh("_el$");
        names[0] = root;
        let _ = write!(out, "{}\n  var {root} = ", if is_block { "{" } else { "(() => {" });
        self.push_template(out, template.html, template.namespace);
        for walk in &template.walks {
            let name = self.fresh("_el$");
            names[walk.node.index()] = name;
            let _ = write!(out, ",\n    {name} = ");
            if walk.next_siblings > 0 {
                let next = self.helper(Helper::Next);
                for _ in 0..walk.next_siblings {
                    out.push(next);
                    out.push("(");
                }
            }
            match walk.from {
                From::FirstChildOf(parent) => {
                    let child = self.helper(Helper::Child);
                    let _ = write!(out, "{child}({})", names[parent.index()]);
                }
                From::Node(previous) => out.push(names[previous.index()]),
            }
            for _ in 0..walk.next_siblings {
                out.push(")");
            }
        }
        let previous: std::vec::Vec<&'a str> = if template.binds.len() > 1 {
            template.binds.iter().map(|_| self.fresh("_p$")).collect()
        } else {
            std::vec::Vec::new()
        };
        for name in &previous {
            out.push(",\n    ");
            out.push(name);
        }
        out.push(";\n");
        for op in &template.ops {
            out.push("  ");
            self.op(out, op, &names);
            out.push(";\n");
        }
        if !template.binds.is_empty() {
            out.push("  ");
            self.binds(out, &template.binds, &names, &previous);
            out.push(";\n");
        }
        let _ = write!(out, "  return {root};\n}}{}", if is_block { "" } else { ")()" });
    }

    fn op(&mut self, out: &mut Code, op: &Op<'a>, names: &[&'a str]) {
        match op {
            Op::Set { node, target, value } => {
                self.set_open(out, names[node.index()], *target);
                self.value(out, value);
                self.set_close(out, *target, None);
            }
            Op::Event { node, event, handler } => {
                self.event(out, names[node.index()], event, handler)
            }
            Op::Ref { node, target } => self.element_ref(out, names[node.index()], target),
            Op::Spread { node, props, is_svg, has_children } => {
                let spread = self.helper(Helper::Spread);
                let _ = write!(out, "{spread}({}, ", names[node.index()]);
                self.props(out, props);
                let _ = write!(out, ", {is_svg}, {has_children})");
            }
            Op::Insert { parent, value, anchor } => {
                let insert = self.helper(match anchor {
                    Anchor::End => Helper::Append,
                    Anchor::Only | Anchor::Before(_) => Helper::Insert,
                });
                if let Child::Expr(ExprChild::Conditional(conditional)) = value {
                    self.conditional(out, insert, names[parent.index()], conditional);
                } else {
                    let _ = write!(out, "{insert}({}, ", names[parent.index()]);
                    self.child(out, value);
                }
                if let Anchor::Before(node) = anchor {
                    out.push(", ");
                    out.push(names[node.index()]);
                }
                out.push(")");
            }
        }
    }

    /// `var c = computed(() => !!(test));` and the opening of its insert, up to the marker.
    fn conditional(
        &mut self,
        out: &mut Code,
        insert: &str,
        parent: &str,
        conditional: &Conditional<'a>,
    ) {
        let memo = self.fresh("_c$");
        let computed = self.helper(Helper::Computed);
        let _ = write!(out, "var {memo} = {computed}(() => !!(");
        self.embed(out, &conditional.test);
        let _ = write!(out, "));\n  {insert}({parent}, () => {memo}() ");
        match &conditional.alternate {
            Some(alternate) => {
                out.push("? ");
                self.child(out, &conditional.consequent);
                out.push(" : ");
                self.child(out, alternate);
            }
            None => {
                out.push("&& ");
                self.child(out, &conditional.consequent);
            }
        }
    }

    fn event(&mut self, out: &mut Code, element: &str, event: &'a str, handler: &Handler<'a>) {
        let (handler, is_delegated) = match handler {
            Handler::Delegated { handler, data } => {
                self.events.insert(event);
                let _ = write!(out, "{element}.$${event} = ");
                self.embed(out, handler);
                if let Some(data) = data {
                    let _ = write!(out, ";\n  {element}.$${event}Data = ");
                    self.embed(out, data);
                }
                return;
            }
            Handler::DelegatedDynamic(handler) => {
                self.events.insert(event);
                (handler, true)
            }
            Handler::Direct(handler) => (handler, false),
        };
        let listen = self.helper(Helper::AddEventListener);
        let _ = write!(out, "{listen}({element}, ");
        push_js_string(&mut out.text, event);
        out.push(", ");
        self.embed(out, handler);
        out.push(if is_delegated { ", true)" } else { ")" });
    }

    /// `ref` on a native element: the value is evaluated once.
    fn element_ref(&mut self, out: &mut Code, element: &str, target: &RefTarget<'a>) {
        let use_ = self.helper(Helper::Use);
        match target {
            RefTarget::Callback(callback) => {
                let _ = write!(out, "{use_}(");
                self.embed(out, callback);
                let _ = write!(out, ", {element})");
            }
            RefTarget::Assign(AssignTarget::Identifier(span)) => {
                let name = &self.source[span.start as usize..span.end as usize];
                let _ =
                    write!(out, "typeof {name} === \"function\" ? {use_}({name}, {element}) : ");
                self.src(out, *span);
                let _ = write!(out, " = {element}");
            }
            RefTarget::Assign(AssignTarget::Member { object, key }) => {
                let access = self.member_access(out, object, key);
                let _ = write!(
                    out,
                    "typeof {0} === \"function\" ? {use_}({0}, {element}) : {1} = {element}",
                    access.value, access.target
                );
            }
            RefTarget::Expr(expr) => {
                let value = self.fresh("_r$");
                let _ = write!(out, "var {value} = ");
                self.embed(out, expr);
                let _ = write!(
                    out,
                    ";\n  typeof {value} === \"function\" && {use_}({value}, {element})"
                );
            }
        }
    }

    /// Writes `var _o$ = object[, _k$ = key], _r$ = _o$.key;\n  ` and returns the names.
    pub(super) fn member_access(
        &mut self,
        out: &mut Code,
        object: &Embed<'a>,
        key: &MemberKey<'a>,
    ) -> MemberAccess<'a> {
        let object_name = self.fresh("_o$");
        let _ = write!(out, "var {object_name} = ");
        self.embed(out, object);
        let mut target = String::from(object_name);
        match key {
            MemberKey::Static(name) => {
                target.push('.');
                target.push_str(name);
            }
            MemberKey::Computed(key) => {
                let key_name = self.fresh("_k$");
                let _ = write!(out, ", {key_name} = ");
                self.embed(out, key);
                let _ = write!(target, "[{key_name}]");
            }
        }
        let value = self.fresh("_r$");
        let _ = write!(out, ", {value} = {target};\n  ");
        MemberAccess { value, target: self.alloc.alloc_str(&target) }
    }

    /// The template's binds as one render effect; several targets compare against `previous`.
    fn binds(
        &mut self,
        out: &mut Code,
        binds: &[Bind<'a>],
        names: &[&'a str],
        previous: &[&'a str],
    ) {
        let effect = self.helper(Helper::RenderEffect);
        if let [single] = binds {
            let element = names[single.node.index()];
            if single.target.threads_prev() {
                let previous = self.fresh("_p$");
                let _ = write!(out, "{effect}({previous} => ");
                self.set_open(out, element, single.target);
                self.value(out, &single.value);
                self.set_close(out, single.target, Some(previous));
            } else {
                let _ = write!(out, "{effect}(() => ");
                self.set_open(out, element, single.target);
                self.value(out, &single.value);
                self.set_close(out, single.target, None);
            }
            out.push(")");
            return;
        }
        let _ = write!(out, "{effect}(() => {{\n    var ");
        let mut values = std::vec::Vec::with_capacity(binds.len());
        for (i, bind) in binds.iter().enumerate() {
            let value = self.fresh("_v$");
            if i > 0 {
                out.push(",\n      ");
            }
            let _ = write!(out, "{value} = ");
            self.value(out, &bind.value);
            values.push(value);
        }
        out.push(";\n");
        for ((bind, value), previous) in binds.iter().zip(&values).zip(previous) {
            let element = names[bind.node.index()];
            if bind.target.compares_itself() {
                let _ = write!(out, "    {previous} = ");
                self.set_open(out, element, bind.target);
                out.push(value);
                self.set_close(out, bind.target, Some(previous));
                out.push(";\n");
                continue;
            }
            let _ = write!(out, "    {value} !== {previous} && (");
            if bind.target.threads_prev() {
                let _ = write!(out, "{previous} = ");
                self.set_open(out, element, bind.target);
                out.push(value);
                self.set_close(out, bind.target, Some(previous));
            } else {
                self.set_open(out, element, bind.target);
                let _ = write!(out, "{previous} = {value}");
                self.set_close(out, bind.target, None);
            }
            out.push(");\n");
        }
        out.push("  })");
    }

    fn set_open(&mut self, out: &mut Code, element: &str, target: BindTarget<'a>) {
        let helper = match target {
            BindTarget::Text => {
                let _ = write!(out, "{element}.data = ");
                return;
            }
            BindTarget::Prop { name } => {
                push_member(&mut out.text, element, name);
                out.push(" = ");
                return;
            }
            BindTarget::Attr(_) => Helper::SetAttribute,
            BindTarget::AttrNs(..) => Helper::SetAttributeNs,
            BindTarget::Bool(_) => Helper::SetBoolAttribute,
            BindTarget::Class => Helper::ClassName,
            BindTarget::ClassToggle(_) => Helper::ToggleClass,
            BindTarget::Style => Helper::Style,
        };
        let helper = self.helper(helper);
        let _ = write!(out, "{helper}({element}, ");
        match target {
            BindTarget::Attr(name) | BindTarget::Bool(name) | BindTarget::ClassToggle(name) => {
                push_js_string(&mut out.text, name);
                out.push(", ");
            }
            BindTarget::AttrNs(namespace, name) => {
                push_js_string(&mut out.text, namespace);
                out.push(", ");
                push_js_string(&mut out.text, name);
                out.push(", ");
            }
            BindTarget::Class | BindTarget::Style | BindTarget::Prop { .. } | BindTarget::Text => {}
        }
    }

    fn set_close(&mut self, out: &mut Code, target: BindTarget<'a>, previous: Option<&str>) {
        if matches!(target, BindTarget::Prop { .. } | BindTarget::Text) {
            return;
        }
        if let Some(previous) = previous {
            out.push(", ");
            out.push(previous);
        }
        out.push(")");
    }
}

pub(super) struct MemberAccess<'a> {
    /// Holds the member's value.
    pub value: &'a str,
    /// Assignable access to the member.
    pub target: &'a str,
}
