// Consolidated integration suite: rest.
#[path = "rest/authentication/redaction.rs"]
mod authentication_redaction;
#[path = "support/authentication_redaction.rs"]
mod support_authentication_redaction;
#[path = "rest/transactions/errors.rs"]
mod transaction_errors;

#[path = "support/retrieval.rs"]
mod support_retrieval;
#[path = "support/retrieval_provider.rs"]
mod support_retrieval_provider;

#[path = "rest/queries/vector_memory_controls.rs"]
mod vector_memory_controls;
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/tls.rs"]
mod support_tls;

// Formerly tests/network_listener_authentication.rs.
#[path = "rest/authentication/network_listener_authentication.rs"]
mod network_listener_authentication;

// Formerly tests/rest.rs.
#[path = "rest/queries/semantics/mod.rs"]
mod rest;

// Formerly tests/rest_admin_databases.rs.
#[path = "rest/databases/admin_databases.rs"]
mod rest_admin_databases;

// Formerly tests/rest_admin_query.rs.
#[path = "rest/queries/admin_query/mod.rs"]
mod rest_admin_query;

// Formerly tests/rest_admin_query_method_errors.rs.
#[path = "rest/queries/admin_query_method_errors.rs"]
mod rest_admin_query_method_errors;

// Formerly tests/rest_admin_ui_static.rs.
#[path = "rest/static_ui/admin_ui_static.rs"]
mod rest_admin_ui_static;

// Formerly tests/rest_embeddings.rs.
#[path = "rest/embeddings/semantics/mod.rs"]
mod rest_embeddings;

// Formerly tests/rest_json_contract.rs.
#[path = "rest/json/semantics.rs"]
mod rest_json_contract;

// Formerly tests/rest_metrics.rs.
#[path = "rest/metrics/semantics.rs"]
mod rest_metrics;

// Formerly tests/rest_sessions.rs.
#[path = "rest/sessions/semantics.rs"]
mod rest_sessions;
