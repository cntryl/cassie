use crate::types::DataType;

use super::{bool_value, int_value, string, VirtualRow};

pub(super) const DEFAULT_COLLATION_OID: i64 = 100;
const C_COLLATION_OID: i64 = 950;

pub(in crate::catalog::virtual_views) fn schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        text("collname"),
        text("collprovider"),
        boolean("collisdeterministic"),
    ]
}

pub(in crate::catalog::virtual_views) fn rows() -> Vec<VirtualRow> {
    [("default", DEFAULT_COLLATION_OID, "d"), ("C", C_COLLATION_OID, "c"), ("POSIX", 951, "c")]
        .into_iter()
        .map(|(name, oid, provider)| {
            vec![
                int_value("oid", oid),
                string("collname", name),
                string("collprovider", provider),
                bool_value("collisdeterministic", true),
            ]
        })
        .collect()
}

pub(super) fn type_oid(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => {
            DEFAULT_COLLATION_OID
        }
        DataType::Name => C_COLLATION_OID,
        DataType::Array(element) => type_oid(element),
        _ => 0,
    }
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(crate::types::ObjectIdKind::Oid))
}

fn text(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Text)
}

fn boolean(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Boolean)
}
