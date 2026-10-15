import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('verify_lock', Path(__file__).with_name('verify_lock.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def package(name='turso', version='0.8.2'):
    return {'name': name, 'version': version, 'id': name + '@' + version}


def metadata(packages, nodes=None):
    if nodes is None:
        nodes = [{'id': p['id'], 'features': []} for p in packages]
    return {'packages': packages, 'resolve': {'nodes': nodes}}


class LockTests(unittest.TestCase):
    def test_all_native_rows_pass(self):
        packages = [package(), package('turso_core'), package('tokio', '1.53.1')]
        self.assertEqual(module.verify(packages, metadata(packages))['status'], 'PASS')

    def test_duplicate_wrong_version_rejected_in_both_orders(self):
        for versions in [('0.9.0', '0.8.2'), ('0.8.2', '0.9.0')]:
            with self.subTest(versions=versions), self.assertRaises(ValueError):
                module.verify([package(version=v) for v in versions])

    def test_missing_sdk_rejected(self):
        with self.assertRaises(ValueError):
            module.verify([package('turso_core')])

    def test_missing_sdk_metadata_rejected(self):
        with self.assertRaises(ValueError):
            module.verify([package()], metadata([]))

    def test_missing_sdk_resolve_node_rejected(self):
        with self.assertRaises(ValueError):
            module.verify([package()], metadata([package()], []))

    def test_metadata_version_mismatch_rejected(self):
        with self.assertRaises(ValueError):
            module.verify([package()], metadata([package(version='0.9.0')]))

    def test_forbidden_features_rejected(self):
        for feature in ['default', 'fts', 'sync', 'mimalloc']:
            with self.subTest(feature=feature), self.assertRaises(ValueError):
                module.verify([package()], metadata([package()], [{'id': 'turso@0.8.2', 'features': [feature]}]))

    def test_lance_arrow_rejected(self):
        for name in ['lancedb', 'lance-core', 'arrow-array']:
            with self.subTest(name=name), self.assertRaises(ValueError):
                module.verify([package(), package(name)])


if __name__ == '__main__':
    unittest.main()
