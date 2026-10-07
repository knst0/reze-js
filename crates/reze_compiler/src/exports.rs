use crate::frontend::analysis::{Intrinsic, Primitive, RuntimeCallKind};
use crate::frontend::imports::Syntax;
use crate::frontend::props::PropsMethod;

pub const PUBLIC: &str = "reze-js";
pub const RUNTIME: &str = "reze-js/internal/runtime";
pub const REACTIVITY: &str = "reze-js/internal/reactivity";
pub const ASYNC: &str = "reze-js/internal/async";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Syntax(Syntax),
    Props(PropsMethod),
    Primitive(Primitive),
    Call(RuntimeCallKind),
    Plain,
}

#[derive(Debug)]
pub struct Entry {
    pub specifier: &'static str,
    pub name: &'static str,
    pub role: Role,
    pub lower_to: Option<&'static str>,
}

impl Entry {
    pub fn syntax(&self) -> Option<Syntax> {
        match self.role {
            Role::Syntax(syntax) => Some(syntax),
            _ => None,
        }
    }

    pub fn props(&self) -> Option<PropsMethod> {
        match self.role {
            Role::Props(method) => Some(method),
            _ => None,
        }
    }

    pub fn primitive(&self) -> Option<Primitive> {
        match self.role {
            Role::Primitive(primitive) => Some(primitive),
            _ => None,
        }
    }

    pub fn call(&self) -> Option<RuntimeCallKind> {
        match self.role {
            Role::Call(kind) => Some(kind),
            _ => None,
        }
    }

    pub fn is_compiled_away(&self) -> bool {
        !matches!(self.role, Role::Plain)
    }
}

const fn row(
    specifier: &'static str,
    name: &'static str,
    role: Role,
    lower_to: Option<&'static str>,
) -> Entry {
    Entry { specifier, name, role, lower_to }
}

const fn syntax(name: &'static str, syntax: Syntax) -> [Entry; 2] {
    [
        row(PUBLIC, name, Role::Syntax(syntax), Some(REACTIVITY)),
        row(REACTIVITY, name, Role::Primitive(syntax.primitive()), None),
    ]
}

const fn props(name: &'static str, method: PropsMethod) -> Entry {
    row(PUBLIC, name, Role::Props(method), None)
}

const fn call(name: &'static str, kind: RuntimeCallKind) -> [Entry; 2] {
    [row(PUBLIC, name, Role::Call(kind), Some(RUNTIME)), row(RUNTIME, name, Role::Call(kind), None)]
}

const fn intrinsic(specifier: &'static str, name: &'static str, tag: Intrinsic) -> Entry {
    row(specifier, name, Role::Primitive(Primitive::Intrinsic(tag)), None)
}

const fn plain(name: &'static str) -> [Entry; 2] {
    [row(PUBLIC, name, Role::Plain, Some(RUNTIME)), row(RUNTIME, name, Role::Plain, None)]
}

static TABLE: &[&[Entry]] = &[
    &syntax("signal", Syntax::Signal),
    &syntax("computed", Syntax::Computed),
    &syntax("action", Syntax::Action),
    &[props("mergeProps", PropsMethod::Merge)],
    &[props("splitProps", PropsMethod::Split)],
    &[props("omitProps", PropsMethod::Omit)],
    &call("dynamic", RuntimeCallKind::Dynamic),
    &call("dynamicElement", RuntimeCallKind::DynamicElement),
    &call("island", RuntimeCallKind::Island),
    &call("createUniqueId", RuntimeCallKind::UniqueId),
    &[row(RUNTIME, "asyncComponent", Role::Call(RuntimeCallKind::AsyncComponent), None)],
    &[row(ASYNC, "asyncComputed", Role::Call(RuntimeCallKind::AsyncComputed), None)],
    &[intrinsic(PUBLIC, "Show", Intrinsic::Show)],
    &[intrinsic(PUBLIC, "For", Intrinsic::For)],
    &[intrinsic(PUBLIC, "Repeat", Intrinsic::Repeat)],
    &[intrinsic(PUBLIC, "Switch", Intrinsic::Switch)],
    &[intrinsic(PUBLIC, "Match", Intrinsic::Match)],
    &[intrinsic(PUBLIC, "Portal", Intrinsic::Portal)],
    &[intrinsic(ASYNC, "Loading", Intrinsic::Loading)],
    &[intrinsic(ASYNC, "Errored", Intrinsic::Errored)],
    &plain("store"),
    &plain("effect"),
    &plain("createContext"),
    &plain("provideContext"),
    &plain("useContext"),
    &plain("catchError"),
    &plain("ContextNotFoundError"),
    &plain("effectScope"),
    &plain("flush"),
    &plain("getOwner"),
    &plain("onCleanup"),
    &plain("readonly"),
    &plain("root"),
    &plain("runWithOwner"),
    &plain("selector"),
    &plain("trigger"),
    &plain("untrack"),
];

pub fn entries() -> impl Iterator<Item = &'static Entry> {
    TABLE.iter().flat_map(|rows| rows.iter())
}

pub fn lookup(specifier: &str, name: &str) -> Option<&'static Entry> {
    entries().find(|entry| entry.specifier == specifier && entry.name == name)
}

pub fn module(specifier: &str) -> Option<&'static str> {
    [PUBLIC, RUNTIME, REACTIVITY, ASYNC].into_iter().find(|module| *module == specifier)
}

pub fn syntax_named(specifier: &str, name: &str) -> Option<Syntax> {
    lookup(specifier, name)?.syntax()
}

pub fn primitive_named(specifier: &str, name: &str) -> Option<Primitive> {
    lookup(specifier, name)?.primitive()
}

pub fn call_named(specifier: &str, name: &str) -> Option<RuntimeCallKind> {
    lookup(specifier, name)?.call()
}

pub fn props_named(specifier: &str, name: &str) -> Option<PropsMethod> {
    lookup(specifier, name)?.props()
}

pub fn lowered_home(specifier: &str, name: &str) -> Option<&'static str> {
    lookup(specifier, name)?.lower_to
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lowering_lands_on_a_row() {
        for entry in entries() {
            if let Some(home) = entry.lower_to {
                assert!(
                    lookup(home, entry.name).is_some(),
                    "{} from {} lowers to {home} without a row",
                    entry.name,
                    entry.specifier
                );
            }
        }
    }

    #[test]
    fn rows_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in entries() {
            assert!(
                seen.insert((entry.specifier, entry.name)),
                "{} from {}",
                entry.name,
                entry.specifier
            );
        }
    }

    #[test]
    fn public_rows_match_the_declarations_of_reze_js() {
        let index = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/reze-js/src/index.ts");
        let source = std::fs::read_to_string(index).expect("reze-js index");
        let declared: std::collections::BTreeSet<&str> = source
            .lines()
            .filter_map(|line| line.strip_prefix("export declare "))
            .filter_map(|rest| {
                let rest =
                    rest.strip_prefix("function ").or_else(|| rest.strip_prefix("class "))?;
                rest.split(|c: char| !c.is_alphanumeric() && c != '_').next()
            })
            .collect();
        let rows: std::collections::BTreeSet<&str> =
            entries().filter(|entry| entry.specifier == PUBLIC).map(|entry| entry.name).collect();
        assert_eq!(declared, rows);
    }
}
