use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Project files addressed by posix paths relative to the project root.
pub trait FileSystem {
    fn read(&self, path: &str) -> Option<String>;
    /// Every file under `dir`, recursively, as paths relative to the project root; `""` is the
    /// project root.
    fn list(&self, dir: &str) -> Vec<String>;
}

#[derive(Default)]
pub struct MemoryFs {
    files: BTreeMap<String, String>,
}

impl MemoryFs {
    pub fn new<P: Into<String>, S: Into<String>>(files: impl IntoIterator<Item = (P, S)>) -> Self {
        Self { files: files.into_iter().map(|(p, s)| (p.into(), s.into())).collect() }
    }

    pub fn insert(&mut self, path: impl Into<String>, source: impl Into<String>) {
        self.files.insert(path.into(), source.into());
    }
}

impl FileSystem for MemoryFs {
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn list(&self, dir: &str) -> Vec<String> {
        let dir = dir.trim_end_matches('/');
        if dir.is_empty() {
            return self.files.keys().cloned().collect();
        }
        let prefix = format!("{dir}/");
        self.files.keys().filter(|path| path.starts_with(&prefix)).cloned().collect()
    }
}

pub struct DiskFs {
    root: PathBuf,
}

impl DiskFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn walk(&self, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if entry.file_name() != "node_modules" {
                    self.walk(&path, out);
                }
            } else if let Ok(relative) = path.strip_prefix(&self.root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

impl FileSystem for DiskFs {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(path)).ok()
    }

    fn list(&self, dir: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&self.root.join(dir), &mut out);
        out.sort();
        out
    }
}

/// Joins `relative` onto the directory of `from`, resolving `.` and `..`.
pub fn join(from: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}
