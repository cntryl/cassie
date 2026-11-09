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

pub(crate) use document::diagnostic::OwnedDiagnostic as JsonbDiagnostic;
pub(crate) use document::jsonb::{parse_field as parse_jsonb_field, JsonbFieldError};
