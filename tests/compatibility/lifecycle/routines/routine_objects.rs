use super::*;

#[path = "argument_mode_results/semantics.rs"]
mod argument_mode_results_wave7;
#[path = "catalog_identity_lookups.rs"]
mod catalog_identity_lookups;
#[path = "../comments/comment_catalog_coverage.rs"]
mod comment_catalog_coverage;
#[path = "../comments/comment_owner_lifecycle.rs"]
mod comment_owner_lifecycle;
#[path = "../comments/comment_sequence_identity.rs"]
mod comment_sequence_identity;
#[path = "cursor_variables.rs"]
mod cursor_variables;
#[path = "../security/default_privileges.rs"]
mod default_privileges;
#[path = "../security/definer_search_path_resolution.rs"]
mod definer_search_path_resolution;
#[path = "do_block_cancellation.rs"]
mod do_block_cancellation;
#[path = "do_blocks_hub.rs"]
mod do_blocks;
#[path = "do_dynamic_into_strict.rs"]
mod do_dynamic_into_strict;
#[path = "do_format_using_result.rs"]
mod do_format_using_result;
#[path = "do_null_using.rs"]
mod do_null_using;
#[path = "../triggers/event_triggers.rs"]
mod event_triggers;
#[path = "../statistics/expression_statistics.rs"]
mod expression_statistics;
#[path = "foreach_slice.rs"]
mod foreach_slice;
#[path = "get_stacked_diagnostics.rs"]
mod get_stacked_diagnostics;
#[path = "signature_results.rs"]
mod issue_830_signature_results;
#[path = "language_sql.rs"]
mod language_sql;
#[path = "../security/ownership_privileges.rs"]
mod ownership_privileges;
#[path = "variadic_calls/semantics.rs"]
mod postgresql18_variadic_calls_wave7;
#[path = "procedural_qualification.rs"]
mod procedural_qualification_wave;
#[path = "return_query_execute.rs"]
mod return_query_execute;
#[path = "../security/role_membership_admin_option/semantics.rs"]
mod role_membership_admin_option_wave7;
#[path = "scoped_setting_resets.rs"]
mod scoped_setting_resets;
#[path = "../security/security_invoker_alter.rs"]
mod security_invoker_alter;
#[path = "../casts/sql_defined_builtin_casts.rs"]
mod sql_defined_builtin_casts;
#[path = "transaction_control_procedures.rs"]
mod transaction_control_procedures;
#[path = "transition_table_update.rs"]
mod transition_table_update;
#[path = "../triggers/trigger_ddl.rs"]
mod trigger_ddl;
#[path = "../triggers/trigger_definer_context.rs"]
mod trigger_definer_context;
#[path = "../triggers/trigger_introspection.rs"]
mod trigger_introspection;
#[path = "../triggers/trigger_replace_syntax.rs"]
mod trigger_replace_syntax;
#[path = "../triggers/trigger_replication_modes.rs"]
mod trigger_replication_modes;
#[path = "../triggers/trigger_tsvector_update.rs"]
mod trigger_tsvector_update;
#[path = "../triggers/trigger_tsvector_update_column.rs"]
mod trigger_tsvector_update_column;
#[path = "../triggers/trigger_update_of.rs"]
mod trigger_update_of;
#[path = "user_defined_variadic.rs"]
mod user_defined_variadic;

include!("../comments/routine_objects/metadata.rs");

include!("../comments/routine_objects/restart.rs");

include!("../comments/routine_objects/semantics.rs");

include!("routine_objects/parameters.rs");

include!("routine_objects/semantics.rs");

include!("routine_objects/transactions.rs");

include!("../security/routine_objects/semantics.rs");

include!("../statistics/routine_objects/restart.rs");

include!("../statistics/routine_objects/semantics.rs");

include!("../triggers/routine_objects/semantics.rs");

include!("../triggers/routine_objects/transactions.rs");
