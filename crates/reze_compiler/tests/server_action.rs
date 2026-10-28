use reze_compiler::{Code, Options, compile};

fn errors(source: &str) -> Vec<Code> {
    match compile(source, "src/action.tsx", &Options::default()) {
        Ok(Some(out)) => out.diagnostics.iter().map(|d| d.code).collect(),
        Ok(None) => Vec::new(),
        Err(errors) => errors.iter().map(|d| d.code).collect(),
    }
}

#[test]
fn a_server_action_without_a_schema_is_refused() {
    let source = "import { server } from \"reze-js\";\nexport const save = server(async (input: { title: string }) => input.title);";
    assert_eq!(errors(source), [Code::ServerInputSchema]);
}

#[test]
fn a_literal_schema_and_a_module_schema_are_accepted() {
    let source = "import { server } from \"reze-js\";\nimport * as s from \"reze-js/schema\";\nexport const save = server(s.object({ title: s.string() }), async (input) => input.title);";
    assert!(errors(source).is_empty());
    let shared = "import { server } from \"reze-js\";\nimport * as s from \"reze-js/schema\";\nconst input = s.object({ title: s.string() });\nexport const save = server(input, async (value) => value.title);";
    assert!(errors(shared).is_empty());
}

#[test]
fn a_schema_from_another_module_is_not_guessed() {
    let source = "import { server } from \"reze-js\";\nimport { input } from \"./schema\";\nexport const save = server(input, async (value) => value);";
    assert_eq!(errors(source), [Code::ServerInputSchema]);
}
