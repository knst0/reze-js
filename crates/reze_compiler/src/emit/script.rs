use oxc_span::Span;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::{is_identifier_name, push_js_string};
use crate::ir::{Embed, HotEdit, PropsFallback, PropsSplit, PropsTemporary, ScriptEdit};

impl<'a> Emitter<'a, '_> {
    pub(super) fn script(&mut self, out: &mut Code, span: Span, edit: &ScriptEdit<'a>) {
        match edit {
            ScriptEdit::ConstSignalDecl { getter, init } => {
                self.src(out, *getter);
                out.push(" = ");
                self.embed(out, init);
            }
            ScriptEdit::ConstSignalRead { getter } => self.src(out, *getter),
            ScriptEdit::PropsParam { name } => out.push(name),
            ScriptEdit::PropsRead { props, path, fallback, shorthand } => {
                if *shorthand {
                    self.src(out, span);
                    out.push(": ");
                }
                self.props_read(out, props, path, fallback.as_ref());
            }
            ScriptEdit::PropsEntry { props, rest, defaults, body } => {
                self.props_entry(out, props, rest.as_ref(), defaults, body.as_ref());
            }
            ScriptEdit::SelectorRead { selector, key, is_negated } => {
                if *is_negated {
                    out.push("!");
                }
                out.push(selector);
                out.push("(");
                self.embed(out, key);
                out.push(")");
            }
            ScriptEdit::Insert(text) => out.push(text),
            ScriptEdit::Hot(HotEdit::AfterDeclaration { name }) => {
                out.push("\n");
                out.push(name);
                out.push(" = ");
                self.hot_component(out, name);
                out.push(name);
                out.push(");");
            }
            ScriptEdit::Hot(HotEdit::WrapInit { name, init }) => {
                self.hot_component(out, name);
                self.embed(out, init);
                out.push(")");
            }
            ScriptEdit::Hot(HotEdit::Accept) => {
                out.push("\nif (import.meta.hot) import.meta.hot.accept();\n")
            }
        }
    }

    /// `hotComponent(import.meta.hot, "<file>#name", `.
    fn hot_component(&mut self, out: &mut Code, name: &str) {
        let hot = self.helper(Helper::HotComponent);
        out.push(hot);
        out.push("(import.meta.hot, ");
        push_js_string(&mut out.text, &format!("{}#{name}", self.filename));
        out.push(", ");
    }

    fn props_read(
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

    fn props_entry(
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
            out.push(", [");
            for (i, key) in rest.keys.iter().enumerate() {
                if i > 0 {
                    out.push(", ");
                }
                push_js_string(&mut out.text, key);
            }
            out.push("])[1];");
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
