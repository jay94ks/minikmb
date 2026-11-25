# MiniKMB local CLI

[한국어](CLI.md)

`minikmb-cli.exe` talks to the **updated, running Rust GUI** through a Windows
named pipe. It reuses the full REST router, including JSON commands, binary
uploads, JPEG/PNG snapshots and SSE. It does not open a second MCU connection.
Local IPC works with TCP REST/MCP disabled and needs no API key. The existing
TCP authentication policy is unchanged.

## Usage

Start `minikmb-native.exe`, then run the CLI as the same Windows user:

```powershell
.\minikmb-cli.exe channels
.\minikmb-cli.exe state
.\minikmb-cli.exe activate 0x0123456789ABCDEF01234567
.\minikmb-cli.exe call video_open --json '{}'
.\minikmb-cli.exe capture --format png --output screen.png
.\minikmb-cli.exe deactivate
```

Replace the example UID with a real value from `channels`. GUI, REST, MCP and
CLI share **one active channel**. Only selection takes a UID; other controls
operate on that active channel. Active-channel commands fail when none is
selected. Snapshots require a fresh frame from the currently open video stream.

Both applications use the normal app profile by default. For a custom profile,
pass `--profile C:\path\profile` to both. `endpoint` prints its pipe name;
`--pipe NAME` selects an explicit local MiniKMB pipe. Remote pipes and TCP URLs
are rejected.

## Conventional input and JSON mode

Tables and text are the default output. Use individual typed options instead of
writing a JSON request:

```powershell
.\minikmb-cli.exe hid_key_down --usage 4
.\minikmb-cli.exe hid_key_up --usage 4
.\minikmb-cli.exe hid_key_sequence --keys a --keys b --keys c
.\minikmb-cli.exe hid_click --x 100 --y 200 --button 1
.\minikmb-cli.exe hid_key_down --help
.\minikmb-cli.exe channels --json
```

Field options use the GUI's input schema for string, number and boolean types.
Hyphens and underscores are interchangeable in field names. Only boolean fields
may omit their value (`--confirmed`); `--confirmed false` is also supported.
Integers accept hexadecimal (`--usage 0x04`). Repeat array options, or use dotted
paths for nesting, such as `--rules.0.protocol tcp --rules.0.target-port 80`.
Use `--arg FIELD=VALUE` for field names reserved by global CLI options. Unknown
fields, duplicate scalars and missing values fail before submitting the command.

`--json` / `-j` selects JSON output. Existing `--json '{...}'` and `--json '[...]'`
request syntax is retained; `--args-json JSON` is the explicit request option.
`--json-file` and `--pretty` also select JSON output. Do not mix field flags with
a JSON request body. JPEG/PNG remain binary, and `openapi` remains a JSON document.
`--raw` preserves the exact REST response bytes.

## Complete REST coverage

```powershell
.\minikmb-cli.exe commands --pretty
.\minikmb-cli.exe schema hid_key_sequence
.\minikmb-cli.exe call hid_key_sequence --json '["a","b","c"]'
.\minikmb-cli.exe network_get_status --pretty
.\minikmb-cli.exe request GET /api/v1/active/storage --pretty
.\minikmb-cli.exe openapi --output openapi.json
```

The GUI supplies the current command catalog, routes and input schemas. Use
`call COMMAND` or its short form `COMMAND`. `request METHOD PATH` accesses any
REST route directly. Existing keyboard, capture-pixel absolute mouse, click,
double-click, sequence, TTY, NCM/sharing/forwarding/scanner, storage/MSC/optical,
jobs, WOL, diagnostics/logs and firmware commands retain their arguments,
preconditions and results. Consult `commands`, `schema` and the GUI REST docs
for exact names/fields. Input examples act on the selected physical target.

GET/DELETE command arguments become URL query parameters. POST/PUT arguments
are sent as JSON, preserving arrays. Use `--json-file FILE` for UTF-8 JSON or
`--json-file -` for stdin (up to 2 MiB). `--pretty` formats JSON up to 16 MiB.

Separate invocations share the local input owner. An `input_session` returned
by `input_open` can be used across DOWN/UP calls. Existing 30-second leases and
heartbeat/close rules apply; sessions become invalid after channel changes or
GUI shutdown. Use the sequence command for consecutive complete key strokes.
Turning off TCP REST does not cancel local IPC input leases.

## Files and events

Prepare an upload with the appropriate existing `*_upload_prepare` command.
Then pass its returned `ipc:/api/v1/...` URL and the original file:

```powershell
.\minikmb-cli.exe upload 'ipc:/api/v1/active/storage/uploads/RETURNED_ID' 'C:\images\boot.iso'
.\minikmb-cli.exe events
.\minikmb-cli.exe request GET '/api/v1/logs?limit=20' --pretty
```

Uploads stream the original bytes with Content-Length, without Base64. Prepare
and upload must use the same running GUI. Receiving a firmware upload does not
program a device; use the existing start command. Poll job status or subscribe
to events for progress. `events` runs until Ctrl+C: text by default, NDJSON with
`--json`, and the original SSE with `--raw`.

JSON responses are rendered as text by default. Binary response bytes go to
stdout unchanged. Use `--output FILE` for snapshots/binary data;
omit `--pretty` for binary/SSE. Successful file output replaces the destination
atomically. Failed API responses do not overwrite existing output files.
`--headers` writes HTTP status/headers to stderr as text, or JSON in JSON mode. Direct requests also
accept `--data-binary FILE` and repeated `--header NAME:VALUE`; transport framing
and authentication headers are reserved.

Exit codes: **0** success, **2** usage/IPC/I/O failure, **3** unsuccessful API
response. Errors go to stderr. Submitted requests are never automatically
replayed. `--connect-timeout-ms` only limits waiting for a busy pipe, not command
execution. After a disconnect, query state/job status before deciding to retry.

## AI agents

The distribution includes `SKILL.md` for agents using this CLI. It explains JSON
output, live schema discovery, the single active channel, frame coordinates,
input sessions and file/job workflows. Its source is `packaging/SKILL.md`; the
packager includes it beside the executables. No global skill installation or
application configuration change is performed.

## REST API reference and console

The distribution includes `REST-API.md`, `openapi.json` and `REST-API.html`.
The Markdown reference lists all 66 HTTP operations with methods, paths, arguments,
required fields, allowed values, bounds and request examples. OpenAPI 3.1 JSON also
contains exact response and component schemas. Packaging regenerates these files
from the application's actual API contract.

Open the HTML file to read the reference offline. To send requests, enable REST
in the updated app, open `/api/v1/reference` at its displayed address and enter a
key created in the app. **Open local tester** in the HTML file navigates there.
Select an operation, fill its arguments and click **Send request** to inspect the
status, headers and JSON/image response. Binary uploads and SSE start/stop are
supported. Keys remain in page memory only. Browser file-origin restrictions do
not need to be relaxed. HTTP controls require Bearer authentication, independently
of the CLI's keyless local IPC.

## Implementation and builds

The pipe is restricted to the current user SID, rejects remote clients, checks
the server's user, and is scoped by profile. At most 32 connections are served.
Other local programs running as the same Windows user have this control access.
`src/remote/ipc.rs` wraps the existing REST router rather than duplicating device
dispatch or bypassing its active-channel and ownership checks.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bins
```

Outputs: `target/release/minikmb-cli.exe` and `minikmb-native.exe`. Tests exercise
real Windows named pipes, catalog coverage, file reception, image/SSE responses,
restart, single-channel selection and input ownership using test backends. They
do not manipulate physical HID, SD, NIC or flash devices. Extended soak testing
is excluded.
