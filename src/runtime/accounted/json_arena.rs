//! Isolated, not-yet-wired JSON arena storage experiments.
#[path = "json_arena/cursor.rs"]
mod cursor;
#[path = "json_arena/directory.rs"]
mod directory;
#[path = "json_arena/document.rs"]
mod document;
#[path = "json_arena/forest.rs"]
mod forest;
#[path = "json_arena/jsonb_array_envelope.rs"]
pub(crate) mod jsonb_array_envelope;
#[path = "json_arena/jsonb_array_output.rs"]
pub(crate) mod jsonb_array_output;
#[path = "json_arena/object.rs"]
mod object;
#[path = "json_arena/raw_decode.rs"]
pub(crate) mod raw_decode;
#[path = "json_arena/raw_number.rs"]
pub(crate) mod raw_number;
#[path = "json_arena/slots.rs"]
mod slots;
#[path = "json_arena/sources.rs"]
mod sources;
#[path = "json_arena/value.rs"]
mod value;

pub(crate) use document::binary::{binary_json_text, BinaryJsonError};
pub(crate) use document::comparison::compare as compare_jsonb_candidate;
pub(crate) use document::binary::{binary_json_text, BinaryJsonError};
pub(crate) use document::diagnostic::OwnedDiagnostic as JsonbDiagnostic;
#[cfg(test)]
pub(crate) use document::exists::cancel_existence_parse_once;
pub(crate) use document::exists::parse as parse_existence_candidate;
pub(crate) use document::jsonb::{
    canonical_text as canonical_jsonb_text, parse_field as parse_jsonb_field, JsonbFieldError,
    JsonbTextError,
};
pub(crate) use document::text_array::{parse as parse_text_array_candidate, OwnedTextArray};
#[cfg(test)]
pub(crate) use forest::comparison::{
    cancel_comparison_frame_move_once, cancel_comparison_reverse_once, cancel_comparison_work_once,
};
pub(crate) use forest::object_keys::{build_object_keys_from_map, OwnedObjectKeys};
pub(crate) use forest::InspectionRoot;
pub(crate) use value::{
    parse_value, parse_value_in_reserved_memory, parse_value_with_optional_controls, syntax_error,
    ValueParseError,
};

#[cfg(test)]
pub(crate) use forest::object_keys::{
    cancel_object_keys_copy_once, cancel_object_keys_sort_compare_once,
    cancel_object_keys_sort_move_once, cancel_object_keys_visit_once,
};
