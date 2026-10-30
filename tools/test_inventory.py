#!/usr/bin/env python3
"""Inventory tests/ declarations and record subsystem-owned destinations.

This reads Rust syntax trees, Python ASTs and Cargo metadata.
It does not execute tests, evaluate cfg expressions or move source files.
"""

from __future__ import annotations

import argparse
import ast
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

from tree_sitter import Language, Parser
import tree_sitter_rust


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/testing"
RUST = Parser(Language(tree_sitter_rust.language()))
SPECIAL_TARGETS = {
    "compatibility_matrix": "compatibility",
    "executor_parallel": "executor",
}


def relative(path: Path) -> str:
    return path.resolve().relative_to(ROOT).as_posix()


def text_of(node, data: bytes) -> str:
    return data[node.start_byte:node.end_byte].decode()


def rust_declarations(path: Path):
    data = path.read_bytes()
    tree = RUST.parse(data)
    if tree.root_node.has_error:
        raise ValueError(f"Rust syntax tree has errors: {relative(path)}")
    tests, modules, includes = [], [], []

    def visit(container, namespace=(), module_lines=0):
        attributes = []
        for node in container.named_children:
            if node.type == "attribute_item":
                attributes.append(text_of(node, data))
                continue
            if node.type in {"line_comment", "block_comment"}:
                continue
            if node.type == "function_item":
                if any(re.fullmatch(r"#\[\s*(?:\w+::)?test(?:\([^]]*\))?\s*\]", a) for a in attributes):
                    name = text_of(node.child_by_field_name("name"), data)
                    tests.append({
                        "name": name,
                        "line": node.start_point.row + 1,
                        "end_line": node.end_point.row + 1,
                        "inline_modules": list(namespace),
                        "inline_module_lines": module_lines,
                        "attributes": list(attributes),
                        "body": text_of(node, data),
                    })
            elif node.type == "mod_item":
                name = text_of(node.child_by_field_name("name"), data)
                body = node.child_by_field_name("body")
                matches = [re.search(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]', a) for a in attributes]
                explicit = next((m.group(1) for m in matches if m), None)
                modules.append({"name": name, "parents": namespace, "explicit": explicit, "inline": body is not None})
                if body is not None:
                    visit(body, namespace + (name,), node.end_point.row - node.start_point.row + 1)
            else:
                value = text_of(node, data)
                match = re.fullmatch(r'include!\(\s*"([^"]+)"\s*\)\s*;?', value)
                if match:
                    includes.append({"path": match.group(1), "parents": namespace})
            attributes = []

    visit(tree.root_node)
    return {"tests": tests, "modules": modules, "includes": includes, "data": data}


def strip_wave_suffix(name: str) -> str:
    """Strip the earliest valid wave suffix in one reverse pass over its tokens.

    Preserve the old Unicode decimal-digit and final-newline behavior without
    repartitioning a digit run through nested regex repetitions.
    """
    end = len(name) - int(name.endswith("\n"))
    parts = name[:end].split("_")
    offset = end
    suffix_valid = True
    cut = None
    for index in range(len(parts) - 1, -1, -1):
        part = parts[index]
        offset -= len(part)
        if index and suffix_valid and part.startswith("wave") and all(c.isdecimal() for c in part[4:]):
            cut = offset - 1
        ascii_end = 0
        while ascii_end < len(part) and ("a" <= part[ascii_end] <= "z" or "0" <= part[ascii_end] <= "9"):
            ascii_end += 1
        suffix_valid = suffix_valid and ascii_end > 0 and all(c.isdecimal() for c in part[ascii_end:])
        offset -= 1
    return name if cut is None else name[:cut] + name[end:]


def clean_name(name: str) -> str:
    name = re.sub(r"^(?:(?:postgresql18|pg18|integration_sql|integration|sprint\d+|issue_\d+|wave\d+|residual)_)+", "", name)
    name = strip_wave_suffix(name)
    name = re.sub(r"^issue_\d+_", "", name)
    name = re.sub(r"^type_", "", name)
    name = re.sub(r"(?:_(?:inventory|compat|gaps|residuals?|contracts?))+$", "", name)
    return name or "semantics"


DOMAINS = [
    (r"large_object|\blo_(?:get|open|create|from)|\bloread|\blowrite", "large_objects"),
    (r"materialized_view|matview", "ddl/materialized_views"),
    (r"create_table|table_clone|table_forms|ctas", "ddl/tables"),
    (r"jsonpath|jsonb_path", "types/json/jsonpath"),
    (r"json_table|json_record|recordset", "types/json/table_functions"),
    (r"generate_series|generate_subscripts|set_returning|rows_from|ordinality", "set_returning"),
    (r"text_search|tsvector|tsquery|ts_config|ts_dict|to_ascii", "text_search"),
    (r"json", "types/json"),
    (r"array|unnest|subscripts", "types/arrays"),
    (r"numeric|decimal|bigint_aggregate", "types/numeric"),
    (r"bytea|bit_string|binary_bytea|bytea_bits", "types/bit_bytea"),
    (r"timestamptz|timetz|timestamp|interval|temporal|timezone|date_bin|clock|pg_sleep", "types/temporal"),
    (r"network|inet|cidr|macaddr", "types/network"),
    (r"uuid|lsn", "types/uuid_lsn"),
    (r"multirange|range", "types/ranges"),
    (r"xml|xpath", "types/xml"),
    (r"geometry|geometric|polygon|circle|lseg", "types/geometry"),
    (r"money|monetary", "types/money"),
    (r"scalar_subquery|subquery|subqueries|exists", "subqueries"),
    (r"recursive|(?:^|[/_ ])cycle(?:$|[/_ ])|\bcte|ctes|materializ", "ctes"),
    (r"tablesample|table_sampling", "table_sampling"),
    (r"window|(?:^|_)ntile(?:_|$)|ranking|lag_|lead_|frame|exclude", "aggregates/windows"),
    (r"aggregate|grouping|rollup|cube|percentile|bool_and|bool_or|return_every_for_boolean|every_aggregate|ordered_set_mode", "aggregates"),
    (r"generate_series|set_returning|rows_from|ordinality", "set_returning"),
    (r"string|split_part|format_|strpos|parse_ident", "functions/string"),
    (r"math|round_|trunc_|ceiling|power|width_bucket", "functions/math"),
    (r"regex|regexp|similar|like_|pattern|escape", "patterns"),
    (r"null_safe|distinct_from", "comparisons/null_safe"),
    (r"coalesce|nullif|greatest|least|conditional|case_", "expressions/conditional"),
    (r"polymorphic|unknown|coercion|type_resolution|type_output|type_declared|type_scalar|type_codec", "type_resolution"),
    (r"inheritance|inherited", "inheritance"),
    (r"partition", "partitions"),
    (r"sequence|identity", "sequences"),
    (r"operator|opclass|opfamily", "operators"),
    (r"cast|literal|grammar|identifier|collation|comment|unicode", "grammar"),
    (r"merge", "merge"),
    (r"upsert|conflict|excluded", "conflicts"),
    (r"returning", "returning"),
    (r"copy", "copy"),
    (r"trigger", "triggers"),
    (r"routine|function|procedure|plpgsql|dynamic_sql|do_block", "routines"),
    (r"constraint|foreign_key|exclusion", "constraints"),
    (r"statistics|analyze", "statistics"),
    (r"advisory|notify|notification|listen", "coordination"),
    (r"lock|isolation|transaction|snapshot|savepoint|prepared_xact|two_phase", "transactions"),
    (r"privilege|ownership|grant|role_|authorization|security", "security"),
    (r"schema|database|temp_|atomic_drop|ddl|table_form", "ddl"),
    (r"portal", "portals"),
    (r"parameter|bind", "parameters"),
    (r"cancel|deadline|timeout", "cancellation"),
    (r"memory|budget|resource|limit", "resources"),
    (r"join", "joins"),
    (r"projection", "projection"),
    (r"sort|order", "ordering"),
    (r"union|intersect|except|set_operation", "set_operations"),
    (r"values|derived_source|relation", "relations"),
    (r"filter|predicate|comparison", "predicates"),
    (r"index|reindex", "indexes"),
]


def domain_for(hint: str, target: str) -> str:
    if target == "bench_operations":
        return "artifacts"
    if target == "configuration_isolated":
        return "configuration"
    if target == "domain_models":
        family = "graph" if "graph" in hint else "time_series"
        for token, folder in [("transaction", "transactions"), ("rollup", "rollups"), ("retention", "retention"), ("partition", "partitions"), ("index", "indexes"), ("neighbor", "neighbors"), ("scope", "database_scope"), ("ddl", "ddl")]:
            if token in hint:
                return family + "/" + folder
        return family + "/queries"
    if target == "vector_embeddings":
        for token, folder in [("provider", "providers"), ("publication", "publication"), ("hnsw", "indexes/hnsw"), ("ivfflat", "indexes/ivfflat"), ("hybrid", "hybrid"), ("embedding", "embeddings")]:
            if token in hint:
                return folder
        return "vectors"
    if target == "rest":
        for token, folder in [("auth", "authentication"), ("static", "static_ui"), ("session", "sessions"), ("metric", "metrics"), ("embedding", "embeddings"), ("database", "databases"), ("json", "json"), ("transaction", "transactions")]:
            if token in hint:
                return folder
        return "queries"
    if target == "analytics":
        for token, folder in [("dictionary", "dictionaries"), ("projection", "projections"), ("join", "joins"), ("read_model", "read_models"), ("column", "column_store")]:
            if token in hint:
                return folder
        return "execution"
    if target == "search":
        return "index_lifecycle" if re.search(r"restart|recover|persist|rebuild|drop", hint) else "retrieval"
    if target == "metrics":
        for token in ["adaptive", "capacity", "feedback", "joins", "runtime", "search", "read_paths", "plan"]:
            if token in hint:
                return token
        return "runtime"
    for pattern, domain in DOMAINS:
        if re.search(pattern, hint):
            return domain
    return {
        "catalog_security": "catalogs",
        "compatibility": "clients",
        "executor": "execution",
        "planner_controls": "planning",
        "query_scan_controls": "scans",
        "storage_indexes": "storage",
        "parser_types": "grammar",
        "pgwire_core": "protocol",
        "pgwire_extended": "protocol",
        "sql_mutations": "mutations",
        "sql_queries": "expressions",
    }.get(target, "semantics")


def observation_for(test: dict) -> str:
    hint = test["name"]
    for pattern, name in [
        (r"cancel|interrupt|deadline|timeout", "cancellation"),
        (r"restart|reopen|crash|persist", "restart"),
        (r"recoverable_state|recover_after.*error", "recovery"),
        (r"memory|budget|resource|growth|bounded|limit", "resources"),
        (r"reject|error|invalid|malform|unsupported|overflow|sqlstate", "errors"),
        (r"binary|codec|framing", "binary"),
        (r"parameter|bind|infer|unknown", "parameters"),
        (r"metadata|descriptor|describe|oid|typmod|catalog", "metadata"),
        (r"rollback|savepoint|atomic|transaction", "transactions"),
        (r"null|empty", "nulls"),
    ]:
        if re.search(pattern, hint):
            return name
    return "semantics"


def owner_domain(path: str, name: str, target: str, domain: str, namespace: list[str]) -> str:
    """Give explicit source ownership precedence over incidental SQL type words."""
    source = "/".join(list(Path(path).parts[2:]) + namespace).lower()
    if target == "planner_controls":
        if "statement_routes" in source:
            return "planning"
        if "planner_commands" in source and re.search(r"create_table|drop_table", name):
            return "ddl/tables"
        if "planner_physical" in source and re.search(r"filter|predicate", name):
            return "predicates"
    if target == "catalog_security":
        if "index_property_functions" in source:
            return "catalogs/indexes"
        if "type_base_information" in source or "type_catalog_discovery" in source:
            return "catalogs/types"
        if "type_catalog_gaps" in source:
            if re.search(r"enum|custom_type|regtype|composite", name):
                return "catalogs/types"
            if "system_columns" in name:
                return "catalogs/columns"
        if "visibility_functions" in source:
            return "catalogs/visibility"
        if "call_authorization" in source:
            return "security/authorization"
        if "catalog_relation_shapes" in source or "catalog_information" in source or "catalog_lookup" in source:
            for pattern, folder in [(r"constraint|foreign_key", "constraints"), (r"sequence", "sequences"), (r"index|access_method|opclass", "indexes"), (r"function|routine|procedure|parameter", "routines"), (r"type|enum|array|basetype", "types"), (r"column|attribute|default", "columns"), (r"size|toast|compression", "sizes"), (r"description|comment", "descriptions"), (r"partition", "partitions"), (r"trigger", "triggers"), (r"polic", "policies"), (r"collation", "collations"), (r"current_user|owner|privilege|role", "identity_privileges"), (r"session|backend|recovery|postmaster|database|configuration|encoding|keyword", "system_information")]:
                if re.search(pattern, name):
                    return "catalogs/" + folder
            return "catalogs/relations"
    if target == "compatibility" and "lifecycle/" in source:
        for marker, folder in [("copy_forms", "copy"), ("exclusion_constraints", "constraints"), ("type_object_lifecycle", "types"), ("large_objects", "large_objects")]:
            if marker in source:
                return folder
        if "session_transaction" in source:
            if "temporary_view" in name:
                return "namespaces"
            return "coordination" if re.search(r"advisory|notify|listen", source + name) else "sessions" if re.search(r"cursor|settings|set_session|discard|reset|prepared_state", source + name) else "transactions"
        if "ddl_extensions" in source:
            for pattern, folder in [(r"extension|pgcrypto", "extensions"), (r"partition", "partitions"), (r"index_contract|reindex", "indexes"), (r"sequence", "sequences"), (r"table_forms|table_storage", "tables"), (r"database_temp|temp_views", "namespaces")]:
                if re.search(pattern, source.replace("ddl_extensions", "") + name):
                    return folder
            return "ddl"
        if "routine_objects" in source:
            for pattern, folder in [(r"trigger", "triggers"), (r"comment", "comments"), (r"ownership|privilege|role_membership|security", "security"), (r"operator", "operators"), (r"cast", "casts"), (r"conversion", "conversions"), (r"statistics", "statistics"), (r"aggregate", "aggregates")]:
                if re.search(pattern, source + name):
                    return folder
            return "routines"
        if "mutation_ddl" in source:
            for pattern, folder in [(r"inherit", "inheritance"), (r"materialized_view", "materialized_views"), (r"temporary_table", "namespaces"), (r"enum|domain_constraint", "types"), (r"identity_generation", "sequences"), (r"index_maintenance", "indexes"), (r"partition", "partitions"), (r"truncate", "truncate"), (r"merge", "merge"), (r"conflict", "conflicts"), (r"cte", "ctes"), (r"update|assign", "update_from"), (r"delete", "delete_using")]:
                if re.search(pattern, name):
                    return folder
            return "mutations"
    if target == "compatibility" and "lifecycle_inventory" in source:
        for pattern, folder in [(r"large_object", "large_objects"), (r"inheritance", "inheritance"), (r"extension", "extensions"), (r"comment", "comments"), (r"revoke|privilege", "security/privileges")]:
            if re.search(pattern, name):
                return folder
    if target == "compatibility" and "pg_dump_restore" in source:
        return "clients/dump_restore"
    if "temporal_functions" in source or "temporal_function_compat" in source:
        return "functions/temporal"
    if "string_functions" in source or "string_compat" in source:
        return "functions/string"
    if "pattern_matching" in source:
        return "patterns"
    if "window_ranking" in source or "window_function" in source:
        return "aggregates/windows"
    if target == "sql_mutations" and "merge_tuple_assignments" in source:
        return "merge"
    if "math_compat" in source:
        return "functions/math"
    if "sql_casts" in source:
        return "casts"
    if "type_scalar_matrix" in source or "type_scalar_array_matrix" in source:
        return "types/scalars"
    mixed = bool(re.search(r"inventory|wave|residual|gaps", source))
    aggregate_hint = name if mixed else source + " " + name
    if "subquer" not in source and re.search(r"aggregate|aggregation|grouping|rollup|cube|percentile|hypothetical|ordered_set_mode|bool_and|bool_or|string_agg|array_agg|json_agg|jsonb_agg|return_every_for_boolean|every_aggregate|(?:^|_)(?:sum|avg)(?:_|$)", aggregate_hint):
        for pattern, folder in [(r"window|frame|exclude|ranking|(?:^|_)ntile(?:_|$)", "windows"), (r"sql_defined|sql_aggregate", "sql_defined"), (r"percentile|hypothetical|ordered_set|mode_", "ordered_set"), (r"grouping|rollup|cube", "grouping"), (r"bool|every", "boolean"), (r"range", "ranges"), (r"json", "json"), (r"string_agg|string_aggregate", "strings"), (r"bytea|bitwise|bit_", "bit_bytea"), (r"array", "arrays"), (r"interval|temporal", "temporal"), (r"money", "money"), (r"xml", "xml"), (r"numeric|bigint|decimal", "numeric"), (r"corr|covar|stddev|variance|regr|statistic", "statistical"), (r"collection", "collections"), (r"distinct|filter|order_by|order_local", "modifiers")]:
            if re.search(pattern, aggregate_hint):
                return "aggregates/" + folder
        return "aggregates/core"
    return domain


def choose_placement(path: str, test: dict, targets: list[str], file_lines: int, decision=None) -> dict:
    if decision is not None:
        return {
            "suite": decision["suite"], "category": decision["category"],
            "directory": str(Path(path).parent), "file": path,
            "basis": "reviewed_physical_layout", "review": "selected", "migration": "keep",
        }
    parts = Path(path).parts
    inferred = Path(path).stem if len(parts) == 2 else parts[1]
    target = sorted(targets)[0] if targets else SPECIAL_TARGETS.get(inferred, inferred)
    target = SPECIAL_TARGETS.get(target, target)
    module = test["inline_modules"][-1] if test["inline_modules"] else Path(path).stem
    broad = len(parts) == 2 or bool(re.search(r"inventory|wave|residual|gaps|scalar_types", path))
    oversized = file_lines > 1000 and len(parts) > 2
    hint = (" ".join(test["inline_modules"] + [test["name"]]) if broad or oversized else "/".join(parts[2:])).lower()
    domain = domain_for(hint, target)
    domain = owner_domain(path, test["name"], target, domain, test["inline_modules"])
    if path == "tests/compatibility.rs":
        scope = test["inline_modules"][0] if test["inline_modules"] else ""
        client = "desktop_traces" if scope == "desktop_client_trace_replay" else "tokio_postgres"
        for name in ["sqlalchemy", "diesel", "sqlx", "prisma", "psql", "pgadmin"]:
            if name in scope + test["name"]:
                client = name
                break
        domain = "clients/" + client
    if path.endswith("scalar_subqueries/issue_800_row_arity_aliases.rs"):
        domain = "grammar/rows"
    if target == "catalog_security" and test["inline_modules"] and test["inline_modules"][0] == "auth":
        domain = "security/authentication"
    if target == "compatibility" and not domain.startswith(("clients", "extensions/")):
        domain = "lifecycle/" + domain
    if target == "catalog_security" and not domain.startswith(("security", "catalogs")):
        domain = "catalogs/" + domain
    if target == "parser_types" and domain != "grammar":
        domain = "grammar/" + domain
    leaf = clean_name(module)
    if leaf in {target, "pgwire_extended", "compatibility", "catalog_security", "semantics", "grammar", "type_array_text_inputs", "type_json_scalar_documents"}:
        leaf = observation_for(test)
    inline_oversized = test.get("inline_module_lines", 0) > 1000
    if broad or oversized or inline_oversized:
        scope = clean_name(module)
        if scope in {target, "pgwire_extended", "compatibility", "catalog_security", "semantics", "grammar", "residuals"}:
            scope = ""
        domain = "/".join(filter(None, [domain, scope]))
        leaf = observation_for(test)
    if Path(path).name == "type_json_scalar_documents.rs":
        name = test["name"]
        family = "sql_json" if "sql_json" in name or "returning_identity" in name else "numeric_carriers" if re.search(r"numeric|unsigned|signed", name) else "table_functions" if re.search(r"expand|record", name) else "constructors" if re.search(r"construct|unique|to_json", name) else "operators"
        domain, leaf = f"types/json/{family}", observation_for(test)
    if Path(path).name == "type_array_text_inputs.rs":
        name = test["name"]
        family = "aggregate_identity" if "aggregate" in name else "dml_identity" if re.search(r"assign|update", name) else "index_queries" if re.search(r"btree|index", name) else "bounds_subscripts" if re.search(r"bound|dimension|subscript|slice|fill", name) else "operators" if re.search(r"contain|overlap|concat|operator", name) else "functions" if re.search(r"function|join|position|remove|replace|append|prepend|unnest", name) else "text_inputs"
        domain, leaf = f"types/arrays/{family}", observation_for(test)
    if target == "analytics" and module == "projection_lifecycle":
        name = test["name"]
        family = "replay" if re.search(r"replay|event", name) else "publication" if re.search(r"stops|publish|activate", name) else "refresh" if "refresh" in name else "versions"
        domain, leaf = f"projections/{family}", observation_for(test)
    destination = f"tests/{target}/{domain}/{leaf}.rs"
    if path.startswith("tests/support/"):
        return {"suite": "support", "category": "shared_helper_tests", "directory": str(Path(path).parent), "file": path, "basis": "helper_unit_test", "review": "recommended", "migration": "keep_shared_helper_namespace"}
    if len(parts) > 3 and parts[2] in {"grammar", "types", "catalogs", "aggregates", "large_objects"} and not re.search(r"postgresql18|issue_\d|wave|residual|gaps", path) and not oversized:
        return {"suite": target, "category": "/".join(parts[2:-1]), "directory": str(Path(path).parent), "file": path, "basis": "canonical_domain_path", "review": "recommended", "migration": "keep"}
    return {
        "suite": target,
        "category": domain,
        "directory": str(Path(destination).parent),
        "file": destination,
        "basis": "individual_test_contract" if broad or oversized or inline_oversized else "focused_source_module",
        "review": "required" if broad or oversized or inline_oversized else "recommended",
        "migration": "preserve_module_alias_and_fixture_scope",
    }


def is_test_inventory_file(path):
    return path.is_file() and path.suffix != ".pyc" and "__pycache__" not in path.parts


def gather():
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT))
    targets = {t["name"]: Path(t["src_path"]) for p in metadata["packages"] for t in p["targets"] if t["test"] and "test" in t["kind"]}
    files = sorted(p for p in (ROOT / "tests").rglob("*") if is_test_inventory_file(p))
    parsed = {p.resolve(): rust_declarations(p) for p in files if p.suffix == ".rs"}
    registrations = defaultdict(list)
    unresolved = []
    repeated_registrations = []
    seen = set()
    decisions_path = OUTPUT / "layout-decisions.json"
    decisions = json.loads(decisions_path.read_text())["files"] if decisions_path.is_file() else {}

    def register(path, target, namespace=(), root=False, path_override=False):
        path = path.resolve()
        key = (path, target, namespace)
        if key in seen:
            repeated_registrations.append({"source": relative(path), "suite": target, "module": "::".join(namespace)})
            return
        seen.add(key)
        if path not in parsed:
            if path.is_file():
                return  # A benchmark implementation imported by its test target.
            unresolved.append({"source": str(path), "suite": target, "module": "::".join(namespace)})
            return
        registrations[path].append({"suite": target, "module": "::".join(namespace)})
        record = parsed[path]
        default_base = path.parent if root or path_override or path.name == "mod.rs" else path.parent / path.stem
        for module in record["modules"]:
            if module["inline"]:
                continue
            parents = module["parents"]
            attr_base = default_base.joinpath(*parents) if parents else path.parent
            base = default_base.joinpath(*parents)
            if module["explicit"]:
                child = attr_base / module["explicit"]
            else:
                candidates = [base / (module["name"] + ".rs"), base / module["name"] / "mod.rs"]
                child = next((p for p in candidates if p.is_file()), candidates[0])
            register(child, target, namespace + parents + (module["name"],), path_override=bool(module["explicit"]))
        for included in record["includes"]:
            register(path.parent / included["path"], target, namespace + included["parents"])

    for target, path in sorted(targets.items()):
        register(path, target, root=True)

    inventory, file_records = [], []
    for path in files:
        rel = relative(path)
        record = parsed.get(path.resolve())
        source_tests = record["tests"] if record else []
        python_tests = []
        if path.suffix == ".py":
            tree = ast.parse(path.read_text(), filename=rel)
            for node in ast.walk(tree):
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith(("should_", "test_")):
                    python_tests.append({"name": node.name, "line": node.lineno, "end_line": node.end_lineno, "inline_modules": [], "attributes": [], "body": ast.get_source_segment(path.read_text(), node)})
        source_tests = source_tests + python_tests
        bindings = registrations.get(path.resolve(), [])
        suites = sorted({b["suite"] for b in bindings})
        role = "test_module"
        if rel.startswith("tests/support/"):
            role = "shared_support"
        elif rel.startswith("tests/fixtures/"):
            role = "external_client_fixture"
        elif rel.startswith("tests/corpus/"):
            role = "oracle_data"
        elif rel.startswith("tests/pg18_parity/"):
            role = "parity_harness"
        elif path in targets.values():
            role = "cargo_target"
        elif not source_tests:
            if record:
                role = "module_registration" if record["modules"] or record["includes"] else "test_support"
            else:
                role = "fixture_data"
        destinations = set()
        lines = len(path.read_bytes().splitlines())
        for test in source_tests:
            local = "::".join(test["inline_modules"] + [test["name"]])
            instances = [{"suite": b["suite"], "name": "::".join(filter(None, [b["module"], local]))} for b in bindings]
            decision = decisions.get(rel)
            if decision and decision["suite"] not in suites:
                raise ValueError(f"Reviewed layout has an unregistered owner: {rel}")
            placement = choose_placement(rel, test, suites, lines, decision)
            if path.suffix == ".py":
                suite = "test_inventory" if rel.startswith("tests/test_inventory/") else "pg18_parity"
                category = "tooling_harness" if suite == "test_inventory" else "parity_harness"
                placement = {"suite": suite, "category": category, "directory": f"tests/{suite}", "file": rel, "basis": "distinct_python_runtime", "review": "recommended", "migration": "keep"}
            destinations.add(placement["file"])
            inventory.append({
                "id": f"{rel}::{local}", "name": test["name"], "current_file": rel,
                "line": test["line"], "end_line": test["end_line"], "inline_modules": test["inline_modules"],
                "inline_module_lines": test.get("inline_module_lines", 0),
                "language": "python" if path.suffix == ".py" else "rust",
                "registration": "python_harness" if path.suffix == ".py" else ("registered" if instances else "unregistered"),
                "instances": instances, "attributes": test["attributes"],
                "ignored": any(re.match(r"#\[\s*ignore", a) for a in test["attributes"]),
                "execution_status": "not_assessed", "placement": placement,
            })
        if role in {"shared_support", "external_client_fixture", "oracle_data", "parity_harness"}:
            destinations.add(rel)
        if role == "cargo_target" or not source_tests:
            destinations.add(rel)
        action = "keep_harness_and_extract_tests" if role == "cargo_target" and source_tests else "split_by_contract" if len(destinations) > 1 else "keep" if destinations == {rel} else "move_with_module_alias"
        file_records.append({"current_file": rel, "role": role, "action": action, "lines": lines, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "test_declarations": len(source_tests), "registered_suites": suites, "destinations": sorted(destinations), "line_limit": 12000 if role == "cargo_target" else (1000 if path.suffix == ".rs" else None)})
    planned = json.loads((OUTPUT / "planned-tests.json").read_text())
    if len({t["id"] for t in inventory}) != len(inventory):
        raise ValueError("Duplicate source declaration IDs; inspect conditional or macro definitions")
    if len({t["name"] for t in planned}) != len(planned):
        raise ValueError("Duplicate proposed test names")
    identities = defaultdict(list)
    for test in inventory:
        for instance in test["instances"]:
            identities[(instance["suite"], instance["name"])].append(test["id"])
    duplicate_identities = [
        {"suite": suite, "name": name, "sources": sources}
        for (suite, name), sources in sorted(identities.items()) if len(sources) > 1
    ]
    for test in planned:
        destination = Path(test["file"])
        if test["suite"] not in targets or destination.parts[:2] != ("tests", test["suite"]) or str(destination.parent) != test["directory"] or "/".join(destination.parts[2:-1]) != test["category"]:
            raise ValueError(f"Invalid proposed suite/directory/file: {test['name']}")
    projected = defaultdict(list)
    for test in inventory:
        projected[test["placement"]["file"]].append(test)
    conflicts = []
    large_destinations = []
    for destination, tests in sorted(projected.items()):
        names = Counter("::".join(t["inline_modules"] + [t["name"]]) if t["placement"]["basis"] == "reviewed_physical_layout" else t["name"] for t in tests)
        for name, count in names.items():
            if count > 1:
                conflicts.append({"file": destination, "name": name, "sources": [t["id"] for t in tests if t["name"] == name or "::".join(t["inline_modules"] + [t["name"]]) == name]})
        body_lines = sum(t["end_line"] - t["line"] + 1 for t in tests)
        if body_lines > 800:
            large_destinations.append({"file": destination, "test_body_lines": body_lines, "test_declarations": len(tests)})
    summary = {
        "scope": "All files under tests/, including untracked files; Rust #[test] declarations and Python should_/test_ functions. Excludes unit tests under src/ and benchmark definitions under benches/.",
        "cargo_targets": [{"name": name, "file": relative(path)} for name, path in sorted(targets.items())],
        "files": len(files), "rust_files": len(parsed),
        "rust_test_declarations": sum(t["language"] == "rust" for t in inventory),
        "python_test_declarations": sum(t["language"] == "python" for t in inventory),
        "registered_rust_instances": sum(len(t["instances"]) for t in inventory if t["language"] == "rust"),
        "unregistered_rust_tests": [t["id"] for t in inventory if t["registration"] == "unregistered"],
        "unresolved_module_paths": unresolved,
        "repeated_module_registrations": repeated_registrations,
        "duplicate_registered_identities": duplicate_identities,
        "planned_tests": len(planned),
        "oversized_files": [f["current_file"] for f in file_records if f["line_limit"] and f["lines"] > f["line_limit"]],
        "destination_name_conflicts": conflicts,
        "large_projected_destinations": large_destinations,
        "reviewed_layout_files": len(decisions),
        "placement_status": "Reviewed physical placements are recorded in layout-decisions.json; remaining placements are proposals. No test result is inferred.",
    }
    return {"schema_version": 1, "summary": summary, "tests": inventory, "files": file_records, "planned_tests": planned}


def render(data):
    outputs = {OUTPUT / "inventory.json": json.dumps(data, indent=2) + "\n"}
    groups = defaultdict(list)
    for test in data["tests"]:
        groups[test["placement"]["suite"]].append(test)
    summary = data["summary"]
    lines = [
        "# Inventory summary", "", "Generated from `inventory.json`. Counts describe source declarations and static registrations, not test outcomes.", "",
        "| Measure | Count |", "| --- | ---: |",
        f"| Files under `tests/` | {summary['files']} |",
        f"| Rust files | {summary['rust_files']} |",
        f"| Rust test declarations | {summary['rust_test_declarations']} |",
        f"| Python test declarations | {summary['python_test_declarations']} |",
        f"| Static Rust registration instances | {summary['registered_rust_instances']} |",
        f"| Cargo test targets | {len(summary['cargo_targets'])} |",
        f"| Reconciled backfill names | {summary['planned_tests']} |",
        f"| Unregistered Rust declarations | {len(summary['unregistered_rust_tests'])} |",
        f"| Unresolved module paths | {len(summary['unresolved_module_paths'])} |",
        f"| Duplicate registered test identities | {len(summary['duplicate_registered_identities'])} |",
        f"| Repeated module registrations | {len(summary['repeated_module_registrations'])} |",
        f"| Files with a reviewed physical placement | {summary['reviewed_layout_files']} |",
        f"| Existing files over their line limit | {len(summary['oversized_files'])} |",
        f"| Destination name conflicts | {len(summary['destination_name_conflicts'])} |", "",
        "Shared support tests compile into several Cargo suites, so registration instances exceed physical source declarations. `cfg` expressions are not evaluated.", "",
        "## Existing tests by owner", "",
        "Each declaration appears on one owner page. Shared helper tests appear under `support`; their individual suite instances are retained in the JSON.", "",
        "| Owner | Source declarations | Review required |", "| --- | ---: | ---: |",
    ]
    for suite, tests in sorted(groups.items()):
        lines.append(f"| [{suite}](inventory/{suite}.md) | {len(tests)} | {sum(t['placement']['review'] == 'required' for t in tests)} |")
    lines += ["", "## Cargo targets", "", "Each suite's Cargo target and source root are recorded here.", "", "| Cargo target | Root |", "| --- | --- |"]
    for target in summary["cargo_targets"]:
        lines.append(f"| `{target['name']}` | `{target['file']}` |")
    lines += ["", "## Files to split", "", "Root Cargo suites have a 12,000 line limit. Other Rust files have a 1,000 line limit.", ""]
    lines += [f"- `{path}`" for path in summary["oversized_files"]] or ["None."]
    lines += ["", "## Proposed groups requiring size review", "", "These groups already exceed 800 lines of test bodies, before imports and helpers. Split further if the complete file would exceed 1,000 lines.", "", "| Proposed file | Test body lines | Tests |", "| --- | ---: | ---: |"]
    for group in summary["large_projected_destinations"]:
        lines.append(f"| `{group['file']}` | {group['test_body_lines']} | {group['test_declarations']} |")
    lines += ["", "## Backfill authoring status", "", "Each item links to a registered assertion in the backfill list. Authoring status and Cassie execution status are separate.", "", "| Status | Names |", "| --- | ---: |"]
    for status, count in sorted(Counter(t["status"] for t in data["planned_tests"]).items()):
        lines.append(f"| `{status}` | {count} |")
    outputs[OUTPUT / "summary.md"] = "\n".join(lines) + "\n"
    for suite, tests in sorted(groups.items()):
        lines = [f"# {suite} test placements", "", "Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.", ""]
        if suite == "support":
            lines += ["`support` is a placement group, not a Cargo target. Helper tests compile inside the Cargo suites listed in each inventory record's `instances` field.", ""]
        lines += ["| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |", "| --- | --- | --- | --- | --- | ---: | --- |"]
        for t in tests:
            label = "::".join(t["inline_modules"] + [t["name"]])
            link = f"../../../{t['current_file']}#L{t['line']}"
            lines.append(f"| `{label}` | [{t['current_file']}:{t['line']}]({link}) | `{t['placement']['file']}` | {t['registration']} | {'yes' if t['ignored'] else 'no'} | {len(t['instances'])} | {t['placement']['review']} |")
        outputs[OUTPUT / "inventory" / f"{suite}.md"] = "\n".join(lines) + "\n"
    lines = ["# Test file inventory", "", "Every file under `tests/`, including support, fixtures, Python harness tests and oracle data.", "", "| Current file | Role | Lines | Tests | Registered suites | Action | Selected destinations |", "| --- | --- | ---: | ---: | --- | --- | --- |"]
    for f in data["files"]:
        destinations = "; ".join(f"`{d}`" for d in f["destinations"]) or "Registration/imports follow relocated children; no test body in this file."
        lines.append(f"| `{f['current_file']}` | {f['role']} | {f['lines']} | {f['test_declarations']} | {', '.join(f['registered_suites']) or '—'} | {f['action']} | {destinations} |")
    outputs[OUTPUT / "files.md"] = "\n".join(lines) + "\n"
    outputs[OUTPUT / "planned-tests.md"] = render_planned_tests(data["planned_tests"])
    return outputs


def render_planned_tests(planned):
    lines = [
        "# Reconciled backfill tests",
        "",
        "A current assertion links to its source file. When the recorded source path is absent, the row keeps that path as text and links the owning issue instead. This records missing source evidence without treating it as executed or passed.",
        "A refined name or an existing equivalent retains the original planning name for traceability. Authoring and Cassie execution status are separate.",
        "",
        "| Issue | Owner / category | Inventory name | Assertion / source record | Authoring | Cassie execution |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for test in planned:
        actual = test.get("actual_test", test["name"])
        if (ROOT / test["file"]).is_file():
            assertion = f"[`{actual}`](../../{test['file']})"
        else:
            issue_link = f"https://github.com/cntryl/cassie/issues/{test['issue']}"
            assertion = (
                f"`{actual}` (no current source at `{test['file']}`; "
                f"see [#{test['issue']}]({issue_link}))"
            )
        lines.append(
            f"| #{test['issue']} | `{test['suite']}/{test['category']}` | "
            f"`{test['name']}` | {assertion} | {test['status']} | "
            f"{test['execution_status']} |"
        )
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Check generated inventory files without writing them")
    args = parser.parse_args()
    data = gather()
    outputs = render(data)
    drift = []
    # Only this directory's generated suite pages are eligible for stale cleanup.
    stale = [path for path in (OUTPUT / "inventory").glob("*.md") if path not in outputs and any(line.startswith("Generated from `inventory.json`.") for line in path.read_text().splitlines()[:4])]
    for path in stale:
        if args.check:
            drift.append(relative(path))
        else:
            path.unlink()
    for path, content in outputs.items():
        if args.check:
            if not path.is_file() or path.read_text() != content:
                drift.append(relative(path))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    if drift:
        print("Inventory differs: " + ", ".join(drift), file=sys.stderr)
        return 1
    summary = data["summary"]
    print(json.dumps({k: summary[k] for k in ["files", "rust_files", "rust_test_declarations", "python_test_declarations", "registered_rust_instances", "planned_tests", "unregistered_rust_tests", "unresolved_module_paths", "repeated_module_registrations", "duplicate_registered_identities", "oversized_files", "destination_name_conflicts", "large_projected_destinations"]}, indent=2))
    errors = [key for key in ["unregistered_rust_tests", "unresolved_module_paths", "repeated_module_registrations", "duplicate_registered_identities", "oversized_files", "destination_name_conflicts"] if summary[key]]
    if errors:
        print("Inventory requires repair: " + ", ".join(errors), file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
