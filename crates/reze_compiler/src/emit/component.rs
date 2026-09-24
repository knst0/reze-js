//! Components and props objects.

use std::fmt::Write;

use super::{Emitter, Helper};
use crate::Target;
use crate::code::Code;
use crate::facts::IslandMode;
use crate::html::push_property_key;
use crate::ir::{AssignTarget, Component, Island, Prop, PropValue, Props, PropsPart};

impl<'a> Emitter<'a, '_> {
    pub(super) fn component(&mut self, out: &mut Code, component: &Component<'a>) {
        match component.island.as_ref().filter(|_| self.target == Target::Server) {
            Some(island) => {
                let ssr = self.helper(Helper::SsrIsland);
                out.push(ssr);
                out.push("(");
                crate::html::push_js_string(&mut out.text, island.id);
                out.push(", ");
                self.embed(out, &component.callee);
                out.push(", ");
                self.island_props(out, &component.props, island);
                if !island.slots.is_empty() || island.mode != IslandMode::Eager {
                    out.push(", ");
                    self.island_slots(out, &component.props, island);
                }
                if island.mode != IslandMode::Eager {
                    out.push(", ");
                    crate::html::push_js_string(&mut out.text, island.mode.as_str());
                }
                out.push(")");
                return;
            }
            None => {
                let create = self.helper(Helper::CreateComponent);
                out.push(create);
                out.push("(");
                self.embed(out, &component.callee);
                out.push(", ");
                self.props(out, &component.props);
                out.push(")");
            }
        }
    }

    fn island_props(&mut self, out: &mut Code, props: &Props<'a>, island: &Island<'a>) {
        let entries: Vec<&Prop<'a>> = props
            .parts
            .iter()
            .flat_map(|part| match part {
                PropsPart::Object(entries) => entries.iter().collect::<Vec<_>>(),
                PropsPart::Spread { .. } => Vec::new(),
            })
            .filter(|entry| !island.slots.contains(&Self::prop_key(entry)))
            .collect();
        self.object_refs(out, &entries);
    }

    fn island_slots(&mut self, out: &mut Code, props: &Props<'a>, island: &Island<'a>) {
        let mut slots: Vec<&Prop<'a>> = Vec::new();
        for part in props.parts.iter() {
            if let PropsPart::Object(entries) = part {
                for entry in entries.iter() {
                    if island.slots.contains(&Self::prop_key(entry)) {
                        slots.push(entry);
                    }
                }
            }
        }
        if slots.is_empty() {
            out.push("null");
            return;
        }
        out.push("{ ");
        for (i, entry) in slots.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            out.push("get ");
            push_property_key(&mut out.text, Self::prop_key(entry));
            out.push("() { return ");
            self.slot_value(out, entry);
            out.push("; }");
        }
        out.push(" }");
    }

    fn slot_value(&mut self, out: &mut Code, entry: &Prop<'a>) {
        match entry {
            Prop::Value { value, .. } => self.prop_value(out, value),
            Prop::Getter { value, .. } => self.prop_value(out, value),
            Prop::ForwardRef(target) => self.forward_ref(out, target),
        }
    }
    fn prop_key<'p, 'b>(entry: &'p Prop<'b>) -> &'p str {
        match entry {
            Prop::Value { key, .. } | Prop::Getter { key, .. } => key,
            Prop::ForwardRef(_) => "ref",
        }
    }

    /// An object literal, a single static spread value, or `mergeProps` over every part.
    pub(super) fn props(&mut self, out: &mut Code, props: &Props<'a>) {
        match props.parts.as_slice() {
            [] => out.push("{}"),
            [PropsPart::Object(entries)] => self.object(out, entries),
            [PropsPart::Spread { value, is_dynamic: false }] => self.embed(out, value),
            parts => {
                let merge = self.helper(Helper::MergeProps);
                out.push(merge);
                out.push("(");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push(", ");
                    }
                    match part {
                        PropsPart::Object(entries) => self.object(out, entries),
                        PropsPart::Spread { value, is_dynamic: false } => self.embed(out, value),
                        PropsPart::Spread { value, is_dynamic: true } => {
                            out.push("() => (");
                            self.embed(out, value);
                            out.push(")");
                        }
                    }
                }
                out.push(")");
            }
        }
    }

    fn object(&mut self, out: &mut Code, entries: &[Prop<'a>]) {
        let refs: Vec<&Prop<'a>> = entries.iter().collect();
        self.object_refs(out, &refs);
    }

    fn object_refs(&mut self, out: &mut Code, entries: &[&Prop<'a>]) {
        out.push("{ ");
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            match entry {
                Prop::Value { key, value } => {
                    push_property_key(&mut out.text, key);
                    out.push(": ");
                    self.prop_value(out, value);
                }
                Prop::Getter { key, value } => {
                    out.push("get ");
                    push_property_key(&mut out.text, key);
                    out.push("() { return ");
                    self.prop_value(out, value);
                    out.push("; }");
                }
                Prop::ForwardRef(target) => self.forward_ref(out, target),
            }
        }
        out.push(" }");
    }

    fn prop_value(&mut self, out: &mut Code, value: &PropValue<'a>) {
        match value {
            PropValue::True => out.push("true"),
            PropValue::Str(s) => crate::html::push_js_string(&mut out.text, s),
            PropValue::Expr(embed) => self.embed(out, embed),
            PropValue::Jsx(jsx) => self.jsx(out, jsx),
            PropValue::Children(children) => self.child_array(out, children),
        }
    }

    /// `ref(r$) { … }`: the component calls it with the element it renders (SPEC §7.6).
    fn forward_ref(&mut self, out: &mut Code, target: &AssignTarget<'a>) {
        let element = self.fresh("r$");
        let _ = write!(out, "ref({element}) {{ ");
        match target {
            AssignTarget::Identifier(span) => {
                let name = &self.source[span.start as usize..span.end as usize];
                let _ = write!(out, "typeof {name} === \"function\" ? {name}({element}) : ");
                self.src(out, *span);
                let _ = write!(out, " = {element};");
            }
            AssignTarget::Member { object, key } => {
                let access = self.member_ref_prelude(out, object, key);
                let _ = write!(
                    out,
                    "typeof {} === \"function\" ? {}({element}) : {} = {element};",
                    access.value, access.value, access.target
                );
            }
        }
        out.push(" }");
    }
}
