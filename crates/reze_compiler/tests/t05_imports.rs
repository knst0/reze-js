mod common;

use reze_compiler::{Options, compile};

#[test]
fn import_removal_preserves_source_marks_after_the_folded_declaration() {
    let source = "import { signal as make } from '@rezejs/signals'; const [a] = make(1); console.log(a()); const end = 2;";
    let out = compile(source, "t05.tsx", &Options::default()).unwrap().unwrap();
    common::assert_binding_origins(source, &out, &["end"]);
}
