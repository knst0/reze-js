use oxc_allocator::Allocator;
use oxc_ast::ast::BindingIdentifier;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use reze_compiler::Output;

struct Bindings<'s> {
    names: &'s [&'s str],
    spans: Vec<Option<Span>>,
}

impl<'a> Visit<'a> for Bindings<'_> {
    fn visit_binding_identifier(&mut self, binding: &BindingIdentifier<'a>) {
        if let Some(index) = self.names.iter().position(|name| *name == binding.name.as_str()) {
            assert!(self.spans[index].replace(binding.span).is_none(), "unique fixture binding");
        }
    }
}

fn positions(source: &str, names: &[&str]) -> Vec<(u32, u32)> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "source-map fixture must parse: {:?}",
        parsed.diagnostics
    );
    let mut bindings = Bindings { names, spans: vec![None; names.len()] };
    bindings.visit_program(&parsed.program);
    bindings
        .spans
        .into_iter()
        .map(|span| {
            let prefix = &source[..span.expect("fixture binding").start as usize];
            (
                prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
                prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
            )
        })
        .collect()
}

pub fn assert_binding_origins(source: &str, output: &Output, names: &[&str]) {
    let source_positions = positions(source, names);
    let generated_positions = positions(&output.code, names);
    let map =
        oxc_sourcemap::SourceMap::from_json_string(output.map.as_deref().expect("source map"))
            .expect("valid source map");
    let lookup = map.generate_lookup_table();
    for ((name, source), generated) in names.iter().zip(source_positions).zip(generated_positions) {
        let token =
            map.lookup_token(&lookup, generated.0, generated.1).expect("mapped user binding");
        assert_eq!((token.get_src_line(), token.get_src_col()), source, "origin of {name}");
    }
}
