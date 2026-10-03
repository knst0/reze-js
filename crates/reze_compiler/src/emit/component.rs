use std::fmt::Write;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::{push_js_string, push_property_key};
use crate::ir::{AssignTarget, Component, Island, IslandLoader, Prop, PropValue, Props, PropsPart};

impl<'a> Emitter<'a, '_> {
    pub(super) fn component(&mut self, out: &mut Code, component: &Component<'a>) {
        if let Some(island) = &component.island {
            self.island(out, component, island);
            return;
        }
        let create = self.helper(Helper::CreateComponent);
        out.push(create);
        out.push("(");
        self.embed(out, &component.callee);
        out.push(", ");
        self.props(out, &component.props);
        if self.options.debug_names {
            let tag = &self.source
                [component.callee.span.start as usize..component.callee.span.end as usize];
            out.push(", ");
            push_js_string(&mut out.text, &format!("{}#{tag}", self.filename));
        }
        out.push(")");
    }

    fn island(&mut self, out: &mut Code, component: &Component<'a>, island: &Island<'a>) {
        let helper = self.helper(Helper::Island);
        out.push(helper);
        out.push("(\"");
        out.push(island.trigger.name());
        out.push("\", () => ");
        match &island.loader {
            IslandLoader::Direct => self.embed(out, &component.callee),
            IslandLoader::Split { source, path } => {
                out.push("import(");
                self.src(out, *source);
                out.push(").then(");
                let module = self.fresh("_m$");
                out.push(module);
                out.push(" => ");
                out.push(module);
                for segment in path {
                    if crate::html::is_identifier_name(segment) {
                        out.push(".");
                        out.push(segment);
                    } else {
                        out.push("[");
                        push_js_string(&mut out.text, segment);
                        out.push("]");
                    }
                }
                out.push(")");
            }
        }
        out.push(", ");
        self.props(out, &component.props);
        out.push(", ");
        match &island.fallback {
            Some(fallback) => self.render(out, fallback),
            None => out.push("void 0"),
        }
        if island.media.is_some() || island.root_margin.is_some() {
            out.push(", { ");
            if let Some(media) = island.media {
                out.push("media: ");
                push_js_string(&mut out.text, media);
                if island.root_margin.is_some() {
                    out.push(", ");
                }
            }
            if let Some(root_margin) = island.root_margin {
                out.push("rootMargin: ");
                push_js_string(&mut out.text, root_margin);
            }
            out.push(" }");
        }
        out.push(")");
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
            PropValue::Str(s) => push_js_string(&mut out.text, s),
            PropValue::Expr(embed) => self.embed(out, embed),
            PropValue::Jsx(jsx) => self.jsx(out, jsx),
            PropValue::Children(children) => self.child_array(out, children),
        }
    }

    /// `ref(r$) { … }`: the component calls it with the element it renders.
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
                let access = self.member_access(out, object, key);
                let _ = write!(
                    out,
                    "typeof {0} === \"function\" ? {0}({element}) : {1} = {element};",
                    access.value, access.target
                );
            }
        }
        out.push(" }");
    }
}
