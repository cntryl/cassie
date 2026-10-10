"""Build a pending inventory from the checksum-verified PostgreSQL 18.6 source.

The inventory is an input to coverage review. It does not assert that every
syntax variant or function overload has an executable cell yet.
"""

import argparse
import hashlib
import html
import json
import pathlib
import re
import tarfile


def plain(value):
    return " ".join(html.unescape(re.sub(r"<[^>]*>", " ", value)).split())


def fields(entry):
    return dict(re.findall(r"(\w+)\s*=>\s*'((?:\\.|[^'\\])*)'", entry))


def catalog_entries(text):
    """Catalog strings can contain braces and escaped quotes; retain entire rows."""
    quoted = comment = escaped = False
    start = None
    for index, char in enumerate(text):
        if comment:
            comment = char != "\n"
        elif quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == "'":
                quoted = False
        elif char == "#":
            comment = True
        elif char == "'":
            quoted = True
        elif char == "{":
            if start is not None:
                raise ValueError("unexpected nested catalog record")
            start = index + 1
        elif char == "}":
            if start is None:
                raise ValueError("unmatched catalog record delimiter")
            yield text[start:index]
            start = None
    if start is not None or quoted:
        raise ValueError("unterminated catalog record")


def build(archive, manifest):
    if hashlib.sha256(archive.read_bytes()).hexdigest() != manifest["source_archive_sha256"]:
        raise ValueError("PostgreSQL source archive checksum mismatch")
    entries = []
    regression = []
    with tarfile.open(archive) as source:
        for member in source:
            name = member.name.removeprefix("postgresql-18.6/")
            if not member.isfile():
                continue
            if name.startswith(("src/test/regress/sql/", "src/pl/plpgsql/src/sql/", "src/test/isolation/specs/")):
                data = source.extractfile(member).read()
                regression.append({"path": name, "sha256": hashlib.sha256(data).hexdigest()})
            interesting = name.startswith("doc/src/sgml/ref/") and name.endswith(".sgml")
            interesting |= name in ["src/include/catalog/pg_type.dat", "src/include/catalog/pg_proc.dat",
                                    "doc/src/sgml/plpgsql.sgml", "doc/src/sgml/catalogs.sgml"]
            if not interesting:
                continue
            text = source.extractfile(member).read().decode()
            if name.startswith("doc/src/sgml/ref/"):
                if '<manvolnum>7</manvolnum>' not in text:
                    continue  # SQL command reference, not application/server executable manuals.
                title = re.search(r"<refentrytitle>(.*?)</refentrytitle>", text, re.S)
                forms = re.findall(r"<synopsis>(.*?)</synopsis>", text, re.S)
                for index, form in enumerate(forms):
                    entries.append({"id": f"command:{pathlib.Path(name).stem}:{index}",
                                    "kind": "command_form", "name": plain(title[1]),
                                    "form": plain(form), "source": name, "owner": 848,
                                    "outcome": "pending", "classification": "needs_owner_review"})
            elif name.endswith(".dat"):
                for entry in catalog_entries(text):
                    values = fields(entry)
                    kind = "type" if "typname" in values else "function_signature"
                    key = "typname" if kind == "type" else "proname"
                    if key in values:
                        entries.append({"id": f"{kind}:{values.get('oid', values[key])}", "kind": kind,
                                        "name": values[key], "catalog_fields": values, "source": name,
                                        "owner": 793 if kind == "type" else 848,
                                        "outcome": "pending", "classification": "needs_owner_review",
                                        "source_entry_sha256": hashlib.sha256(entry.encode()).hexdigest()})
                        if "array_type_oid" in values:
                            entries.append({"id": f"type:{values['array_type_oid']}", "kind": "type",
                                            "name": "_" + values[key], "element_oid": values["oid"],
                                            "source": name, "owner": 807, "outcome": "pending",
                                            "classification": "needs_owner_review"})
            else:
                for anchor in re.findall(r'<(?:sect[1-5]|varlistentry)\b[^>]*\bid="([^"]+)"', text):
                    kind = "plpgsql_construct" if "plpgsql" in name else "catalog"
                    entries.append({"id": f"{kind}:{anchor}", "kind": kind, "name": anchor,
                                    "source": name, "owner": 835 if kind == "plpgsql_construct" else 817,
                                    "outcome": "pending", "classification": "needs_form_expansion"})
    contexts = ["select_output", "where", "join_on", "having", "group_by", "order_by", "distinct_on",
                "aggregate_argument", "aggregate_filter", "aggregate_order", "window_argument",
                "window_partition", "window_order", "window_frame", "limit", "offset", "values",
                "insert_value", "update_assignment", "delete_predicate", "returning", "conflict_target",
                "conflict_update", "conflict_predicate", "cte", "recursive_cte", "lateral",
                "scalar_subquery", "row_subquery", "any_all_subquery", "check_constraint", "default",
                "generated_column", "index_expression", "index_predicate", "routine_body", "trigger_body"]
    entries += [{"id": f"context:{name}", "kind": "query_context", "name": name, "owner": 848,
                 "outcome": "pending", "classification": "needs_cross_product", "source": "delivery-contract"}
                for name in contexts]
    identities = [entry["id"] for entry in entries]
    if len(set(identities)) != len(identities):
        raise ValueError("duplicate upstream inventory identities")
    return {"schema": "cassie-pg18-inventory.v1", "reference": manifest["source_archive_sha256"],
            "coverage": "incomplete", "entries": sorted(entries, key=lambda x: x["id"]),
            "upstream_regression_inputs": sorted(regression, key=lambda x: x["path"])}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=pathlib.Path, required=True)
    parser.add_argument("--manifest", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    result = build(args.archive, json.loads(args.manifest.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"entries": len(result["entries"]), "regression_inputs": len(result["upstream_regression_inputs"])}))
