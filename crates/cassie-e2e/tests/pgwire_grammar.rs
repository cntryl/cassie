// pgwire_grammar: subsystem-owned wire contracts; shared fixtures stay in support/.

#[path = "pgwire_extended/grammar/rows/arity_aliases.rs"]
mod row_arity_aliases;

#[path = "pgwire_extended/grammar/values_casts_date.rs"]
mod values_casts_date;

#[path = "pgwire_extended/grammar/values_casts_time.rs"]
mod values_casts_time;

#[path = "pgwire_extended/grammar/values_casts_timestamp.rs"]
mod values_casts_timestamp;

#[path = "pgwire_extended/grammar/values_casts_uuid.rs"]
mod values_casts_uuid;

#[path = "pgwire_extended/grammar/values_casts_bytea.rs"]
mod values_casts_bytea;

#[path = "pgwire_extended/grammar/values_casts_char.rs"]
mod values_casts_char;

#[path = "pgwire_extended/grammar/values_casts_varchar.rs"]
mod values_casts_varchar;

#[path = "pgwire_extended/grammar/values_casts_parameters.rs"]
mod values_casts_parameters;

#[path = "pgwire_extended/grammar/values_casts_character.rs"]
mod values_casts_character;

#[path = "pgwire_extended/grammar/values_casts_semantics.rs"]
mod values_casts_semantics;

#[path = "pgwire_extended/grammar/values_casts_cte.rs"]
mod values_casts_cte;

#[path = "pgwire_extended/grammar/varchar_scalar.rs"]
mod varchar_scalar;

#[path = "pgwire_extended/grammar/varchar_values.rs"]
mod varchar_values;

#[path = "pgwire_extended/grammar/varchar_derived.rs"]
mod varchar_derived;

#[path = "pgwire_extended/grammar/varchar_cte.rs"]
mod varchar_cte;

#[path = "pgwire_extended/grammar/varchar_set.rs"]
mod varchar_set;

#[path = "pgwire_extended/grammar/varchar_parameters.rs"]
mod varchar_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_01.rs"]
mod scalar_batch_01;

#[path = "pgwire_extended/grammar/scalar_batch_01_parameters.rs"]
mod scalar_batch_01_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_02.rs"]
mod scalar_batch_02;

#[path = "pgwire_extended/grammar/scalar_batch_02_errors.rs"]
mod scalar_batch_02_errors;

#[path = "pgwire_extended/grammar/scalar_batch_02_special.rs"]
mod scalar_batch_02_special;

#[path = "pgwire_extended/grammar/scalar_batch_02_pending.rs"]
mod scalar_batch_02_pending;

#[path = "pgwire_extended/grammar/scalar_batch_03.rs"]
mod scalar_batch_03;

#[path = "pgwire_extended/grammar/scalar_batch_03_parameters.rs"]
mod scalar_batch_03_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_03_gap.rs"]
mod scalar_batch_03_gap;

#[path = "pgwire_extended/grammar/scalar_batch_03_unicode.rs"]
mod scalar_batch_03_unicode;

#[path = "pgwire_extended/grammar/scalar_batch_04.rs"]
mod scalar_batch_04;

#[path = "pgwire_extended/grammar/scalar_batch_04_parameters.rs"]
mod scalar_batch_04_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_04_errors.rs"]
mod scalar_batch_04_errors;

#[path = "pgwire_extended/grammar/scalar_batch_04_parameter_errors.rs"]
mod scalar_batch_04_parameter_errors;

#[path = "pgwire_extended/grammar/scalar_batch_04_interactions.rs"]
mod scalar_batch_04_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_04_prefix.rs"]
mod scalar_batch_04_prefix;

#[path = "pgwire_extended/grammar/scalar_batch_05.rs"]
mod scalar_batch_05;

#[path = "pgwire_extended/grammar/scalar_batch_05_parameters.rs"]
mod scalar_batch_05_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_05_interactions.rs"]
mod scalar_batch_05_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_05_errors.rs"]
mod scalar_batch_05_errors;

#[path = "pgwire_extended/grammar/scalar_batch_05_null_parameters.rs"]
mod scalar_batch_05_null_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_06.rs"]
mod scalar_batch_06;

#[path = "pgwire_extended/grammar/scalar_batch_06_parameters.rs"]
mod scalar_batch_06_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_06_interactions.rs"]
mod scalar_batch_06_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_06_index.rs"]
mod scalar_batch_06_index;

#[path = "pgwire_extended/grammar/scalar_batch_07.rs"]
mod scalar_batch_07;

#[path = "pgwire_extended/grammar/scalar_batch_07_interactions.rs"]
mod scalar_batch_07_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_07_errors.rs"]
mod scalar_batch_07_errors;

#[path = "pgwire_extended/grammar/scalar_batch_07_parameters.rs"]
mod scalar_batch_07_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_07_routines.rs"]
mod scalar_batch_07_routines;

#[path = "pgwire_extended/grammar/scalar_batch_08.rs"]
mod scalar_batch_08;

#[path = "pgwire_extended/grammar/scalar_batch_08_interactions.rs"]
mod scalar_batch_08_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_08_errors.rs"]
mod scalar_batch_08_errors;

#[path = "pgwire_extended/grammar/scalar_batch_08_parameters.rs"]
mod scalar_batch_08_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_09.rs"]
mod scalar_batch_09;

#[path = "pgwire_extended/grammar/scalar_batch_09_catalog.rs"]
mod scalar_batch_09_catalog;

#[path = "pgwire_extended/grammar/scalar_batch_09_scopes.rs"]
mod scalar_batch_09_scopes;

#[path = "pgwire_extended/grammar/scalar_batch_09_parameters.rs"]
mod scalar_batch_09_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_10.rs"]
mod scalar_batch_10;

#[path = "pgwire_extended/grammar/scalar_batch_10_errors.rs"]
mod scalar_batch_10_errors;

#[path = "pgwire_extended/grammar/scalar_batch_10_arithmetic_scopes.rs"]
mod scalar_batch_10_arithmetic_scopes;

#[path = "pgwire_extended/grammar/qualified_operators_0.rs"]
mod qualified_operators_0;

#[path = "pgwire_extended/grammar/qualified_operators_1.rs"]
mod qualified_operators_1;

#[path = "pgwire_extended/grammar/qualified_operators_mixed.rs"]
mod qualified_operators_mixed;

#[path = "pgwire_extended/grammar/qualified_operators_bytea.rs"]
mod qualified_operators_bytea;

#[path = "pgwire_extended/grammar/qualified_operators_parameters.rs"]
mod qualified_operators_parameters;

#[path = "pgwire_extended/grammar/qualified_operators_errors.rs"]
mod qualified_operators_errors;

#[path = "pgwire_extended/grammar/bytea_functions_values_0.rs"]
mod bytea_functions_values_0;

#[path = "pgwire_extended/grammar/bytea_functions_values_1.rs"]
mod bytea_functions_values_1;

#[path = "pgwire_extended/grammar/bytea_functions_values_2.rs"]
mod bytea_functions_values_2;

#[path = "pgwire_extended/grammar/bytea_functions_values_3.rs"]
mod bytea_functions_values_3;

#[path = "pgwire_extended/grammar/bytea_functions_values_4.rs"]
mod bytea_functions_values_4;

#[path = "pgwire_extended/grammar/bytea_functions_errors_0.rs"]
mod bytea_functions_errors_0;

#[path = "pgwire_extended/grammar/bytea_functions_errors_1.rs"]
mod bytea_functions_errors_1;

#[path = "pgwire_extended/grammar/bytea_functions_errors_2.rs"]
mod bytea_functions_errors_2;

#[path = "pgwire_extended/grammar/bytea_functions_parameters.rs"]
mod bytea_functions_parameters;

#[path = "pgwire_extended/grammar/bytea_functions_empty.rs"]
mod bytea_functions_empty;

#[path = "pgwire_extended/grammar/bytea_functions_gaps.rs"]
mod bytea_functions_gaps;

#[path = "pgwire_extended/grammar/bytea_functions_overloads.rs"]
mod bytea_functions_overloads;

use cassie_test_support::support_bind_planning;

use cassie_test_support::support_parameter_occurrence_wire;

use cassie_test_support::support_pgwire;

use cassie_test_support::support_sql;

use cassie_test_support::support_type_metadata_contract;

#[path = "support/unary_math_parameter_recovery.rs"]
mod support_unary_math_parameter_recovery;

use cassie_test_support::support_unary_numeric;

#[path = "support/values_casts.rs"]
mod support_values_casts;

#[path = "pgwire_extended/grammar_suite.rs"]
mod pgwire_extended;
