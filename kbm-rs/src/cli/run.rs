use super::{
    args::{self, Command},
    human, Options, Plan,
};
use axum::{
    body::Body,
    http::{Request, Response},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    io::{self, Write},
    path::Path,
    time::Duration,
};

async fn send(
    endpoint: &str,
    plan: Plan,
    options: &Options,
) -> io::Result<Response<hyper::body::Incoming>> {
    let mut builder = Request::builder()
        .method(plan.method.as_str())
        .uri(args::local_path(&plan.path)?)
        .header("host", "minikmb.local");
    let (body, length) = if let Some(file) = &options.binary {
        let file = tokio::fs::File::open(file).await?;
        let length = file.metadata().await?.len();
        builder = builder.header("content-type", "application/octet-stream");
        (
            Body::from_stream(tokio_util::io::ReaderStream::new(file)),
            length,
        )
    } else if let Some(json) = plan.json {
        let bytes = serde_json::to_vec(&json).map_err(io::Error::other)?;
        builder = builder.header("content-type", "application/json");
        let length = bytes.len() as u64;
        (Body::from(bytes), length)
    } else {
        (Body::empty(), 0)
    };
    builder = builder.header("content-length", length);
    for (name, value) in &options.headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(body)
        .map_err(|error| args::invalid(error.to_string()))?;
    crate::ipc::client::request(endpoint, request, Duration::from_millis(options.connect_ms)).await
}

async fn json_response(response: Response<hyper::body::Incoming>) -> io::Result<Value> {
    let status = response.status();
    let bytes = axum::body::to_bytes(Body::new(response.into_body()), 16 * 1024 * 1024)
        .await
        .map_err(io::Error::other)?;
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "API {status}: {}",
            String::from_utf8_lossy(&bytes)
        )));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

async fn print_response(
    response: Response<hyper::body::Incoming>,
    options: &Options,
) -> io::Result<i32> {
    let status = response.status();
    if options.show_headers {
        let headers: std::collections::BTreeMap<_, _> = response
            .headers()
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap_or("<binary>")))
            .collect();
        if options.wants_json() {
            eprintln!("{}", json!({"status": status.as_u16(), "headers": headers}));
        } else {
            eprintln!("HTTP {}", status.as_u16());
            for (name, value) in headers {
                eprintln!("{name}: {value}");
            }
        }
    }
    if !status.is_success() {
        let bytes = axum::body::to_bytes(Body::new(response.into_body()), 2 * 1024 * 1024)
            .await
            .map_err(io::Error::other)?;
        if !options.wants_json() && !options.raw {
            let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            io::stderr().write_all(human::error(&value).as_bytes())?;
        } else {
            io::stderr().write_all(&bytes)?;
        }
        eprintln!();
        return Ok(3);
    }
    let mut temporary = match &options.output {
        Some(path) if path != Path::new("-") => Some(tempfile::NamedTempFile::new_in(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?),
        _ => None,
    };
    let mut output: Box<dyn Write + '_> = match temporary.as_mut() {
        Some(file) => Box::new(file.as_file_mut()),
        None => Box::new(io::stdout().lock()),
    };
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !options.raw && content_type.starts_with("text/event-stream") {
        let mut body = response.into_body();
        let mut decoder = human::Events::default();
        while let Some(frame) = body.frame().await {
            if let Ok(bytes) = frame.map_err(io::Error::other)?.into_data() {
                for event in decoder.push(&bytes)? {
                    if options.wants_json() {
                        serde_json::to_writer(&mut output, &event).map_err(io::Error::other)?;
                        output.write_all(b"\n")?;
                    } else {
                        output.write_all(human::render(&event).as_bytes())?;
                    }
                    output.flush()?;
                }
            }
        }
    } else if !options.raw && content_type.starts_with("application/json") && !options.wants_json()
    {
        let value = json_response(response).await?;
        output.write_all(human::render(&value).as_bytes())?;
    } else if options.pretty {
        let value = json_response(response).await?;
        serde_json::to_writer_pretty(&mut output, &value).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
    } else {
        let mut body = response.into_body();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(io::Error::other)?;
            if let Ok(bytes) = frame.into_data() {
                output.write_all(&bytes)?;
            }
        }
        if !options.raw && content_type.starts_with("application/json") {
            output.write_all(b"\n")?;
        }
    }
    output.flush()?;
    drop(output);
    if let Some(file) = temporary {
        file.as_file().sync_all()?;
        file.persist(options.output.as_ref().unwrap())
            .map_err(|error| error.error)?;
    }
    Ok(0)
}

fn write_document(bytes: &[u8], options: &Options) -> io::Result<()> {
    if let Some(path) = options.output.as_ref().filter(|p| *p != Path::new("-")) {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
    } else {
        let mut out = io::stdout().lock();
        out.write_all(bytes)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

pub async fn execute(mut options: Options) -> io::Result<i32> {
    if options.positionals.is_empty() {
        print!("{}", super::HELP);
        return Ok(0);
    }
    if options.help {
        let verb = options.positionals[0].as_str();
        let command = match verb {
            "call" | "schema" => options
                .positionals
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| args::invalid("Specify a command name"))?,
            "state" => "app_get_state",
            "channels" => "channels_list",
            "activate" => "channel_activate",
            "deactivate" => "channel_deactivate",
            "capture" => "video_snapshot",
            "request" | "upload" | "events" | "openapi" | "endpoint" | "commands" => {
                print!("{}", super::HELP);
                return Ok(0);
            }
            other => other,
        }
        .to_owned();
        options.positionals = vec!["schema".into(), command];
        options.fields.clear();
        options.json = None;
        options.json_file = None;
        options.binary = None;
    }
    let profile = options
        .profile
        .clone()
        .map(Ok)
        .unwrap_or_else(crate::ipc::default_profile)?;
    let endpoint = options
        .pipe
        .clone()
        .map(Ok)
        .unwrap_or_else(|| crate::ipc::endpoint(&profile))?;
    let verb = options
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or("help");
    if verb == "endpoint" {
        println!("{endpoint}");
        return Ok(0);
    }
    let mut args = options.arguments()?;
    let plan = match verb {
        "request" => {
            if options.positionals.len() != 3 {
                return Err(args::invalid("request requires METHOD PATH"));
            }
            let command = Command {
                method: options.positionals[1].clone(),
                path: args::local_path(&options.positionals[2])?,
            };
            if !options.fields.is_empty() {
                args::command_plan(&command, options.apply_fields(args, None)?)?
            } else {
                Plan {
                    method: command.method,
                    path: command.path,
                    json: (options.json.is_some() || options.json_file.is_some()).then_some(args),
                }
            }
        }
        "upload" => {
            if options.positionals.len() != 3
                || options.binary.is_some()
                || options.json.is_some()
                || options.json_file.is_some()
                || !options.fields.is_empty()
            {
                return Err(args::invalid("upload requires IPC_URL FILE"));
            }
            let path = args::local_path(&options.positionals[1])?;
            if !(path.starts_with("/api/v1/firmware/uploads/")
                || path.starts_with("/api/v1/active/storage/uploads/"))
            {
                return Err(args::invalid(
                    "Use an upload URL returned by a prepare command",
                ));
            }
            options.binary = Some(options.positionals[2].clone().into());
            Plan {
                method: "POST".into(),
                path,
                json: None,
            }
        }
        "events" | "openapi" => {
            if options.positionals.len() != 1 {
                return Err(args::invalid("Unexpected arguments"));
            }
            if !options.fields.is_empty() {
                return Err(args::invalid("This command takes no field options"));
            }
            if verb == "events" && options.pretty {
                return Err(args::invalid("SSE events are a stream; omit --pretty"));
            }
            Plan {
                method: "GET".into(),
                path: format!(
                    "/api/v1/{}",
                    if verb == "events" {
                        "events"
                    } else {
                        "openapi.json"
                    }
                ),
                json: None,
            }
        }
        _ => {
            if options.binary.is_some() {
                return Err(args::invalid("Use request or upload with --data-binary"));
            }
            let catalog = send(
                &endpoint,
                Plan {
                    method: "GET".into(),
                    path: "/ipc/v1/commands".into(),
                    json: None,
                },
                &Options::default_for_catalog(&options),
            )
            .await?;
            if verb == "commands" {
                if options.positionals.len() != 1 || !options.fields.is_empty() {
                    return Err(args::invalid("commands takes no arguments"));
                }
                return print_response(catalog, &options).await;
            }
            let catalog = json_response(catalog).await?;
            let mut requested = verb;
            match verb {
                "call" | "schema" => {
                    if options.positionals.len() != 2 {
                        return Err(args::invalid("Specify exactly one command name"));
                    }
                    requested = &options.positionals[1];
                }
                "state" => requested = "app_get_state",
                "channels" => requested = "channels_list",
                "activate" => {
                    if options.positionals.len() != 2
                        || options.json.is_some()
                        || options.json_file.is_some()
                    {
                        return Err(args::invalid("activate requires one UID"));
                    }
                    requested = "channel_activate";
                    args = json!({"uid":options.positionals[1]});
                }
                "deactivate" => requested = "channel_deactivate",
                "capture" => {
                    requested = "video_snapshot";
                }
                _ => {}
            }
            if !matches!(verb, "call" | "schema" | "activate") && options.positionals.len() != 1 {
                return Err(args::invalid("Unexpected positional arguments"));
            }
            let command = catalog["commands"]
                .as_array()
                .and_then(|list| list.iter().find(|c| c["name"].as_str() == Some(requested)))
                .ok_or_else(|| {
                    args::invalid(format!("Unknown command {requested}; use commands"))
                })?;
            if verb == "schema" {
                if !options.fields.is_empty() {
                    return Err(args::invalid("schema takes no field options"));
                }
                let bytes = if options.wants_json() || options.raw {
                    serde_json::to_vec_pretty(command).map_err(io::Error::other)?
                } else {
                    human::schema(command).into_bytes()
                };
                write_document(&bytes, &options)?;
                return Ok(0);
            }
            args = options.apply_fields(args, Some(&command["input_schema"]))?;
            let command: Command =
                serde_json::from_value(command.clone()).map_err(io::Error::other)?;
            args::command_plan(&command, args)?
        }
    };
    if plan.path == "/api/v1/openapi.json" && !options.wants_json() {
        options.raw = true;
    }
    let response = send(&endpoint, plan, &options).await?;
    print_response(response, &options).await
}
impl Options {
    fn default_for_catalog(other: &Self) -> Self {
        Self {
            connect_ms: other.connect_ms,
            ..Default::default()
        }
    }
}
