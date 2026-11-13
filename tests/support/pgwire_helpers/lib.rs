//! Shared wire fixtures compiled once for the subsystem integration targets.

#[path = "../bind_planning.rs"]
pub mod support_bind_planning;

#[path = "../bytea_input_errors.rs"]
pub mod support_bytea_input_errors;

#[path = "../composite_types.rs"]
pub mod support_composite_types;

#[path = "../enum_types.rs"]
pub mod support_enum_types;

#[path = "../parameter_occurrence_wire.rs"]
pub mod support_parameter_occurrence_wire;

#[path = "../pgwire.rs"]
pub mod support_pgwire;

#[path = "../sql.rs"]
pub mod support_sql;

#[path = "../sql_fixture.rs"]
pub mod support_sql_fixture;

#[path = "../temp_dirs.rs"]
pub mod support_temp_dirs;

#[path = "../type_codec_matrix.rs"]
pub mod support_type_codec_matrix;

#[path = "../type_metadata_contract.rs"]
pub mod support_type_metadata_contract;

#[path = "../unary_numeric.rs"]
pub mod support_unary_numeric;
