//! Text-search DDL has no `QueryStatement` AST variants, so syntax-only checks
//! here were not evidence of supported behavior. Configuration and dictionary
//! execution is covered by `sql_queries/postgresql18_text_search.rs`:
//! `should_manage_a_builtin_text_search_configuration`,
//! `should_apply_text_search_configuration_mapping_lifecycle`,
//! `should_move_a_renamed_text_search_configuration`, and
//! `should_manage_builtin_text_search_dictionaries`. PostgreSQL callback lookup
//! errors for parser/template DDL are checked over pgwire in
//! `pgwire_extended/postgresql18_text_compat.rs`.
