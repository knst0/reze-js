use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingIdentifier, BindingPattern, Expression, IdentifierReference, JSXElementName, Program,
    VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::symbol::{SymbolFlags, SymbolId};

use crate::frontend::analysis::{Intrinsic, Primitive};
use crate::frontend::imports::{Syntax, SyntaxImports};
use crate::frontend::{self, Namer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Signal,
    Computed,
    Action,
    Store,
    Plain,
    Imported,
    Intrinsic(&'static str),
}

impl BindingKind {
    pub fn label(&self) -> String {
        match self {
            BindingKind::Signal => String::from("signal"),
            BindingKind::Computed => String::from("computed"),
            BindingKind::Action => String::from("action"),
            BindingKind::Store => String::from("store"),
            BindingKind::Plain => String::from("plain binding"),
            BindingKind::Imported => String::from("imported binding"),
            BindingKind::Intrinsic(name) => format!("intrinsic `{name}`"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub name: String,
    pub kind: BindingKind,
    pub declaration_start: u32,
    pub declaration_end: u32,
    pub use_start: u32,
    pub use_end: u32,
    pub lowered: String,
}

/// Inspection at byte `offset` in `source`, with all spans as byte offsets in the original
/// source. Returns prescan-level results (signal/computed/action/store/intrinsic kind,
/// declaration and lowered form) even when the file has error diagnostics, as long as it
/// parses; `None` when no binding covers `offset` or the source cannot be parsed.
pub fn inspect(source: &str, filename: &str, offset: u32) -> Option<Inspection> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let program: &Program<'_> = allocator.alloc(parsed.program);
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(program);
    let (scoping, nodes) = semantic.semantic.into_scoping_and_nodes();
    let syntax = frontend::imports::SyntaxImports::collect(program);
    let mut namer = Namer::seeded(&scoping, &allocator);
    let mut reports = Vec::new();
    let pre = frontend::dsl::prescan(program, &scoping, &syntax, &mut namer, &mut reports);
    let facts = frontend::analysis::collect(program, &scoping, &nodes, &mut reports);
    drop(nodes);
    let mut visitor = Visitor {
        scoping: &scoping,
        syntax: &syntax,
        pre: &pre,
        facts: &facts,
        bindings: HashMap::new(),
        hits: Vec::new(),
        jsx_hits: Vec::new(),
    };
    visitor.visit_program(program);
    visitor.at(offset as usize)
}

struct RawBinding {
    kind: BindingKind,
    setter: Option<String>,
}

struct Hit {
    span: Span,
    symbol: Option<SymbolId>,
    intrinsic: Option<Intrinsic>,
}

struct Visitor<'s, 'x, 'p, 'f> {
    scoping: &'s Scoping,
    syntax: &'x SyntaxImports,
    pre: &'p frontend::dsl::PreScan,
    facts: &'f frontend::SharedFacts,
    bindings: HashMap<SymbolId, RawBinding>,
    hits: Vec<Hit>,
    jsx_hits: Vec<Hit>,
}

impl Visitor<'_, '_, '_, '_> {
    fn at(self, offset: usize) -> Option<Inspection> {
        if let Some(hit) = self
            .jsx_hits
            .iter()
            .find(|hit| contains(hit.span, offset))
            .filter(|hit| hit.intrinsic.is_some())
        {
            let intrinsic = hit.intrinsic.expect("filtered intrinsic");
            let (declaration_start, declaration_end) = hit
                .symbol
                .map(|symbol| {
                    let span = self.scoping.symbol_span(symbol);
                    (span.start, span.end)
                })
                .unwrap_or((hit.span.start, hit.span.end));
            return Some(Inspection {
                name: intrinsic.name().to_string(),
                kind: BindingKind::Intrinsic(intrinsic.name()),
                declaration_start,
                declaration_end,
                use_start: hit.span.start,
                use_end: hit.span.end,
                lowered: intrinsic_lowered(intrinsic),
            });
        }
        let hit = self
            .hits
            .iter()
            .chain(self.jsx_hits.iter())
            .filter(|hit| contains(hit.span, offset))
            .min_by_key(|hit| hit.span.end.saturating_sub(hit.span.start))?;
        let symbol = hit.symbol?;
        let declaration = self.scoping.symbol_span(symbol);
        let name = self.scoping.symbol_name(symbol).to_string();
        let kind = self.kind_of(symbol);
        let lowered = self.lowered(symbol, &name, &kind);
        Some(Inspection {
            name,
            kind,
            declaration_start: declaration.start,
            declaration_end: declaration.end,
            use_start: hit.span.start,
            use_end: hit.span.end,
            lowered,
        })
    }

    fn kind_of(&self, symbol: SymbolId) -> BindingKind {
        if let Some(raw) = self.bindings.get(&symbol) {
            return raw.kind.clone();
        }
        if let Some(start) = self.pre.by_symbol.get(&symbol)
            && let Some(plan) = self.pre.decls.get(start)
        {
            return syntax_kind(plan.primitive);
        }
        if self.facts.signal_setters.contains_key(&symbol)
            || self.facts.signal_setters.values().any(|setter| *setter == Some(symbol))
        {
            return BindingKind::Signal;
        }
        if self.facts.computed_getters.contains(&symbol) {
            return BindingKind::Computed;
        }
        if let Some(primitive) = self.facts.named.get(&symbol) {
            match primitive {
                Primitive::Intrinsic(intrinsic) => {
                    return BindingKind::Intrinsic(intrinsic.name());
                }
                Primitive::Signal => return BindingKind::Signal,
                Primitive::Computed => return BindingKind::Computed,
                Primitive::Action => return BindingKind::Action,
                _ => {}
            }
        }
        if self.scoping.symbol_flags(symbol).contains(SymbolFlags::Import) {
            return BindingKind::Imported;
        }
        BindingKind::Plain
    }

    fn lowered(&self, symbol: SymbolId, name: &str, kind: &BindingKind) -> String {
        match kind {
            BindingKind::Signal => {
                let setter = self
                    .bindings
                    .get(&symbol)
                    .and_then(|raw| raw.setter.clone())
                    .or_else(|| {
                        self.pre
                            .by_symbol
                            .get(&symbol)
                            .and_then(|start| self.pre.decls.get(start))
                            .and_then(|plan| plan.setter.clone())
                    })
                    .or_else(|| {
                        self.facts
                            .signal_setters
                            .get(&symbol)
                            .and_then(|setter| *setter)
                            .map(|setter| self.scoping.symbol_name(setter).to_string())
                    });
                match setter {
                    Some(setter) => format!(
                        "const [{name}, {setter}] = signal(\u{2026}); reads `{name}()`, writes `{setter}(v)`"
                    ),
                    None => {
                        format!("const [{name}] = signal(\u{2026}); reads `{name}()`")
                    }
                }
            }
            BindingKind::Computed => {
                format!("const {name} = computed(() => \u{2026}); reads `{name}()`")
            }
            BindingKind::Action => {
                format!(
                    "const {name} = action((\u{2026}) => \u{2026}); calls `{name}(...)` run as an action"
                )
            }
            BindingKind::Store => {
                format!(
                    "const {name} = store(\u{2026}); member reads `{name}.prop` stay plain property reads"
                )
            }
            BindingKind::Plain => format!("plain binding `{name}`; no reactive lowering"),
            BindingKind::Imported => format!("imported binding `{name}`"),
            BindingKind::Intrinsic(intrinsic_name) => {
                let intrinsic = match *intrinsic_name {
                    "Show" => Intrinsic::Show,
                    "For" => Intrinsic::For,
                    "Repeat" => Intrinsic::Repeat,
                    "Switch" => Intrinsic::Switch,
                    "Match" => Intrinsic::Match,
                    "Portal" => Intrinsic::Portal,
                    "Loading" => Intrinsic::Loading,
                    "Errored" => Intrinsic::Errored,
                    _ => Intrinsic::Head,
                };
                intrinsic_lowered(intrinsic)
            }
        }
    }

    fn record(&mut self, symbol: SymbolId, kind: BindingKind, setter: Option<String>) {
        self.bindings.insert(symbol, RawBinding { kind, setter });
    }
}

fn contains(span: Span, offset: usize) -> bool {
    (span.start as usize) <= offset && offset < (span.end as usize).max(span.start as usize + 1)
}

fn syntax_kind(syntax: Syntax) -> BindingKind {
    match syntax {
        Syntax::Signal => BindingKind::Signal,
        Syntax::Computed => BindingKind::Computed,
        Syntax::Action => BindingKind::Action,
    }
}

fn intrinsic_lowered(intrinsic: Intrinsic) -> String {
    let runtime = match intrinsic {
        Intrinsic::Show => "branch",
        Intrinsic::For => "list",
        Intrinsic::Repeat => "repeat",
        Intrinsic::Switch | Intrinsic::Match => "choose",
        Intrinsic::Portal => "portal",
        Intrinsic::Head => "head",
        Intrinsic::Loading => "loading",
        Intrinsic::Errored => "errored",
    };
    format!("<{}> lowers to `{runtime}(...)`", intrinsic.name())
}

fn syntax_of_callee(
    scoping: &Scoping,
    syntax: &SyntaxImports,
    callee: &Expression<'_>,
) -> Option<Syntax> {
    match callee.without_parentheses() {
        Expression::Identifier(id) => id
            .reference_id
            .get()
            .and_then(|reference| scoping.get_reference(reference).symbol_id())
            .and_then(|symbol| syntax.syntax_of(symbol)),
        Expression::StaticMemberExpression(member) if !member.optional => {
            let primitive = [Syntax::Signal, Syntax::Computed, Syntax::Action]
                .into_iter()
                .find(|candidate| candidate.name() == member.property.name.as_str())?;
            let Expression::Identifier(object) = &member.object else { return None };
            let symbol = object
                .reference_id
                .get()
                .and_then(|reference| scoping.get_reference(reference).symbol_id())?;
            let source = syntax.namespaces.get(&symbol)?;
            (crate::exports::syntax_named(source, primitive.name()) == Some(primitive))
                .then_some(primitive)
        }
        _ => None,
    }
}

impl<'a> Visit<'a> for Visitor<'_, '_, '_, '_> {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let Some(init) = &it.init
            && let Expression::CallExpression(call) = init.without_parentheses()
        {
            let kind = syntax_of_callee(self.scoping, self.syntax, &call.callee)
                .map(syntax_kind)
                .or_else(|| match self.facts.primitive(self.scoping, &call.callee) {
                    Some(Primitive::Signal) => Some(BindingKind::Signal),
                    Some(Primitive::Computed) => Some(BindingKind::Computed),
                    Some(Primitive::Action) => Some(BindingKind::Action),
                    _ => None,
                });
            match kind {
                Some(BindingKind::Signal) => match &it.id {
                    BindingPattern::BindingIdentifier(id) => {
                        let setter =
                            self.pre.decls.get(&it.span.start).and_then(|plan| plan.setter.clone());
                        self.record(id.symbol_id(), BindingKind::Signal, setter);
                    }
                    BindingPattern::ArrayPattern(pattern) => {
                        let binding = |index: usize| match pattern.elements.get(index) {
                            Some(Some(BindingPattern::BindingIdentifier(id))) => {
                                Some((id.symbol_id(), id.name.to_string()))
                            }
                            _ => None,
                        };
                        if let Some((getter, _)) = binding(0) {
                            let setter = binding(1).map(|(_, name)| name);
                            self.record(getter, BindingKind::Signal, setter);
                        }
                        if let Some((setter, setter_name)) = binding(1) {
                            self.record(setter, BindingKind::Signal, Some(setter_name));
                        }
                    }
                    _ => {}
                },
                Some(BindingKind::Computed) => {
                    if let BindingPattern::BindingIdentifier(id) = &it.id {
                        self.record(id.symbol_id(), BindingKind::Computed, None);
                    }
                }
                Some(BindingKind::Action) => {
                    if let BindingPattern::BindingIdentifier(id) = &it.id {
                        self.record(id.symbol_id(), BindingKind::Action, None);
                    }
                }
                _ => {
                    if self.facts.imported(self.scoping, &call.callee) == Some("store")
                        && let BindingPattern::BindingIdentifier(id) = &it.id
                    {
                        self.record(id.symbol_id(), BindingKind::Store, None);
                    }
                }
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.hits.push(Hit { span: it.span, symbol: Some(it.symbol_id()), intrinsic: None });
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let symbol = it
            .reference_id
            .get()
            .and_then(|reference| self.scoping.get_reference(reference).symbol_id());
        self.hits.push(Hit { span: it.span, symbol, intrinsic: None });
    }

    fn visit_jsx_element_name(&mut self, it: &JSXElementName<'a>) {
        let intrinsic = self.facts.intrinsic(self.scoping, it);
        let (span, symbol) = match it {
            JSXElementName::IdentifierReference(id) => {
                let symbol = id
                    .reference_id
                    .get()
                    .and_then(|reference| self.scoping.get_reference(reference).symbol_id());
                (id.span, symbol)
            }
            JSXElementName::Identifier(id) => (id.span, None),
            JSXElementName::MemberExpression(member) => {
                let symbol = match &member.object {
                    oxc_ast::ast::JSXMemberExpressionObject::IdentifierReference(namespace) => {
                        namespace
                            .reference_id
                            .get()
                            .and_then(|reference| self.scoping.get_reference(reference).symbol_id())
                    }
                    _ => None,
                };
                (member.property.span, symbol)
            }
            JSXElementName::NamespacedName(name) => (name.name.span, None),
            _ => (it.span(), None),
        };
        self.jsx_hits.push(Hit { span, symbol, intrinsic });
    }
}
