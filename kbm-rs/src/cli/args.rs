use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Default, Debug)]
pub struct Options {
    pub profile: Option<PathBuf>,
    pub pipe: Option<String>,
    pub json: Option<String>,
    pub json_output: bool,
    pub raw: bool,
    pub help: bool,
    pub fields: Vec<(String, Option<String>)>,
    pub json_file: Option<String>,
    pub binary: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub headers: Vec<(String, String)>,
    pub show_headers: bool,
    pub pretty: bool,
    pub connect_ms: u64,
    pub positionals: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct Command {
    pub method: String,
    pub path: String,
}
#[derive(Debug)]
pub struct Plan {
    pub method: String,
    pub path: String,
    pub json: Option<Value>,
}
pub(super) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> io::Result<Self> {
        fn next(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<String> {
            args.next()
                .ok_or_else(|| invalid(format!("{flag} requires a value")))
        }
        let mut options = Self {
            connect_ms: 5000,
            ..Default::default()
        };
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--profile" => options.profile = Some(next(&mut args, &arg)?.into()),
                "--pipe" => options.pipe = Some(next(&mut args, &arg)?),
                "--json" | "-j" => {
                    options.json_output = true;
                    // Preserve --json JSON while allowing a standalone output switch.
                    if args
                        .peek()
                        .is_some_and(|s| s.trim_start().starts_with(['{', '[']))
                    {
                        options.json = args.next();
                    }
                }
                "--args-json" => options.json = Some(next(&mut args, &arg)?),
                "--json-file" => options.json_file = Some(next(&mut args, &arg)?),
                "--data-binary" => options.binary = Some(next(&mut args, &arg)?.into()),
                "--output" | "-o" => options.output = Some(next(&mut args, &arg)?.into()),
                "--help" | "-h" => options.help = true,
                "--raw" => options.raw = true,
                "--arg" => {
                    let value = next(&mut args, &arg)?;
                    let (key, value) = value
                        .split_once('=')
                        .ok_or_else(|| invalid("--arg requires FIELD=VALUE"))?;
                    options
                        .fields
                        .push((key.replace('-', "_"), Some(value.into())));
                }
                "--header" => {
                    let value = next(&mut args, &arg)?;
                    let (name, value) = value
                        .split_once(':')
                        .ok_or_else(|| invalid("Header must be NAME:VALUE"))?;
                    let name = name.trim().to_ascii_lowercase();
                    if [
                        "host",
                        "authorization",
                        "content-length",
                        "transfer-encoding",
                        "connection",
                        "upgrade",
                        "trailer",
                    ]
                    .contains(&name.as_str())
                    {
                        return Err(invalid("That header is managed by the IPC transport"));
                    }
                    options.headers.push((name, value.trim().into()));
                }
                "--headers" => options.show_headers = true,
                "--pretty" => options.pretty = true,
                "--connect-timeout-ms" => {
                    options.connect_ms = next(&mut args, &arg)?
                        .parse()
                        .map_err(|_| invalid("Invalid connect timeout"))?;
                    if !(1..=60_000).contains(&options.connect_ms) {
                        return Err(invalid("Connect timeout must be 1..60000 ms"));
                    }
                }
                s if s.starts_with("--") => {
                    let field = &s[2..];
                    let (field, value) = if let Some((key, value)) = field.split_once('=') {
                        (key, Some(value.to_owned()))
                    } else {
                        let value = if args.peek().is_some_and(|s| !s.starts_with("--")) {
                            args.next()
                        } else {
                            None
                        };
                        (field, value)
                    };
                    if field.is_empty()
                        || !field
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
                    {
                        return Err(invalid(format!("Invalid field option --{field}")));
                    }
                    options.fields.push((field.replace('-', "_"), value));
                }
                s if s.starts_with('-') => return Err(invalid(format!("Unknown option {s}"))),
                _ => options.positionals.push(arg),
            }
        }
        if usize::from(options.json.is_some())
            + usize::from(options.json_file.is_some())
            + usize::from(options.binary.is_some())
            > 1
        {
            return Err(invalid("Choose one of --json, --json-file, --data-binary"));
        }
        if options.profile.is_some() && options.pipe.is_some() {
            return Err(invalid("Choose --profile or --pipe"));
        }
        if !options.fields.is_empty()
            && (options.json.is_some() || options.json_file.is_some() || options.binary.is_some())
        {
            return Err(invalid(
                "Choose field options or a JSON/binary request body",
            ));
        }
        if options.raw && options.pretty {
            return Err(invalid("--raw and --pretty cannot be combined"));
        }
        if options.binary.is_some() && options.positionals.first().is_none_or(|s| s != "request") {
            return Err(invalid("--data-binary is only used by request"));
        }
        Ok(options)
    }
    pub fn wants_json(&self) -> bool {
        self.json_output || self.json.is_some() || self.json_file.is_some() || self.pretty
    }
    pub fn arguments(&self) -> io::Result<Value> {
        let bytes = if let Some(json) = &self.json {
            json.as_bytes().to_vec()
        } else if let Some(file) = &self.json_file {
            let input: Box<dyn Read> = if file == "-" {
                Box::new(io::stdin())
            } else {
                Box::new(std::fs::File::open(file)?)
            };
            let mut bytes = Vec::new();
            input.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            bytes
        } else {
            return Ok(json!({}));
        };
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(invalid("JSON exceeds 2 MiB"));
        }
        serde_json::from_slice(&bytes).map_err(|error| invalid(format!("Invalid JSON: {error}")))
    }
}
pub fn local_path(value: &str) -> io::Result<String> {
    let path = value.strip_prefix("ipc:").unwrap_or(value);
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('#')
        || path.contains('\r')
        || path.contains('\n')
    {
        return Err(invalid(
            "Use a local API path or ipc:/ URL; TCP URLs are not followed",
        ));
    }
    let uri: axum::http::Uri = path.parse().map_err(|_| invalid("Invalid API path"))?;
    if uri.scheme().is_some() || uri.authority().is_some() {
        return Err(invalid("IPC requests cannot target a network server"));
    }
    Ok(path.into())
}
fn escaped(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
pub fn command_plan(command: &Command, args: Value) -> io::Result<Plan> {
    let mut path = local_path(&command.path)?;
    let mut body = None;
    if matches!(command.method.as_str(), "GET" | "DELETE") {
        let values = args
            .as_object()
            .ok_or_else(|| invalid("This command takes a JSON object"))?;
        let mut query = Vec::new();
        for (key, value) in values {
            let value = match value {
                Value::String(value) => value.clone(),
                Value::Bool(_) | Value::Number(_) => value.to_string(),
                _ => return Err(invalid("Query values must be strings, booleans or numbers")),
            };
            query.push(format!("{}={}", escaped(key), escaped(&value)));
        }
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query.join("&"));
        }
    } else {
        body = Some(args);
    }
    Ok(Plan {
        method: command.method.clone(),
        path,
        json: body,
    })
}
