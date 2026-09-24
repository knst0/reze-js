fn main() {
    if let Err(error) = reze_lsp::server::run() {
        eprintln!("reze-lsp: {error}");
        std::process::exit(1);
    }
}
