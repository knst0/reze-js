//! Rewritten props destructuring (SPEC §15.7).

use oxc_span::Span;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::{is_identifier_name, push_js_string};
use crate::ir::Embed;

impl<'a> Emitter<'a, '_> {
    pub(super) fn props_read(
        &mut self,
        out: &mut Code,
        props: &str,
        path: &[&str],
        default: Option<&Embed<'a>>,
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
        let Some(default) = default else {
            out.push(&access);
            return;
        };
        out.push("(");
        out.push(&access);
        out.push(" === undefined ? ");
        self.embed(out, default);
        out.push(" : ");
        out.push(&access);
        out.push(")");
    }

    pub(super) fn props_rest(
        &mut self,
        out: &mut Code,
        props: &str,
        binding: Span,
        keys: &[&str],
        body: Option<&Embed<'a>>,
    ) {
        let split = self.helper(Helper::SplitProps);
        out.push(if body.is_some() { "{ const " } else { " const " });
        self.src(out, binding);
        out.push(" = ");
        out.push(split);
        out.push("(");
        out.push(props);
        let mut list = String::from(", [");
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                list.push_str(", ");
            }
            push_js_string(&mut list, key);
        }
        list.push_str("])[1];");
        out.push(&list);
        if let Some(body) = body {
            out.push(" return (");
            self.embed(out, body);
            out.push("); }");
        }
    }
}
