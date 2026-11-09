// Registrations retain the existing pgwire_extended module namespace.

#[path = "types/arrays/constructor_contracts.rs"]
mod array_constructor_contracts;

#[path = "types/arrays/array_position_start_overloads.rs"]
mod array_position_start_overloads;

#[path = "types/arrays/multidimensional_slices.rs"]
mod multidimensional_array_slices;

#[path = "types/arrays/array_boundaries.rs"]
mod postgresql18_array_boundaries;

#[path = "types/bit_bytea/binary_bytea.rs"]
mod postgresql18_binary_bytea_compat;

#[path = "functions/math/math.rs"]
mod postgresql18_math_compat;

#[path = "types/network/network.rs"]
mod postgresql18_network_compat;

#[path = "functions/string_functions.rs"]
mod postgresql18_string_compat;

#[path = "functions/string/string_scalar_edges.rs"]
mod postgresql18_string_scalar_edges;

#[path = "functions/temporal/temporal_function.rs"]
mod postgresql18_temporal_function_compat;

#[path = "types/uuid_lsn/uuid_hash_lsn.rs"]
mod postgresql18_uuid_hash_lsn_compat;

#[path = "types/arrays/unnest_sources.rs"]
mod unnest_sources;

#[path = "functions/math/overload_errors.rs"]
mod functions_math_overload_errors;

#[path = "functions/math/width_bucket_unknown_literal.rs"]
mod width_bucket_unknown_literal;

#[path = "functions/string/error_sqlstates.rs"]
mod functions_string_error_sqlstates;

#[path = "type_resolution/prepared_descriptors.rs"]
mod type_resolution_prepared_descriptors;

#[path = "types/json/jsonpath_timezone.rs"]
mod types_json_jsonpath_timezone;

#[path = "types/uuid_lsn/restart.rs"]
mod types_uuid_lsn_restart;

#[path = "types/network/operator_errors.rs"]
mod types_network_operator_errors;
