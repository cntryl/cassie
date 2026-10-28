//! JSON object expansion and SQL/JSON predicates/variables over pgwire.

#[path = "../function_alias/errors.rs"]
mod function_alias_residuals;
#[path = "../json_legacy_builders_and_paths.rs"]
mod json_legacy_builders_and_paths;

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

include!("parameters.rs");

include!("semantics.rs");
