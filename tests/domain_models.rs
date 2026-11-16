// Consolidated integration suite: domain_models.
// Shared fixtures live in tests/support; former test targets remain named modules.

use cassie_test_support::support_sql;
#[path = "support/time_series_evidence.rs"]
mod support_time_series_evidence;

// Formerly tests/time_series_index_completeness.rs.
#[path = "domain_models/time_series/indexes/time_series_index_completeness.rs"]
mod time_series_index_completeness;

// Formerly tests/time_series_indexes.rs.
#[path = "domain_models/time_series/indexes/time_series_indexes/mod.rs"]
mod time_series_indexes;

// Formerly tests/time_series_retention.rs.
#[path = "domain_models/time_series/retention/time_series_retention.rs"]
mod time_series_retention;
// Formerly tests/time_series_rollups.rs.
#[path = "domain_models/time_series/rollups/time_series_rollups/mod.rs"]
mod time_series_rollups;

#[path = "domain_models/time_series/partitions/time_series_partition_delimiters.rs"]
mod time_series_partition_delimiters;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "domain_models/time_series/rollups/boolean_predicates.rs"]
mod rollup_boolean_predicates;
use cassie_test_support::support_sql_fixture;

#[path = "domain_models/time_series/queries/column_time_series_read_equivalence.rs"]
mod column_time_series_read_equivalence;

#[path = "domain_models/time_series/qualification.rs"]
mod time_series_qualification;
