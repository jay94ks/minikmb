//! Offline inspection only. Never opens a serial port, probe or target MCU.
use minikmb_native::firmware::{image, Target};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: firmware_inspect stm32|ms2130 FILE".into());
    }
    let target = match args[1].as_str() {
        "stm32" => Target::Stm32,
        "ms2130" => Target::Ms2130,
        _ => return Err("unknown firmware target".into()),
    };
    let source = std::fs::read(&args[2])?;
    let prepared = image::prepare(target, &source)?;
    println!(
        "{}",
        serde_json::json!({"source_bytes":source.len(),"normalized_bytes":prepared.bytes.len(),"base_address":prepared.base,"normalized_sha256":prepared.sha256})
    );
    Ok(())
}
