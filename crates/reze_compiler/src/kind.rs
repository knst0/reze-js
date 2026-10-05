#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A number or bigint, whose text is never empty.
    Numeric,
    String,
}

pub(crate) const STRING_METHODS: [&str; 7] =
    ["toString", "toFixed", "join", "toUpperCase", "toLowerCase", "trim", "padStart"];
