//! Result types of the hashing and UUID builtins evaluated by
//! `executor::filter::hash_functions`.
use crate::types::extended::ExtendedType;
use crate::types::DataType;

const NAMES: &[&str] = &[
    "md5",
    "crc32",
    "crc32c",
    "hashtext",
    "hashtextextended",
    "gen_random_uuid",
    "uuidv4",
    "uuidv7",
    "uuid_extract_version",
    "uuid_extract_timestamp",
];

fn local(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("pg_catalog.")
        .map_or_else(|| lower.clone(), str::to_string)
}

/// Whether `name` is one of these builtins.
#[must_use]
pub(crate) fn is_hash_function(name: &str) -> bool {
    NAMES.contains(&local(name).as_str())
}

/// The PostgreSQL result type of the builtin `name`.
#[must_use]
pub(crate) fn return_type(name: &str) -> Option<DataType> {
    Some(match local(name).as_str() {
        "md5" => DataType::Text,
        "crc32" | "crc32c" | "hashtextextended" => DataType::BigInt,
        "hashtext" => DataType::Int,
        "gen_random_uuid" | "uuidv4" | "uuidv7" => DataType::Uuid,
        "uuid_extract_version" => DataType::SmallInt,
        "uuid_extract_timestamp" => {
            DataType::Extended(ExtendedType::TimestampTz { precision: None })
        }
        _ => return None,
    })
}
