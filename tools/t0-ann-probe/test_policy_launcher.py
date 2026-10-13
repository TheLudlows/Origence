"""Combination gates never let one good rebuild rescue another or count twice."""
import copy
import unittest
import policy


def result(number):
    return {"report":f"build-{number}/native-report.json","quality_passed":True,
            "performance":{"performance_passed":True},"fixture_indexed_sha256":"a"*64,
            "fixture_final_sha256":"b"*64,"contract_sha256":"c"*64,
            "initial_index_uuids":[f"00000000-0000-4000-8000-{number:012d}"]}


class PolicyCombinationTests(unittest.TestCase):
    def test_two_successful_distinct_builds(self):
        self.assertEqual(policy.combine([result(1),result(2)])["status"],"BOUNDED_CANDIDATE")

    def test_missing_extra_or_copied_builds_fail(self):
        for values in ([result(1)],[result(1),result(1)],[result(1),result(2),result(3)]):
            self.assertEqual(policy.combine(values)["status"],"RECOMMEND_EXACT")
        second=result(2);second["initial_index_uuids"]=result(1)["initial_index_uuids"]
        self.assertEqual(policy.combine([result(1),second])["status"],"RECOMMEND_EXACT")

    def test_either_quality_or_performance_failure_stops(self):
        for number in (0,1):
            values=[result(1),result(2)];values[number]["quality_passed"]=False
            self.assertEqual(policy.combine(values)["status"],"RECOMMEND_EXACT")
            values=[result(1),result(2)];values[number]["performance"]["performance_passed"]=False
            self.assertEqual(policy.combine(values)["status"],"RECOMMEND_EXACT")

    def test_fixture_and_contract_identity_required(self):
        for key in ("fixture_indexed_sha256","fixture_final_sha256","contract_sha256"):
            values=[result(1),result(2)];values[1][key]="d"*64
            self.assertEqual(policy.combine(values)["status"],"RECOMMEND_EXACT")
            values=copy.deepcopy(values);values[1][key]=None
            self.assertEqual(policy.combine(values)["status"],"RECOMMEND_EXACT")
