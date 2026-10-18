// Consolidated integration suite: parser_types.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "parser_types/grammar/dialect_syntax.rs"]
mod dialect_syntax;

#[path = "parser_types/mod.rs"]
mod parser_types;

#[path = "parser_types/grammar/insert_source_boundary.rs"]
mod insert_source_boundary;

#[path = "parser_types/grammar/aliases.rs"]
mod parser_aliases;

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/parser_core.rs.
#[path = "parser_types/grammar/core.rs"]
mod parser_core;

// Non-ASCII SQL text must keep byte offsets on UTF-8 character boundaries.
#[path = "parser_types/grammar/identifiers/sql_text_utf8_offsets.rs"]
mod sql_text_utf8_offsets;

// Formerly tests/parser_cte_schema.rs.
#[path = "parser_types/grammar/ctes/cte_schema.rs"]
mod parser_cte_schema;

// Formerly tests/parser_database_scope.rs.
#[path = "parser_types/grammar/identifiers/database_scope.rs"]
mod parser_database_scope;

// Formerly tests/parser_dml_transactions.rs.
#[path = "parser_types/grammar/mutations/dml_transactions.rs"]
mod parser_dml_transactions;

// Formerly tests/parser_expressions.rs.
#[path = "parser_types/grammar/expressions/semantics.rs"]
mod parser_expressions;

// Delimited (double-quoted) identifiers in expression, DML-target and alias positions.
#[path = "parser_types/grammar/identifiers/sql_quoted_identifiers/mod.rs"]
mod sql_quoted_identifiers;

// Formerly tests/parser_functions_roles.rs.
#[path = "parser_types/grammar/routines/functions_roles.rs"]
mod parser_functions_roles;

// Formerly tests/parser_indexes.rs.
#[path = "parser_types/grammar/indexes/semantics.rs"]
mod parser_indexes;

// Formerly tests/parser_sources_sets.rs.
#[path = "parser_types/grammar/relations/sources_sets.rs"]
mod parser_sources_sets;

// Formerly tests/parser_transport_boundaries.rs.
#[path = "parser_types/grammar/boundaries/transport_boundaries.rs"]
mod parser_transport_boundaries;

// Formerly tests/types.rs.
#[path = "parser_types/types/semantics.rs"]
mod types;

// Formerly tests/sql_string_literals.rs.
#[path = "parser_types/grammar/literals/sql_string_literals.rs"]
mod sql_string_literals;

#[path = "parser_types/grammar/ddl/ddl_check_literals.rs"]
mod ddl_check_literals;

#[path = "parser_types/grammar/ddl/ddl_default_values.rs"]
mod ddl_default_values;

// Bound string parameters compared against typed columns.
#[path = "parser_types/types/typed_parameter_canonicalization.rs"]
mod typed_parameter_canonicalization;

#[path = "parser_types/types/sql_float_integer_casts.rs"]
mod sql_float_integer_casts;

#[path = "parser_types/types/typed_value_consistency.rs"]
mod typed_value_consistency;

#[path = "parser_types/grammar/identifiers/sql_separator_consistency.rs"]
mod sql_separator_consistency;
