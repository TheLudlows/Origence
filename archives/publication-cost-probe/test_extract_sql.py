import importlib.util
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).parent
spec = importlib.util.spec_from_file_location('extract_sql', HERE / 'extract_sql.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExtractionTests(unittest.TestCase):
    def test_rust_line_continuation_and_quotes(self):
        self.assertEqual(module.rust_string('SELECT \\\n   1 AS \\"x\\"'), 'SELECT 1 AS "x"')

    def test_only_one_selected_literal(self):
        self.assertEqual(module.literal_starting('call("SELECT one")', 'SELECT '), 'SELECT one')
        with self.assertRaises(ValueError):
            module.literal_starting('"SELECT one"; "SELECT two"', 'SELECT ')

    def test_missing_constant_fails_closed(self):
        with self.assertRaises(ValueError):
            module.constant('const X: &str = "SELECT 1";', 'Y')

    def test_source_hash_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / 'src/storage/sqlite/publication.rs'
            file.parent.mkdir(parents=True)
            file.write_text('changed source')
            with self.assertRaises(ValueError):
                module.read_source(directory, '0' * 64)

    def test_constants_are_cooked_literals(self):
        self.assertEqual(module.constant('const X: &str = "SELECT ?";', 'X'), 'SELECT ?')


if __name__ == '__main__':
    unittest.main()
