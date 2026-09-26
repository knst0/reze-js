use std::fmt::Write;

use super::catalog::{CATALOG, Example};
use super::render::placeholders;

const HEADER: &str = "---
name: reze-compiler-diagnostics
description: Repair guide for reze-js compiler diagnostics. Use when a build, dev server or test prints a bracketed reze code such as [UNKNOWN_ATTRIBUTE], or when diagnostics.jsonl contains reze diagnostics.
---

# reze-js compiler diagnostics

<!-- Generated from crates/reze_compiler/src/diagnostic/catalog.rs. Do not edit by hand: regenerate with `REZE_UPDATE_SKILL=1 cargo test -p reze_compiler --test catalog`. -->

Every diagnostic starts with its code in brackets, `[CODE]`, and says what was seen, why it is wrong and what to do.

## How to act

1. When the diagnostic has `fixes`, apply them: each fix is a list of exact byte edits (`start`, `end`, `text`) into the source that removes the diagnostic.
2. Otherwise follow the **Repair** of the code below.
3. Never suppress, silence or work around an `error` or a `warn`; fix its cause.
4. An `info` explains what the compiler did; it needs no action.

## Reading the `in` path

The `in` line lists what encloses the reported code, outermost first: components as `<Name>` and native elements by tag, for example `in <App> › <TodoList> › li › button`. Start at the last entry.

## JSONL channel

With the Vite plugin option `reze({ diagnostics: { jsonl: \"path\" } })`, every diagnostic of every severity is appended to `path` as one JSON object per line, with `code`, `severity`, `message`, `file`, `start`/`end` (`offset`, 1-based `line`, 0-based UTF-16 `column`), `path`, `labels`, `fixes`, `data`, `docs` and `rendered`.

## Codes
";

pub fn render_skill() -> String {
    let mut out = String::from(HEADER);
    for entry in CATALOG {
        let _ = write!(
            out,
            "\n## {}\n\n**{}** · severity `{}`\n\n> {}\n\n",
            entry.name,
            entry.title,
            entry.severity.as_str(),
            entry.message
        );
        let mut keys: Vec<&str> = placeholders(entry.message)
            .chain(entry.fix.into_iter().flat_map(placeholders))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() {
            out.push_str("`data` keys: none\n\n");
        } else {
            out.push_str("`data` keys:");
            for (i, key) in keys.iter().enumerate() {
                out.push_str(if i == 0 { " `" } else { ", `" });
                out.push_str(key);
                out.push('`');
            }
            out.push_str("\n\n");
        }
        let _ = write!(
            out,
            "Automatic fix: {}\n\n{}\n\n**Repair:** {}\n\n",
            if entry.fix.is_some() { "yes" } else { "no" },
            entry.explanation,
            entry.repair
        );
        match entry.example {
            Example::Pair { bad, good } => {
                let _ = write!(out, "Before:\n\n```tsx\n{bad}```\n\nAfter:\n\n```tsx\n{good}```\n");
            }
            Example::Shows(module) => {
                let _ = write!(out, "Example:\n\n```tsx\n{module}```\n");
            }
        }
    }
    out
}
