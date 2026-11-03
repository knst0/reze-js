//! Whole-program compiler: route modules and the modules they import become server render code,
//! client chunks, the route table, and diagnostics.

mod analyze;
mod client;
mod diagnostic;
mod emit;
mod fs;
mod html;
mod project;
mod routes;
mod rust;
mod view;

use std::collections::BTreeMap;

use serde::Serialize;

pub use diagnostic::{
    CATALOG, Code, Diagnostic, Edit, Entry, Example, Fix, Label, LineIndex, Position, Severity,
    render_skill,
};
pub use fs::{DiskFs, FileSystem, MemoryFs};
pub use routes::{RouteNode, RouteTable, Segment};
pub use rust::{CrateIr, FieldDef, TypeDef, TypeRef};

/// The module compiled server code imports render helpers from.
pub const SERVER_RUNTIME: &str = "reze-js/runtime/server";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Server,
    Spa,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chunks {
    #[default]
    Route,
    Component,
    Handler,
}

pub struct Options {
    /// Project-relative directory of route modules; the same path plus an extension is the root
    /// layout.
    pub routes_dir: String,
    pub dev: bool,
    pub target: Target,
    pub chunks: Chunks,
    /// The type IR of the Rust crates that `loader.external<T>()` types come from; when set, the
    /// output has the Rust render.
    pub rust: Option<Vec<CrateIr>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            routes_dir: "src/routes".into(),
            dev: false,
            target: Target::Server,
            chunks: Chunks::Route,
            rust: None,
        }
    }
}

/// Compiled code of one project module, as TypeScript.
#[derive(Debug, Default, Serialize)]
pub struct ModuleOutput {
    pub server: Option<String>,
    pub client: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct ProjectOutput {
    /// Project-relative path to compiled code, for every module the routes reach.
    pub modules: BTreeMap<String, ModuleOutput>,
    pub routes: RouteTable,
    /// Source of `routes.gen.d.ts`, next to the routes directory.
    pub types: String,
    /// Source of `virtual:rz-app`: the route modules and the route table for `createHandler`.
    pub app: String,
    /// Source of `virtual:rz-routes`: the route table of client navigation.
    pub navigation: String,
    /// Source of `virtual:rz-route/{node}`: the client factories of each page route that has any.
    pub chunks: BTreeMap<usize, String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Source of `reze_routes.rs`, when the options name Rust crates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust: Option<String>,
}

impl ProjectOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

pub fn compile_project(fs: &dyn FileSystem, options: &Options) -> ProjectOutput {
    project::compile(fs, options)
}
