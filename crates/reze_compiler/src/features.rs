//! Runtime features behind define flags (SPEC §15.11).

use std::collections::BTreeMap;

pub struct Flag {
    /// Key in `Features` and in the plugin's `features` option.
    pub name: &'static str,
    /// Identifier the plugin replaces with `true`/`false`.
    pub define: &'static str,
    /// Runtime exports whose use in the program turns the flag on.
    pub exports: &'static [&'static str],
}

/// `hydration` is decided by the build target, not by the program, so `link` leaves it out.
pub const FLAGS: &[Flag] = &[
    Flag { name: "hydration", define: "__REZE_HYDRATION__", exports: &[] },
    Flag { name: "loading", define: "__REZE_LOADING__", exports: &["Loading"] },
];

/// Values of the program-decided flags, by `Flag::name`.
pub type Features = BTreeMap<String, bool>;

/// The flag that removes `export` from the runtime when it is off.
pub fn flag_of(export: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|flag| flag.exports.contains(&export))
}

pub fn program_flags() -> impl Iterator<Item = &'static Flag> {
    FLAGS.iter().filter(|flag| !flag.exports.is_empty())
}
