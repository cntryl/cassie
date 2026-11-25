//! PostgreSQL JSON record conversion and `JSON_TABLE` contracts over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[path = "expansion_resource_limits.rs"]
mod expansion_resource_limits;
#[path = "table_functions/json_table_encoding_path_aliases.rs"]
mod json_table_encoding_path_aliases;
#[path = "table_functions/json_table_nested_semantics.rs"]
mod json_table_nested_semantics;

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
