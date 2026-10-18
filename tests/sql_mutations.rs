#[path = "sql_mutations/constraints/unique/reservation_lookup.rs"]
mod unique_reservation_lookup;

#[path = "sql_mutations/types/numeric/mixed_numeric_comparisons.rs"]
mod mixed_numeric_comparisons;
#[path = "support/mixed_numeric_dml.rs"]
mod support_mixed_numeric_dml;

// Consolidated integration suite: sql_mutations.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "sql_mutations/mutations/boolean_adapters.rs"]
mod sql_boolean_adapters;
#[path = "sql_mutations/mutations/boolean_checks.rs"]
mod sql_boolean_checks;
#[path = "sql_mutations/ddl/boolean_ddl_predicates.rs"]
mod sql_boolean_ddl_predicates;
#[path = "sql_mutations/mutations/boolean_contexts.rs"]
mod sql_boolean_mutations;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;

#[path = "sql_mutations/types/json/json_array_copy.rs"]
mod type_json_array_copy;

#[path = "sql_mutations/conflicts/skipped_conflict_returning.rs"]
mod skipped_conflict_returning;
#[path = "sql_mutations/mutations/constant_planning.rs"]
mod sql_constant_planning;

#[path = "sql_mutations/transactions/transaction_errors.rs"]
mod transaction_errors;

#[path = "sql_mutations/transactions/transactional_ddl.rs"]
mod postgresql18_transactional_ddl;

#[path = "sql_mutations/transactions/transaction_isolation.rs"]
mod postgresql18_transaction_isolation;

#[path = "sql_mutations/large_objects/large_objects.rs"]
mod postgresql18_large_objects;

#[path = "sql_mutations/mutations/statement_atomicity.rs"]
mod postgresql18_mutation_inventory_wave4;

#[path = "sql_mutations/operators/conflict_inference/parameters.rs"]
mod postgresql18_conflict_inference_wave_c;

#[path = "sql_mutations/merge/merge_tuple_assignments/semantics.rs"]
mod postgresql18_merge_tuple_assignments_wave_c;

// Formerly tests/copy_transaction_boundaries.rs.
#[path = "sql_mutations/copy/copy_transaction_boundaries.rs"]
mod copy_transaction_boundaries;

#[path = "sql_mutations/copy/copy_array_text_input.rs"]
mod copy_array_text_input;

#[path = "sql_mutations/copy/copy_csv_null_fields.rs"]
mod copy_csv_null_fields;

#[path = "sql_mutations/constraints/alter_add_column_constraints.rs"]
mod alter_add_column_constraints;

// Formerly tests/dml_statement_atomicity.rs.
#[path = "sql_mutations/atomicity/dml_statement_atomicity.rs"]
mod dml_statement_atomicity;

// Formerly tests/foreign_key_concurrency.rs.
#[path = "sql_mutations/constraints/foreign_keys/foreign_key_concurrency.rs"]
mod foreign_key_concurrency;

// Formerly tests/integration_sql_constraints.rs.
#[path = "sql_mutations/constraints/semantics/mod.rs"]
mod integration_sql_constraints;

// Formerly tests/integration_sql_delete.rs.
#[path = "sql_mutations/delete/semantics.rs"]
mod integration_sql_delete;

// Formerly tests/integration_sql_drop_constraint.rs.
#[path = "sql_mutations/constraints/drop_constraint.rs"]
mod integration_sql_drop_constraint;

// Formerly tests/integration_sql_foreign_keys.rs.
#[path = "sql_mutations/constraints/foreign_keys/semantics.rs"]
mod integration_sql_foreign_keys;

// Formerly tests/integration_sql_idempotent_ddl.rs.
#[path = "sql_mutations/ddl/idempotent_ddl.rs"]
mod integration_sql_idempotent_ddl;

// Formerly tests/integration_sql_insert_select.rs.
#[path = "sql_mutations/insert/insert_select.rs"]
mod integration_sql_insert_select;

// Formerly tests/integration_sql_insert_values.rs.
#[path = "sql_mutations/insert/insert_values.rs"]
mod integration_sql_insert_values;

// Formerly tests/integration_sql_transaction_storage_failures.rs.
#[path = "sql_mutations/transactions/transaction_storage_failures.rs"]
mod integration_sql_transaction_storage_failures;

// Formerly tests/integration_sql_transaction_visibility.rs.
#[path = "sql_mutations/transactions/transaction_visibility.rs"]
mod integration_sql_transaction_visibility;

// Formerly tests/integration_sql_transactions.rs.
#[path = "sql_mutations/transactions/semantics/mod.rs"]
mod integration_sql_transactions;

// Formerly tests/integration_sql_update.rs.
#[path = "sql_mutations/update/semantics.rs"]
mod integration_sql_update;

// Formerly tests/integration_sql_upsert.rs.
#[path = "sql_mutations/conflicts/upsert.rs"]
mod integration_sql_upsert;
// Formerly tests/integration_sql_upsert_concurrency.rs.
#[path = "sql_mutations/conflicts/upsert_concurrency.rs"]
mod integration_sql_upsert_concurrency;

// Formerly tests/migration_ddl_sequences.rs.
#[path = "sql_mutations/ddl/migration_ddl_sequences.rs"]
mod migration_ddl_sequences;

// Formerly tests/transaction_commit_boundary.rs.
#[path = "sql_mutations/transactions/transaction_commit_boundary.rs"]
mod transaction_commit_boundary;
// Formerly tests/transaction_semantics.rs.
#[path = "sql_mutations/transactions/transaction_semantics.rs"]
mod transaction_semantics;

// Formerly tests/transaction_staging.rs.
#[path = "sql_mutations/transactions/transaction_staging.rs"]
mod transaction_staging;

#[path = "sql_mutations/constraints/unique/unique_ddl_reservations.rs"]
mod unique_ddl_reservations;

// Formerly tests/unique_reservations.rs.
#[path = "sql_mutations/constraints/unique/unique_reservations.rs"]
mod unique_reservations;

#[path = "sql_mutations/constraints/foreign_keys/foreign_key_ddl_lifecycle.rs"]
mod foreign_key_ddl_lifecycle;

#[path = "sql_mutations/constraints/foreign_keys/foreign_key_integrity.rs"]
mod foreign_key_integrity;

#[path = "sql_mutations/constraints/unique/unique_violation_reporting.rs"]
mod unique_violation_reporting;

#[path = "sql_mutations/constraints/unique/on_conflict_resolution.rs"]
mod on_conflict_resolution;

#[path = "sql_mutations/constraints/unique/staged_unique_write_order.rs"]
mod staged_unique_write_order;

#[path = "sql_mutations/constraints/unique/composite_unique_constraints.rs"]
mod composite_unique_constraints;

#[path = "sql_mutations/constraints/unique/char_unique_trailing_blanks.rs"]
mod char_unique_trailing_blanks;

#[path = "sql_mutations/ddl/alter_column_declared_case.rs"]
mod alter_column_declared_case;
#[path = "sql_mutations/types/arrays/array_input_copy.rs"]
mod type_array_input_copy;

#[path = "sql_mutations/rules/returning.rs"]
mod rules_returning;

#[path = "sql_mutations/types/numeric/domain_restart.rs"]
mod types_numeric_domain_restart;

#[path = "sql_mutations/types/numeric/returning.rs"]
mod types_numeric_returning;
