"""Exclusive offline report writing shared by integrity/diagnosis tools."""
import json
from aml_drill import DrillError


def write_report(path, report, operation, *, error_code="evaluation_failed"):
    """Retain partial evidence and safe errors; never overwrite a previous run."""
    with open(path, "x", encoding="utf-8") as output:
        try:
            operation(report)
            report["status"] = "completed"
        except DrillError as error:
            report.update(status="failed", error=str(error))
        except Exception:
            report.update(status="failed", error=error_code)
        finally:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write("\n")
    return 0 if report["status"] == "completed" else 1
