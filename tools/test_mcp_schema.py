"""A valid closed empty object schema may omit the optional properties keyword."""
import unittest

from mcp_smoke import schema_object


class EmptyToolSchemaTests(unittest.TestCase):
    def test_zero_argument_tool_accepts_absent_properties(self):
        schema = {"type": "object", "additionalProperties": False}
        self.assertEqual(schema_object(schema, schema, {}, []), {})

    def test_nonempty_contract_still_requires_its_fields(self):
        schema = {"type": "object", "additionalProperties": False}
        with self.assertRaises(RuntimeError):
            schema_object(schema, schema, {"query": "string"}, ["query"])

    def test_empty_schema_does_not_allow_unknown_fields(self):
        schema = {"type": "object", "additionalProperties": True}
        with self.assertRaises(RuntimeError):
            schema_object(schema, schema, {}, [])
