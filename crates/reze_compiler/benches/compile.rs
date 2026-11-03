extern crate codspeed_divan_compat as divan;

use std::path::PathBuf;

use reze_compiler::{DiskFs, Options, compile_project};

fn main() {
    divan::main();
}

#[divan::bench(args = ["menu", "static", "todos"])]
fn app(bencher: divan::Bencher, name: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/apps").join(name);
    let fs = DiskFs::new(&root);
    let options = Options::default();
    bencher.bench(|| compile_project(&fs, &options));
}
