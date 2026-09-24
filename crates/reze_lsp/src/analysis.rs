use std::collections::BTreeMap;

use reze_compiler::{
    Diagnostic, LinkOptions, ModuleInput, Options, SummaryOptions, Target, compile, link, summarize,
};

pub struct Config {
    pub module_name: String,
    pub optimize: bool,
    pub islands: bool,
    pub root: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            module_name: "reze-js".to_string(),
            optimize: true,
            islands: true,
            root: String::new(),
        }
    }
}

impl Config {
    pub fn link_options(&self) -> LinkOptions {
        LinkOptions { optimize: self.optimize, islands: self.islands, root: self.root.clone() }
    }
}

pub fn compile_single(source: &str, filename: &str, config: &Config) -> Vec<Diagnostic> {
    let options = Options {
        module_name: config.module_name.clone(),
        source_map: false,
        optimize: config.optimize,
        target: Target::Client,
        debug_names: false,
        facts: None,
    };
    match compile(source, filename, &options) {
        Ok(None) => Vec::new(),
        Ok(Some(output)) => output.diagnostics,
        Err(errors) => errors,
    }
}

pub fn analyze_program(
    files: &BTreeMap<String, String>,
    config: &Config,
) -> BTreeMap<String, Vec<Diagnostic>> {
    let mut summaries = BTreeMap::new();
    let mut specifiers_of = BTreeMap::new();
    let mut failed: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
    let summary_options = SummaryOptions { module_name: config.module_name.clone() };
    for (id, text) in files {
        match summarize(text, id, &summary_options) {
            Ok(summary) => {
                specifiers_of.insert(id.clone(), summary.specifiers.clone());
                summaries.insert(id.clone(), summary);
            }
            Err(errors) => {
                failed.insert(id.clone(), errors);
            }
        }
    }
    let mut inputs: Vec<ModuleInput> = Vec::new();
    for (id, summary) in &summaries {
        let specifiers = specifiers_of.get(id).cloned().unwrap_or_default();
        let ids: Vec<String> = summaries.keys().cloned().collect();
        let mut resolved = Vec::with_capacity(specifiers.len());
        for specifier in &specifiers {
            resolved.push(resolve_specifier(id, specifier, &ids));
        }
        inputs.push(ModuleInput {
            id: id.clone(),
            summary: summary.clone(),
            resolved,
            is_entry: false,
        });
    }
    let linked = link(&inputs, &config.link_options());
    let mut out: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
    for (id, text) in files {
        if let Some(errors) = failed.get(id) {
            out.insert(id.clone(), errors.clone());
            continue;
        }
        let Some(facts) = linked.facts.get(id) else {
            out.insert(id.clone(), Vec::new());
            continue;
        };
        let options = Options {
            module_name: config.module_name.clone(),
            source_map: false,
            optimize: config.optimize,
            target: Target::Client,
            debug_names: false,
            facts: Some(facts.clone()),
        };
        match compile(text, id, &options) {
            Ok(None) => {
                out.insert(id.clone(), Vec::new());
            }
            Ok(Some(output)) => {
                out.insert(id.clone(), output.diagnostics);
            }
            Err(errors) => {
                out.insert(id.clone(), errors);
            }
        }
    }
    out
}

const PROBE_SUFFIXES: [&str; 7] = ["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx"];

pub fn resolve_specifier(from: &str, specifier: &str, ids: &[String]) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let from_path = path_of_id(from);
    let base = match from_path.rfind('/') {
        Some(index) => &from_path[..index + 1],
        None => "",
    };
    let joined = join_relative(base, specifier);
    let normalized = normalize_dots(&joined);
    for suffix in PROBE_SUFFIXES {
        let candidate_path = format!("{normalized}{suffix}");
        if let Some(id) = id_for_path(ids, &candidate_path) {
            return Some(id);
        }
    }
    None
}

fn path_of_id(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("file://") {
        return percent_decode(rest);
    }
    id.to_string()
}

fn id_for_path(ids: &[String], path: &str) -> Option<String> {
    for id in ids {
        if path_of_id(id) == *path {
            return Some(id.clone());
        }
    }
    None
}

fn join_relative(base: &str, specifier: &str) -> String {
    let mut out = base.to_string();
    out.push_str(specifier);
    out
}

fn normalize_dots(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        } else if part == ".." {
            if parts.last().is_some_and(|last| *last != "..") {
                parts.pop();
            } else if !absolute {
                parts.push("..");
            }
        } else {
            parts.push(part);
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
