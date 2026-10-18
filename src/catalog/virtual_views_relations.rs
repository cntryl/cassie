//! Database-scoped discovery of the table, view and sequence objects Cassie stores.
use super::VirtualRow;
use crate::catalog::{
    local_name, relation_belongs_to_database, relation_schema_name, Catalog, SequenceMeta,
};
use crate::types::{DataType, Value};

pub(super) fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    Some(match name {
        "pg_catalog.pg_tables" => vec![
            text("schemaname"),
            text("tablename"),
            text("tableowner"),
            text("tablespace"),
            boolean("hasindexes"),
            boolean("hasrules"),
            boolean("hastriggers"),
            boolean("rowsecurity"),
        ],
        "pg_catalog.pg_views" => vec![
            text("schemaname"),
            text("viewname"),
            text("viewowner"),
            text("definition"),
        ],
        "pg_catalog.pg_sequences" => vec![
            text("schemaname"),
            text("sequencename"),
            text("sequenceowner"),
            text("data_type"),
            bigint("start_value"),
            bigint("min_value"),
            bigint("max_value"),
            bigint("increment_by"),
            boolean("cycle"),
            bigint("cache_size"),
            bigint("last_value"),
        ],
        "pg_catalog.pg_sequence" => vec![
            oid("seqrelid", crate::types::ObjectIdKind::Oid),
            oid("seqtypid", crate::types::ObjectIdKind::Oid),
            bigint("seqstart"),
            bigint("seqincrement"),
            bigint("seqmax"),
            bigint("seqmin"),
            bigint("seqcache"),
            boolean("seqcycle"),
        ],
        _ => return None,
    })
}

pub(super) fn rows(
    catalog: &Catalog,
    name: &str,
    database: Option<&str>,
) -> Option<Vec<VirtualRow>> {
    let mut rows = match name {
        "pg_catalog.pg_tables" => {
            catalog
                .list_collections_canonical()
                .into_iter()
                .filter(|table| visible(&table.name, database))
                .map(|table| {
                    vec![
                        string("schemaname", relation_schema_name(&table.name)),
                        string("tablename", local_name(&table.name)),
                        // Ownership is not recorded in the current metadata format.
                        null("tableowner"),
                        null("tablespace"),
                        flag("hasindexes", !catalog.list_indexes(&table.name).is_empty()),
                        flag("hasrules", false),
                        flag("hastriggers", false),
                        flag("rowsecurity", false),
                    ]
                })
                .collect::<Vec<_>>()
        }
        "pg_catalog.pg_views" => catalog
            .list_views()
            .into_iter()
            .filter(|view| visible(&view.name, database))
            .map(|view| {
                vec![
                    string("schemaname", relation_schema_name(&view.name)),
                    string("viewname", local_name(&view.name)),
                    null("viewowner"),
                    string("definition", view.query),
                ]
            })
            .collect(),
        "pg_catalog.pg_sequences" => catalog
            .list_sequences()
            .into_iter()
            .filter(|sequence| visible(&sequence.name, database))
            .map(|sequence| sequence_row(&sequence))
            .collect(),
        "pg_catalog.pg_sequence" => catalog
            .list_sequences()
            .into_iter()
            .filter(|sequence| visible(&sequence.name, database))
            .map(|sequence| {
                vec![
                    number(
                        "seqrelid",
                        crate::catalog::object_identity::stored_relation_oid(
                            catalog,
                            &sequence.name,
                        )
                        .map_or(0, i64::from),
                    ),
                    number("seqtypid", sequence.data_type.type_oid()),
                    number("seqstart", sequence.start_value),
                    number("seqincrement", sequence.increment_by),
                    number("seqmax", sequence.max_value),
                    number("seqmin", sequence.min_value),
                    number("seqcache", sequence.cache),
                    flag("seqcycle", sequence.cycle),
                ]
            })
            .collect(),
        _ => return None,
    };
    rows.sort_by(|left, right| {
        left[0]
            .1
            .as_str()
            .cmp(&right[0].1.as_str())
            .then_with(|| left[1].1.as_str().cmp(&right[1].1.as_str()))
    });
    Some(rows)
}

fn sequence_row(sequence: &SequenceMeta) -> VirtualRow {
    vec![
        string("schemaname", relation_schema_name(&sequence.name)),
        string("sequencename", local_name(&sequence.name)),
        null("sequenceowner"),
        string(
            "data_type",
            match sequence.data_type {
                DataType::SmallInt => "smallint",
                DataType::Int => "integer",
                DataType::BigInt => "bigint",
                _ => "unknown",
            },
        ),
        number("start_value", sequence.start_value),
        number("min_value", sequence.min_value),
        number("max_value", sequence.max_value),
        number("increment_by", sequence.increment_by),
        flag("cycle", sequence.cycle),
        number("cache_size", sequence.cache),
        (
            "last_value".to_owned(),
            if sequence.is_called {
                Value::Int64(sequence.current_value)
            } else {
                Value::Null
            },
        ),
    ]
}

fn visible(name: &str, database: Option<&str>) -> bool {
    database.is_none_or(|database| relation_belongs_to_database(name, database))
}
fn oid(name: &str, kind: crate::types::ObjectIdKind) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(kind))
}

fn text(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Text)
}
fn bigint(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::BigInt)
}
fn boolean(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Boolean)
}
fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_owned(), Value::String(value.into()))
}
fn null(name: &str) -> (String, Value) {
    (name.to_owned(), Value::Null)
}
fn flag(name: &str, value: bool) -> (String, Value) {
    (name.to_owned(), Value::Bool(value))
}
fn number(name: &str, value: i64) -> (String, Value) {
    (name.to_owned(), Value::Int64(value))
}
