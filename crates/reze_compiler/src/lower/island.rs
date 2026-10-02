use std::collections::HashMap;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_span::Span;
use oxc_syntax::symbol::SymbolId;

use super::Lowerer;
use crate::analyze::exported_symbols;
use crate::diagnostic::{Code, Report};
use crate::html::decode_entities;
use crate::ir::{Island, IslandLoader, IslandTrigger, Prop, PropValue, Props, PropsPart, Render};

pub const ISLAND: &str = "island";
pub const ISLAND_MEDIA: &str = "islandMedia";
pub const ISLAND_ROOT_MARGIN: &str = "islandRootMargin";
pub const ISLAND_FALLBACK: &str = "islandFallback";

pub fn is_island_attr(name: &str) -> bool {
    matches!(name, "island" | "islandMedia" | "islandRootMargin" | "islandFallback")
}

fn attr_name<'a>(a: &JSXAttribute<'a>) -> Option<&'a str> {
    match &a.name {
        JSXAttributeName::Identifier(id) => Some(id.name.as_str()),
        JSXAttributeName::NamespacedName(_) => None,
    }
}

#[derive(Clone)]
enum ImportBase {
    Named(String),
    Default,
    Namespace,
}

struct ImportUse {
    request: Span,
    base: ImportBase,
    decl: Span,
    single: bool,
    exported: bool,
    total_refs: usize,
}

#[derive(Default)]
pub struct IslandPlan {
    imports: HashMap<SymbolId, ImportUse>,
    uses: HashMap<SymbolId, u32>,
    pruned: HashMap<u32, Span>,
}
enum Decision<'p> {
    Split(&'p ImportUse),
    Warn(&'static str),
    Direct,
}

impl IslandPlan {
    pub fn scan<'a>(program: &Program<'a>, scoping: &Scoping) -> Self {
        let mut plan =
            Self { imports: HashMap::new(), uses: HashMap::new(), pruned: HashMap::new() };
        let mut imports = Imports { plan: &mut plan, scoping };
        imports.visit_program(program);
        let exported = exported_symbols(program, scoping);
        for (symbol, entry) in plan.imports.iter_mut() {
            entry.exported = exported.contains(symbol);
            entry.total_refs = scoping.get_resolved_references(*symbol).count();
            let uses = plan.uses.get(symbol).copied().unwrap_or(0) as usize;
            if !entry.exported && entry.single && uses == entry.total_refs && uses > 0 {
                plan.pruned.insert(entry.decl.start, entry.decl);
            }
        }
        plan
    }

    pub fn prune_span(&self, decl_start: u32) -> Option<Span> {
        self.pruned.get(&decl_start).copied()
    }

    fn decide(&self, symbol: SymbolId) -> Decision<'_> {
        let Some(entry) = self.imports.get(&symbol) else { return Decision::Direct };
        if entry.exported {
            return Decision::Warn("exported");
        }
        let uses = self.uses.get(&symbol).copied().unwrap_or(0) as usize;
        if uses < entry.total_refs {
            return Decision::Warn("used outside islands");
        }
        if !entry.single {
            return Decision::Warn("sharing its import with other names");
        }
        Decision::Split(entry)
    }
}

struct Imports<'p> {
    plan: &'p mut IslandPlan,
    scoping: &'p Scoping,
}

impl<'a> Visit<'a> for Imports<'_> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if matches!(it.import_kind, ImportOrExportKind::Type) {
            return;
        }
        let Some(specifiers) = &it.specifiers else { return };
        let single = specifiers.len() == 1;
        for specifier in specifiers {
            let (local, base) = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                    let name = match &spec.imported {
                        ModuleExportName::IdentifierName(name) => name.name.as_str(),
                        ModuleExportName::IdentifierReference(name) => name.name.as_str(),
                        ModuleExportName::StringLiteral(_) => return,
                    };
                    (&spec.local, ImportBase::Named(String::from(name)))
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                    (&spec.local, ImportBase::Default)
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                    (&spec.local, ImportBase::Namespace)
                }
            };
            let symbol = local.symbol_id();
            if self.plan.imports.contains_key(&symbol) {
                continue;
            }
            self.plan.imports.insert(
                symbol,
                ImportUse {
                    request: it.source.span,
                    base,
                    decl: it.span,
                    single,
                    exported: false,
                    total_refs: 0,
                },
            );
        }
        walk::walk_import_declaration(self, it);
    }

    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if has_island(&it.opening_element.attributes)
            && let Some(symbol) = self.callee_symbol(&it.opening_element.name)
        {
            *self.plan.uses.entry(symbol).or_insert(0) += 1;
        }
        walk::walk_jsx_element(self, it);
    }
}
fn member_property<'a>(property: &JSXIdentifier<'a>) -> Option<&'a str> {
    Some(property.name.as_str())
}

fn member_path<'a>(name: &JSXElementName<'a>) -> Option<std::vec::Vec<&'a str>> {
    let mut reversed = std::vec::Vec::new();
    let mut object = match name {
        JSXElementName::IdentifierReference(_) => return Some(std::vec::Vec::new()),
        JSXElementName::MemberExpression(member) => {
            reversed.push(member_property(&member.property)?);
            &member.object
        }
        _ => return None,
    };
    loop {
        match object {
            JSXMemberExpressionObject::IdentifierReference(_) => {
                reversed.reverse();
                return Some(reversed);
            }
            JSXMemberExpressionObject::MemberExpression(member) => {
                reversed.push(member_property(&member.property)?);
                object = &member.object;
            }
            JSXMemberExpressionObject::ThisExpression(_) => return None,
        }
    }
}

impl Imports<'_> {
    fn callee_symbol(&self, name: &JSXElementName<'_>) -> Option<SymbolId> {
        let root = match name {
            JSXElementName::IdentifierReference(id) => id.as_ref(),
            JSXElementName::MemberExpression(member) => root_object(&member.object)?,
            _ => return None,
        };
        let reference = root.reference_id.get()?;
        self.scoping.get_reference(reference).symbol_id()
    }
}

fn root_object<'a, 'b>(
    object: &'b JSXMemberExpressionObject<'a>,
) -> Option<&'b IdentifierReference<'a>> {
    match object {
        JSXMemberExpressionObject::IdentifierReference(id) => Some(id),
        JSXMemberExpressionObject::MemberExpression(member) => root_object(&member.object),
        JSXMemberExpressionObject::ThisExpression(_) => None,
    }
}

fn has_island(attributes: &[JSXAttributeItem<'_>]) -> bool {
    attributes.iter().any(|item| match item {
        JSXAttributeItem::Attribute(a) => attr_name(a) == Some(ISLAND),
        JSXAttributeItem::SpreadAttribute(_) => false,
    })
}

pub(super) struct IslandAttributes<'b, 'a> {
    pub present: bool,
    island: Option<&'b JSXAttribute<'a>>,
    media: Option<&'b JSXAttribute<'a>>,
    root_margin: Option<&'b JSXAttribute<'a>>,
    fallback: Option<&'b JSXAttribute<'a>>,
}

impl<'a, 'f> Lowerer<'a, 'f> {
    pub(super) fn island_attributes<'b>(&self, el: &'b JSXElement<'a>) -> IslandAttributes<'b, 'a> {
        let mut found = IslandAttributes {
            present: false,
            island: None,
            media: None,
            root_margin: None,
            fallback: None,
        };
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(a) = item else { continue };
            match attr_name(a) {
                Some(ISLAND) => {
                    found.present = true;
                    found.island = Some(a);
                }
                Some(ISLAND_MEDIA) => found.media = Some(a),
                Some(ISLAND_ROOT_MARGIN) => found.root_margin = Some(a),
                Some(ISLAND_FALLBACK) => found.fallback = Some(a),
                _ => {}
            }
        }
        found
    }

    pub(super) fn island(
        &mut self,
        el: &JSXElement<'a>,
        found: &IslandAttributes<'_, 'a>,
        callee: Span,
        tag: &str,
    ) -> Option<Island<'a>> {
        let trigger = self.island_trigger(found)?;
        let media = found.media.and_then(|a| self.island_string(a));
        let root_margin = found.root_margin.and_then(|a| self.island_string(a));
        if trigger == IslandTrigger::Media && media.is_none() {
            let span = found.island.map_or(el.opening_element.span, |a| a.span);
            self.report(Report::new(Code::IslandMediaMissing, span));
            return None;
        }
        let fallback = found.fallback.and_then(|a| self.island_fallback(a));
        let loader = self.island_loader(&el.opening_element.name, callee, tag);
        Some(Island { trigger, media, root_margin, loader, fallback })
    }

    fn island_trigger(&mut self, found: &IslandAttributes<'_, 'a>) -> Option<IslandTrigger> {
        let a = found.island?;
        match &a.value {
            None => Some(IslandTrigger::Eager),
            Some(JSXAttributeValue::StringLiteral(s)) => {
                self.island_trigger_name(&decode_entities(s.value.as_str()), a.span)
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(Expression::StringLiteral(s)) => {
                    self.island_trigger_name(s.value.as_str(), a.span)
                }
                Some(Expression::BooleanLiteral(b)) if b.value => Some(IslandTrigger::Eager),
                _ => {
                    self.report(
                        Report::new(Code::IslandTrigger, a.span).arg("value", self.text(a.span)),
                    );
                    None
                }
            },
            Some(_) => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span).arg("value", self.text(a.span)),
                );
                None
            }
        }
    }

    fn island_trigger_name(&mut self, name: &str, span: Span) -> Option<IslandTrigger> {
        match name {
            "eager" => Some(IslandTrigger::Eager),
            "idle" => Some(IslandTrigger::Idle),
            "visible" => Some(IslandTrigger::Visible),
            "media" => Some(IslandTrigger::Media),
            "interaction" => Some(IslandTrigger::Interaction),
            _ => {
                self.report(Report::new(Code::IslandTrigger, span).arg("value", name));
                None
            }
        }
    }

    fn island_string(&mut self, a: &JSXAttribute<'a>) -> Option<&'a str> {
        match &a.value {
            None => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span).arg("value", self.text(a.span)),
                );
                None
            }
            Some(JSXAttributeValue::StringLiteral(s)) => {
                Some(self.str(&decode_entities(s.value.as_str())))
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(Expression::StringLiteral(s)) => Some(self.str(s.value.as_str())),
                _ => {
                    self.report(
                        Report::new(Code::IslandTrigger, a.span).arg("value", self.text(a.span)),
                    );
                    None
                }
            },
            Some(_) => {
                self.report(
                    Report::new(Code::IslandTrigger, a.span).arg("value", self.text(a.span)),
                );
                None
            }
        }
    }

    fn island_fallback(&mut self, a: &JSXAttribute<'a>) -> Option<Render<'a>> {
        self.fallback(&[("fallback", a)])
    }

    fn island_loader(
        &mut self,
        name: &JSXElementName<'a>,
        callee: Span,
        tag: &str,
    ) -> IslandLoader<'a> {
        let root = match name {
            JSXElementName::IdentifierReference(id) => id.as_ref(),
            JSXElementName::MemberExpression(member) => match root_object(&member.object) {
                Some(root) => root,
                None => return IslandLoader::Direct,
            },
            _ => return IslandLoader::Direct,
        };
        let Some(reference) = root.reference_id.get() else { return IslandLoader::Direct };
        let Some(symbol) = self.analysis.scoping.get_reference(reference).symbol_id() else {
            return IslandLoader::Direct;
        };
        let (request, base) = match self.islands.decide(symbol) {
            Decision::Split(entry) => (entry.request, entry.base.clone()),
            Decision::Warn(reason) => {
                self.report(
                    Report::new(Code::IslandNotSplit, callee)
                        .arg("component", tag)
                        .arg("reason", reason),
                );
                return IslandLoader::Direct;
            }
            Decision::Direct => return IslandLoader::Direct,
        };
        let mut path = self.vec();
        match &base {
            ImportBase::Named(name) => path.push(self.str(name)),
            ImportBase::Default => path.push("default"),
            ImportBase::Namespace => {}
        }
        if let Some(members) = member_path(name) {
            for member in members {
                path.push(self.str(member));
            }
        }
        IslandLoader::Split { source: request, path }
    }

    pub(super) fn island_orphans(&mut self, found: &IslandAttributes<'_, 'a>) {
        for (name, attr) in [
            (ISLAND_MEDIA, found.media),
            (ISLAND_ROOT_MARGIN, found.root_margin),
            (ISLAND_FALLBACK, found.fallback),
        ] {
            if let Some(a) = attr {
                self.report(Report::new(Code::IslandOrphan, a.span).arg("attribute", name));
            }
        }
    }
}

pub fn defer_props(props: &mut Props<'_>) {
    for part in props.parts.iter_mut() {
        match part {
            PropsPart::Object(entries) => {
                for entry in entries.iter_mut() {
                    if let Prop::Value { key, value } = entry {
                        let deferred = matches!(value, PropValue::Expr(_) | PropValue::Children(_));
                        if deferred {
                            let key = *key;
                            let value = std::mem::replace(value, PropValue::True);
                            *entry = Prop::Getter { key, value };
                        }
                    }
                }
            }
            PropsPart::Spread { is_dynamic, .. } => *is_dynamic = true,
        }
    }
}
