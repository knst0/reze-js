use std::{env, fmt::Write, fs, path::PathBuf};

use serde_json::Value;

fn main() {
    println!("cargo:rerun-if-changed=src/html-data.json");
    let tables: Value = serde_json::from_str(include_str!("src/html-data.json"))
        .expect("valid shared HTML metadata");
    let mut code = String::new();
    for (function, table) in [
        ("is_property", "Properties"),
        ("is_delegated_event", "DelegatedEvents"),
        ("is_void", "VoidElements"),
        ("is_svg_element", "SVGElements"),
    ] {
        let names = tables[table]
            .as_object()
            .expect(table)
            .keys()
            .filter(|name| table != "SVGElements" || name.as_str() != "svg")
            .map(|name| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(" | ");
        writeln!(code, "pub fn {function}(name: &str) -> bool {{ matches!(name, {names}) }}")
            .unwrap();
    }
    code.push_str(
        "pub fn attribute_namespace(prefix: &str) -> Option<&'static str> { match prefix {\n",
    );
    for (prefix, namespace) in
        tables["AttributeNamespaces"].as_object().expect("AttributeNamespaces")
    {
        writeln!(code, "{prefix:?} => Some({:?}),", namespace.as_str().expect("namespace URI"))
            .unwrap();
    }
    code.push_str("_ => None } }\n");
    code.push_str("fn escape_html_byte(byte: u8, attribute: bool) -> Option<&'static str> { match (byte, attribute) {\n");
    for (table, attribute) in [("TextEscapes", false), ("AttributeEscapes", true)] {
        for (character, escaped) in tables[table].as_object().expect(table) {
            assert!(
                character.len() == 1 && character.is_ascii(),
                "HTML escape must be one ASCII byte"
            );
            writeln!(
                code,
                "({}, {attribute}) => Some({:?}),",
                character.as_bytes()[0],
                escaped.as_str().expect("HTML escape")
            )
            .unwrap();
        }
    }
    code.push_str("_ => None } }\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(output.join("html-data.rs"), code).expect("write generated HTML predicates");
}
