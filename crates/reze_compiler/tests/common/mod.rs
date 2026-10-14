#![allow(dead_code)]

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, compile};

pub fn without_source_map() -> Options {
    Options { source_map: false, ..Options::default() }
}

pub fn assert_valid(code: &str, source_type: SourceType) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

pub fn output_for(source: &str, options: &Options) -> Output {
    let out = compile(source, "test.tsx", options).expect("compiles").expect("rewrites");
    assert_valid(&out.code, SourceType::tsx());
    out
}

pub fn checked(source: &str, options: &Options) -> (String, Vec<Diagnostic>) {
    let (code, diagnostics) = match compile(source, "test.tsx", options).expect("compiles") {
        Some(out) => (out.code, out.diagnostics),
        None => (source.to_string(), Vec::new()),
    };
    assert_valid(&code, SourceType::tsx());
    (code, diagnostics)
}

pub fn compiled(source: &str, options: &Options) -> String {
    checked(source, options).0
}

pub fn diagnostics(source: &str, options: &Options) -> Vec<Diagnostic> {
    match compile(source, "test.tsx", options) {
        Ok(Some(out)) => out.diagnostics,
        Ok(None) => Vec::new(),
        Err(errors) => errors,
    }
}

pub fn errors(source: &str, options: &Options) -> Vec<Diagnostic> {
    compile(source, "test.tsx", options).err().expect("fails")
}

pub fn only(source: &str, options: &Options, code: Code) -> Diagnostic {
    let found: Vec<_> =
        diagnostics(source, options).into_iter().filter(|d| d.code == code).collect();
    assert_eq!(found.len(), 1, "{source}\n{found:?}");
    found.into_iter().next().unwrap()
}

pub fn apply_fix(source: &str, diagnostic: &Diagnostic) -> String {
    let mut edits: Vec<_> = diagnostic.fixes[0].edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut fixed = source.to_string();
    for edit in edits {
        fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    }
    fixed
}
