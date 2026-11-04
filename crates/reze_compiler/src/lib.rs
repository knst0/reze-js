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
pub use rust::{CrateIr, FieldDef, TypeDef, TypeRef, VariantDef};

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
    pub rust: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            routes_dir: "src/routes".into(),
            dev: false,
            target: Target::Server,
            chunks: Chunks::Route,
            rust: false,
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
    /// Source of `virtual:rz-route/{key}`: the client chunks, by the key that the route table names.
    pub chunks: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustOutput>,
}

#[derive(Debug, Default, Serialize)]
pub struct RustOutput {
    pub main: Option<String>,
    pub types: BTreeMap<String, String>,
}

impl ProjectOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

pub fn compile_project(fs: &dyn FileSystem, options: &Options) -> ProjectOutput {
    project::compile(fs, options)
}
