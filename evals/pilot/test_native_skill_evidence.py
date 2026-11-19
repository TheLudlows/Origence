import unittest

from live_skill_preflight import codex_request_matches


class NativeRequestEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.prompt = self.message("user", "Public test, Run 123")
        self.skill = self.message("developer", "expense-check: synthetic instructions")

    def message(self, role, text):
        return {"type": "message", "role": role, "content": [{"type": "input_text", "text": text}]}

    def matches(self, *items):
        return codex_request_matches({"input": list(items)}, "expense-check", "synthetic instructions", "Run 123")

    def test_requires_both_skill_and_expected_run(self):
        self.assertTrue(self.matches(self.prompt, self.skill))
        self.assertFalse(self.matches(self.message("user", "Public test, Run 456"), self.skill))
        self.assertFalse(self.matches(self.prompt, self.message("developer", "expense-check")))

    def test_output_cannot_supply_the_skill(self):
        self.assertFalse(self.matches(self.prompt, self.message("assistant", "expense-check: synthetic instructions")))
        self.assertFalse(self.matches(self.prompt, {"type": "function_call_output", "output": self.skill}))


if __name__ == "__main__":
    unittest.main()
