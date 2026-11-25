//! Convert conventional field flags with the GUI's actual JSON schema.
use super::{args::invalid, Options};
use serde_json::{Map, Value};
use std::io;

fn deref<'a>(schema: &'a Value, root: &'a Value, depth: u32) -> &'a Value {
    if depth < 32 {
        if let Some(path) = schema["$ref"].as_str().and_then(|s| s.strip_prefix('#')) {
            if let Some(value) = root.pointer(path) {
                return deref(value, root, depth + 1);
            }
        }
    }
    schema
}
fn property<'a>(schema: &'a Value, root: &'a Value, name: &str, depth: u32) -> Option<&'a Value> {
    if depth >= 32 {
        return None;
    }
    let schema = deref(schema, root, depth);
    if let Some(value) = schema["properties"].get(name) {
        return Some(value);
    }
    if let Some(items) = schema
        .get("items")
        .filter(|_| name.parse::<usize>().is_ok())
    {
        return Some(items);
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(parts) = schema[key].as_array() {
            if let Some(value) = parts
                .iter()
                .find_map(|s| property(s, root, name, depth + 1))
            {
                return Some(value);
            }
        }
    }
    schema.get("additionalProperties").filter(|s| s.is_object())
}
fn has_type(schema: &Value, root: &Value, kind: &str, depth: u32) -> bool {
    if depth >= 32 {
        return false;
    }
    let schema = deref(schema, root, depth);
    schema["type"] == kind
        || schema["type"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == kind))
        || ["oneOf", "anyOf", "allOf"].iter().any(|key| {
            schema[key]
                .as_array()
                .is_some_and(|a| a.iter().any(|s| has_type(s, root, kind, depth + 1)))
        })
}
fn typed(raw: Option<&str>, schema: Option<&Value>, root: &Value) -> io::Result<Value> {
    let Some(raw) = raw else {
        if schema.is_none_or(|s| has_type(s, root, "boolean", 0)) {
            return Ok(Value::Bool(true));
        }
        return Err(invalid("This field requires a value"));
    };
    if let Some(schema) = schema {
        if has_type(schema, root, "string", 0)
            && !has_type(schema, root, "number", 0)
            && !has_type(schema, root, "integer", 0)
        {
            return Ok(Value::String(raw.into()));
        }
        if has_type(schema, root, "integer", 0) && (raw.starts_with("0x") || raw.starts_with("0X"))
        {
            let number = u64::from_str_radix(&raw[2..], 16)
                .map_err(|_| invalid("Invalid hexadecimal integer"))?;
            return Ok(number.into());
        }
        let parsed: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("Expected a number, true/false, or a structured field value"))?;
        let valid = match &parsed {
            Value::Bool(_) => has_type(schema, root, "boolean", 0),
            Value::Number(n) => {
                has_type(schema, root, "number", 0)
                    || (has_type(schema, root, "integer", 0) && (n.is_i64() || n.is_u64()))
            }
            Value::Array(_) => has_type(schema, root, "array", 0),
            Value::Object(_) => has_type(schema, root, "object", 0),
            Value::Null => has_type(schema, root, "null", 0),
            Value::String(_) => has_type(schema, root, "string", 0),
        };
        if !valid {
            return Err(invalid(
                "Value has the wrong type for this field; use schema COMMAND",
            ));
        }
        return Ok(parsed);
    }
    Ok(serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into())))
}
fn slot<'a>(value: &'a mut Value, path: &[&str]) -> io::Result<&'a mut Value> {
    if path.is_empty() {
        return Ok(value);
    }
    let name = path[0];
    let child = if let Ok(index) = name.parse::<usize>() {
        if index >= 1024 {
            return Err(invalid("Array index must be below 1024"));
        }
        if value.is_null() {
            *value = Value::Array(Vec::new());
        }
        let array = value
            .as_array_mut()
            .ok_or_else(|| invalid("A field path conflicts with an existing value"))?;
        if array.len() <= index {
            array.resize(index + 1, Value::Null);
        }
        &mut array[index]
    } else {
        if value.is_null() {
            *value = Value::Object(Map::new());
        }
        value
            .as_object_mut()
            .ok_or_else(|| invalid("A field path conflicts with an existing value"))?
            .entry(name)
            .or_insert(Value::Null)
    };
    slot(child, &path[1..])
}
impl Options {
    pub(super) fn apply_fields(
        &self,
        mut args: Value,
        schema: Option<&Value>,
    ) -> io::Result<Value> {
        let empty = Value::Null;
        let root = schema.unwrap_or(&empty);
        for (name, raw) in &self.fields {
            let path: Vec<_> = name.split('.').collect();
            if path.iter().any(|s| s.is_empty()) || path.len() > 16 {
                return Err(invalid("Invalid field path"));
            }
            let mut field = schema;
            for part in &path {
                field = if let Some(current) = field {
                    Some(property(current, root, part, 0).ok_or_else(|| {
                        invalid(format!("Unknown option --{name}; use schema COMMAND"))
                    })?)
                } else {
                    None
                };
            }
            let destination = slot(&mut args, &path)?;
            let array = field.is_some_and(|s| has_type(s, root, "array", 0));
            if array {
                let field = deref(field.unwrap(), root, 0);
                let item = field.get("items");
                let value = if raw.as_ref().is_some_and(|s| s.starts_with('[')) {
                    typed(raw.as_deref(), Some(field), root)?
                } else {
                    typed(raw.as_deref(), item, root)?
                };
                if destination.is_null() {
                    *destination = Value::Array(Vec::new());
                }
                let values = destination
                    .as_array_mut()
                    .ok_or_else(|| invalid("Array field conflicts with another value"))?;
                if let Value::Array(mut list) = value {
                    values.append(&mut list);
                } else {
                    values.push(value);
                }
            } else {
                if !destination.is_null() {
                    return Err(invalid(format!("Duplicate option --{name}")));
                }
                *destination = typed(raw.as_deref(), field, root)?;
            }
        }
        Ok(args)
    }
}
