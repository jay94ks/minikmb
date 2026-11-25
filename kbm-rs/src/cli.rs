//! Console frontend. All device operations remain in the running GUI process.
mod args;
mod fields;
mod human;
pub use args::{Options, Plan};
#[cfg(windows)]
mod run;
#[cfg(windows)]
pub use run::execute;
#[cfg(test)]
mod tests;

pub const HELP: &str = r#"MiniKMB CLI — Windows named-pipe client

  minikmb-cli commands                     List every current API command/schema
  minikmb-cli schema COMMAND               Show one command and its input schema
  minikmb-cli call COMMAND [--FIELD VALUE]  Call a command by its API/MCP name
  minikmb-cli hid_key_down --usage 4       Conventional typed options
  minikmb-cli hid_key_sequence --keys a --keys b --keys c
  minikmb-cli COMMAND --help               Show that command's fields (GUI required)
  minikmb-cli COMMAND [--json-file FILE]    Short form of call
  minikmb-cli request METHOD PATH          Direct access to any REST route
  minikmb-cli upload IPC_URL FILE          Upload original bytes to a prepared URL
  minikmb-cli state | channels | activate UID | deactivate
  minikmb-cli capture --format png --output screen.png
  minikmb-cli events                       Stream state events until interrupted
  minikmb-cli openapi [--output api.json]   Export the live API specification
  minikmb-cli endpoint                     Show this user's/profile's pipe name

Options (before or after the command):
  --profile DIRECTORY     Match GUI --profile DIRECTORY; default is the normal app profile
  --pipe NAME             Explicit local MiniKMB named pipe
  --json, -j              JSON output; events become newline-delimited JSON
  --json JSON             Legacy JSON request + JSON output (object/array)
  --args-json JSON        Explicit JSON request + JSON output
  --json-file FILE|-      Read request JSON from a UTF-8 file/stdin (up to 2 MiB)
  --data-binary FILE      Stream an existing file as application/octet-stream
  --output, -o FILE|-    Save formatted output or binary bytes; replace files atomically
  --raw                  Exact REST response bytes, including JSON/SSE framing
  --arg FIELD=VALUE       Request field, including reserved CLI names (repeatable)
  --header NAME:VALUE     Additional request header (repeatable; framing headers reserved)
  --headers              Emit status/response headers to stderr
  --pretty               Pretty-print JSON (up to 16 MiB); omit for binary/SSE output
  --connect-timeout-ms N  Wait for a busy pipe, default 5000; no command execution timeout
  --help, -h | --version, -V

The updated GUI must be running. REST/MCP and API keys are not required for local IPC.
One active channel is shared by GUI/REST/MCP/CLI. Only activate takes a UID.
Default output is readable tables/text. Field names accept hyphens or underscores;
repeat array fields and use dotted paths (e.g. --rules.0.target-port 80) for nesting.
Only boolean fields may omit a value. Do not mix field flags and a JSON request body.
Binary captures stay binary; openapi remains a JSON document. --raw preserves SSE.
No request is automatically replayed after submission. Exit: 0 success, 2 usage/IPC/I/O,
3 non-success API response. Errors go to stderr; successful output goes to stdout.
"#;
