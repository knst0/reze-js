//! `ModuleFacts`: the cross-module decisions `link` made for one module (SPEC §15.2, §15.12).
//! Everything is keyed by source offsets of the module the facts were built for; `source_hash`
//! guarantees the offsets still point at the same code.

use oxc_span::Span;
use serde::{Deserialize, Serialize};

use crate::diagnostic::{Code, Related};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ModuleFacts {
    pub version: String,
    pub source_hash: String,
    /// Imports that resolve to runtime primitives through program re-exports.
    pub primitives: Vec<PrimitiveImport>,
    /// Exported top-level signals of this module that the program folds (§15.5).
    pub folded_signals: Vec<FoldedSignal>,
    /// Imported getters folded in their own module: their calls read a constant.
    pub folded_imports: Vec<FoldedImport>,
    /// Imported `signal`/`computed` getters, for `SIGNAL_NOT_CALLED`.
    pub getter_imports: Vec<ImportRef>,
    /// Exported stores of this module that the program unproxies (§15.6).
    pub stores: Vec<StoreExport>,
    /// Imported store bindings whose store is unproxied: the specifier is replaced by leaves.
    pub store_imports: Vec<StoreImport>,
    /// Every component of this module; `client: None` when it is static (§15.8).
    pub components: Vec<ComponentFact>,
    pub islands: Vec<IslandFact>,
    pub roots: Vec<RootFact>,
}

/// An import binding, by the start of its local identifier, or `member` of a namespace import.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImportRef {
    pub binding: u32,
    pub member: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    Signal,
    Computed,
    Store,
    RenderToString,
    Hydrate,
    Suspense,
}

impl Primitive {
    pub fn from_export(name: &str) -> Option<Primitive> {
        Some(match name {
            "signal" => Primitive::Signal,
            "computed" => Primitive::Computed,
            "store" => Primitive::Store,
            "renderToString" => Primitive::RenderToString,
            "hydrate" => Primitive::Hydrate,
            "Suspense" => Primitive::Suspense,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PrimitiveImport {
    pub import: ImportRef,
    pub primitive: Primitive,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FoldedSignal {
    /// Start of the getter's binding identifier.
    pub getter: u32,
    /// Uses in other modules.
    pub related: Vec<Related>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FoldedImport {
    pub import: ImportRef,
    /// The text a literal initializer renders as (SPEC §7.10).
    pub literal: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoreExport {
    /// Start of the `state` binding identifier.
    pub state: u32,
    /// Leaves other modules use, with the export names `link` gave them.
    pub leaves: Vec<LeafNames>,
    pub related: Vec<Related>,
}

/// Export names of one leaf: `getter` when some importer reads it, `setter` when one writes it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LeafNames {
    pub path: Vec<String>,
    pub getter: Option<String>,
    pub setter: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreRole {
    State,
    Setter,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoreImport {
    /// Start of the import specifier's local identifier.
    pub binding: u32,
    pub role: StoreRole,
    /// The leaves this module uses through the binding, with their export names.
    pub leaves: Vec<LeafNames>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ComponentFact {
    /// Start of the component's binding identifier.
    pub binding: u32,
    pub client: Option<Reason>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct IslandFact {
    /// Start of the boundary JSX element.
    pub element: u32,
    pub id: String,
    /// Sorted runtime exports the island's client code uses (§15.11).
    pub features: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RootFact {
    /// Start of the `renderToString`/`hydrate` call.
    pub call: u32,
    pub islands: Vec<RootIsland>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RootIsland {
    pub id: String,
    /// Import specifier of the island's module, relative to the root's module.
    pub specifier: String,
    pub export: String,
}

/// Why a decision was taken; `cause` continues the chain, possibly in another module.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reason {
    pub code: Code,
    pub module: String,
    #[serde(with = "span_pair")]
    pub span: Span,
    pub message: String,
    pub cause: Option<Box<Reason>>,
}

mod span_pair {
    use oxc_span::Span;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(span: &Span, serializer: S) -> Result<S::Ok, S::Error> {
        (span.start, span.end).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Span, D::Error> {
        let (start, end) = <(u32, u32)>::deserialize(deserializer)?;
        Ok(Span::new(start, end))
    }
}

/// FNV-1a 64 of the source bytes, as 16 lower-case hex digits.
pub fn source_hash(source: &str) -> String {
    format!("{:016x}", fnv1a64(source.as_bytes()))
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
