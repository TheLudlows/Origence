"""Reject incomplete evidence and inconsistent maintenance receipts without model calls."""
import copy
import unittest
from aml_drill import DrillError
from aml_artifacts import PHASES
from aml_maintenance_verify import validate_receipts, validate_schedule


def receipts():
    first = {"version": 10, "rows": 100, "fragments": 100, "bytes": 5000}
    compacted = {"version": 12, "rows": 100, "fragments": 2, "bytes": 4000}
    grown = {"version": 268, "rows": 356, "fragments": 258, "bytes": 15000}
    second = {"version": 270, "rows": 356, "fragments": 1, "bytes": 14000}
    def receipt(before, after=None):
        return {"dry_run": after is None, "historical_versions_retained": True,
                "concurrency": 2, "target_rows_per_fragment": 4096,
                "tables": [{"table_index": 0, "before": before, "after": after}]}
    return copy.deepcopy({"compacted_preview": receipt(first), "compacted": receipt(first, compacted),
                          "reopened_preview": receipt(compacted), "recompacted_preview": receipt(grown),
                          "recompacted": receipt(grown, second)})


class MaintenanceAuditTests(unittest.TestCase):
    def test_exact_schedule_rejects_missing_duplicate_or_reordered_queries(self):
        ids = [f"q{i}" for i in range(10)]
        rows = []
        for phase in PHASES:
            if phase == "grown":
                rows.extend({"phase": "during_growth", "question_id": q} for q in ids[:8])
            rows.extend({"phase": phase, "question_id": q} for q in ids)
        validate_schedule(rows, ids)
        bad = copy.deepcopy(rows)
        bad[1], bad[2] = bad[2], bad[1]
        for invalid in [rows[:-1], rows + [rows[-1]], bad]:
            with self.subTest(kind=len(invalid)), self.assertRaises(DrillError):
                validate_schedule(invalid, ids)

    def test_valid_receipts(self):
        validate_receipts(receipts())

    def test_row_loss_or_extra_table_rejected(self):
        for mutate in [lambda r: r["compacted"]["tables"][0]["after"].update(rows=99),
                       lambda r: r["compacted"]["tables"].append(copy.deepcopy(r["compacted"]["tables"][0]))]:
            invalid = receipts()
            mutate(invalid)
            with self.assertRaises(DrillError):
                validate_receipts(invalid)

    def test_changed_preview_and_restart_rejected(self):
        for name in ["compacted_preview", "reopened_preview", "recompacted_preview"]:
            invalid = receipts()
            # Break shared fixture references before changing one recorded observation.
            invalid[name] = copy.deepcopy(invalid[name])
            invalid[name]["tables"][0]["before"]["version"] += 1
            with self.subTest(name=name), self.assertRaises(DrillError):
                validate_receipts(invalid)

    def test_pruning_or_missing_growth_rejected(self):
        invalid = receipts()
        invalid["recompacted"]["historical_versions_retained"] = False
        with self.assertRaises(DrillError):
            validate_receipts(invalid)
        invalid = receipts()
        invalid["recompacted"]["tables"][0]["before"]["rows"] -= 1
        invalid["recompacted"]["tables"][0]["after"]["rows"] -= 1
        with self.assertRaises(DrillError):
            validate_receipts(invalid)


if __name__ == "__main__":
    unittest.main()
