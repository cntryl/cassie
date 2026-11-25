//! PostgreSQL 18 pattern and regular-expression behavior tracked by #799.

use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config, SqlFixture};

fn current_accounted_memory_bytes(fixture: &SqlFixture) -> u64 {
    fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted query memory")
}

include!("pattern_matching/semantics.rs");

include!("pattern_matching/nulls.rs");

include!("pattern_matching/parameters.rs");

include!("pattern_matching/cancellation.rs");
