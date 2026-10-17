use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolFlags};
use oxc_syntax::symbol::SymbolId;

pub enum Unresolved {
    Imported(String),
    NotConstant(String),
    Expression,
}

pub struct StringConstants {
    values: HashMap<SymbolId, String>,
}

impl StringConstants {
    pub fn scan(program: &Program<'_>, scoping: &Scoping) -> Self {
        let mut collector = Declarators { scoping, values: HashMap::new() };
        collector.visit_program(program);
        Self { values: collector.values }
    }

    pub fn resolve(&self, scoping: &Scoping, e: &Expression<'_>) -> Result<String, Unresolved> {
        fold(e, &mut |id| {
            let name = id.name.to_string();
            let Some(symbol) = resolved(scoping, id) else {
                return Err(Unresolved::NotConstant(name));
            };
            if let Some(value) = self.values.get(&symbol) {
                Ok(value.clone())
            } else if scoping.symbol_flags(symbol).contains(SymbolFlags::Import) {
                Err(Unresolved::Imported(name))
            } else {
                Err(Unresolved::NotConstant(name))
            }
        })
    }
}

fn fold(
    e: &Expression<'_>,
    identifier: &mut impl FnMut(&IdentifierReference<'_>) -> Result<String, Unresolved>,
) -> Result<String, Unresolved> {
    match e.get_inner_expression() {
        Expression::StringLiteral(s) => Ok(String::from(s.value.as_str())),
        Expression::TemplateLiteral(template) => {
            let mut text = String::new();
            for (index, quasi) in template.quasis.iter().enumerate() {
                text.push_str(quasi.value.cooked.as_ref().ok_or(Unresolved::Expression)?);
                if let Some(part) = template.expressions.get(index) {
                    text.push_str(&fold(part, identifier)?);
                }
            }
            Ok(text)
        }
        Expression::Identifier(id) => identifier(id),
        _ => Err(Unresolved::Expression),
    }
}

fn resolved(scoping: &Scoping, id: &IdentifierReference<'_>) -> Option<SymbolId> {
    scoping.get_reference(id.reference_id.get()?).symbol_id()
}

struct Declarators<'s> {
    scoping: &'s Scoping,
    values: HashMap<SymbolId, String>,
}

impl<'a> Visit<'a> for Declarators<'_> {
    fn visit_variable_declaration(&mut self, variables: &VariableDeclaration<'a>) {
        if variables.kind == VariableDeclarationKind::Const {
            for declarator in &variables.declarations {
                let (BindingPattern::BindingIdentifier(id), Some(init)) =
                    (&declarator.id, &declarator.init)
                else {
                    continue;
                };
                let symbol = id.symbol_id();
                if self.scoping.symbol_is_mutated(symbol)
                    || !self.scoping.symbol_redeclarations(symbol).is_empty()
                {
                    continue;
                }
                let scoping = self.scoping;
                let values = &self.values;
                let folded = fold(init, &mut |id| {
                    resolved(scoping, id)
                        .and_then(|inner| values.get(&inner).cloned())
                        .ok_or(Unresolved::Expression)
                });
                if let Ok(value) = folded {
                    self.values.insert(symbol, value);
                }
            }
        }
        walk::walk_variable_declaration(self, variables);
    }
}
