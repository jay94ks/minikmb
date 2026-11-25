import json
from pathlib import Path
import unittest
from api_reference import fields, html, markdown, operations


class ApiReferenceTests(unittest.TestCase):
    def fixture(self):
        return {"openapi": "3.1.0", "paths": {"/api/v1/test": {"post": {
            "operationId": "test_action", "requestBody": {"required": True, "content": {"application/json": {
                "schema": {"oneOf": [{"$ref": "#/components/schemas/Choice"}, {"type": "array", "minItems": 1, "maxItems": 128, "items": {"type": "string"}}]}}}},
            "responses": {"200": {"description": "accepted", "content": {"application/json": {"schema": {"type": "object"}}}}}}}},
            "components": {"schemas": {"Choice": {"type": "object", "additionalProperties": False,
                "properties": {"kind": {"const": "write", "type": "string"}, "size": {"type": "integer", "minimum": 1, "maximum": 512},
                               "mode": {"type": "string", "enum": ["fs", "hs"]}}, "required": ["kind", "size"]}}}}

    def test_reference_preserves_required_variants_enums_bounds_and_unknown_field_rules(self):
        spec = self.fixture()
        schema = spec["paths"]["/api/v1/test"]["post"]["requestBody"]["content"]["application/json"]["schema"]
        rows = fields(spec, schema)
        self.assertTrue(any(row[0].endswith(".size") and row[1] == "yes" and "maximum=512" in row[3] for row in rows))
        self.assertTrue(any(row[0].endswith(".mode") and row[1] == "no" and 'enum=["fs","hs"]' in row[3] for row in rows))
        text = markdown(spec)
        for fragment in ["POST `/api/v1/test`", "oneOf[1]", "oneOf[2]", "minItems=1", "unknown fields forbidden", 'const="write"']:
            self.assertIn(fragment, text)

    def test_html_json_data_cannot_inject_a_script(self):
        template = '<script type="application/json">__OPENAPI_JSON__</script>'
        result = html({"value": "</script><script>alert(1)</script>\u2028"}, template)
        self.assertNotIn("<script>alert(1)</script>", result)
        payload = result.split(">", 1)[1].rsplit("</script>", 1)[0]
        self.assertEqual(json.loads(payload)["value"], "</script><script>alert(1)</script>\u2028")

    def test_generated_reference_covers_every_bundled_operation_and_request_parameter(self):
        root = Path(__file__).resolve().parents[1]
        spec = json.loads((root / "packaging/openapi.json").read_text(encoding="utf-8"))
        text = (root / "packaging/REST-API.md").read_text(encoding="utf-8")
        self.assertEqual(markdown(spec), text)
        for name, method, path, operation in operations(spec):
            self.assertIn(f"## {name}\n", text)
            self.assertIn(f"**{method} `{path}`**", text)
            for parameter in operation.get("parameters", []):
                self.assertIn(parameter["in"] + "." + parameter["name"], text)
        self.assertEqual(html(spec, (root / "scripts/api-browser.html").read_text(encoding="utf-8")),
                         (root / "packaging/REST-API.html").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
