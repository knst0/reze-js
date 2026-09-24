//! §15.16: reads of props that every program call site passes as the same literal.

use oxc_ast::ast::*;
use oxc_semantic::{AstNodes, Scoping};
use oxc_syntax::symbol::SymbolId;

use super::Facts;
use crate::diagnostic::{Code, Report};
use crate::facts::{FoldedProp, ModuleFacts};
use crate::usage::{self, Context};

pub struct FoldedValue {
    pub text: String,
    pub source: String,
    pub is_numeric: bool,
}

impl FoldedValue {
    fn of(prop: &FoldedProp) -> Self {
        FoldedValue {
            text: prop.literal.text.clone(),
            source: prop.literal.source.clone(),
            is_numeric: !prop.literal.source.starts_with('"'),
        }
    }

    pub fn is_truthy(&self) -> bool {
        if self.is_numeric { self.text != "0" } else { !self.text.is_empty() }
    }
}

enum PropsParam<'b, 'a> {
    Object(SymbolId),
    Pattern(&'b ObjectPattern<'a>),
}

fn component_param<'b, 'a>(program: &'b Program<'a>, binding: u32) -> Option<PropsParam<'b, 'a>> {
    let params = program.body.iter().find_map(|statement| {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => &export.declaration,
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    return (function.id.as_ref()?.span.start == binding)
                        .then_some(&*function.params);
                }
                _ => return None,
            },
            statement => statement.as_declaration()?,
        };
        match declaration {
            Declaration::FunctionDeclaration(function) => {
                (function.id.as_ref()?.span.start == binding).then_some(&*function.params)
            }
            Declaration::VariableDeclaration(declaration) => {
                declaration.declarations.iter().find_map(|declarator| {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else { return None };
                    if id.span.start != binding {
                        return None;
                    }
                    match declarator.init.as_ref()?.without_parentheses() {
                        Expression::ArrowFunctionExpression(arrow) => Some(&*arrow.params),
                        Expression::FunctionExpression(function) => Some(&*function.params),
                        _ => None,
                    }
                })
            }
            _ => None,
        }
    })?;
    match &params.items.first()?.pattern {
        BindingPattern::BindingIdentifier(id) => Some(PropsParam::Object(id.symbol_id())),
        BindingPattern::ObjectPattern(pattern) => Some(PropsParam::Pattern(pattern)),
        _ => None,
    }
}

fn property_symbol<'b>(property: &'b BindingProperty<'_>) -> Option<(&'b str, SymbolId)> {
    if property.computed {
        return None;
    }
    let key = match &property.key {
        PropertyKey::StaticIdentifier(id) => id.name.as_str(),
        PropertyKey::StringLiteral(s) => s.value.as_str(),
        _ => return None,
    };
    let binding = match &property.value {
        BindingPattern::BindingIdentifier(id) => id,
        BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
            BindingPattern::BindingIdentifier(id) => id,
            _ => return None,
        },
        _ => return None,
    };
    Some((key, binding.symbol_id()))
}

/// Records every read of a folded prop: `props.k` read as a value, or a destructured binding the
/// props rewrite (§15.7) turned into such a read.
pub fn fold<'a>(
    facts: &mut Facts,
    program: &Program<'a>,
    scoping: &Scoping,
    nodes: &AstNodes<'a>,
    module_facts: &ModuleFacts,
    reports: &mut Vec<Report>,
) {
    for prop in &module_facts.folded_props {
        let Some(param) = component_param(program, prop.component) else { continue };
        let mut first_read = None;
        match param {
            PropsParam::Object(symbol) => {
                for &reference in scoping.get_resolved_reference_ids(symbol) {
                    let Some(access) = usage::classify(reference, scoping, nodes) else { continue };
                    if access.context == Context::Read
                        && access.tail.is_empty()
                        && access.keys == [prop.key.as_str()]
                    {
                        facts.folded_prop_members.insert(access.span.start, FoldedValue::of(prop));
                        first_read.get_or_insert(access.span);
                    }
                }
            }
            PropsParam::Pattern(pattern) => {
                let Some(symbol) = pattern
                    .properties
                    .iter()
                    .filter_map(property_symbol)
                    .find_map(|(key, symbol)| (key == prop.key).then_some(symbol))
                else {
                    continue;
                };
                for &reference in scoping.get_resolved_reference_ids(symbol) {
                    if facts.props.rewrites(reference) {
                        facts.folded_prop_refs.insert(reference, FoldedValue::of(prop));
                        let node = scoping.get_reference(reference).node_id();
                        first_read.get_or_insert(oxc_span::GetSpan::span(&nodes.kind(node)));
                    }
                }
            }
        }
        let Some(span) = first_read else { continue };
        let mut report = Report::new(
            Code::PropFolded,
            span,
            format!(
                "Every call site passes `{}={}`, so reads of `{}` compiled to that literal.",
                prop.key, prop.literal.source, prop.key
            ),
        )
        .data("prop", prop.key.clone());
        for related in &prop.related {
            report = report.related(related.clone());
        }
        reports.push(report);
    }
}
