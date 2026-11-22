# MiniKMB Native (0.1.0 preview)

Rust + egui/eframe + wgpu Windows desktop app. Video uses Media Foundation and GPU presentation. All GUI/REST/MCP operations share one active-channel service. The native keyboard hook runs in an in-process DLL with a dedicated message thread. No Electron runtime is needed.

From this directory, with the pinned Rust toolchain and Windows MSVC:

```powershell
$env:CARGO_INCREMENTAL = '0'
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python scripts/build-native-keyboard.py
cargo build --release --locked
```

Hardware tests are ignored by default; normal unit tests do not generate real HID input. See `../docs/BUILDING.md` for tool/firmware staging and packaging, `../docs/KNOWN-ISSUES.md` for current limits, and `docs/INPUT-SEQUENCES.md` for click/double-click/key-array API behavior.

REST documentation and MCP setup guidance are available inside the app while the servers are OFF. Servers bind loopback, use individually revocable API keys, and are started explicitly. Ordinary controls target the active channel without a UID. MCP ON also starts REST; REST alone is supported.

MIT for MiniKMB-authored code. See `LICENSE`, `licenses/THIRD-PARTY.md`, font notices and the repository `NOTICE.md`. GPL SPI-helper code and MS2130 ROM retain separate conditions. Development used Codex with GPT 6 Astra and Sol.
