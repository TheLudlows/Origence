"""Cross-language AML v1 identity contract; no services or model calls."""
import hashlib
import json
from pathlib import Path
import unittest
import uuid


class WorkspaceIdentityVectors(unittest.TestCase):
    def test_frozen_bytes_and_uuid_v5(self):
        path = Path(__file__).resolve().parents[1] / "docs/contracts/aml-workspace-v1-vectors.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        namespace = uuid.UUID(data["namespace"])
        self.assertEqual(str(namespace), "9b6adf90-8252-4a40-83e0-64f14f6febed")
        self.assertGreaterEqual(len(data["vectors"]), 8)
        seen = set()
        for vector in data["vectors"]:
            fields = ["aml", str(uuid.UUID(vector["tenant"])),
                      str(uuid.UUID(vector["root"])), vector["user"]]
            encoded = b"origence:owned-workspace:v1\0"
            for field in fields:
                raw = field.encode("utf-8")
                encoded += len(raw).to_bytes(8, "big") + raw
            self.assertEqual(encoded.hex(), vector["input_hex"])
            digest = hashlib.sha1(namespace.bytes + encoded).digest()[:16]
            identifier = str(uuid.UUID(bytes=digest, version=5))
            self.assertEqual(identifier, vector["workspace_id"])
            self.assertNotIn(identifier, seen)
            seen.add(identifier)


if __name__ == "__main__":
    unittest.main()
