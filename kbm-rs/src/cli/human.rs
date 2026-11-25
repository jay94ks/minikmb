//! Human output has no JSON punctuation and escapes terminal control characters.
use serde_json::Value;
use std::fmt::Write;

fn text(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
fn scalar(value: &Value) -> String {
    match value {
        Value::Null => "-".into(),
        Value::Bool(v) => if *v { "yes" } else { "no" }.into(),
        Value::String(v) => text(v),
        Value::Number(v) => v.to_string(),
        _ => String::new(),
    }
}
fn table(rows: &[Value], fields: &[&str]) -> String {
    let mut widths: Vec<usize> = fields.iter().map(|s| s.len()).collect();
    for row in rows {
        for (i, field) in fields.iter().enumerate() {
            widths[i] = widths[i].max(scalar(&row[field]).chars().count());
        }
    }
    let mut out = String::new();
    for (i, field) in fields.iter().enumerate() {
        let _ = write!(out, "{:<width$}  ", field.to_uppercase(), width = widths[i]);
    }
    out.push('\n');
    for row in rows {
        for (i, field) in fields.iter().enumerate() {
            let _ = write!(out, "{:<width$}  ", scalar(&row[field]), width = widths[i]);
        }
        out.push('\n');
    }
    out
}
fn tree(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(values) if !values.is_empty() => {
            for (key, value) in values {
                let _ = write!(out, "{}{}:", "  ".repeat(depth), text(key));
                if value.is_object() || value.is_array() {
                    out.push('\n');
                    tree(out, value, depth + 1);
                } else {
                    let _ = writeln!(out, " {}", scalar(value));
                }
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for value in values {
                if value.is_object() || value.is_array() {
                    let _ = writeln!(out, "{}-", "  ".repeat(depth));
                    tree(out, value, depth + 1);
                } else {
                    let _ = writeln!(out, "{}- {}", "  ".repeat(depth), scalar(value));
                }
            }
        }
        Value::Object(_) | Value::Array(_) => {
            let _ = writeln!(out, "{}(empty)", "  ".repeat(depth));
        }
        _ => {
            let _ = writeln!(out, "{}{}", "  ".repeat(depth), scalar(value));
        }
    }
}
pub(super) fn render(value: &Value) -> String {
    if let Some(commands) = value["commands"].as_array() {
        return table(commands, &["name", "method", "title"]);
    }
    if let Some(rows) = value["channels"].as_array() {
        let mut out = if rows.is_empty() {
            "No channels found.\n".into()
        } else {
            table(rows, &["uid", "connected", "target_state", "name"])
        };
        if let Some(error) = value["error"].as_str().filter(|s| !s.is_empty()) {
            let _ = writeln!(out, "Discovery error: {}", text(error));
        }
        return out;
    }
    if let Some(rows) = value
        .as_array()
        .filter(|r| !r.is_empty() && r.iter().all(|v| v.get("uid").is_some()))
    {
        return table(rows, &["uid", "connected", "target_state", "name"]);
    }
    let mut out = String::new();
    tree(&mut out, value, 0);
    out
}

#[derive(Default)]
pub(super) struct Events {
    pending: Vec<u8>,
}
impl Events {
    pub(super) fn push(&mut self, bytes: &[u8]) -> std::io::Result<Vec<Value>> {
        self.pending.extend_from_slice(bytes);
        let mut result = Vec::new();
        loop {
            let boundary = [
                self.pending
                    .windows(2)
                    .position(|s| s == b"\n\n")
                    .map(|p| (p, 2)),
                self.pending
                    .windows(4)
                    .position(|s| s == b"\r\n\r\n")
                    .map(|p| (p, 4)),
            ]
            .into_iter()
            .flatten()
            .min_by_key(|v| v.0);
            let Some((end, length)) = boundary else {
                break;
            };
            if end > 2 * 1024 * 1024 {
                return Err(std::io::Error::other("Event exceeds 2 MiB"));
            }
            let block = std::str::from_utf8(&self.pending[..end]).map_err(std::io::Error::other)?;
            let mut event = serde_json::json!({"event":"message"});
            let mut data = Vec::new();
            for line in block.lines() {
                if line.starts_with(':') {
                    continue;
                }
                let (key, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match key {
                    "id" | "event" => event[key] = value.into(),
                    "data" => data.push(value),
                    _ => {}
                }
            }
            if !data.is_empty() {
                let joined = data.join("\n");
                event["data"] = serde_json::from_str(&joined).unwrap_or(Value::String(joined));
                result.push(event);
            }
            self.pending.drain(..end + length);
        }
        if self.pending.len() > 2 * 1024 * 1024 {
            return Err(std::io::Error::other("Event exceeds 2 MiB"));
        }
        Ok(result)
    }
}
pub(super) fn error(value: &Value) -> String {
    let value = value.get("error").unwrap_or(value);
    if let (Some(code), Some(message)) = (value["code"].as_str(), value["message"].as_str()) {
        format!("Error {}: {}\n", text(code), text(message))
    } else {
        render(value)
    }
}
pub(super) fn schema(value: &Value) -> String {
    let mut out = format!(
        "{}\n{} {}\n{}\n\n",
        scalar(&value["name"]),
        scalar(&value["method"]),
        scalar(&value["path"]),
        scalar(&value["description"])
    );
    fn properties(schema: &Value, out: &mut String) {
        if let Some(props) = schema["properties"].as_object() {
            for (key, field) in props {
                let ty = field["type"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "value (see --json schema)".into());
                let required = schema["required"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == key));
                let _ = writeln!(
                    out,
                    "  --{:<26} {:<12}{}",
                    key.replace('_', "-"),
                    ty,
                    if required { "required" } else { "" }
                );
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(parts) = schema[key].as_array() {
                for p in parts {
                    properties(p, out);
                }
            }
        }
    }
    properties(&value["input_schema"], &mut out);
    out.push_str("\nRepeat array options; use dotted paths for nested fields. --json prints the full schema.\n");
    out
}
