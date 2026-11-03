//! `reze-ir <crate-dir> <out.json>`: writes the type IR of a crate's `#[derive(reze::Data)]` types,
//! which the Vite plugin reads for `virtual:reze-types/<crate>`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, out] = args.as_slice() else {
        eprintln!("usage: reze-ir <crate-dir> <out.json>");
        std::process::exit(2);
    };
    let ir = reze_build::extract(dir).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let json = serde_json::to_string_pretty(&ir).expect("the IR serializes");
    if let Err(error) = std::fs::write(out, format!("{json}\n")) {
        eprintln!("{out}: {error}");
        std::process::exit(1);
    }
}
