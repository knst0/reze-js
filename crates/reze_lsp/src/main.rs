fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, threads) = lsp_server::Connection::stdio();
    reze_lsp::run(connection)?;
    threads.join()?;
    Ok(())
}
