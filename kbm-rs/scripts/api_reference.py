"""Generate bundled REST reference documents from the app's offline OpenAPI."""
import hashlib
import json
from pathlib import Path

METHODS = {"get", "post", "put", "delete", "patch", "head", "options"}


def encoded(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def cell(value):
    return str(value).replace("|", "\\|").replace("\r", "").replace("\n", "<br>")


def resolve(spec, schema, seen=()):
    while isinstance(schema, dict) and "$ref" in schema:
        ref = schema["$ref"]
        if ref in seen:
            return {"description": "Recursive schema: " + ref}
        if not ref.startswith("#/"):
            raise ValueError("Only bundled, internal schema references are supported")
        value = spec
        for part in ref[2:].split("/"):
            value = value[part.replace("~1", "/").replace("~0", "~")]
        schema = {**value, **{k: v for k, v in schema.items() if k != "$ref"}}
        seen = (*seen, ref)
    return schema


def typename(spec, schema, depth=0):
    if depth > 16:
        return "recursive"
    schema = resolve(spec, schema)
    if isinstance(schema, bool):
        return "any" if schema else "forbidden"
    for kind in ["oneOf", "anyOf", "allOf"]:
        if kind in schema:
            joiner = " & " if kind == "allOf" else " / "
            return joiner.join(typename(spec, p, depth + 1) for p in schema[kind])
    kind = schema.get("type", "value")
    if isinstance(kind, list):
        return "/".join(kind)
    if kind == "array":
        return "array<" + typename(spec, schema.get("items", {}), depth + 1) + ">"
    return kind


def limits(schema):
    if isinstance(schema, bool):
        return "allowed" if schema else "forbidden"
    result = []
    for key in ["enum", "const", "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum",
                "multipleOf", "minLength", "maxLength", "pattern", "format", "minItems",
                "maxItems", "uniqueItems", "minProperties", "maxProperties", "default"]:
        if key in schema:
            result.append(f"{key}={encoded(schema[key])}")
    if schema.get("additionalProperties") is False:
        result.append("unknown fields forbidden")
    return "; ".join(result) or "-"


def fields(spec, schema, name="body", required=True, depth=0):
    schema = resolve(spec, schema)
    description = schema.get("description", "") if isinstance(schema, dict) else ""
    rows = [(name, "yes" if required else "no", typename(spec, schema), limits(schema), description)]
    if depth > 16 or not isinstance(schema, dict):
        return rows
    for key, child in schema.get("properties", {}).items():
        rows.extend(fields(spec, child, name + "." + key, key in schema.get("required", []), depth + 1))
    if isinstance(schema.get("items"), dict):
        rows.extend(fields(spec, schema["items"], name + "[]", False, depth + 1))
    for kind in ["oneOf", "anyOf", "allOf"]:
        for index, branch in enumerate(schema.get(kind, []), 1):
            rows.extend(fields(spec, branch, f"{name} / {kind}[{index}]", required, depth + 1))
    if isinstance(schema.get("additionalProperties"), dict):
        rows.extend(fields(spec, schema["additionalProperties"], name + ".*", False, depth + 1))
    return rows


def field_table(rows):
    out = ["| Field | Required | Type | Allowed values / constraints | Description |",
           "|---|---|---|---|---|"]
    out.extend("| " + " | ".join(cell(part) for part in row) + " |" for row in rows)
    return out


def operations(spec):
    result = []
    for path, methods in spec["paths"].items():
        for method, operation in methods.items():
            if method in METHODS:
                result.append((operation.get("operationId", method + path), method.upper(), path, operation))
    return sorted(result)


def markdown(spec):
    digest = hashlib.sha256(encoded(spec).encode("utf-8")).hexdigest()
    out = ["# MiniKMB REST API reference", "",
           "Generated from the application's OpenAPI contract; do not edit the field tables manually.",
           "OpenAPI JSON: `openapi.json`. Interactive tester: `REST-API.html`.",
           f"Contract SHA-256 (canonical JSON): `{digest}`.", "",
           "## Connection and interpretation", "",
           "Enable REST in the GUI and use its actual loopback address. HTTP controls require `Authorization: Bearer API_KEY` from the app. The offline server URL `/` is relative, not a listening address. Local CLI IPC does not require HTTP or an API key.", "",
           "GUI, CLI, REST and MCP share one active channel. Only selection takes a UID. Mouse x/y are capture pixels. Submitting a job or completing a USB report does not prove target application/media completion.", "",
           "Required fields apply within their chosen parent/variant. oneOf means exactly one alternative, anyOf one or more, allOf all constraints. Nested rows and [] describe child/item fields. Unknown fields are rejected where additionalProperties=false. Scalar query constraints are taken from the app's x-input-schema when no standard parameter entry is present.", "",
           "These are structural schema limits. Endpoint descriptions and current channel/device/resource state add runtime conditions; satisfying a type range alone does not make every value valid. Read errors and status rather than replaying uncertain mutations.", "",
           "To test from a browser, open `/api/v1/reference` on that REST address. The bundled HTML can browse the contract offline, then open the same-origin tester; no broad CORS exemption is needed. Keys stay in page memory and are not embedded in these files.", "",
           "## Operations", "", "| Operation | Method | Path | Active channel |", "|---|---|---|---|"]
    for name, method, path, op in operations(spec):
        out.append(f"| [{name}](#{name}) | {method} | `{path}` | {'yes' if op.get('x-active-required') else 'see description'} |")
    for name, method, path, op in operations(spec):
        out.extend(["", f"## {name}", "", f"**{method} `{path}`** — {op.get('summary', '')}", "",
                    op.get("description", ""), ""])
        parameters = op.get("parameters", [])
        if parameters:
            out.extend(["### Path, query and header parameters", ""])
            rows = []
            for p in parameters:
                rows.extend(fields(spec, p.get("schema", {}), p["in"] + "." + p["name"], p.get("required", False)))
            out.extend(field_table(rows))
        if method in ["GET", "DELETE"] and "x-input-schema" in op:
            out.extend(["", "### Query arguments", ""])
            out.extend(field_table(fields(spec, op["x-input-schema"], "query", False)))
        for mime, content in op.get("requestBody", {}).get("content", {}).items():
            out.extend(["", f"### Request body: `{mime}`", ""])
            out.extend(field_table(fields(spec, content.get("schema", {}), "body", op["requestBody"].get("required", False))))
            if "example" in content:
                out.extend(["", "Request example (not executed automatically):", "", "```json",
                            json.dumps(content["example"], ensure_ascii=False, indent=2), "```"])
        if not parameters and "requestBody" not in op and "x-input-schema" not in op:
            out.append("No request arguments are declared.")
        out.extend(["", "### Responses", "", "| HTTP | Content type | Description |", "|---|---|---|"])
        for code, response in op.get("responses", {}).items():
            out.append("| " + " | ".join(cell(v) for v in [code, ", ".join(response.get("content", {})), response.get("description", "")]) + " |")
        out.extend(["", "Exact response objects, headers and component schemas are in `openapi.json` under this operation."])
    return "\n".join(out) + "\n"


def html(spec, template):
    # JSON data must not terminate its application/json script element.
    data = encoded(spec).replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e").replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
    if template.count("__OPENAPI_JSON__") != 1:
        raise ValueError("The API tester template needs exactly one schema marker")
    return template.replace("__OPENAPI_JSON__", data)


def generate(root, spec_bytes):
    spec = json.loads(spec_bytes)
    if spec.get("openapi") != "3.1.0" or not spec.get("paths"):
        raise ValueError("Expected the application's OpenAPI 3.1 contract")
    for server in spec.get("servers", []):
        if server.get("url") != "/":
            raise ValueError("Bundle the offline contract, not a live server address")
    outputs = {"openapi.json": json.dumps(spec, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
               "REST-API.md": markdown(spec),
               "REST-API.html": html(spec, (root / "scripts/api-browser.html").read_text(encoding="utf-8"))}
    for name, text in outputs.items():
        target = root / "packaging" / name
        data = text.encode("utf-8")
        if not target.is_file() or target.read_bytes() != data:
            target.write_bytes(data)
    return outputs
