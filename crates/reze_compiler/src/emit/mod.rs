mod server;

pub use server::server;

use std::collections::BTreeMap;

use crate::project::Module;
use crate::routes::RouteTable;

/// Source of `virtual:rz-app`: the route table with each level's module.
pub fn app(
    modules: &[Module<'_>],
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
        out.push_str(&format!(
            "  {{ pattern: {:?}, segments: {segments}, chain: [{}], chunk: {chunk} }},\n",
            node.pattern,
            chain.join(", ")
        ));
    }
    out.push_str("];\n");
    out
}
