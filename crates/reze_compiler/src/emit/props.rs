//! Rewritten props destructuring (SPEC §15.7, §16.7).

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::{is_identifier_name, push_js_string};
use crate::ir::{Embed, PropsFallback, PropsSplit, PropsTemporary};

impl<'a> Emitter<'a, '_> {
    pub(super) fn props_read(
        &mut self,
        out: &mut Code,
        props: &str,
        path: &[&str],
        fallback: Option<&PropsFallback<'a>>,
    ) {
        let mut access = String::from(props);
        for key in path {
            if is_identifier_name(key) {
                access.push('.');
                access.push_str(key);
            } else {
                access.push('[');
                push_js_string(&mut access, key);
                access.push(']');
            }
        }
        let Some(fallback) = fallback else {
            out.push(&access);
            return;
        };
        out.push("(");
        out.push(&access);
        out.push(" === undefined ? ");
        match fallback {
            PropsFallback::Literal(value) => self.embed(out, value),
            PropsFallback::Temporary(name) => out.push(name),
        }
        out.push(" : ");
        out.push(&access);
        out.push(")");
    }

    pub(super) fn props_entry(
        &mut self,
        out: &mut Code,
        props: &str,
        rest: Option<&PropsSplit<'a>>,
        defaults: &[PropsTemporary<'a>],
        body: Option<&Embed<'a>>,
    ) {
        if body.is_some() {
            out.push("{");
        }
        if let Some(rest) = rest {
            let split = self.helper(Helper::SplitProps);
            out.push(" const ");
            self.src(out, rest.binding);
            out.push(" = ");
            out.push(split);
            out.push("(");
            out.push(props);
            let mut list = String::from(", [");
            for (i, key) in rest.keys.iter().enumerate() {
                if i > 0 {
                    list.push_str(", ");
                }
                push_js_string(&mut list, key);
            }
            list.push_str("])[1];");
            out.push(&list);
        }
        for default in defaults {
            out.push(" const ");
            out.push(default.name);
            out.push(" = ");
            self.embed(out, &default.value);
            out.push(";");
        }
        if let Some(body) = body {
            out.push(" return (");
            self.embed(out, body);
            out.push("); }");
        }
    }
}
