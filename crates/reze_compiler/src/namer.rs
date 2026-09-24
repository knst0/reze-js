//! Fresh identifiers for one module: lowering and emission draw from the same pool, and every
//! name avoids every identifier of the source.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use oxc_semantic::Scoping;

pub struct Namer<'s> {
    source_names: HashSet<&'s str>,
    generated: HashSet<String>,
    next_suffix: HashMap<String, u32>,
}

impl<'s> Namer<'s> {
    pub fn new(scoping: &'s Scoping) -> Self {
        let mut source_names: HashSet<&'s str> = scoping.symbol_names().collect();
        source_names.extend(scoping.root_unresolved_references().keys().map(|name| name.as_str()));
        Self { source_names, generated: HashSet::new(), next_suffix: HashMap::new() }
    }

    /// `base`, or `base` with the smallest numeric suffix (from 2) that is still free.
    pub fn fresh(&mut self, base: &str) -> String {
        let suffix = self.next_suffix.entry(base.to_string()).or_insert(1);
        let mut name = String::with_capacity(base.len() + 2);
        loop {
            name.clear();
            name.push_str(base);
            if *suffix > 1 {
                let _ = write!(name, "{suffix}");
            }
            *suffix += 1;
            if !self.source_names.contains(name.as_str()) && !self.generated.contains(&name) {
                self.generated.insert(name.clone());
                return name;
            }
        }
    }
}
