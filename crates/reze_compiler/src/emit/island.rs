//! Island roots (SPEC §15.9): `renderToString(code, true)` on the server, `hydrateIslands` in
//! the browser, the call as written for the client target.

use std::fmt::Write;

use oxc_span::Span;

use super::{Emitter, Helper};
use crate::Target;
use crate::code::Code;
use crate::html::push_js_string;
use crate::ir::{Embed, IslandImport, RootKind};

impl<'a> Emitter<'a, '_> {
    pub(super) fn island_root(
        &mut self,
        out: &mut Code,
        kind: RootKind,
        callee: Span,
        code: &Embed<'a>,
        element: Option<&Embed<'a>>,
        islands: &[IslandImport<'a>],
    ) {
        match (kind, self.target, element) {
            (RootKind::Hydrate, Target::Hydrate, Some(element)) => {
                let hydrate = self.helper(Helper::HydrateIslands);
                let _ = write!(out, "{hydrate}(");
                self.embed(out, element);
                out.push(", {");
                for (i, island) in islands.iter().enumerate() {
                    let alias = self.island_import(island, i);
                    out.push(if i == 0 { " " } else { ", " });
                    push_js_string(&mut out.text, island.id);
                    let _ = write!(out, ": {alias}");
                }
                out.push(if islands.is_empty() { "})" } else { " })" });
            }
            (RootKind::RenderToString, Target::Server, _) => {
                self.src(out, callee);
                out.push("(");
                self.embed(out, code);
                out.push(", true)");
            }
            _ => {
                self.src(out, callee);
                out.push("(");
                self.embed(out, code);
                if let Some(element) = element {
                    out.push(", ");
                    self.embed(out, element);
                }
                out.push(")");
            }
        }
    }

    /// The local name of an island's component, imported once per module.
    fn island_import(&mut self, island: &IslandImport<'a>, index: usize) -> &'a str {
        if let Some((_, alias, _)) = self.island_imports.iter().find(|(export, _, specifier)| {
            *export == island.export && *specifier == island.specifier
        }) {
            return alias;
        }
        let alias = self.fresh(&format!("_$island{index}"));
        self.island_imports.push((island.export, alias, island.specifier));
        alias
    }
}
