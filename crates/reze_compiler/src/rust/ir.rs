//! The type IR of a Rust crate's `#[derive(reze::Data)]` types, which `reze-ir` writes and the
//! Rust target and the TS types read.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeRef {
    String,
    Boolean,
    /// A Rust number type that crosses as a JS `number`: `f64`, `u32`.
    Number {
        rust: String,
    },
    /// `u64` or `i64`, which cross as a decimal string.
    Bigint {
        rust: String,
    },
    Option {
        of: Box<TypeRef>,
    },
    List {
        of: Box<TypeRef>,
    },
    /// A `BTreeSet` or `HashSet`, a JS `Set`.
    Set {
        of: Box<TypeRef>,
    },
    /// A `BTreeMap` or `HashMap` with `String` keys, a JS `Map`.
    Map {
        value: Box<TypeRef>,
    },
    /// `std::time::SystemTime`, a JS `Date`.
    Date,
    /// A tuple, a JS tuple.
    Tuple {
        items: Vec<TypeRef>,
    },
    /// A `reze::Defer` struct field, which streams into its `<Loading>` boundary.
    Defer {
        of: Box<TypeRef>,
    },
    /// A type of the same crate.
    Named {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeDef {
    Struct {
        name: String,
        fields: Vec<FieldDef>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
    },
    /// An enum with fields: a union of objects tagged with `kind`.
    Union {
        name: String,
        variants: Vec<VariantDef>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariantDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

impl TypeDef {
    pub fn name(&self) -> &str {
        match self {
            TypeDef::Struct { name, .. }
            | TypeDef::Enum { name, .. }
            | TypeDef::Union { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CrateIr {
    #[serde(rename = "crate")]
    pub name: String,
    pub types: Vec<TypeDef>,
}
