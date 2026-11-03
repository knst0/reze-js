//! Build-time half of the Rust server: reads the `#[derive(reze::Data)]` types of a crate into the
//! compiler's type IR, and compiles the route files into `reze_routes.rs` from a `build.rs`:
//!
//! ```no_run
//! reze_build::compile(reze_build::Config::new("../web").types("../types"));
//! ```
//!
//! The server then includes `include!(concat!(env!("OUT_DIR"), "/reze_routes.rs"));`.

use std::path::{Path, PathBuf};

pub use reze_compiler::Chunks;
use reze_compiler::{
    CrateIr, DiskFs, FieldDef, Options, TypeDef, TypeRef, VariantDef, compile_project,
};
use syn::{Fields, FieldsNamed, GenericArgument, Item, PathArguments, Type};

/// What [`compile`] builds: the Vite project and the crates of its loader data types.
pub struct Config {
    root: PathBuf,
    routes_dir: String,
    types: Vec<PathBuf>,
    chunks: Chunks,
}

impl Config {
    /// `root` is the Vite project, relative to the crate that builds.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            routes_dir: "src/routes".into(),
            types: Vec::new(),
            chunks: Chunks::default(),
        }
    }

    /// The routes directory, relative to the Vite project.
    pub fn routes_dir(mut self, dir: impl Into<String>) -> Self {
        self.routes_dir = dir.into();
        self
    }

    /// A crate of `#[derive(reze::Data)]` types that `virtual:reze-types/<crate>` imports from.
    pub fn types(mut self, dir: impl Into<PathBuf>) -> Self {
        self.types.push(dir.into());
        self
    }

    /// The chunk mode, which must match the `chunks` option of the Vite plugin.
    pub fn chunks(mut self, chunks: Chunks) -> Self {
        self.chunks = chunks;
        self
    }
}

/// Writes `$OUT_DIR/reze_routes.rs`, and fails the build with the compiler's diagnostics.
pub fn compile(config: Config) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_default();
    let root = manifest.join(&config.root);
    println!("cargo:rerun-if-changed={}", root.join("src").display());
    let mut crates = Vec::new();
    for dir in &config.types {
        let dir = manifest.join(dir);
        println!("cargo:rerun-if-changed={}", dir.join("src").display());
        crates.push(extract(&dir).unwrap_or_else(|error| panic!("{error}")));
    }
    let options = Options {
        routes_dir: config.routes_dir,
        rust: Some(crates),
        chunks: config.chunks,
        ..Options::default()
    };
    let output = compile_project(&DiskFs::new(&root), &options);
    let rendered: Vec<&str> = output.diagnostics.iter().map(|d| d.rendered.as_str()).collect();
    if output.has_errors() {
        panic!("reze-js found errors in the route files:\n\n{}", rendered.join("\n\n"));
    }
    for diagnostic in rendered {
        for line in diagnostic.lines() {
            println!("cargo:warning={line}");
        }
    }
    let source = output.rust.expect("the Rust render is emitted");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set in a build script"));
    std::fs::write(out.join("reze_routes.rs"), source).expect("OUT_DIR is writable");
}

/// The type IR of the `#[derive(reze::Data)]` types in the crate at `dir`.
pub fn extract(dir: impl AsRef<Path>) -> Result<CrateIr, String> {
    let dir = dir.as_ref();
    let manifest = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    let table: toml::Table =
        text.parse().map_err(|error| format!("{}: {error}", manifest.display()))?;
    let name = table
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .ok_or_else(|| format!("{}: no package name", manifest.display()))?;
    let mut files = Vec::new();
    walk(&dir.join("src"), &mut files);
    files.sort();
    let mut types = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let parsed =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", file.display()))?;
        collect(&parsed.items, &file, &mut types)?;
    }
    Ok(CrateIr { name: name.replace('-', "_"), types })
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn derives_data(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().filter(|attr| attr.path().is_ident("derive")).any(|attr| {
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            found |= meta.path.segments.last().is_some_and(|segment| segment.ident == "Data");
            Ok(())
        });
        found
    })
}

fn collect(items: &[Item], file: &Path, out: &mut Vec<TypeDef>) -> Result<(), String> {
    for item in items {
        match item {
            Item::Struct(item) if derives_data(&item.attrs) => {
                let Fields::Named(named) = &item.fields else { continue };
                let fields = field_defs(named, &item.ident.to_string(), file)?;
                out.push(TypeDef::Struct { name: item.ident.to_string(), fields });
            }
            Item::Enum(item) if derives_data(&item.attrs) => {
                let name = item.ident.to_string();
                if item.variants.iter().all(|v| matches!(v.fields, Fields::Unit)) {
                    let variants = item.variants.iter().map(|v| v.ident.to_string()).collect();
                    out.push(TypeDef::Enum { name, variants });
                    continue;
                }
                let mut variants = Vec::new();
                for variant in &item.variants {
                    let owner = format!("{name}::{}", variant.ident);
                    let fields = match &variant.fields {
                        Fields::Named(named) => field_defs(named, &owner, file)?,
                        _ => Vec::new(),
                    };
                    variants.push(VariantDef { name: variant.ident.to_string(), fields });
                }
                out.push(TypeDef::Union { name, variants });
            }
            Item::Mod(module) => {
                if let Some((_, inner)) = &module.content {
                    collect(inner, file, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn field_defs(named: &FieldsNamed, owner: &str, file: &Path) -> Result<Vec<FieldDef>, String> {
    named
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.as_ref().expect("a named field");
            let ty = field_ref(&field.ty).map_err(|ty| {
                format!(
                    "{}: `{owner}.{ident}` has the type `{ty}`, which has no wire encoding",
                    file.display()
                )
            })?;
            Ok(FieldDef { name: ident.to_string(), ty })
        })
        .collect()
}

/// The last segment of a type path and its type arguments.
fn path_of(ty: &Type) -> Option<(String, Vec<&Type>)> {
    let Type::Path(path) = ty else { return None };
    let last = path.path.segments.last().filter(|_| path.qself.is_none())?;
    let arguments = match &last.arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some((last.ident.to_string(), arguments))
}

/// The type of a struct field, which alone may be `reze::Defer`.
fn field_ref(ty: &Type) -> Result<TypeRef, String> {
    match path_of(ty) {
        Some((name, arguments)) if name == "Defer" && arguments.len() == 1 => {
            Ok(TypeRef::Defer { of: Box::new(type_ref(arguments[0])?) })
        }
        _ => type_ref(ty),
    }
}

fn type_ref(ty: &Type) -> Result<TypeRef, String> {
    let unsupported = || Err(quote_type(ty));
    match ty {
        Type::Reference(reference) => type_ref(&reference.elem),
        Type::Slice(slice) => Ok(TypeRef::List { of: Box::new(type_ref(&slice.elem)?) }),
        Type::Tuple(tuple) if !tuple.elems.is_empty() => Ok(TypeRef::Tuple {
            items: tuple.elems.iter().map(type_ref).collect::<Result<_, _>>()?,
        }),
        Type::Path(_) => {
            let Some((name, arguments)) = path_of(ty) else { return unsupported() };
            Ok(match (name.as_str(), arguments.as_slice()) {
                ("String" | "str", []) => TypeRef::String,
                ("bool", []) => TypeRef::Boolean,
                ("f64" | "f32" | "u8" | "u16" | "u32" | "i8" | "i16" | "i32", []) => {
                    TypeRef::Number { rust: name }
                }
                ("u64" | "i64", []) => TypeRef::Bigint { rust: name },
                ("SystemTime", []) => TypeRef::Date,
                ("Option", [of]) => TypeRef::Option { of: Box::new(type_ref(of)?) },
                ("Vec", [of]) => TypeRef::List { of: Box::new(type_ref(of)?) },
                ("BTreeSet", [of]) | ("HashSet", [of, ..]) => {
                    TypeRef::Set { of: Box::new(type_ref(of)?) }
                }
                ("BTreeMap", [key, value]) | ("HashMap", [key, value, ..])
                    if type_ref(key) == Ok(TypeRef::String) =>
                {
                    TypeRef::Map { value: Box::new(type_ref(value)?) }
                }
                ("Box", [of]) => type_ref(of)?,
                (_, []) if name.starts_with(char::is_uppercase) => TypeRef::Named { name },
                _ => return unsupported(),
            })
        }
        _ => unsupported(),
    }
}

fn quote_type(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        _ => "_".into(),
    }
}
