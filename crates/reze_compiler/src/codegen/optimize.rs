use std::collections::{BTreeMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::Scoping;
use oxc_span::SPAN;
use oxc_str::{Ident, Str};
use oxc_syntax::symbol::SymbolId;

use crate::RUNTIME_MODULE;
use crate::ast::Ast;
use crate::exports::{self, Entry};

use super::EmitContext;

pub(super) fn hoist_templates<'a>(ctx: &mut EmitContext<'a, '_>, program: &mut Program<'a>) {
    let aliases = ["template", "templateSVG", "templateMathML"]
        .map(|name| ctx.helpers.alias(RUNTIME_MODULE, name));
    if aliases.iter().all(Option::is_none) {
        return;
    }
    let mut templates = Templates { aliases, counts: BTreeMap::new() };
    templates.visit_program(program);
    let mut names = BTreeMap::new();
    let ast = Ast::new(ctx.allocator);
    for (source, count) in templates.counts {
        if count < 2 {
            continue;
        }
        let literal_bytes = source.len() + 2;
        if (count - 1) * literal_bytes <= (count + 1) * 5 + 12 {
            continue;
        }
        let name = ctx.fresh("_tpl$");
        if (count - 1) * literal_bytes <= (count + 1) * name.len() + 12 {
            continue;
        }
        let value = Expression::new_string_literal(SPAN, Str::from(source), None, &ast.builder);
        ctx.hoisted.push(ast.declaration(VariableDeclarationKind::Const, name, Some(value)));
        names.insert(source, name);
    }
    if !names.is_empty() {
        TemplateReferences { aliases, names, ast }.visit_program(program);
    }
}

struct Templates<'a> {
    aliases: [Option<&'a str>; 3],
    counts: BTreeMap<&'a str, usize>,
}

fn template_source<'b, 'a>(
    call: &'b CallExpression<'a>,
    aliases: &[Option<&str>; 3],
) -> Option<&'b StringLiteral<'a>> {
    let Expression::Identifier(callee) = &call.callee else { return None };
    if !aliases.contains(&Some(callee.name.as_str())) {
        return None;
    }
    let [Argument::StringLiteral(source)] = call.arguments.as_slice() else { return None };
    (source.span == SPAN).then_some(source)
}

impl<'a> Visit<'a> for Templates<'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(source) = template_source(call, &self.aliases) {
            *self.counts.entry(source.value.as_str()).or_default() += 1;
        }
        walk::walk_call_expression(self, call);
    }
}

struct TemplateReferences<'a> {
    aliases: [Option<&'a str>; 3],
    names: BTreeMap<&'a str, &'a str>,
    ast: Ast<'a>,
}

impl<'a> VisitMut<'a> for TemplateReferences<'a> {
    fn visit_call_expression(&mut self, call: &mut CallExpression<'a>) {
        walk_mut::walk_call_expression(self, call);
        if let Some(source) = template_source(call, &self.aliases)
            && let Some(name) = self.names.get(source.value.as_str())
        {
            call.arguments[0] = Argument::from(self.ast.ident(name));
        }
    }
}

pub(super) fn prune_imports(program: &mut Program<'_>, scoping: &Scoping) -> bool {
    let mut references = References { scoping, symbols: HashSet::new(), generated: HashSet::new() };
    references.visit_program(program);
    let mut changed = false;
    program.body.retain_mut(|statement| {
        let Statement::ImportDeclaration(import) = statement else { return true };
        let Some(source) = exports::module(import.source.value.as_str()) else { return true };
        if import.import_kind.is_type() {
            return true;
        }
        let Some(specifiers) = &mut import.specifiers else { return true };
        let before = specifiers.len();
        specifiers.retain(|specifier| {
            let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else { return true };
            named.import_kind.is_type()
                || !exports::lookup(source, named.imported.name().as_str())
                    .is_some_and(Entry::is_compiled_away)
                || named
                    .local
                    .symbol_id
                    .get()
                    .is_some_and(|symbol| references.symbols.contains(&symbol))
                || references.generated.contains(&named.local.name)
        });
        changed |= before != specifiers.len();
        before == 0 || !specifiers.is_empty()
    });
    changed
}

struct References<'a, 's> {
    scoping: &'s Scoping,
    symbols: HashSet<SymbolId>,
    generated: HashSet<Ident<'a>>,
}

impl<'a> Visit<'a> for References<'a, '_> {
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if let Some(reference) = identifier.reference_id.get() {
            if let Some(symbol) = self.scoping.get_reference(reference).symbol_id() {
                self.symbols.insert(symbol);
            }
        } else {
            self.generated.insert(identifier.name.clone());
        }
        walk::walk_identifier_reference(self, identifier);
    }
}
