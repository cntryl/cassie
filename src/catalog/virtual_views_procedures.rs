//! Finite built-in procedure metadata from the pinned PostgreSQL identity table.

use crate::catalog::builtin_identity::builtin_identities;
use crate::catalog::virtual_views::VirtualRow;
use crate::types::{DataType, ObjectIdKind, Value};

pub(super) fn schema() -> Vec<(String, DataType)> {
    vec![
        ("oid".to_owned(), DataType::ObjectId(ObjectIdKind::Oid)),
        ("proname".to_owned(), DataType::Text),
        (
            "prorettype".to_owned(),
            DataType::ObjectId(ObjectIdKind::Oid),
        ),
    ]
}

pub(super) fn rows() -> Vec<VirtualRow> {
    builtin_identities()
        .procs
        .iter()
        .map(|procedure| {
            vec![
                ("oid".to_owned(), Value::Int64(i64::from(procedure.oid))),
                ("proname".to_owned(), Value::String(procedure.name.clone())),
                (
                    "prorettype".to_owned(),
                    Value::Int64(i64::from(procedure.return_type)),
                ),
            ]
        })
        .collect()
}
