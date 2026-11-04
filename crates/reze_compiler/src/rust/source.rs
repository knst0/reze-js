use std::collections::{BTreeMap, HashMap, HashSet};

use oxc_span::Span;
use syn::spanned::Spanned;
use syn::{
    Expr, Fields, FieldsNamed, FnArg, GenericArgument, Item, ItemFn, Lit, PathArguments,
    ReturnType, Type, Visibility,
};

use super::ir::{CrateIr, FieldDef, TypeDef, TypeRef, VariantDef};
use crate::diagnostic::{Code, Report};
use crate::fs::FileSystem;

pub const CRATE: &str = "crate";

pub struct Mutation {
    pub name: String,
    pub export: String,
    pub input: String,
    pub output: Option<String>,
    pub revalidate: Option<Vec<String>>,
}

pub struct Route {
    pub file: String,
    pub module: String,
    pub load: Option<String>,
    pub has_guard: bool,
    pub mutations: Vec<Mutation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Unit,
    Sync,
    Async,
}

pub struct SourceFile {
    pub path: String,
    pub source: String,
    pub reports: Vec<Report>,
}

pub struct Project {
    pub ir: CrateIr,
    pub paths: HashMap<String, String>,
    pub routes: BTreeMap<usize, Route>,
    pub server: Option<String>,
    pub context: Context,
    pub has_main: bool,
    pub files: Vec<SourceFile>,
}

impl Project {
    pub fn has_errors(&self) -> bool {
        self.files.iter().any(|file| !file.reports.is_empty())
    }

    pub fn route_of_file(&self, file: &str) -> Option<&Route> {
        self.routes.values().find(|route| route.file == file)
    }
}

struct Found {
    def: TypeDef,
    module: String,
    is_public: bool,
    file: usize,
    span: Span,
    fields: HashMap<String, Span>,
}

struct Use {
    name: String,
    file: usize,
    span: Span,
}

#[derive(Default)]
struct Collect {
    files: Vec<SourceFile>,
    found: Vec<Found>,
    uses: Vec<Use>,
}

impl Collect {
    fn report(&mut self, file: usize, span: Span, reason: impl Into<String>) {
        self.files[file]
            .reports
            .push(Report::new(Code::RustRouteInvalid, span).arg("reason", reason.into()));
    }

    fn add_file(&mut self, path: String, source: String) -> usize {
        self.files.push(SourceFile { path, source, reports: Vec::new() });
        self.files.len() - 1
    }

    fn parse(&mut self, file: usize) -> Option<syn::File> {
        match syn::parse_file(&self.files[file].source) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                let span = span_of(error.span());
                self.report(file, span, format!("This file does not parse as Rust: {error}"));
                None
            }
        }
    }
}

fn span_of(span: proc_macro2::Span) -> Span {
    let range = span.byte_range();
    Span::new(range.start as u32, range.end as u32)
}

pub fn rust_sibling(file: &str) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    format!("{stem}.rs")
}

pub fn snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

pub fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut is_upper = false;
    for c in name.chars() {
        if c == '_' {
            is_upper = !out.is_empty();
        } else if is_upper {
            out.push(c.to_ascii_uppercase());
            is_upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn extract(
    fs: &dyn FileSystem,
    routes: &[(usize, String, String)],
    files: &[String],
    routes_dir: &str,
    server_path: &str,
) -> Project {
    let mut collect = Collect::default();
    let mut project_routes = BTreeMap::new();
    let mut siblings = HashSet::new();
    for (index, tsx, module) in routes {
        let file = rust_sibling(tsx);
        siblings.insert(file.clone());
        let Some(source) = fs.read(&file) else { continue };
        let at = collect.add_file(file.clone(), source);
        if let Some(route) = route_file(&mut collect, at, module) {
            project_routes.insert(*index, route);
        }
        proc_macro2::extra::invalidate_current_thread_spans();
    }
    let prefix = format!("{}/", routes_dir.trim_end_matches('/'));
    for path in files.iter().filter(|path| path.starts_with(&prefix) && path.ends_with(".rs")) {
        if siblings.contains(path) {
            continue;
        }
        let Some(source) = fs.read(path) else { continue };
        let at = collect.add_file(path.clone(), source);
        collect.report(
            at,
            Span::empty(0),
            "This Rust file has no route module beside it: name it like the `.tsx` route it serves",
        );
    }

    let mut context = Context::Unit;
    let mut has_main = false;
    let server = fs.read(server_path).map(|source| {
        let at = collect.add_file(server_path.to_string(), source);
        if let Some(parsed) = collect.parse(at) {
            for item in &parsed.items {
                let Item::Fn(function) = item else { continue };
                if !is_public(&function.vis) {
                    continue;
                }
                match function.sig.ident.to_string().as_str() {
                    "context" if function.sig.asyncness.is_some() => context = Context::Async,
                    "context" => context = Context::Sync,
                    "main" => has_main = true,
                    _ => {}
                }
            }
            let dir = server_path.strip_suffix(".rs").unwrap_or(server_path).to_string();
            server_items(fs, &mut collect, at, &parsed.items, &dir, "server", true);
        }
        proc_macro2::extra::invalidate_current_thread_spans();
        server_path.to_string()
    });

    let mut paths = HashMap::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for index in 0..collect.found.len() {
        let name = collect.found[index].def.name().to_string();
        if name == "Data" || name == "Params" || name == "Context" {
            let (file, span) = (collect.found[index].file, collect.found[index].span);
            collect.report(
                file,
                span,
                format!("`{name}` is a name reze takes in the route module: rename the type"),
            );
            continue;
        }
        if let Some(&first) = seen.get(&name) {
            let (file, span) = (collect.found[index].file, collect.found[index].span);
            let other = collect.files[collect.found[first].file].path.clone();
            collect.report(
                file,
                span,
                format!("`{name}` is also a data type in `{other}`: data type names are unique in the crate"),
            );
            continue;
        }
        seen.insert(name.clone(), index);
        paths.insert(name, collect.found[index].module.clone());
    }

    let mut reachable = HashSet::new();
    let mut stack: Vec<(String, usize, Span)> =
        collect.uses.iter().map(|u| (u.name.clone(), u.file, u.span)).collect();
    while let Some((name, file, span)) = stack.pop() {
        let Some(&index) = seen.get(&name) else {
            collect.report(
                file,
                span,
                format!(
                    "`{name}` is not a `#[derive(reze::Data)]` type of a route file or of `src/server.rs`"
                ),
            );
            continue;
        };
        if !reachable.insert(name.clone()) {
            continue;
        }
        let found = &collect.found[index];
        if !found.is_public {
            let (file, span, module) = (found.file, found.span, found.module.clone());
            collect.report(
                file,
                span,
                format!("`{name}` crosses to the page, so it is `pub` in a `pub` module: `{module}` does not reach it"),
            );
        }
        let found = &collect.found[index];
        let mut refs = Vec::new();
        let owner_fields: Vec<(String, &Vec<FieldDef>)> = match &found.def {
            TypeDef::Struct { name, fields } => vec![(name.clone(), fields)],
            TypeDef::Union { name, variants } => variants
                .iter()
                .map(|variant| (format!("{name}::{}", variant.name), &variant.fields))
                .collect(),
            TypeDef::Enum { .. } => Vec::new(),
        };
        for (owner, fields) in owner_fields {
            for field in fields {
                let mut names = Vec::new();
                named_refs(&field.ty, &mut names);
                let span = found
                    .fields
                    .get(&format!("{owner}.{}", field.name))
                    .copied()
                    .unwrap_or(found.span);
                refs.extend(names.into_iter().map(|name| (name, found.file, span)));
            }
        }
        stack.extend(refs);
    }

    let mut types: Vec<TypeDef> = Vec::new();
    for (name, &index) in &seen {
        if reachable.contains(name) {
            types.push(collect.found[index].def.clone());
        }
    }
    types.sort_by(|a, b| a.name().cmp(b.name()));
    paths.retain(|name, _| reachable.contains(name));
    Project {
        ir: CrateIr { name: CRATE.into(), types },
        paths,
        routes: project_routes,
        server,
        context,
        has_main,
        files: collect.files,
    }
}

fn named_refs(ty: &TypeRef, out: &mut Vec<String>) {
    match ty {
        TypeRef::Named { name } => out.push(name.clone()),
        TypeRef::Option { of }
        | TypeRef::List { of }
        | TypeRef::Set { of }
        | TypeRef::Defer { of } => named_refs(of, out),
        TypeRef::Map { value } => named_refs(value, out),
        TypeRef::Tuple { items } => items.iter().for_each(|item| named_refs(item, out)),
        TypeRef::String
        | TypeRef::Boolean
        | TypeRef::Number { .. }
        | TypeRef::Bigint { .. }
        | TypeRef::Date => {}
    }
}

fn is_public(vis: &Visibility) -> bool {
    match vis {
        Visibility::Public(_) => true,
        Visibility::Restricted(restricted) => restricted.path.is_ident("crate"),
        Visibility::Inherited => false,
    }
}

fn server_items(
    fs: &dyn FileSystem,
    collect: &mut Collect,
    file: usize,
    items: &[Item],
    dir: &str,
    module: &str,
    is_visible: bool,
) {
    collect_types(collect, file, items, module, is_visible);
    for item in items {
        let Item::Mod(child) = item else { continue };
        if child.attrs.iter().any(|attr| attr.path().is_ident("path")) {
            continue;
        }
        let name = child.ident.to_string();
        let child_module = format!("{module}::{name}");
        let child_dir = format!("{dir}/{name}");
        let is_child_visible = is_visible && is_public(&child.vis);
        match &child.content {
            Some((_, inner)) => {
                server_items(fs, collect, file, inner, &child_dir, &child_module, is_child_visible)
            }
            None => {
                let candidates = [format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")];
                let Some((path, source)) = candidates
                    .into_iter()
                    .find_map(|path| fs.read(&path).map(|source| (path, source)))
                else {
                    continue;
                };
                let at = collect.add_file(path, source);
                if let Some(parsed) = collect.parse(at) {
                    server_items(
                        fs,
                        collect,
                        at,
                        &parsed.items,
                        &child_dir,
                        &child_module,
                        is_child_visible,
                    );
                }
            }
        }
    }
}

fn route_file(collect: &mut Collect, file: usize, module: &str) -> Option<Route> {
    let parsed = collect.parse(file)?;
    if let Some(attr) = parsed.attrs.first() {
        collect.report(
            file,
            span_of(attr.span()),
            "reze includes the route file into the generated crate, so it has no inner attributes or `//!` comments",
        );
    }
    let mut route = Route {
        file: collect.files[file].path.clone(),
        module: module.to_string(),
        load: None,
        has_guard: false,
        mutations: Vec::new(),
    };
    for item in &parsed.items {
        match item {
            Item::Mod(child) => collect.report(
                file,
                span_of(child.ident.span()),
                format!(
                    "A route file holds items only: move `mod {}` into the module tree of `src/server.rs`",
                    child.ident
                ),
            ),
            Item::Fn(function) if is_public(&function.vis) => {
                endpoint(collect, file, function, &mut route);
            }
            _ => {}
        }
    }
    collect_types(collect, file, &parsed.items, &format!("routes::{module}"), true);
    Some(route)
}

fn endpoint(collect: &mut Collect, file: usize, function: &ItemFn, route: &mut Route) {
    let name = function.sig.ident.to_string();
    let span = span_of(function.sig.ident.span());
    let (arity, contract) = match name.as_str() {
        "load" => (2, "pub async fn load(params: Params, cx: &Context) -> Result<T>"),
        "guard" => {
            (3, "pub async fn guard(params: Params, request: &Parts, cx: &Context) -> Result<()>")
        }
        _ if function.sig.asyncness.is_none() => return,
        _ => (3, "pub async fn name(input: I, request: &Parts, cx: &Context) -> Result<T>"),
    };
    let inputs: Vec<&Type> = function
        .sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            FnArg::Typed(typed) => Some(&*typed.ty),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let output = match &function.sig.output {
        ReturnType::Type(_, ty) => match path_of(ty) {
            Some((result, arguments)) if result == "Result" && !arguments.is_empty() => {
                Some(arguments[0])
            }
            _ => None,
        },
        ReturnType::Default => None,
    };
    let (Some(output), true, true) =
        (output, function.sig.asyncness.is_some(), inputs.len() == arity)
    else {
        collect.report(file, span, format!("`{name}` has the signature `{contract}`"));
        return;
    };
    let first_span = span_of(inputs[0].span());
    match name.as_str() {
        "load" | "guard" if path_of(inputs[0]).is_none_or(|(ty, _)| ty != "Params") => {
            collect.report(file, first_span, format!("`{name}` takes the route's `Params` first"));
        }
        "load" => match named(output) {
            Some(data) => {
                collect.uses.push(Use { name: data.clone(), file, span: span_of(output.span()) });
                route.load = Some(data);
            }
            None => collect.report(
                file,
                span_of(output.span()),
                "`load` returns `Result<T>` where `T` is a `#[derive(reze::Data)]` struct",
            ),
        },
        "guard" if !is_unit(output) => {
            collect.report(file, span_of(output.span()), "`guard` returns `Result<()>`")
        }
        "guard" => route.has_guard = true,
        _ => {
            let Some(input) = named(inputs[0]) else {
                collect.report(
                    file,
                    first_span,
                    format!("The input of `{name}` is a `#[derive(reze::Data)]` struct"),
                );
                return;
            };
            collect.uses.push(Use { name: input.clone(), file, span: first_span });
            let output = if is_unit(output) {
                None
            } else if let Some(output_name) = named(output) {
                collect.uses.push(Use {
                    name: output_name.clone(),
                    file,
                    span: span_of(output.span()),
                });
                Some(output_name)
            } else {
                collect.report(
                    file,
                    span_of(output.span()),
                    format!("`{name}` returns `Result<()>` or `Result<T>` where `T` is a `#[derive(reze::Data)]` type"),
                );
                return;
            };
            let revalidate = revalidate(collect, file, function);
            route.mutations.push(Mutation {
                export: camel(&name),
                name,
                input,
                output,
                revalidate,
            });
        }
    }
}

fn revalidate(collect: &mut Collect, file: usize, function: &ItemFn) -> Option<Vec<String>> {
    let attr = function.attrs.iter().find(|attr| {
        attr.path().segments.last().is_some_and(|segment| segment.ident == "mutation")
    })?;
    if matches!(attr.meta, syn::Meta::Path(_)) {
        return None;
    }
    let mut patterns = None;
    let parsed = attr.parse_nested_meta(|meta| {
        if !meta.path.is_ident("revalidate") {
            return Err(meta.error("`#[reze::mutation]` takes `revalidate = [\"/pattern\"]`"));
        }
        let array: syn::ExprArray = meta.value()?.parse()?;
        let mut list = Vec::new();
        for element in &array.elems {
            match element {
                Expr::Lit(literal) if let Lit::Str(text) = &literal.lit => list.push(text.value()),
                _ => {
                    return Err(syn::Error::new(
                        element.span(),
                        "a route pattern is a string literal",
                    ));
                }
            }
        }
        patterns = Some(list);
        Ok(())
    });
    if let Err(error) = parsed {
        collect.report(file, span_of(error.span()), error.to_string());
    }
    patterns
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn named(ty: &Type) -> Option<String> {
    match path_of(ty) {
        Some((name, arguments)) if arguments.is_empty() && name.starts_with(char::is_uppercase) => {
            Some(name)
        }
        _ => None,
    }
}

fn derives_data(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().filter(|attr| attr.path().is_ident("derive")).any(|attr| {
        let mut is_found = false;
        let _ = attr.parse_nested_meta(|meta| {
            is_found |= meta.path.segments.last().is_some_and(|segment| segment.ident == "Data");
            Ok(())
        });
        is_found
    })
}

fn collect_types(
    collect: &mut Collect,
    file: usize,
    items: &[Item],
    module: &str,
    is_visible: bool,
) {
    for item in items {
        match item {
            Item::Struct(item) if derives_data(&item.attrs) => {
                let Fields::Named(named) = &item.fields else { continue };
                let name = item.ident.to_string();
                let mut spans = HashMap::new();
                let fields = field_defs(collect, file, named, &name, &mut spans);
                collect.found.push(Found {
                    def: TypeDef::Struct { name, fields },
                    module: module.to_string(),
                    is_public: is_visible && is_public(&item.vis),
                    file,
                    span: span_of(item.ident.span()),
                    fields: spans,
                });
            }
            Item::Enum(item) if derives_data(&item.attrs) => {
                let name = item.ident.to_string();
                let mut spans = HashMap::new();
                let def = if item.variants.iter().all(|v| matches!(v.fields, Fields::Unit)) {
                    TypeDef::Enum {
                        name,
                        variants: item.variants.iter().map(|v| v.ident.to_string()).collect(),
                    }
                } else {
                    let mut variants = Vec::new();
                    for variant in &item.variants {
                        let owner = format!("{name}::{}", variant.ident);
                        let fields = match &variant.fields {
                            Fields::Named(named) => {
                                field_defs(collect, file, named, &owner, &mut spans)
                            }
                            _ => Vec::new(),
                        };
                        variants.push(VariantDef { name: variant.ident.to_string(), fields });
                    }
                    TypeDef::Union { name, variants }
                };
                collect.found.push(Found {
                    def,
                    module: module.to_string(),
                    is_public: is_visible && is_public(&item.vis),
                    file,
                    span: span_of(item.ident.span()),
                    fields: spans,
                });
            }
            _ => {}
        }
    }
}

fn field_defs(
    collect: &mut Collect,
    file: usize,
    named: &FieldsNamed,
    owner: &str,
    spans: &mut HashMap<String, Span>,
) -> Vec<FieldDef> {
    let mut out = Vec::new();
    for field in &named.named {
        let Some(ident) = &field.ident else { continue };
        let span = span_of(field.ty.span());
        spans.insert(format!("{owner}.{ident}"), span);
        match field_ref(&field.ty) {
            Ok(ty) => out.push(FieldDef { name: ident.to_string(), ty }),
            Err(ty) => collect.report(
                file,
                span,
                format!("`{owner}.{ident}` has the type `{ty}`, which has no wire encoding"),
            ),
        }
    }
    out
}

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
