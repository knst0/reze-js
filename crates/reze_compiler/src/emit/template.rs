//! Client and hydrate templates: clone (or claim), walk to the referenced nodes, run the ops,
//! merge the binds.

use std::fmt::Write;

use super::{Emitter, Helper};
use crate::Target;
use crate::code::Code;
use crate::html::{push_js_string, push_member};
use crate::ir::{
    Anchor, AssignTarget, Bind, BindTarget, Embed, From, Handler, MemberKey, Op, RefTarget,
    Template,
};

impl<'a> Emitter<'a, '_> {
    pub(super) fn template(&mut self, out: &mut Code, template: &Template<'a>) {
        let factory = self.template_name(template.html, template.namespace);
        let is_hydrate = self.target == Target::Hydrate;
        let mut create = String::new();
        if is_hydrate {
            let claim = self.helper(Helper::Claim);
            let _ = write!(create, "{claim}({factory}, ");
            push_js_string(&mut create, template.root_tag());
            create.push(')');
        } else {
            let _ = write!(create, "{factory}()");
        }
        if template.walks.is_empty() && template.ops.is_empty() && template.binds.is_empty() {
            out.push(&create);
            return;
        }

        let mut names: std::vec::Vec<&'a str> = vec![""; template.nodes.len()];
        let root = self.fresh("_el$");
        names[0] = root;
        let _ = write!(out, "(() => {{\n  var {root} = {create}");
        for walk in &template.walks {
            let name = self.fresh("_el$");
            names[walk.node.index()] = name;
            let _ = write!(out, ",\n    {name} = ");
            match (walk.from, is_hydrate) {
                (From::FirstChildOf(parent), false) => {
                    out.push(names[parent.index()]);
                    out.push(".firstChild");
                    for _ in 0..walk.next_siblings {
                        out.push(".nextSibling");
                    }
                }
                (From::Node(previous), false) => {
                    out.push(names[previous.index()]);
                    for _ in 0..walk.next_siblings {
                        out.push(".nextSibling");
                    }
                }
                (From::FirstChildOf(parent), true) => {
                    let child = self.helper(Helper::ClaimChild);
                    let _ =
                        write!(out, "{child}({}, {})", names[parent.index()], walk.next_siblings);
                }
                (From::Node(previous), true) => {
                    let sibling = self.helper(Helper::ClaimSibling);
                    let _ = write!(
                        out,
                        "{sibling}({}, {})",
                        names[previous.index()],
                        walk.next_siblings
                    );
                }
            }
        }
        out.push(";\n");

        let mut memos: std::vec::Vec<&'a str> = vec![""; template.memo_count as usize];
        for op in &template.ops {
            out.push("  ");
            self.op(out, op, &names, &mut memos);
            out.push(";\n");
        }
        if !template.binds.is_empty() {
            out.push("  ");
            self.binds(out, &template.binds, &names);
            out.push(";\n");
        }
        let _ = write!(out, "  return {root};\n}})()");
    }

    fn op(&mut self, out: &mut Code, op: &Op<'a>, names: &[&'a str], memos: &mut [&'a str]) {
        match op {
            Op::Set { node, target, value } => {
                self.set_open(out, names[node.index()], *target);
                self.value(out, value);
                self.set_close(out, *target, None);
            }
            Op::Event { node, event, handler } => {
                self.event(out, names[node.index()], event, handler);
            }
            Op::Ref { node, target } => self.element_ref(out, names[node.index()], target),
            Op::Spread { node, props, is_svg, has_children } => {
                let spread = self.helper(Helper::Spread);
                let _ = write!(out, "{spread}({}, ", names[node.index()]);
                self.props(out, props);
                let _ = write!(out, ", {is_svg}, {has_children})");
            }
            Op::Memo { id, test } => {
                let name = self.fresh("_c$");
                memos[id.0 as usize] = name;
                let memo = self.helper(Helper::Memo);
                let _ = write!(out, "var {name} = {memo}(() => !!(");
                self.embed(out, test);
                out.push("))");
            }
            Op::Insert { parent, value, anchor, inserts_after } => {
                let is_hydrate = self.target == Target::Hydrate;
                let insert =
                    self.helper(if is_hydrate { Helper::ClaimInsert } else { Helper::Insert });
                let _ = write!(out, "{insert}({}, ", names[parent.index()]);
                self.child(out, value, memos);
                match anchor {
                    Anchor::Only => {}
                    Anchor::Before(node) => {
                        out.push(", ");
                        out.push(names[node.index()]);
                    }
                    Anchor::End => out.push(", null"),
                }
                if is_hydrate && *inserts_after > 0 {
                    let _ = write!(out, ", {inserts_after}");
                }
                out.push(")");
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

    /// `ref` on a native element (SPEC §7.6): the value is evaluated once.
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
                let access = self.member_ref_prelude(out, object, key);
                let _ = write!(
                    out,
                    "typeof {} === \"function\" ? {use_}({}, {element}) : {} = {element}",
                    access.value, access.value, access.target
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
    pub(super) fn member_ref_prelude(
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

    /// Merges the dynamic attribute updates of a template into one `bind` (O2).
    fn binds(&mut self, out: &mut Code, binds: &[Bind<'a>], names: &[&'a str]) {
        let bind = self.helper(Helper::Bind);
        if let [single] = binds {
            let element = names[single.node.index()];
            if single.target.threads_prev() {
                let previous = self.fresh("_p$");
                let _ = write!(out, "{bind}({previous} => ");
                self.set_open(out, element, single.target);
                self.value(out, &single.value);
                self.set_close(out, single.target, Some(previous));
            } else {
                let _ = write!(out, "{bind}(() => ");
                self.set_open(out, element, single.target);
                self.value(out, &single.value);
                self.set_close(out, single.target, None);
            }
            out.push(")");
            return;
        }

        let previous = self.fresh("_p$");
        let _ = write!(out, "{bind}({previous} => {{\n    var ");
        let mut values = std::vec::Vec::with_capacity(binds.len());
        for (i, b) in binds.iter().enumerate() {
            let value = self.fresh("_v$");
            if i > 0 {
                out.push(",\n      ");
            }
            let _ = write!(out, "{value} = ");
            self.value(out, &b.value);
            values.push(value);
        }
        out.push(";\n");
        for (i, (b, value)) in binds.iter().zip(&values).enumerate() {
            let slot = format!("{previous}[{i}]");
            let element = names[b.node.index()];
            let _ = write!(out, "    {value} !== {slot} && (");
            if b.target.threads_prev() {
                let _ = write!(out, "{slot} = ");
                self.set_open(out, element, b.target);
                out.push(value);
                self.set_close(out, b.target, Some(&slot));
            } else {
                self.set_open(out, element, b.target);
                let _ = write!(out, "{slot} = {value}");
                self.set_close(out, b.target, None);
            }
            out.push(");\n");
        }
        let _ = write!(out, "    return {previous};\n  }}, [])");
    }

    fn set_open(&mut self, out: &mut Code, element: &str, target: BindTarget<'a>) {
        let helper = match target {
            BindTarget::Attr(_) => Helper::SetAttribute,
            BindTarget::AttrNs(..) => Helper::SetAttributeNs,
            BindTarget::Bool(_) => Helper::SetBoolAttribute,
            BindTarget::Class => Helper::ClassName,
            BindTarget::Style => Helper::Style,
            BindTarget::Prop { name, .. } => {
                push_member(&mut out.text, element, name);
                out.push(" = ");
                return;
            }
        };
        let helper = self.helper(helper);
        let _ = write!(out, "{helper}({element}, ");
        match target {
            BindTarget::Attr(name) | BindTarget::Bool(name) => {
                push_js_string(&mut out.text, name);
                out.push(", ");
            }
            BindTarget::AttrNs(namespace, name) => {
                push_js_string(&mut out.text, namespace);
                out.push(", ");
                push_js_string(&mut out.text, name);
                out.push(", ");
            }
            BindTarget::Class | BindTarget::Style | BindTarget::Prop { .. } => {}
        }
    }

    fn set_close(&mut self, out: &mut Code, target: BindTarget<'a>, previous: Option<&str>) {
        if matches!(target, BindTarget::Prop { .. }) {
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
    /// Holds the current value of the member.
    pub value: &'a str,
    /// Assignable access to the member.
    pub target: &'a str,
}
