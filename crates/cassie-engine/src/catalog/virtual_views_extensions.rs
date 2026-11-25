//! Finite extension catalog rows for the selected built-in profiles.
use crate::catalog::{object_identity::normal_oid, Catalog};
use crate::types::{DataType, ObjectIdKind, Value};

use super::VirtualRow;

pub(super) fn installed_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        ("extname".to_owned(), DataType::Name),
        oid("extowner"),
        oid("extnamespace"),
        ("extrelocatable".to_owned(), DataType::Boolean),
        ("extversion".to_owned(), DataType::Text),
    ]
}

pub(super) fn available_schema() -> Vec<(String, DataType)> {
    vec![
        ("name".to_owned(), DataType::Name),
        ("default_version".to_owned(), DataType::Text),
        ("installed_version".to_owned(), DataType::Text),
    ]
}

pub(super) fn installed_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let extensions = current_database.map_or_else(
        || catalog.list_extensions(),
        |database| catalog.list_extensions_for_database(database),
    );
    extensions
        .into_iter()
        .filter_map(|extension| {
            let oid = normal_oid(extension.object_id)?;
            let namespace_oid =
                crate::catalog::object_identity::stored_namespace_oid(catalog, &extension.schema)?;
            Some(vec![
                ("oid".to_owned(), Value::Int64(i64::from(oid))),
                ("extname".to_owned(), Value::String(extension.name)),
                (
                    "extowner".to_owned(),
                    Value::Int64(i64::from(extension.owner_oid)),
                ),
                (
                    "extnamespace".to_owned(),
                    Value::Int64(i64::from(namespace_oid)),
                ),
                ("extrelocatable".to_owned(), Value::Bool(true)),
                ("extversion".to_owned(), Value::String(extension.version)),
            ])
        })
        .collect()
}

pub(super) fn available_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    [("pgcrypto", "1.4"), ("vector", "0.8.7")]
        .into_iter()
        .map(|(name, version)| {
            let installed_version = current_database
                .and_then(|database| catalog.get_extension(database, name))
                .map_or(Value::Null, |extension| Value::String(extension.version));
            vec![
                ("name".to_owned(), Value::String(name.to_owned())),
                (
                    "default_version".to_owned(),
                    Value::String(version.to_owned()),
                ),
                ("installed_version".to_owned(), installed_version),
            ]
        })
        .collect()
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}
