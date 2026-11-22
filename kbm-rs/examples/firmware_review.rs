//! Explicit read-only test of the shipped tool invocation layer.
use minikmb_native::firmware::{backend::Backend, native::Native, process::Tools, Target};
use tokio_util::sync::CancellationToken;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("usage: firmware_review ABSOLUTE_STAGED_TOOLS_DIRECTORY".into());
    }
    let backend = Native {
        tools: Tools {
            directory: std::path::PathBuf::from(&args[1]),
        },
    };
    let cancel = CancellationToken::new();
    for target in [Target::Stm32, Target::Ms2130] {
        let programmers = backend.list(target, cancel.clone()).await?;
        println!(
            "{}",
            serde_json::json!({"target":target,"programmers":programmers})
        );
        if programmers.len() != 1 || !programmers[0].selectable {
            return Err("expected exactly one explicitly prepared programmer of each kind".into());
        }
        let info = backend
            .inspect(target, programmers[0].id.clone(), cancel.clone())
            .await?;
        println!(
            "{}",
            serde_json::json!({"inspection":info,"write_requested":false})
        );
    }
    Ok(())
}
