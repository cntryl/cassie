// Consolidated integration suite: pgwire_core.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/wire_queries.rs"]
mod support_type_metadata_contract;
#[path = "support/wire_bind.rs"]
mod support_wire_bind;

// Formerly tests/pgwire.rs.
#[path = "pgwire_core/protocol/pgwire.rs"]
mod pgwire;

// Formerly tests/pgwire_binary_codecs.rs.
#[path = "pgwire_core/codecs/binary/binary_codecs/mod.rs"]
mod pgwire_binary_codecs;
#[path = "pgwire_core/codecs/scalar_casts.rs"]
mod scalar_casts;

#[path = "pgwire_core/parameters/text_parameters.rs"]
mod pgwire_text_parameters;

// Formerly tests/pgwire_cancellation.rs.
#[path = "pgwire_core/cancellation/semantics.rs"]
mod pgwire_cancellation;

// Formerly tests/pgwire_copy_recovery.rs.
#[path = "pgwire_core/copy/copy_recovery.rs"]
mod pgwire_copy_recovery;

// COPY sub-protocol state and framing over the simple-query protocol.
#[path = "pgwire_core/copy/copy_protocol.rs"]
mod pgwire_copy_protocol;

// Formerly tests/pgwire_database_images.rs.
#[path = "pgwire_core/databases/database_images.rs"]
mod pgwire_database_images;
// Formerly tests/pgwire_database_scope.rs.
#[path = "pgwire_core/databases/database_scope.rs"]
mod pgwire_database_scope;

// Formerly tests/pgwire_hardening.rs.
#[path = "pgwire_core/protocol/hardening.rs"]
mod pgwire_hardening;

// Formerly tests/pgwire_metrics.rs.
#[path = "pgwire_core/metrics/semantics.rs"]
mod pgwire_metrics;

// Formerly tests/pgwire_simple_query.rs.
#[path = "pgwire_core/simple_query/semantics/mod.rs"]
mod pgwire_simple_query;

// Formerly tests/pgwire_startup.rs.
#[path = "pgwire_core/startup/bound_listener.rs"]
mod bound_listener;
#[path = "pgwire_core/startup/semantics.rs"]
mod pgwire_startup;
// Formerly tests/pgwire_tls.rs.
#[path = "pgwire_core/tls/semantics.rs"]
mod pgwire_tls;

// Formerly tests/pgwire_transaction_semantics.rs.
#[path = "pgwire_core/transactions/transaction_semantics.rs"]
mod pgwire_transaction_semantics;

// Formerly tests/pgwire_transaction_staging.rs.
#[path = "pgwire_core/transactions/transaction_staging.rs"]
mod pgwire_transaction_staging;
