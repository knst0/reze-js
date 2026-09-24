//! Island roots (SPEC §15.9, §16.3): `renderToString(code, true)` on the server,
//! `hydrateIslands` in the browser, the call as written for the client target.

use std::fmt::Write;

use oxc_span::Span;

use super::{Emitter, Helper};
use crate::Target;
use crate::code::Code;
use crate::facts::IslandMode;
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
                    out.push(if i == 0 { " " } else { ", " });
                    push_js_string(&mut out.text, island.id);
                    out.push(": ");
                    self.island_value(out, island, i);
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
    /// The value of an island in the `hydrateIslands` map: the statically imported component
    /// when some position is eager, otherwise `lazyIsland(load, mode, export)`, whose `load()`
    /// the bundler splits per island.
    fn island_value(&mut self, out: &mut Code, island: &IslandImport<'a>, index: usize) {
        if island.mode == IslandMode::Eager {
            let alias = self.island_import(island, index);
            out.push(alias);
            return;
        }
        out.push(self.helper(Helper::LazyIsland));
        out.push("(() => import(");
        push_js_string(&mut out.text, island.specifier);
        out.push("), ");
        push_js_string(&mut out.text, island.mode.as_str());
        out.push(", ");
        push_js_string(&mut out.text, island.export);
        out.push(")");
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
