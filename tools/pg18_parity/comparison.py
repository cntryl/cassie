"""Exact bootstrap comparisons and fail-closed qualification accounting."""

from collections import Counter


def compare(expected, actual, *, ordered):
    """Compare raw protocol values; preserve SQL NULL and duplicate rows."""
    fields = ("status", "sqlstate", "command_tag", "columns", "parameter_oids")
    differences = [field for field in fields if expected[field] != actual[field]]
    if expected.get("prepared_columns") != actual.get("prepared_columns"):
        differences.append("prepared_columns")
    expected_rows, actual_rows = expected["rows"], actual["rows"]
    if not ordered:
        expected_rows = Counter(tuple(row) for row in expected_rows)
        actual_rows = Counter(tuple(row) for row in actual_rows)
    if expected_rows != actual_rows:
        differences.append("rows")
    return differences


def qualify(required, observations):
    """A missing, duplicate, unknown or nonpassing required cell prevents closure."""
    ids = Counter(item["id"] for item in observations)
    problems = [f"missing:{key}" for key in sorted(required - ids.keys())]
    problems += [f"unknown:{key}" for key in sorted(ids.keys() - required)]
    problems += [f"duplicate:{key}" for key, count in sorted(ids.items()) if count != 1]
    problems += [f"{item['outcome']}:{item['id']}" for item in observations if item["outcome"] != "passed"]
    return problems
