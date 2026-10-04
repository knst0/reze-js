mod common;

use reze_compiler::{Options, Output, compile};

fn run(source: &str) -> Output {
    compile(source, "t06.tsx", &Options::default()).expect("compiles").expect("rewrites")
}

#[test]
fn unicode_marks_follow_replacements_and_inserted_declaration() {
    let source = "const a = <p>😀abcdefghijklmnopqrstuvwxyz0123456789</p>; const tail = '😀';\r\nconst b = <p>😀abcdefghijklmnopqrstuvwxyz0123456789</p>; const end = 2;";
    let out = run(source);
    common::assert_binding_origins(source, &out, &["tail", "end"]);
}

#[test]
fn escaped_literals_and_multiple_groups_relocate_source_marks() {
    let source = "const a = <p title={'a\"b'}>abcdefghijklmnopqrstuvwxyz0123456789</p>; const b = <p title={'a\"b'}>abcdefghijklmnopqrstuvwxyz0123456789</p>; const c = <p>differentabcdefghijklmnopqrstuvwxyz0123456789</p>; const d = <p>differentabcdefghijklmnopqrstuvwxyz0123456789</p>; const end = 1;";
    let out = run(source);
    common::assert_binding_origins(source, &out, &["end"]);
}
