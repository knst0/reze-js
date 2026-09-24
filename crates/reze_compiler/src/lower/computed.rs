//! Reads of computeds another module declares, inlined by the program (SPEC §16.5): `d()` →
//! `(body)`, and the import of `d` → imports of the bindings `body` reads.

use std::collections::{HashMap, HashSet};

use oxc_allocator::Vec;
use oxc_ast::ast::*;

use super::Lowerer;
use crate::analyze::computed::{ImportedName, ReadName};
use crate::ir::{BodyName, Hole, HoleKind, Specifier};

/// Local names of the bindings imported for inlined bodies, by module and export, and the ones
/// whose import specifier is already written.
#[derive(Default)]
pub struct ComputedNames<'a, 'f> {
    aliases: HashMap<(&'f str, &'f str), &'a str>,
    imported: HashSet<(&'f str, &'f str)>,
}

impl<'a, 'f> Lowerer<'a, 'f> {
    fn imported_alias(&mut self, imported: &'f ImportedName) -> &'a str {
        let key = (imported.source.as_str(), imported.export.as_str());
        if let Some(alias) = self.computed_names.aliases.get(&key) {
            return alias;
        }
        let alias = self.fresh(&imported.base);
        self.computed_names.aliases.insert(key, alias);
        alias
    }

    /// `d()` of a computed the program inlined here → `(body)`.
    pub(super) fn computed_read(&mut self, call: &CallExpression<'a>) -> Option<Hole<'a>> {
        let Expression::Identifier(callee) = &call.callee else { return None };
        let facts = self.facts;
        let read = facts.program.computed_reads.get(&callee.span.start)?;
        let mut names = self.vec();
        for reference in &read.references {
            let name = match &reference.name {
                ReadName::Local(name) => self.str(name),
                ReadName::Imported(imported) => self.imported_alias(imported),
            };
            names.push(BodyName { start: reference.start, end: reference.end, name });
        }
        let body = self.str(&read.body);
        Some(Hole { span: call.span, kind: HoleKind::ImportedComputed { body, names } })
    }

    /// The specifiers replacing the import of a computed the program inlined here: the bindings
    /// its body reads that no import in scope names, each imported once per module.
    pub(super) fn computed_import(
        &mut self,
        specifier: &ImportSpecifier<'a>,
    ) -> Option<Vec<'a, Specifier<'a>>> {
        let facts = self.facts;
        let imported = facts.program.computed_imports.get(&specifier.local.span.start)?;
        let mut specifiers = self.vec();
        for name in imported {
            if !self.computed_names.imported.insert((name.source.as_str(), name.export.as_str())) {
                continue;
            }
            let alias = self.imported_alias(name);
            specifiers.push(Specifier::Alias { name: self.str(&name.export), alias });
        }
        Some(specifiers)
    }
}
