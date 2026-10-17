// Consolidated integration suite: catalog_security.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "catalog_security/catalogs/columns/catalog_information/metadata.rs"]
mod postgresql18_catalog_information;

#[path = "catalog_security/catalogs/columns/catalog_relation_shapes.rs"]
mod postgresql18_catalog_relation_shapes;

#[path = "support/backend_sessions.rs"]
mod support_backend_sessions;
#[path = "support/catalog.rs"]
mod support_catalog;
#[path = "support/catalog_policy.rs"]
mod support_catalog_policy;
#[path = "support/catalog_sessions.rs"]
mod support_catalog_sessions;
#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/wire_queries.rs"]
mod support_wire_queries;

#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "catalog_security/catalogs/types/catalog_discovery/metadata.rs"]
mod type_catalog_discovery;

#[path = "catalog_security/security/authorization/call_authorization.rs"]
mod call_authorization;

// Formerly tests/auth.rs.
#[path = "catalog_security/security/auth.rs"]
mod auth;

// Formerly tests/bootstrap_credential_rotation.rs.
#[path = "catalog_security/security/bootstrap_credential_rotation.rs"]
mod bootstrap_credential_rotation;

// Formerly tests/catalog_introspection.rs.
#[path = "catalog_security/catalogs/catalog_introspection.rs"]
mod catalog_introspection;
// Formerly tests/catalog_introspection_foreign_keys.rs.
#[path = "catalog_security/catalogs/catalog_introspection_foreign_keys.rs"]
mod catalog_introspection_foreign_keys;

// Formerly tests/catalog_introspection_pgadmin.rs.
#[path = "catalog_security/catalogs/catalog_introspection_pgadmin.rs"]
mod catalog_introspection_pgadmin;

// Formerly tests/catalog_orm_metadata.rs.
#[path = "catalog_security/catalogs/catalog_orm_metadata.rs"]
mod catalog_orm_metadata;

// Formerly tests/catalog_probes.rs.
#[path = "catalog_security/catalogs/session_information.rs"]
mod catalog_probes;

// Formerly tests/database_connect_grants.rs.
#[path = "catalog_security/security/database_connect_grants.rs"]
mod database_connect_grants;

// Formerly tests/database_connect_network.rs.
#[path = "catalog_security/security/database_connect_network.rs"]
mod database_connect_network;

// Formerly tests/database_images.rs.
#[path = "catalog_security/databases/images/database_images.rs"]
mod database_images;

// Formerly tests/database_scope.rs.
#[path = "catalog_security/namespaces/database_scope.rs"]
mod database_scope;

// Formerly tests/integration_sql_catalog.rs.
#[path = "catalog_security/catalogs/catalog.rs"]
mod integration_sql_catalog;

// Formerly tests/role_authorization.rs.
#[path = "catalog_security/security/role_authorization.rs"]
mod role_authorization;

// Formerly tests/role_database_copy_boundaries.rs.
#[path = "catalog_security/security/role_database_copy_boundaries.rs"]
mod role_database_copy_boundaries;
// Formerly tests/role_statement_boundaries.rs.
#[path = "catalog_security/security/role_statement_boundaries.rs"]
mod role_statement_boundaries;

// Formerly tests/schema_epoch_safety.rs.
#[path = "catalog_security/ddl/schema_epoch_safety.rs"]
mod schema_epoch_safety;

// Formerly tests/schema_operation_recovery.rs.
#[path = "catalog_security/ddl/schema_operation_recovery.rs"]
mod schema_operation_recovery;

#[path = "catalog_security/namespaces/udf_database_scope.rs"]
mod udf_database_scope;

// Formerly tests/schema_scope_storage.rs.
#[path = "catalog_security/namespaces/schema_scope_storage.rs"]
mod schema_scope_storage;

// Formerly tests/session_settings.rs.
#[path = "catalog_security/sessions/session_settings.rs"]
mod session_settings;
// Formerly tests/views.rs.
#[path = "catalog_security/views/semantics.rs"]
mod views;

// Formerly tests/schema_write_conflicts.rs.
#[path = "catalog_security/ddl/schema_write_conflicts.rs"]
mod schema_write_conflicts;

#[path = "catalog_security/catalogs/catalog_missing_object_sqlstates.rs"]
mod catalog_missing_object_sqlstates;

#[path = "catalog_security/namespaces/search_path_resolution.rs"]
mod search_path_resolution;

#[path = "catalog_security/catalogs/foreign_keys.rs"]
mod catalogs_foreign_keys;

#[path = "catalog_security/catalogs/partitions.rs"]
mod catalogs_partitions;

#[path = "catalog_security/catalogs/runtime_capabilities.rs"]
mod catalogs_runtime_capabilities;

#[path = "catalog_security/catalogs/session_identity.rs"]
mod catalogs_session_identity;

#[path = "catalog_security/catalogs/backend_statistics.rs"]
mod catalogs_backend_statistics;
#[path = "catalog_security/catalogs/commit_control.rs"]
mod catalogs_commit_control;
#[path = "catalog_security/catalogs/locations.rs"]
mod catalogs_locations;
#[path = "catalog_security/catalogs/server_files.rs"]
mod catalogs_server_files;
#[path = "catalog_security/catalogs/server_signaling.rs"]
mod catalogs_server_signaling;
#[path = "catalog_security/catalogs/settings.rs"]
mod catalogs_settings;
#[path = "catalog_security/catalogs/statistics_restore.rs"]
mod catalogs_statistics_restore;

#[path = "catalog_security/catalogs/blocking.rs"]
mod catalogs_blocking;

#[path = "catalog_security/catalogs/multixact.rs"]
mod catalogs_multixact;

#[path = "catalog_security/catalogs/object_addresses.rs"]
mod catalogs_object_addresses;

#[path = "catalog_security/catalogs/snapshots.rs"]
mod catalogs_snapshots;

#[path = "catalog_security/catalogs/transactions.rs"]
mod catalogs_transactions;

#[path = "catalog_security/catalogs/catalog_empty_registries.rs"]
mod catalog_empty_registries;

#[path = "catalog_security/catalogs/catalog_read_models.rs"]
mod catalog_read_models;

#[path = "support/catalog_directory.rs"]
mod support_catalog_directory;

#[path = "catalog_security/catalogs/sizes/scalar_execution.rs"]
mod catalog_scalar_execution;

#[path = "catalog_security/catalogs/runtime_profile.rs"]
mod catalog_runtime_profile;
