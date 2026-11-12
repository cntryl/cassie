//! Selected `pg_description` rows backed by persisted object comment metadata.

use crate::catalog::Catalog;
use crate::catalog::virtual_views::VirtualRow;
use crate::types::{DataType, ObjectIdKind, Value};

pub(in crate::catalog::virtual_views) fn schema() -> Vec<(String, DataType)> {
    vec![
        oid("objoid"),
        oid("classoid"),
        int("objsubid"),
        ("description".to_owned(), DataType::Text),
    ]
}

pub(in crate::catalog::virtual_views) fn rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let comments = current_database.map_or_else(
        || catalog.object_comments(),
        |database| catalog.object_comments_for_database(database),
    );
    comments
        .into_iter()
        .map(|metadata| {
            vec![
                (
                    "objoid".to_owned(),
                    Value::Int64(i64::from(metadata.address.object_oid)),
                ),
                (
                    "classoid".to_owned(),
                    Value::Int64(i64::from(metadata.address.class_oid)),
                ),
                (
                    "objsubid".to_owned(),
                    Value::Int64(i64::from(metadata.address.object_sub_id)),
                ),
                (
                    "description".to_owned(),
                    Value::String(metadata.description),
                ),
            ]
        })
        .collect()
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}

fn int(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Int)
}
