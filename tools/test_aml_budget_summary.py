import unittest
from aml_budget_summary import interval


class ClusterTests(unittest.TestCase):
    def test_resamples_whole_clusters_not_correlated_questions(self):
        rows = [{"cluster": "shared", "value": 0}, {"cluster": "shared", "value": 1}]
        result = interval(rows, lambda r: r["value"])
        self.assertEqual(result["clusters"], 1)
        self.assertEqual((result["lower"], result["upper"]), (0.5, 0.5))
        rows[1]["cluster"] = "independent"
        result = interval(rows, lambda r: r["value"])
        self.assertEqual((result["lower"], result["upper"]), (0, 1))


if __name__ == "__main__":
    unittest.main()
