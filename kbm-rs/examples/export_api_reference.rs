//! Offline documentation export: no GUI, device discovery, listener or API key.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &minikmb_native::remote::openapi())?;
    output.write_all(b"\n")?;
    Ok(())
}
