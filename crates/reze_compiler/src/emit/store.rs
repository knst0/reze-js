//! Unproxied stores (SPEC §15.6): leaf signals, setter drafts and replaced specifiers.

use super::{Emitter, Helper};
use crate::code::Code;
use crate::ir::{ArraySuffix, ArrayWriteKind, IndexOp, Specifier, StoreLeaf, StoreWriteKind};

impl<'a> Emitter<'a, '_> {
    pub(super) fn store_declaration(&mut self, out: &mut Code, leaves: &[StoreLeaf<'a>]) {
        let signal = self.helper(Helper::Signal);
        for (i, leaf) in leaves.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            out.push("[");
            out.push(leaf.getter);
            if let Some(setter) = leaf.setter {
                out.push(", ");
                out.push(setter);
            }
            out.push("] = ");
            out.push(signal);
            out.push("(");
            self.embed(out, &leaf.value);
            out.push(")");
        }
    }

    pub(super) fn store_write(&mut self, out: &mut Code, setter: &str, write: &StoreWriteKind<'a>) {
        out.push(setter);
        match write {
            StoreWriteKind::Assign { value, parenthesize } => {
                out.push(if *parenthesize { "(() => (" } else { "(() => " });
                self.embed(out, value);
                out.push(if *parenthesize { "))" } else { ")" });
            }
            StoreWriteKind::Compound { parameter, operator, value, parenthesize } => {
                out.push("((");
                out.push(parameter);
                out.push(") => ");
                out.push(parameter);
                out.push(" ");
                out.push(operator);
                out.push(if *parenthesize { " (" } else { " " });
                self.embed(out, value);
                out.push(if *parenthesize { "))" } else { ")" });
            }
            StoreWriteKind::Update { parameter, operator } => {
                out.push("((");
                out.push(parameter);
                out.push(") => ");
                out.push(operator);
                out.push(parameter);
                out.push(")");
            }
        }
    }

    pub(super) fn specifiers(&mut self, out: &mut Code, specifiers: &[Specifier<'a>]) {
        for (i, specifier) in specifiers.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            match specifier {
                Specifier::Source(span) => self.src(out, *span),
                Specifier::Alias { name, alias } => {
                    out.push(name);
                    if name != alias {
                        out.push(" as ");
                        out.push(alias);
                    }
                }
            }
        }
    }
}

impl<'a> Emitter<'a, '_> {
    pub(super) fn array_read(&mut self, out: &mut Code, getter: &str, suffix: &[ArraySuffix<'a>]) {
        out.push(getter);
        out.push("()");
        self.array_suffix(out, suffix);
    }

    fn array_suffix(&mut self, out: &mut Code, suffix: &[ArraySuffix<'a>]) {
        for step in suffix {
            match step {
                ArraySuffix::Key(key) => {
                    if crate::html::is_identifier_name(key) {
                        out.push(".");
                        out.push(key);
                    } else {
                        out.push("[");
                        crate::html::push_js_string(&mut out.text, key);
                        out.push("]");
                    }
                }
                ArraySuffix::Index(index) => {
                    out.push("[");
                    self.embed(out, index);
                    out.push("]");
                }
            }
        }
    }

    pub(super) fn array_write(&mut self, out: &mut Code, setter: &str, write: &ArrayWriteKind<'a>) {
        match write {
            ArrayWriteKind::Index { index, tail, op, temp } => {
                out.push(setter);
                out.push("((v) => { const c = v.slice(); ");
                match temp {
                    Some(temp) => {
                        out.push("const ");
                        out.push(temp);
                        out.push(" = ");
                        self.embed(out, index);
                        out.push("; ");
                        self.clone_set(out, temp, tail, op);
                    }
                    None => {
                        match op {
                            IndexOp::Update { operator } => {
                                out.push(operator);
                            }
                            _ => {}
                        }
                        out.push("c[");
                        self.embed(out, index);
                        out.push("]");
                        self.key_path(out, tail);
                        match op {
                            IndexOp::Assign { value } => {
                                out.push(" = ");
                                self.embed(out, value);
                            }
                            IndexOp::Compound { operator, value } => {
                                out.push(" ");
                                out.push(operator);
                                out.push("= ");
                                self.embed(out, value);
                            }
                            IndexOp::Update { .. } => {}
                        }
                    }
                }
                out.push("; return c; })");
            }
            ArrayWriteKind::Method { name, args } => {
                out.push(setter);
                out.push("((v) => { const c = v.slice(); c.");
                out.push(name);
                out.push("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push(", ");
                    }
                    self.embed(out, arg);
                }
                out.push("); return c; })");
            }
            ArrayWriteKind::Form { temps, sets } => {
                out.push("{");
                for temp in temps {
                    out.push(" const ");
                    out.push(temp.name);
                    out.push(" = ");
                    self.embed(out, &temp.value);
                    out.push(";");
                }
                for set in sets {
                    out.push(" ");
                    out.push(set.setter);
                    out.push("(() => ");
                    out.push(set.temp);
                    out.push(");");
                }
                out.push(" }");
            }
        }
    }
}

impl<'a> Emitter<'a, '_> {
    fn key_path(&mut self, out: &mut Code, tail: &[&'a str]) {
        for key in tail {
            if crate::html::is_identifier_name(key) {
                out.push(".");
                out.push(key);
            } else {
                out.push("[");
                crate::html::push_js_string(&mut out.text, key);
                out.push("]");
            }
        }
    }

    /// `c[t] = { ...c[t], k1: { ...c[t].k1, k2: <op> } }`: the element is replaced, so
    /// readers of it update, while untouched elements keep their identity.
    fn clone_set(&mut self, out: &mut Code, temp: &str, tail: &[&'a str], op: &IndexOp<'a>) {
        let mut base = String::from("c[");
        base.push_str(temp);
        base.push(']');
        out.push("c[");
        out.push(temp);
        out.push("] = ");
        self.clone_level(out, &base, tail, op);
    }

    fn clone_level(&mut self, out: &mut Code, base: &str, tail: &[&'a str], op: &IndexOp<'a>) {
        let (key, rest) = tail.split_first().expect("a deep write has a tail");
        out.push("{ ...");
        out.push(base);
        out.push(", ");
        if crate::html::is_identifier_name(key) {
            out.push(key);
        } else {
            crate::html::push_js_string(&mut out.text, key);
        }
        out.push(": ");
        if rest.is_empty() {
            match op {
                IndexOp::Assign { value } => self.embed(out, value),
                IndexOp::Compound { operator, value } => {
                    out.push(base);
                    self.key_path(out, std::slice::from_ref(key));
                    out.push(" ");
                    out.push(operator);
                    out.push(" ");
                    self.embed(out, value);
                }
                IndexOp::Update { operator } => {
                    out.push(base);
                    self.key_path(out, std::slice::from_ref(key));
                    out.push(if *operator == "++" { " + 1" } else { " - 1" });
                }
            }
        } else {
            let mut nested = String::from(base);
            if crate::html::is_identifier_name(key) {
                nested.push('.');
                nested.push_str(key);
            } else {
                nested.push('[');
                crate::html::push_js_string(&mut nested, key);
                nested.push(']');
            }
            self.clone_level(out, &nested, rest, op);
        }
        out.push(" }");
    }
}
