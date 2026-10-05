use std::collections::BTreeMap;

use oxc_allocator::{Allocator, ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::SPAN;
use oxc_str::{Ident, Str};

use crate::frontend::Namer;

#[derive(Default)]
pub struct HelperImports<'a> {
    aliases: BTreeMap<&'a str, BTreeMap<&'a str, &'a str>>,
    pending: BTreeMap<&'a str, Vec<(&'a str, &'a str)>>,
}

impl<'a> HelperImports<'a> {
    pub fn alias(&self, source: &str, export: &str) -> Option<&'a str> {
        self.aliases.get(source)?.get(export).copied()
    }

    pub fn require(
        &mut self,
        allocator: &'a Allocator,
        namer: &mut Namer<'a>,
        source: &str,
        export: &str,
    ) -> &'a str {
        if let Some(alias) = self.aliases.get(source).and_then(|exports| exports.get(export)) {
            return alias;
        }
        let source = match self.aliases.get_key_value(source) {
            Some((&source, _)) => source,
            None => allocator.alloc_str(source),
        };
        let export = allocator.alloc_str(export);
        let alias = allocator.alloc_str(&namer.fresh(&format!("_${export}")));
        self.aliases.entry(source).or_default().insert(export, alias);
        self.pending.entry(source).or_default().push((export, alias));
        alias
    }

    pub fn install(&mut self, allocator: &'a Allocator, program: &mut Program<'a>) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let builder = AstBuilder::new(allocator);
        let mut declarations = ArenaVec::new_in(&builder);
        let mut pending = std::mem::take(&mut self.pending);
        let facade = pending.remove_entry("reze-js");
        for (source, bindings) in facade.into_iter().chain(pending) {
            let mut specs = ArenaVec::with_capacity_in(bindings.len(), &builder);
            for (export, alias) in bindings {
                let imported =
                    ModuleExportName::new_identifier_name(SPAN, Ident::from(export), &builder);
                let local = BindingIdentifier::new(SPAN, Ident::from(alias), &builder);
                specs.push(ImportDeclarationSpecifier::new_import_specifier(
                    SPAN,
                    imported,
                    local,
                    ImportOrExportKind::Value,
                    &builder,
                ));
            }
            let existing = program.body.iter_mut().find_map(|statement| {
                let Statement::ImportDeclaration(decl) = statement else { return None };
                if decl.source.value.as_str() != source
                    || decl.import_kind != ImportOrExportKind::Value
                {
                    return None;
                }
                if decl.specifiers.as_ref().is_some_and(|specs| {
                    specs.iter().any(|specifier| {
                        matches!(specifier, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_))
                    })
                }) {
                    return None;
                }
                Some(decl)
            });
            if let Some(decl) = existing {
                decl.specifiers.get_or_insert_with(|| ArenaVec::new_in(&builder)).extend(specs);
            } else {
                let source = StringLiteral::new(SPAN, Str::from(source), None, &builder);
                declarations.push(Statement::ImportDeclaration(ImportDeclaration::boxed(
                    SPAN,
                    Some(specs),
                    source,
                    None,
                    None,
                    ImportOrExportKind::Value,
                    &builder,
                )));
            }
        }
        if !declarations.is_empty() {
            let insert_at = program
                .body
                .iter()
                .rposition(|statement| matches!(statement, Statement::ImportDeclaration(_)))
                .map_or(0, |index| index + 1);
            let mut body =
                ArenaVec::with_capacity_in(program.body.len() + declarations.len(), &builder);
            let old = std::mem::replace(&mut program.body, ArenaVec::new_in(&builder));
            let mut old = old.into_iter();
            body.extend(old.by_ref().take(insert_at));
            body.extend(declarations);
            body.extend(old);
            program.body = body;
        }
        true
    }
}
