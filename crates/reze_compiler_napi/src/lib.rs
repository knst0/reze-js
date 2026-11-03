use std::collections::HashMap;

use napi::bindgen_prelude::{AsyncTask, Env, Task};
use napi_derive::napi;

#[napi(string_enum = "lowercase")]
pub enum Target {
    Server,
    Spa,
}

#[napi(string_enum = "lowercase")]
pub enum Chunks {
    Route,
    Component,
    Handler,
}

#[napi(object)]
pub struct ProjectInput {
    /// Directory read from disk. Ignored when `files` is set.
    pub root: Option<String>,
    /// Project-relative paths to sources, instead of reading `root`.
    pub files: Option<HashMap<String, String>>,
}

#[napi(object)]
pub struct CompileOptions {
    /// Default: `"src/routes"`.
    pub routes_dir: Option<String>,
    /// Default: `false`.
    pub dev: Option<bool>,
    /// Default: `"server"`.
    pub target: Option<Target>,
    /// Default: `"route"`.
    pub chunks: Option<Chunks>,
}

#[napi(object)]
pub struct Position {
    /// Byte offset.
    pub offset: u32,
    /// 1-based.
    pub line: u32,
    /// 0-based, in UTF-16 code units.
    pub column: u32,
}

#[napi(object)]
pub struct Label {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Replaces source bytes `start..end` with `text`.
#[napi(object)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Edits that, applied together, remove the diagnostic.
#[napi(object)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
}

#[napi(object)]
pub struct Diagnostic {
    /// Stable code; see `skills/reze-compiler-diagnostics/SKILL.md`.
    pub code: String,
    #[napi(ts_type = "\"error\" | \"warn\" | \"info\"")]
    pub severity: String,
    /// `[CODE] …`.
    pub message: String,
    pub file: String,
    pub start: Position,
    pub end: Position,
    /// Enclosing components (`<Name>`) and elements, root first.
    pub path: Vec<String>,
    pub labels: Vec<Label>,
    pub fixes: Vec<Fix>,
    pub data: HashMap<String, String>,
    /// Repair guide URL anchored at the code.
    pub docs: String,
    /// Message, `in` path, location, code frame and fixes.
    pub rendered: String,
}

fn position(position: reze_compiler::Position) -> Position {
    Position { offset: position.offset, line: position.line, column: position.column }
}

fn diagnostic(d: reze_compiler::Diagnostic) -> Diagnostic {
    Diagnostic {
        code: d.code.name().to_string(),
        severity: d.severity.as_str().to_string(),
        message: d.message,
        file: d.file,
        start: position(d.start),
        end: position(d.end),
        path: d.path,
        labels: d
            .labels
            .into_iter()
            .map(|l| Label { start: l.start, end: l.end, message: l.message })
            .collect(),
        fixes: d
            .fixes
            .into_iter()
            .map(|fix| Fix {
                title: fix.title,
                edits: fix
                    .edits
                    .into_iter()
                    .map(|e| Edit { start: e.start, end: e.end, text: e.text })
                    .collect(),
            })
            .collect(),
        data: d.data.into_iter().collect(),
        docs: d.docs,
        rendered: d.rendered,
    }
}

#[napi(object)]
pub struct ModuleOutput {
    pub server: Option<String>,
    pub client: Option<String>,
}

#[napi(object)]
pub struct ProjectResult {
    /// Compiled modules by project-relative source path; empty when `diagnostics` holds an error.
    pub modules: HashMap<String, ModuleOutput>,
    /// The route table: `{ file, pattern, segments, parent, is_layout }` per route file.
    pub routes: serde_json::Value,
    /// Source of `virtual:rz-app`.
    pub app: String,
    /// Source of `virtual:rz-route/{node}`, by route node index.
    pub chunks: HashMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
}

fn options(options: Option<CompileOptions>) -> reze_compiler::Options {
    let mut out = reze_compiler::Options::default();
    let Some(options) = options else { return out };
    if let Some(routes_dir) = options.routes_dir {
        out.routes_dir = routes_dir;
    }
    out.dev = options.dev.unwrap_or(out.dev);
    out.target = match options.target {
        Some(Target::Spa) => reze_compiler::Target::Spa,
        Some(Target::Server) | None => reze_compiler::Target::Server,
    };
    out.chunks = match options.chunks {
        Some(Chunks::Component) => reze_compiler::Chunks::Component,
        Some(Chunks::Handler) => reze_compiler::Chunks::Handler,
        Some(Chunks::Route) | None => reze_compiler::Chunks::Route,
    };
    out
}

fn run(input: &ProjectInput, options: &reze_compiler::Options) -> reze_compiler::ProjectOutput {
    match (&input.files, &input.root) {
        (Some(files), _) => {
            let fs = reze_compiler::MemoryFs::new(files.clone());
            reze_compiler::compile_project(&fs, options)
        }
        (None, root) => {
            let fs = reze_compiler::DiskFs::new(root.clone().unwrap_or_else(|| ".".into()));
            reze_compiler::compile_project(&fs, options)
        }
    }
}

fn result(output: reze_compiler::ProjectOutput) -> napi::Result<ProjectResult> {
    let routes = serde_json::to_value(&output.routes.nodes)
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
    Ok(ProjectResult {
        modules: output
            .modules
            .into_iter()
            .map(|(path, m)| (path, ModuleOutput { server: m.server, client: m.client }))
            .collect(),
        routes,
        app: output.app,
        chunks: output.chunks.into_iter().map(|(node, code)| (node.to_string(), code)).collect(),
        diagnostics: output.diagnostics.into_iter().map(diagnostic).collect(),
    })
}

#[napi]
pub fn compile_project(
    input: ProjectInput,
    options: Option<CompileOptions>,
) -> napi::Result<ProjectResult> {
    let options = self::options(options);
    result(run(&input, &options))
}

pub struct CompileTask {
    input: ProjectInput,
    options: reze_compiler::Options,
}

impl Task for CompileTask {
    type Output = reze_compiler::ProjectOutput;
    type JsValue = ProjectResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        Ok(run(&self.input, &self.options))
    }

    fn resolve(&mut self, _: Env, output: Self::Output) -> napi::Result<ProjectResult> {
        result(output)
    }
}

#[napi(ts_return_type = "Promise<ProjectResult>")]
pub fn compile_project_async(
    input: ProjectInput,
    options: Option<CompileOptions>,
) -> AsyncTask<CompileTask> {
    AsyncTask::new(CompileTask { input, options: self::options(options) })
}
