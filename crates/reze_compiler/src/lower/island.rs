//! Island roots and boundary positions (SPEC §15.9, §16.3, §16.4), as the program facts decided
//! them, and `island:*` attributes elsewhere.

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::{Lowerer, attribute_name};
use crate::diagnostic::{Code, Report};
use crate::facts::{IslandMode, Primitive};
use crate::ir::{Hole, HoleKind, Island, IslandImport, RootKind};
use crate::summary::island_load_mode;

impl<'a> Lowerer<'a, '_> {
    /// The island of the component element `el`, reporting `ISLAND` (and `LAZY_ISLAND`) when
    /// it is one.
    pub(super) fn island(&mut self, el: &JSXElement<'a>) -> Option<Island<'a>> {
        let facts = self.facts;
        let island = facts.program.islands.get(&el.span.start)?;
        let name = el.opening_element.name.span();
        let features = island.features.join(",");
        let mode = island.mode.as_str();
        self.report(
            Report::new(
                Code::Island,
                name,
                format!(
                    "An island boundary: the server serializes the props and the browser hydrates \
                     only this component, with the runtime features {features}."
                ),
            )
            .data("id", island.id.as_str())
            .data("features", features.as_str())
            .data("mode", mode),
        );
        if island.mode != IslandMode::Eager {
            self.report(
                Report::new(
                    Code::LazyIsland,
                    name,
                    format!(
                        "A lazy island: the browser loads its module in a separate chunk on \
                         `{mode}` and hydrates it then; events that reach it before its code has \
                         loaded are dropped."
                    ),
                )
                .data("id", island.id.as_str())
                .data("mode", mode),
            );
        }
        let mut slots = self.vec();
        for slot in &island.slots {
            slots.push(self.str(slot));
        }
        Some(Island { id: self.str(&island.id), mode: island.mode, slots })
    }

    /// Whether the `island:*` attribute `a` is dropped: `island:load` with a mode name on a
    /// boundary position (§16.3). Any other stays as written, with `ISLAND_DIRECTIVE_IGNORED`
    /// when the module has program facts.
    pub(super) fn island_directive(
        &mut self,
        a: &JSXAttribute<'a>,
        name: &str,
        site: DirectiveSite<'_>,
    ) -> bool {
        let is_boundary = matches!(site, DirectiveSite::Component { is_boundary: true, .. });
        if is_boundary && island_load_mode(a).is_some() {
            return true;
        }
        let Some(islands_enabled) = self.facts.program.islands_enabled else { return false };
        let why = match site {
            _ if name != "island:load" => "the only island directive is `island:load`".to_string(),
            _ if !islands_enabled => {
                "the build has no `islands`, so no position is an island boundary".to_string()
            }
            DirectiveSite::Component { is_boundary: true, .. } => {
                "its value is not one of \"eager\", \"idle\", \"visible\", \"interaction\", so \
                 the island loads eagerly"
                    .to_string()
            }
            DirectiveSite::Component { name: component, .. } => format!(
                "`<{component}>` is not an island boundary here: it is static, it cannot take \
                 these props, or its parent runs in the browser anyway"
            ),
            DirectiveSite::Native(tag) => {
                format!("`<{tag}>` is a native element, and only a component can be an island")
            }
        };
        let kind = match site {
            DirectiveSite::Native(_) => "attribute",
            DirectiveSite::Component { .. } => "prop",
        };
        let removal = self.removal(a.span);
        self.report(
            Report::new(
                Code::IslandDirectiveIgnored,
                a.span,
                format!(
                    "`{name}` has no effect: {why}. It was compiled as a plain {kind}; remove it."
                ),
            )
            .fix(format!("remove `{name}`"), vec![removal]),
        );
        false
    }

    /// `ISLAND_DIRECTIVE_IGNORED` for the `island:*` attributes of a native element.
    pub(super) fn native_island_directives(&mut self, attrs: &[JSXAttributeItem<'a>], tag: &str) {
        for attr in attrs {
            if let JSXAttributeItem::Attribute(a) = attr {
                let name = attribute_name(self, a);
                if name.starts_with("island:") {
                    self.island_directive(a, name, DirectiveSite::Native(tag));
                }
            }
        }
    }

    /// `renderToString(() => <R/>)` / `hydrate(() => <R/>, el)` of an islands root.
    pub(super) fn island_root(&mut self, call: &CallExpression<'a>) -> Option<Hole<'a>> {
        let facts = self.facts;
        let root = facts.program.roots.get(&call.span.start)?;
        let kind = match facts.primitives.of(&call.callee, self.scoping)? {
            Primitive::RenderToString => RootKind::RenderToString,
            Primitive::Hydrate => RootKind::Hydrate,
            _ => return None,
        };
        let code = self.expr(call.arguments.first()?.as_expression()?);
        let element = match call.arguments.get(1) {
            Some(argument) => Some(self.expr(argument.as_expression()?)),
            None => None,
        };
        let mut islands = self.vec();
        for island in &root.islands {
            islands.push(IslandImport {
                id: self.str(&island.id),
                specifier: self.str(&island.specifier),
                export: self.str(&island.export),
                mode: island.mode,
            });
        }
        let callee = call.callee.span();
        Some(Hole {
            span: call.span,
            kind: HoleKind::IslandRoot { kind, callee, code, element, islands },
        })
    }
}

/// Where an `island:*` attribute stands.
#[derive(Clone, Copy)]
pub(super) enum DirectiveSite<'s> {
    Native(&'s str),
    Component { name: &'s str, is_boundary: bool },
}
