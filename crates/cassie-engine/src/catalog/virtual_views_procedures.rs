//! Finite built-in procedure metadata from the pinned PostgreSQL identity table.

use crate::catalog::builtin_identity::builtin_identities;
use crate::catalog::object_identity::normal_oid;
use crate::catalog::virtual_views::VirtualRow;
use crate::catalog::Catalog;
use crate::types::{DataType, ObjectIdKind, Value};

pub(super) fn schema() -> Vec<(String, DataType)> {
    vec![
        ("oid".to_owned(), DataType::ObjectId(ObjectIdKind::Oid)),
        ("proname".to_owned(), DataType::Text),
        (
            "prorettype".to_owned(),
            DataType::ObjectId(ObjectIdKind::Oid),
        ),
        (
            "pronamespace".to_owned(),
            DataType::ObjectId(ObjectIdKind::Oid),
        ),
    ]
}

pub(super) fn rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = builtin_identities()
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
                ("pronamespace".to_owned(), Value::Int64(11)),
            ]
        })
        .collect::<Vec<_>>();
    let extensions = current_database.map_or_else(
        || catalog.list_extensions(),
        |database| catalog.list_extensions_for_database(database),
    );
    for extension in extensions {
        if !extension.name.eq_ignore_ascii_case("pgcrypto") {
            continue;
        }
        let Some(namespace) =
            crate::catalog::object_identity::stored_namespace_oid(catalog, &extension.schema)
        else {
            continue;
        };
        for object_id in [
            extension.digest_text_object_id,
            extension.digest_bytea_object_id,
        ]
        .into_iter()
        .flatten()
        {
            let Some(oid) = normal_oid(object_id) else {
                continue;
            };
            rows.push(vec![
                ("oid".to_owned(), Value::Int64(i64::from(oid))),
                ("proname".to_owned(), Value::String("digest".to_owned())),
                ("prorettype".to_owned(), Value::Int64(17)),
                (
                    "pronamespace".to_owned(),
                    Value::Int64(i64::from(namespace)),
                ),
            ]);
        }
    }
    rows
}
