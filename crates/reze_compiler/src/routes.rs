use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "lowercase")]
pub enum Segment {
    Static(String),
    Param(String),
    Optional(String),
    Splat(String),
}

impl Segment {
    pub fn name(&self) -> Option<&str> {
        match self {
            Segment::Static(_) => None,
            Segment::Param(n) | Segment::Optional(n) | Segment::Splat(n) => Some(n),
        }
    }
}

/// One route file: a layout when other files nest under it, a page otherwise.
#[derive(Clone, Debug, Serialize)]
pub struct RouteNode {
    /// Project-relative path of the module.
    pub file: String,
    /// Full URL pattern, `/blog/:id`.
    pub pattern: String,
    pub segments: Vec<Segment>,
    pub parent: Option<usize>,
    pub is_layout: bool,
}

#[derive(Default, Debug, Serialize)]
pub struct RouteTable {
    pub nodes: Vec<RouteNode>,
    /// The root layout, `src/routes.tsx`.
    pub root: Option<usize>,
}

#[derive(Debug)]
pub struct RouteError {
    pub file: String,
    pub message: String,
}

const EXTENSIONS: [&str; 4] = [".tsx", ".ts", ".jsx", ".js"];

fn strip_extension(file: &str) -> Option<&str> {
    EXTENSIONS.iter().find_map(|ext| file.strip_suffix(ext))
}

fn is_ignored(relative: &str) -> bool {
    relative.ends_with(".d.ts")
        || relative.split('/').any(|part| part.starts_with('_') || part.starts_with('.'))
        || relative.rsplit('/').next().is_some_and(|name| {
            let mut parts = name.split('.');
            parts.next();
            parts.any(|p| p == "test" || p == "spec")
        })
}

fn parse_segment(segment: &str, is_last: bool) -> Option<Segment> {
    if is_last && segment == "index" {
        return None;
    }
    if segment.starts_with('(') && segment.ends_with(')') {
        return None;
    }
    if let Some(name) = segment.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        return Some(Segment::Optional(name.into()));
    }
    if let Some(name) = segment.strip_prefix("[...").and_then(|s| s.strip_suffix(']')) {
        return Some(Segment::Splat(name.into()));
    }
    if let Some(name) = segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Some(Segment::Param(name.into()));
    }
    Some(Segment::Static(segment.to_ascii_lowercase()))
}

pub fn pattern_of(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "/".into();
    }
    let mut out = String::new();
    for segment in segments {
        out.push('/');
        match segment {
            Segment::Static(s) => out.push_str(s),
            Segment::Param(n) => {
                out.push(':');
                out.push_str(n);
            }
            Segment::Optional(n) => {
                out.push(':');
                out.push_str(n);
                out.push('?');
            }
            Segment::Splat(n) => {
                out.push('*');
                out.push_str(n);
            }
        }
    }
    out
}

/// Builds the route table from project-relative `files` under `dir`. `dir` plus an extension,
/// such as `src/routes.tsx`, is the root layout.
pub fn scan(dir: &str, files: &[String]) -> (RouteTable, Vec<RouteError>) {
    let dir = dir.trim_end_matches('/');
    let mut errors = Vec::new();
    let mut by_id: BTreeMap<String, String> = BTreeMap::new();
    let root_layout = files.iter().find(|f| strip_extension(f) == Some(dir)).cloned();
    for file in files {
        let Some(relative) = file.strip_prefix(dir).and_then(|r| r.strip_prefix('/')) else {
            continue;
        };
        if is_ignored(relative) {
            continue;
        }
        let Some(id) = strip_extension(relative) else { continue };
        if let Some(existing) = by_id.insert(id.to_string(), file.clone()) {
            errors.push(RouteError {
                file: file.clone(),
                message: format!("`{existing}` and `{file}` are the same route"),
            });
        }
    }
    let mut table = RouteTable::default();
    let mut index_of: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(file) = root_layout {
        table.nodes.push(RouteNode {
            file,
            pattern: "/".into(),
            segments: Vec::new(),
            parent: None,
            is_layout: true,
        });
        index_of.insert(String::new(), 0);
        table.root = Some(0);
    }
    let ids: Vec<String> = by_id.keys().cloned().collect();
    for id in &ids {
        let parts: Vec<&str> = id.split('/').collect();
        let mut parent_depth = parts.len() - 1;
        while parent_depth > 0 && !by_id.contains_key(&parts[..parent_depth].join("/")) {
            parent_depth -= 1;
        }
        let parent = if parent_depth > 0 {
            index_of.get(&parts[..parent_depth].join("/")).copied()
        } else {
            index_of.get("").copied()
        };
        let mut segments = parent.map(|p| table.nodes[p].segments.clone()).unwrap_or_default();
        let own = &parts[parent_depth..];
        for (i, part) in own.iter().enumerate() {
            if let Some(segment) = parse_segment(part, i == own.len() - 1) {
                segments.push(segment);
            }
        }
        let pattern = pattern_of(&segments);
        if let Err(message) = validate(&segments) {
            errors.push(RouteError { file: by_id[id].clone(), message });
        }
        if let Some(p) = parent {
            table.nodes[p].is_layout = true;
        }
        index_of.insert(id.clone(), table.nodes.len());
        table.nodes.push(RouteNode {
            file: by_id[id].clone(),
            pattern,
            segments,
            parent,
            is_layout: false,
        });
    }
    for id in &ids {
        let has_children = ids.iter().any(|other| other.starts_with(&format!("{id}/")));
        if has_children {
            table.nodes[index_of[id]].is_layout = true;
        }
    }
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for node in table.nodes.iter().filter(|n| !n.is_layout) {
        for shape in shapes(&node.segments) {
            if let Some(other) = seen.insert(shape, node.file.clone()) {
                errors.push(RouteError {
                    file: node.file.clone(),
                    message: format!("`{other}` and `{}` both match `{}`", node.file, node.pattern),
                });
            }
        }
    }
    (table, errors)
}

fn validate(segments: &[Segment]) -> Result<(), String> {
    let mut names = Vec::new();
    let mut optional_seen = false;
    for (i, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Static(_) | Segment::Param(_) if optional_seen => {
                return Err("an optional param must be followed only by optional params".into());
            }
            Segment::Splat(name) if i != segments.len() - 1 => {
                return Err(format!("catch-all `{name}` must be the last segment"));
            }
            Segment::Optional(_) => optional_seen = true,
            _ => {}
        }
        if let Some(name) = segment.name() {
            if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(format!("`{name}` is not a valid param name"));
            }
            if names.contains(&name) {
                return Err(format!("param `{name}` appears twice"));
            }
            names.push(name);
        }
    }
    Ok(())
}

fn shapes(segments: &[Segment]) -> Vec<String> {
    let mut shapes = vec![String::new()];
    for segment in segments {
        match segment {
            Segment::Optional(_) => {
                let extended: Vec<String> = shapes.iter().map(|s| format!("{s}/:")).collect();
                shapes.extend(extended);
            }
            Segment::Static(s) => shapes.iter_mut().for_each(|shape| *shape += &format!("/{s}")),
            Segment::Param(_) => shapes.iter_mut().for_each(|shape| *shape += "/:"),
            Segment::Splat(_) => shapes.iter_mut().for_each(|shape| *shape += "/*"),
        }
    }
    shapes
}

impl RouteTable {
    /// Layouts from the root down to `node`, then `node` itself.
    pub fn chain(&self, node: usize) -> Vec<usize> {
        let mut chain = vec![node];
        let mut current = node;
        while let Some(parent) = self.nodes[current].parent {
            chain.push(parent);
            current = parent;
        }
        chain.reverse();
        chain
    }

    pub fn by_file(&self, file: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.file == file)
    }
}
