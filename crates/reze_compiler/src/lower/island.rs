//! Island roots and boundary positions (SPEC §15.9), as the program facts decided them.

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::Lowerer;
use crate::diagnostic::{Code, Report};
use crate::facts::Primitive;
use crate::ir::{Hole, HoleKind, IslandImport, RootKind};

impl<'a> Lowerer<'a, '_> {
    /// The island id of the component element `el`, reporting `ISLAND` when it has one.
    pub(super) fn island(&mut self, el: &JSXElement<'a>) -> Option<&'a str> {
        let facts = self.facts;
        let island = facts.program.islands.get(&el.span.start)?;
        let features = island.features.join(",");
        self.report(
            Report::new(
                Code::Island,
                el.opening_element.name.span(),
                format!(
                    "An island boundary: the server serializes the props and the browser hydrates \
                     only this component, with the runtime features {features}."
                ),
            )
            .data("id", island.id.as_str())
            .data("features", features.as_str()),
        );
        Some(self.str(&island.id))
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
            });
        }
        let callee = call.callee.span();
        Some(Hole {
            span: call.span,
            kind: HoleKind::IslandRoot { kind, callee, code, element, islands },
        })
    }
}
