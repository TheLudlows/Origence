"""Build-time guard: external AML protocol stays in the API adapter."""
from pathlib import Path
import re
import unittest


class AmlAdapterBoundary(unittest.TestCase):
    def test_generic_layers_do_not_reference_aml_runtime(self):
        root = Path(__file__).resolve().parents[1]
        files = list((root / "src/storage").rglob("*.rs"))
        for name in ("jobs.rs", "worker.rs", "service.rs", "messages.rs", "parsing.rs"):
            files.append(root / "src" / name)
        for name in ("service", "messages"):
            files.extend((root / "src" / name).rglob("*.rs"))
        forbidden = re.compile(
            r"AmlIngest|aml_ingest|crate::aml|crate::api::aml|super::aml|"
            r"enable_aml_namespace|ensure_aml_user|lookup_aml_user|aml_role"
        )
        for path in files:
            with self.subTest(path=path.relative_to(root)):
                self.assertIsNone(forbidden.search(path.read_text(encoding="utf-8")))

    def test_compose_forwards_explicit_default_off_adapter_flag(self):
        root = Path(__file__).resolve().parents[1]
        self.assertIn("    OC_ENABLE_AML: ${OC_ENABLE_AML:-false}",
                      (root / "compose.yaml").read_text(encoding="utf-8"))
        self.assertIn("OC_ENABLE_AML=false", (root / ".env.example").read_text(encoding="utf-8"))

    def test_only_generic_schema_and_one_adapter_implementation_remain(self):
        root = Path(__file__).resolve().parents[1]
        for name in ("src/aml.rs", "src/aml/service.rs", "src/storage/sqlite/aml.rs",
                     "src/storage/aml-schema.sql", "src/storage/aml-add-schema.sql"):
            self.assertFalse((root / name).exists(), name)
        for path in (root / "src/storage").glob("*.sql"):
            self.assertNotIn("oc_aml_", path.read_text(encoding="utf-8"), str(path))
        self.assertTrue((root / "src/api/aml.rs").is_file())


if __name__ == "__main__":
    unittest.main()
