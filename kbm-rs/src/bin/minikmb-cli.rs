fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty()
        || args.first().is_some_and(|a| a == "help")
        || args == ["--help"]
        || args == ["-h"]
    {
        print!("{}", minikmb_native::cli::HELP);
        return;
    }
    if args == ["--version"] || args == ["-V"] {
        println!("minikmb-cli {} (IPC v1)", env!("CARGO_PKG_VERSION"));
        return;
    }
    let json_errors = args.iter().any(|s| {
        matches!(
            s.as_str(),
            "--json" | "-j" | "--args-json" | "--json-file" | "--pretty"
        )
    });
    #[cfg(windows)]
    let result = minikmb_native::cli::Options::parse(args).and_then(|options| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(minikmb_native::cli::execute(options))
    });
    #[cfg(not(windows))]
    let result: std::io::Result<i32> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "MiniKMB CLI currently requires Windows",
    ));
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if json_errors {
                eprintln!(
                    "{}",
                    serde_json::json!({"error":{"code":"CLI_ERROR","message":error.to_string()}})
                );
            } else {
                eprintln!("Error: {error}");
            }
            std::process::exit(2);
        }
    }
}
