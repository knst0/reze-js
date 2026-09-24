//! `verify`: the closed-world and feature-flag checks against modules outside the program
//! (SPEC §15.3, §15.11).

use oxc_span::Span;

use crate::analyze::RUNTIME_MODULES;
use crate::diagnostic::{self, Code, Diagnostic, Report};
use crate::features;
use crate::link::Linked;
use crate::summary::{ImportName, ModuleSummary};

pub struct OutsideModule {
    pub id: String,
    /// `None` when the module never mentions a runtime module.
    pub summary: Option<ModuleSummary>,
    /// Ids of every module it imports, statically or dynamically.
    pub imported: Vec<String>,
}

pub fn verify(linked: &Linked, outside: &[OutsideModule]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for module in outside {
        for imported in &module.imported {
            if linked.closed.contains(imported) {
                let report = Report::new(
                    Code::ProgramOpenImport,
                    Span::empty(0),
                    format!(
                        "`{}` is outside the program but imports `{imported}`, whose exports the \
                         program rewrote assuming it knows every importer. Add the module to \
                         `program.include`, or disable `optimize`.",
                        module.id
                    ),
                )
                .data("module", imported.as_str());
                diagnostics.push(diagnostic::resolve_detached(report, &module.id));
            }
        }
        let Some(summary) = &module.summary else { continue };
        for export in runtime_exports_used(summary) {
            let Some(flag) = features::flag_of(&export) else { continue };
            if linked.features.get(flag.name).copied().unwrap_or(true) {
                continue;
            }
            let report = Report::new(
                Code::FeatureFlagMismatch,
                Span::empty(0),
                format!(
                    "`{}` is outside the program and uses `{export}`, but the program does not, so \
                     `{}` turned it off in the runtime. Add the module to `program.include`, or \
                     force it on with `features: {{ {}: true }}`.",
                    module.id, flag.define, flag.name
                ),
            )
            .data("feature", flag.name)
            .data("export", export.as_str());
            diagnostics.push(diagnostic::resolve_detached(report, &module.id));
        }
    }
    diagnostics
}

/// Runtime exports a module imports by name or reads through a runtime namespace.
fn runtime_exports_used(summary: &ModuleSummary) -> Vec<String> {
    let is_runtime = |specifier: u32| {
        summary.runtime_specifiers.contains(&specifier)
            || RUNTIME_MODULES.contains(&summary.specifiers[specifier as usize].as_str())
    };
    let mut exports = Vec::new();
    for import in &summary.imports {
        if !is_runtime(import.specifier) {
            continue;
        }
        match &import.name {
            ImportName::Named(name) => exports.push(name.clone()),
            ImportName::Namespace => exports.extend(
                summary
                    .uses
                    .iter()
                    .filter(|u| u.target.binding == import.binding)
                    .filter_map(|u| u.target.member.clone()),
            ),
            ImportName::Default => {}
        }
    }
    exports.sort();
    exports.dedup();
    exports
}
