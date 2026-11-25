# MiniKMB v1.0.1 release guide

[한국어](RELEASE-1.0.1.md) · [English](RELEASE-1.0.1.en.md)

This release adds a local IPC CLI, AI-agent skill, complete REST reference and standalone HTML console to the Windows Rust application. The native Cargo version remains 0.1.0 preview; the distribution bundle is v1.0.1. Firmware and board protocols are unchanged.

## Download and run

Extract all of `MiniKMB-Native-v1.0.1-windows-x64.zip` and run `minikmb-native.exe`. Close the old app first; two instances must not open the same MCU. Tools, firmware, ROM, licenses and corresponding source are bundled. No Python, Node or separate web server is required at runtime. This is an unsigned Windows x64 preview.

`minikmb-cli.exe` connects to the updated app through a Windows named pipe under the same user and profile. TCP REST/MCP and an API key are not needed for local IPC. Individual options and readable text/tables are the default; select JSON with `--json`. Update the GUI along with the CLI: the old GUI does not provide this IPC interface.

## Included documents

- `SKILL.md`: agent-facing CLI instructions.
- `CLI.md`, `CLI.en.md`: Korean and English user guides.
- `REST-API.md`: all 66 HTTP operations, paths, fields, required values, bounds and examples.
- `openapi.json`: exact OpenAPI 3.1 request/response contract.
- `REST-API.html`: open this file directly; no separate web server or CDN is needed.

To call APIs from the HTML, create a key and enable REST in the app, then enter its actual `http://127.0.0.1:PORT` address and key in the console. Browser security settings need no changes. The key stays in page memory, not files, URLs or browser storage. The app-served `/api/v1/reference` page is optional. With the app REST server off, the document remains readable but live API requests cannot connect.

GUI, CLI, REST and MCP share one active channel. Only selection takes a UID; ordinary controls do not. Keyboard, mouse, storage and network operations affect the active physical target.

## Validation and licensing

303 Rust tests, 12 packaging/document-generation tests and Clippy passed. Five explicit-environment tests and extended soak testing were excluded. File-origin preflight, authentication, JSON/binary uploads, revocation and Host/Origin rejection are covered. No new hardware qualification or HS recovery is claimed; target USB remains FS.

Use `SHA256SUMS`, `release-manifest.json` and the ZIP's `manifest.json` to verify payloads. Project-authored code is MIT. Original terms for external tools, libraries, fonts and ROM remain in `NOTICE.md`, `THIRD-PARTY.md`, `licenses/` and `sources/`. MS2130 ROM is not MIT licensed. Development used OpenAI Codex with GPT 6 Astra and Sol.
