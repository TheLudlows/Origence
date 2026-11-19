import unittest

from public_handoff import CASES, answer_differences, parse_answer


class PublicHandoffTests(unittest.TestCase):
    def test_copying_first_answer_fails_each_second_case(self):
        for name, case in CASES.items():
            with self.subTest(scenario=name):
                self.assertEqual([], answer_differences(case, "first", case["first_expected"]))
                self.assertTrue(answer_differences(case, "second", case["first_expected"]))

    def test_pool_requires_a_pool_field_and_a_read_only_check(self):
        case = CASES["database_wait"]
        answer = dict(case["second_expected"], evidence_field="pool_active")
        self.assertEqual([], answer_differences(case, "second", answer))
        self.assertEqual(["evidence_field"], answer_differences(case, "second", dict(answer, evidence_field="blocker_session")))
        self.assertEqual(["next_check"], answer_differences(case, "second", dict(answer, next_check="change_pool_size")))

    def test_amounts_are_integers_and_require_the_current_version(self):
        case = CASES["expense_review"]
        answer = case["second_expected"]
        self.assertEqual([], answer_differences(case, "second", answer))
        self.assertEqual(["policy_version"], answer_differences(case, "second", dict(answer, policy_version=1)))
        self.assertEqual(["payable_minor"], answer_differences(case, "second", dict(answer, payable_minor="97")))
        self.assertEqual(["payable_minor"], answer_differences(case, "first", dict(case["first_expected"], payable_minor=False)))

    def test_rejects_ambiguous_model_output(self):
        self.assertEqual({"answer": 1}, parse_answer('```json\n{"answer":1}\n```'))
        for text in ('{"answer":1,"answer":2}', '{"answer":NaN}', 'null', 'Result: {"answer":1}'):
            with self.subTest(text=text), self.assertRaises(RuntimeError):
                parse_answer(text)
