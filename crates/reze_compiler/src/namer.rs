use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use oxc_semantic::Scoping;

/// Fresh identifiers for one module, avoiding every symbol and unresolved name of the source.
pub struct Namer<'s> {
    taken: HashSet<&'s str>,
    generated: HashSet<String>,
    next_suffix: HashMap<String, u32>,
}

impl<'s> Namer<'s> {
    pub fn new(scoping: &'s Scoping) -> Self {
        let mut taken: HashSet<&'s str> = scoping.symbol_names().collect();
        taken.extend(scoping.root_unresolved_references().keys().map(|name| name.as_str()));
        Self { taken, generated: HashSet::new(), next_suffix: HashMap::new() }
    }

    /// `base`, or `base` with the smallest free suffix from 2.
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
            if !self.taken.contains(name.as_str()) && !self.generated.contains(&name) {
                self.generated.insert(name.clone());
                return name;
            }
        }
    }
}
