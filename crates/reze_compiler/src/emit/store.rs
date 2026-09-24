//! Unproxied stores (SPEC §15.6): leaf signals, setter drafts and replaced specifiers.

use super::{Emitter, Helper};
use crate::code::Code;
use crate::ir::{Specifier, StoreLeaf, StoreWriteKind};

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
