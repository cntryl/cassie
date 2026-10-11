"""Run bootstrap differential cells; unexecuted inventory remains pending."""

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import uuid

from .comparison import compare, qualify
from .libpq import Libpq
from .reference import reference


class FixtureError(RuntimeError):
    def __init__(self, side, index, results):
        super().__init__("corpus fixture did not complete successfully")
        self.side = side
        self.index = index
        self.results = results


def prepare(connection, cell, side):
    for index, sql in enumerate(cell.get("setup", [])):
        results = connection.query(sql)
        if (len(results) != 1 or results[0]["status"] not in (1, 2)
                or results[0]["sqlstate"] is not None):
            raise FixtureError(side, index, results)


def instantiate(cell):
    sql = [cell["sql"], *cell.get("setup", [])]
    if not any("{fixture}" in statement for statement in sql):
        return cell
    namespace = "p18_" + uuid.uuid4().hex
    return {**cell, "fixture_namespace": namespace,
            "sql": cell["sql"].replace("{fixture}", namespace),
            "setup": [statement.replace("{fixture}", namespace) for statement in cell.get("setup", [])]}


def run_cells(libpq, reference_dsn, cassie_dsn, cells):
    outcomes = []
    for cell in cells:
        for mode in cell["modes"]:
            prepared = instantiate(cell)
            context = {"id": f"{cell['id']}:{mode}", "owner": cell["owner"]}
            if "fixture_namespace" in prepared:
                context["fixture_namespace"] = prepared["fixture_namespace"]
            try:
                with libpq.connect(reference_dsn) as oracle, libpq.connect(cassie_dsn) as cassie:
                    prepare(oracle, prepared, "reference")
                    prepare(cassie, prepared, "cassie")
                    expected = oracle.query(prepared["sql"], mode, cell.get("parameters", []))
                    actual = cassie.query(prepared["sql"], mode, cell.get("parameters", []))
                differences = []
                if len(expected) != len(actual):
                    differences.append("result_count")
                else:
                    for index, (left, right) in enumerate(zip(expected, actual)):
                        differences += [f"{index}:{name}" for name in compare(left, right, ordered=cell["ordered"])]
                outcomes.append({**context,
                                 "outcome": "failed" if differences else "passed",
                                 "differences": differences, "reference": expected, "cassie": actual})
            except FixtureError as error:
                outcomes.append({**context, "outcome": "failed",
                                 "stage": "setup", "side": error.side, "setup_index": error.index,
                                 "results": error.results})
            except (ConnectionError, TimeoutError, NotImplementedError) as error:
                outcomes.append({**context, "outcome": "unavailable",
                                 "reason": str(error)})
    return outcomes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-bin-dir", required=True)
    parser.add_argument("--libpq", required=True)
    parser.add_argument("--cassie-dsn-env", default="CASSIE_PARITY_DSN")
    parser.add_argument("--cassie-revision", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.cassie_revision):
        parser.error("--cassie-revision must be a full immutable commit SHA")
    dsn = os.environ.get(args.cassie_dsn_env)
    if not dsn:
        parser.error("Cassie connection environment variable is missing")
    corpus = pathlib.Path(__file__).resolve().parents[2] / "tests/corpus/postgresql18"
    paths = {name: corpus / (name + ".json") for name in ["reference", "bootstrap", "returning", "boolean", "scalar-receive", "inventory"]}
    manifest, bootstrap, returning, boolean, scalar_receive, inventory = [json.loads(paths[name].read_text()) for name in paths]
    cells = bootstrap + returning + boolean + scalar_receive
    required = {f"{cell['id']}:{mode}" for cell in cells for mode in cell["modes"]}
    receipt = {
        "schema": "cassie-pg18-differential.v1", "qualification": "incomplete",
        "cassie_revision": args.cassie_revision, "cassie_revision_provenance": "caller_asserted",
        "reference_artifact": manifest,
        "inputs": {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()},
        "libpq_sha256": hashlib.sha256(pathlib.Path(args.libpq).read_bytes()).hexdigest(),
        "outcomes": [], "pending_inventory": len(inventory["entries"]),
    }
    args.output.mkdir(parents=True, exist_ok=False)
    try:
        libpq = Libpq(args.libpq)
        with reference(args.reference_bin_dir, manifest, args.output / "ref", libpq) as oracle_dsn:
            receipt["outcomes"] = run_cells(libpq, oracle_dsn, dsn, cells)
        receipt["bootstrap_problems"] = qualify(required, receipt["outcomes"])
        receipt["bootstrap"] = "failed" if receipt["bootstrap_problems"] else "passed"
        # Bootstrap fixtures intentionally do not cover every inventoried form/context/path.
        receipt["qualification_problems"] = ["inventory cells remain pending", "instance revision needs deployment proof"]
        code = 1 if receipt["bootstrap_problems"] else 2
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        receipt["bootstrap"] = "unavailable"
        receipt["reason"] = str(error)
        code = 2
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"bootstrap": receipt["bootstrap"], "qualification": "incomplete",
                      "pending_inventory": receipt["pending_inventory"], "receipt": str(args.output / "receipt.json")}))
    return code


if __name__ == "__main__":
    sys.exit(main())
