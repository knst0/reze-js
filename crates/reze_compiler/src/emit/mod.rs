mod server;

pub use server::server;
pub(crate) use server::{head_key, template_escape};

use std::collections::{BTreeMap, HashSet};

use crate::analyze::{Analysis, Api, EndpointKind};
use crate::client::RouteChunk;
use crate::project::Module;
use crate::routes::{RouteTable, Segment};

/// Source of `virtual:rz-routes`: the route table the client matches URLs against, and the
/// inputs that key each segment: its params, the search params when its loader or a router hook
/// reads them, and the path when a router hook reads it.
pub fn navigation(modules: &[Module<'_>], analysis: &Analysis<'_>, routes: &RouteTable) -> String {
    let mut out = String::from("export const routes = [\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        if node.is_layout {
            continue;
        }
        let chain: Vec<String> = routes.chain(index).iter().map(usize::to_string).collect();
        let segments = serde_json::to_string(&node.segments).unwrap();
        out.push_str(&format!("  {{ segments: {segments}, chain: [{}] }},\n", chain.join(", ")));
    }
    out.push_str("];\nexport const keys = {\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        let names: Vec<&str> = node.segments.iter().filter_map(Segment::name).collect();
        let module = modules.iter().position(|m| m.path == node.file);
        let hooks = module.map_or_else(Vec::new, |module| hooks(modules, analysis, module));
        let path = hooks.contains(&Api::UseLocation) || hooks.contains(&Api::UseLinkState);
        let search = hooks.contains(&Api::UseLocation)
            || hooks.contains(&Api::UseSearchParams)
            || module.is_some_and(|module| {
                analysis.facts[module].endpoints.iter().any(|endpoint| {
                    endpoint.kind == EndpointKind::Loader
                        && endpoint.export == "load"
                        && endpoint
                            .reads
                            .as_ref()
                            .is_none_or(|reads| reads.iter().any(|r| r == "search"))
                })
            });
        out.push_str(&format!(
            "  {index}: [{}, {search}, {path}],\n",
            serde_json::to_string(&names).unwrap(),
        ));
    }
    out.push_str("};\n");
    out
}

/// The router hooks that `module` and the modules it imports call.
fn hooks(modules: &[Module<'_>], analysis: &Analysis<'_>, module: usize) -> Vec<Api> {
    let mut seen = HashSet::from([module]);
    let mut pending = vec![module];
    let mut out = Vec::new();
    while let Some(module) = pending.pop() {
        out.extend(analysis.facts[module].apis.values().copied().filter(|api| {
            matches!(api, Api::UseLocation | Api::UseSearchParams | Api::UseLinkState)
        }));
        for &next in modules[module].resolved.values() {
            if seen.insert(next) {
                pending.push(next);
            }
        }
    }
    out
}

/// Whether the loader of `file` reads only params, so its result is the same for every request.
fn reads_only_params(modules: &[Module<'_>], analysis: &Analysis<'_>, file: &str) -> bool {
    modules.iter().position(|m| m.path == file).is_none_or(|module| {
        analysis.facts[module].endpoints.iter().all(|endpoint| {
            endpoint.kind != EndpointKind::Loader
                || endpoint.export != "load"
                || endpoint.reads.as_ref().is_some_and(|reads| reads.iter().all(|r| r == "params"))
        })
    })
}

/// Source of `virtual:rz-app`: the route table with each level's module.
pub fn app(
    modules: &[Module<'_>],
    analysis: &Analysis<'_>,
    routes: &RouteTable,
    chunks: &BTreeMap<usize, RouteChunk>,
) -> String {
    let mut out = String::new();
    let module_of = |file: &str| modules.iter().position(|m| m.path == file);
    for (index, node) in routes.nodes.iter().enumerate() {
        if module_of(&node.file).is_some() {
            out.push_str(&format!(
                "import * as $m{index} from {:?};\n",
                format!("./{}", node.file)
            ));
        }
    }
    out.push_str("export default [\n");
    for (index, node) in routes.nodes.iter().enumerate() {
        if node.is_layout {
            continue;
        }
        let chain: Vec<String> = routes.chain(index).iter().map(|i| format!("$m{i}")).collect();
        let segments = serde_json::to_string(&node.segments).unwrap();
        let chunk = match chunks.get(&index) {
            Some(RouteChunk::Route(key)) => format!("{key:?}"),
            Some(RouteChunk::Factories(keys)) => {
                let keys: Vec<String> =
                    keys.iter().map(|(id, key)| format!("{id}: {key:?}")).collect();
                format!("{{ {} }}", keys.join(", "))
            }
            None => "null".into(),
        };
        let fixed = routes
            .chain(index)
            .iter()
            .all(|&level| reads_only_params(modules, analysis, &routes.nodes[level].file));
        let patterns: Vec<String> = routes
            .chain(index)
            .iter()
            .map(|&level| format!("{:?}", routes.nodes[level].pattern))
            .collect();
        out.push_str(&format!(
            "  {{ pattern: {:?}, segments: {segments}, chain: [{}], patterns: [{}], chunk: {chunk}, static: {fixed} }},\n",
            node.pattern,
            chain.join(", "),
            patterns.join(", ")
        ));
    }
    out.push_str("];\n");
    let root = routes.root.filter(|&node| module_of(&routes.nodes[node].file).is_some());
    out.push_str(&format!(
        "export const root = [{}];\n",
        root.map(|node| format!("$m{node}")).unwrap_or_default()
    ));
    out
}

/// Source of `routes.gen.d.ts` in `dir`: the props of every page and layout by pattern.
pub fn types(routes: &RouteTable, dir: &str) -> String {
    let mut pages = String::new();
    let mut layouts = String::new();
    let mut seen = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for node in &routes.nodes {
        if !node.is_layout {
            let mut shapes = vec![String::new()];
            for segment in &node.segments {
                match segment {
                    Segment::Static(name) => {
                        shapes.iter_mut().for_each(|s| *s += &format!("/{name}"))
                    }
                    Segment::Param(_) | Segment::Splat(_) => {
                        shapes.iter_mut().for_each(|s| *s += "/${string}")
                    }
                    Segment::Optional(_) => {
                        let with: Vec<String> =
                            shapes.iter().map(|s| format!("{s}/${{string}}")).collect();
                        shapes.extend(with);
                    }
                }
            }
            for shape in shapes {
                let shape = if shape.is_empty() { "/".to_string() } else { shape };
                let shape = format!("`{shape}`");
                if !paths.contains(&shape) {
                    paths.push(shape);
                }
            }
        }
        let params: Vec<String> = node
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Static(_) => None,
                Segment::Param(name) | Segment::Splat(name) => Some(format!("{name}: string")),
                Segment::Optional(name) => Some(format!("{name}?: string")),
            })
            .collect();
        let relative = node.file.strip_prefix(dir).unwrap_or(&node.file).trim_start_matches('/');
        let specifier =
            format!("./{}", relative.rsplit_once('.').map_or(relative, |(stem, _)| stem));
        let params = if params.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", params.join("; "))
        };
        let entry = format!(
            "      {:?}: {{ params: {params}; data: RouteData<typeof import({specifier:?})> }};\n",
            node.pattern
        );
        if !node.is_layout {
            pages.push_str(&entry);
        } else if !seen.contains(&node.pattern) {
            seen.push(node.pattern.clone());
            layouts.push_str(&entry);
        }
    }
    let paths = if paths.is_empty() { "never".to_string() } else { paths.join(" | ") };
    format!(
        "// Generated by reze-js from the route files. Do not edit.\nimport type {{ RouteData }} from \"reze-js\";\n\ndeclare module \"reze-js\" {{\n  interface Register {{\n    routes: {{\n{pages}    }};\n    layouts: {{\n{layouts}    }};\n    paths: {paths};\n  }}\n}}\n"
    )
}
