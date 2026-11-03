mod server;

pub(crate) use server::head_key;
pub use server::server;

use std::collections::BTreeMap;

use crate::analyze::{Analysis, EndpointKind};
use crate::project::Module;
use crate::routes::{RouteTable, Segment};

/// Source of `virtual:rz-routes`: the route table the client matches URLs against, and the
/// inputs that key each segment: its params and, when its loader reads them, the search params.
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
        let search = modules.iter().position(|m| m.path == node.file).is_some_and(|module| {
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
            "  {index}: [{}, {}],\n",
            serde_json::to_string(&names).unwrap(),
            search
        ));
    }
    out.push_str("};\n");
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
    chunks: &BTreeMap<usize, String>,
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
        let chunk = if chunks.contains_key(&index) { index.to_string() } else { "null".into() };
        let fixed = routes
            .chain(index)
            .iter()
            .all(|&level| reads_only_params(modules, analysis, &routes.nodes[level].file));
        out.push_str(&format!(
            "  {{ pattern: {:?}, segments: {segments}, chain: [{}], chunk: {chunk}, static: {fixed} }},\n",
            node.pattern,
            chain.join(", ")
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
    for node in &routes.nodes {
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
    format!(
        "// Generated by reze-js from the route files. Do not edit.\nimport type {{ RouteData }} from \"reze-js\";\n\ndeclare module \"reze-js\" {{\n  interface Register {{\n    routes: {{\n{pages}    }};\n    layouts: {{\n{layouts}    }};\n  }}\n}}\n"
    )
}
