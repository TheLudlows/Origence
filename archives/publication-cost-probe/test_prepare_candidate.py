import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('prepare_candidate', Path(__file__).with_name('prepare_candidate.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CandidateTests(unittest.TestCase):
    def test_numbered_paths_preserve_scope_last_binding(self):
        for count in [8, 16]:
            original = 'SELECT ' + ','.join(['json_extract(payload,?)'] * count)
            original += ' FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?'
            candidate = module.materialized_sql(original, count)
            self.assertIn(f'tenant_id=?{count+1} AND workspace_id=?{count+2} AND id=?{count+3}', candidate)
            self.assertEqual([int(n) for n in module.re.findall(r'\?(\d+)', candidate)], list(range(count+1, count+4)) + list(range(1, count+1)))
            self.assertEqual(candidate.count('jsonb(payload)'), 1)

    def test_only_two_constant_literals_change(self):
        def constant(name, count):
            sql = 'SELECT ' + ','.join(['json_extract(payload,?)'] * count)
            sql += ' FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?'
            return f'const {name}: &str = "{sql}";'
        unchanged = '\nasync fn algorithm() { original_flow(); }\nconst OTHER: &str = "SELECT 42";\n'
        source = unchanged + constant('PUBLICATION_IDS', 8) + unchanged + constant('PUBLICATION_DIMENSIONS', 16) + unchanged
        candidate = module.candidate_source(source)
        for name in ['PUBLICATION_IDS', 'PUBLICATION_DIMENSIONS']:
            pattern = r'const ' + name + r': &str = "(?:\\.|[^"\\])*";'
            source = module.re.sub(pattern, '<constant>', source)
            candidate = module.re.sub(pattern, '<constant>', candidate)
        self.assertEqual(candidate, source)

    def test_unexpected_shape_or_parameter_count_fails(self):
        for sql in ['SELECT ?', 'DELETE FROM oc_jobs', 'SELECT ? FROM oc_jobs WHERE tenant_id=? AND workspace_id=? AND id=?']:
            with self.subTest(sql=sql), self.assertRaises(ValueError):
                module.materialized_sql(sql, 8)

    def test_changed_source_fails_before_creating_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'src/storage/sqlite/publication.rs'
            source.parent.mkdir(parents=True)
            source.write_text('unreviewed algorithm')
            output = root / 'candidate'
            with self.assertRaises(ValueError):
                module.prepare(root, output)
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
