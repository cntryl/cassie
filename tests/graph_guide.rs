// Integration suite: every SQL example of `docs/graph-guide.md` runs against an in-memory
// instance and its documented result is compared with the observed result (#1260).
// Subsystem files live under `graph_guide/`; shared fixtures stay in `support/`.

use cassie_test_support::support_sql;
use cassie_test_support::support_sql_fixture;

#[path = "graph_guide/guide.rs"]
mod guide;

#[path = "graph_guide/runner.rs"]
mod runner;

#[path = "graph_guide/examples.rs"]
mod examples;

#[path = "graph_guide/coverage.rs"]
mod coverage;
