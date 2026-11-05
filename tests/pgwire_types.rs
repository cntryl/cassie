// pgwire_types: subsystem-owned wire contracts; shared fixtures stay in support/.

#[path = "pgwire_extended/types/numeric/mixed_numeric_comparisons.rs"]
mod mixed_numeric_comparisons;

#[path = "pgwire_extended/types/native_name.rs"]
mod pgwire_native_name;

#[path = "pgwire_extended/types/json/json_array_nulls.rs"]
mod pgwire_type_json_array_nulls;

#[path = "pgwire_extended/types/json/constructors/errors.rs"]
mod pgwire_type_json_scalar_documents;

#[path = "pgwire_extended/type_resolution/unknown_metadata.rs"]
mod pgwire_type_unknown_metadata;

#[path = "pgwire_extended/types/arrays/vector_array_metadata.rs"]
mod pgwire_type_vector_array_metadata;

#[path = "pgwire_extended/types/scalars/scalar_array_matrix.rs"]
mod pgwire_type_scalar_array_matrix;

#[path = "pgwire_extended/types/scalars/scalar_matrix.rs"]
mod pgwire_type_scalar_matrix;

#[path = "pgwire_extended/types/arrays/character_conditionals.rs"]
mod character_conditionals;

#[path = "pgwire_extended/types/arrays/character_logical_values.rs"]
mod character_logical_values;

#[path = "pgwire_extended/types/arrays/character_row_images.rs"]
mod character_row_images;

#[path = "pgwire_extended/types/arrays/character_using.rs"]
mod character_using;

#[path = "pgwire_extended/types/arrays/character_casts.rs"]
mod pgwire_character_array_casts;

#[path = "pgwire_extended/types/arrays/aggregate_identity/semantics.rs"]
mod pgwire_type_array_text_inputs;

#[path = "pgwire_extended/type_resolution/codec_boundaries.rs"]
mod pgwire_type_codec_boundaries;

#[path = "pgwire_extended/type_resolution/declared_null_matrix.rs"]
mod pgwire_type_declared_null_matrix;

#[path = "pgwire_extended/types/character/bpchar_casts.rs"]
mod pgwire_type_bpchar_casts;

#[path = "pgwire_extended/types/real/float_precision.rs"]
mod pgwire_type_float_precision;

#[path = "pgwire_extended/type_resolution/output_source_seams.rs"]
mod pgwire_type_output_source_seams;

#[path = "pgwire_extended/types/real/real_wire.rs"]
mod pgwire_type_real_wire;

#[path = "pgwire_extended/type_resolution/output_identifier_identity.rs"]
mod pgwire_type_output_identifier_identity;

#[path = "pgwire_extended/types/bit_bytea/casts_int2.rs"]
mod bytea_int2;

#[path = "pgwire_extended/types/bit_bytea/casts_int4.rs"]
mod bytea_int4;

#[path = "pgwire_extended/types/bit_bytea/casts_int8.rs"]
mod bytea_int8;

#[path = "pgwire_extended/types/bit_bytea/decode_int2.rs"]
mod bytea_decode_int2;

#[path = "pgwire_extended/types/bit_bytea/decode_int4.rs"]
mod bytea_decode_int4;

#[path = "pgwire_extended/types/bit_bytea/decode_int8.rs"]
mod bytea_decode_int8;

#[path = "pgwire_extended/types/bit_bytea/cast_errors.rs"]
mod bytea_cast_errors;

#[path = "pgwire_extended/types/bit_bytea/cast_parameters.rs"]
mod bytea_cast_parameters;

#[path = "pgwire_extended/types/bit_bytea/cast_mixed.rs"]
mod bytea_cast_mixed;

#[path = "pgwire_extended/types/bit_bytea/cast_parameter_errors.rs"]
mod bytea_cast_parameter_errors;

#[path = "pgwire_extended/types/bit_bytea/cast_scopes.rs"]
mod bytea_cast_scopes;

#[path = "pgwire_extended/types/bit_bytea/cast_lazy_parameters.rs"]
mod bytea_cast_lazy_parameters;

#[path = "pgwire_extended/types/bit_bytea/input_literal.rs"]
mod bytea_input_literal;

#[path = "pgwire_extended/types/bit_bytea/input_text.rs"]
mod bytea_input_text;

#[path = "pgwire_extended/types/bit_bytea/input_cte.rs"]
mod bytea_input_cte;

#[path = "pgwire_extended/types/bit_bytea/input_errors_scalar.rs"]
mod bytea_input_errors_scalar;

#[path = "pgwire_extended/types/bit_bytea/input_errors_dead.rs"]
mod bytea_input_errors_dead;

#[path = "pgwire_extended/types/bit_bytea/input_errors_cte.rs"]
mod bytea_input_errors_cte;

#[path = "pgwire_extended/types/bit_bytea/input_errors_text.rs"]
mod bytea_input_errors_text;

#[path = "pgwire_extended/types/bit_bytea/input_parameter_errors.rs"]
mod bytea_input_parameter_errors;

#[path = "pgwire_extended/types/bit_bytea/input_parameters.rs"]
mod bytea_input_parameters;

#[path = "pgwire_extended/types/bit_bytea/input_precedence.rs"]
mod bytea_input_precedence;

#[path = "pgwire_extended/types/bit_bytea/input_allbytes.rs"]
mod bytea_input_allbytes;

#[path = "pgwire_extended/types/review_diagnostics.rs"]
mod review_diagnostics;

#[path = "pgwire_extended/types/json/semantic_keys.rs"]
mod jsonb_semantic_keys;

#[path = "pgwire_extended/types/json/numeric_weight_boundaries.rs"]
mod jsonb_numeric_boundaries;

#[path = "pgwire_extended/types/json/search_cycle_lineage.rs"]
mod jsonb_search_cycle_lineage;

use cassie_test_support::support_bind_planning;

#[path = "support/bytea_cast_planning.rs"]
mod support_bytea_cast_planning;

use cassie_test_support::support_bytea_input_errors;

#[path = "support/bytea_parameter_recovery.rs"]
mod support_bytea_parameter_recovery;

#[path = "support/mixed_numeric.rs"]
mod support_mixed_numeric;

use cassie_test_support::support_parameter_occurrence_wire;

use cassie_test_support::support_pgwire;

use cassie_test_support::support_sql;

#[path = "support/temporal_cancellation.rs"]
mod support_temporal_cancellation;

use cassie_test_support::support_type_codec_matrix;

#[path = "support/type_declared_null_matrix.rs"]
mod support_type_declared_null_matrix;

use cassie_test_support::support_type_metadata_contract;

#[path = "support/values_casts.rs"]
mod support_values_casts;

#[path = "pgwire_extended/types_suite.rs"]
mod pgwire_extended;
