use crate::catalog::{
    local_name, name_matches, relation_belongs_to_database, relation_schema_name,
    schema_belongs_to_database, Catalog, FieldConstraint, IndexMeta,
};
use crate::types::{DataType, Value};

use super::VirtualRow;

#[path = "virtual_views_pg/index_definitions.rs"]
mod index_definitions;

pub(super) fn namespace_schema() -> Vec<(String, DataType)> {
    vec![int("oid"), text("nspname"), int("nspowner"), text("nspacl")]
}

pub(super) fn class_schema() -> Vec<(String, DataType)> {
    vec![
        int("oid"),
        text("relname"),
        text("relkind"),
        text("relnamespace"),
        int("relnamespace_oid"),
        int("relowner"),
        bool("relhasindex"),
        text("relpersistence"),
        int("reltuples"),
        text("relacl"),
        text("reloptions"),
    ]
}

pub(super) fn attribute_schema() -> Vec<(String, DataType)> {
    vec![
        text("attrelid"),
        int("attrelid_oid"),
        text("attname"),
        int("attnum"),
        int("atttypid"),
        bool("attnotnull"),
        int("atttypmod"),
        bool("atthasdef"),
        bool("attisdropped"),
        text("attidentity"),
        text("attgenerated"),
        int("attcollation"),
    ]
}

pub(super) fn indexes_schema() -> Vec<(String, DataType)> {
    vec![
        text("schemaname"),
        text("tablename"),
        text("indexname"),
        text("indexdef"),
    ]
}

pub(super) fn index_schema() -> Vec<(String, DataType)> {
    vec![
        text("indexrelid"),
        text("indrelid"),
        int("indexrelid_oid"),
        int("indrelid_oid"),
        bool("indisunique"),
        bool("indisprimary"),
        bool("indnullsnotdistinct"),
        text("indkey"),
        int("indnatts"),
        int("indnkeyatts"),
        text("indoption"),
        bool("indisvalid"),
        bool("indisready"),
        text("indpred"),
        text("indexprs"),
    ]
}

pub(super) fn attrdef_schema() -> Vec<(String, DataType)> {
    vec![text("adrelid"), int("adnum"), text("adsrc")]
}

pub(super) fn type_schema() -> Vec<(String, DataType)> {
    vec![
        int("oid"),
        text("typname"),
        text("typnamespace"),
        int("typlen"),
        bool("typbyval"),
        text("typtype"),
        text("typcategory"),
        int("typelem"),
    ]
}

pub(super) fn pg_namespace(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = vec![
        namespace_row(catalog, "information_schema", "information_schema"),
        namespace_row(catalog, "pg_catalog", "pg_catalog"),
        namespace_row(catalog, "public", "public"),
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .into_iter()
            .filter(|namespace| {
                current_database
                    .is_none_or(|database| schema_belongs_to_database(&namespace.name, database))
            })
            .map(|namespace| namespace_row(catalog, &namespace.name, &local_name(&namespace.name))),
    );
    rows.sort_by_key(row_sort_key);
    rows.dedup_by_key(|row| row_sort_key(row));
    rows
}

pub(super) fn pg_class(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&collection.name, database))
        })
        .map(|collection| class_row(catalog, &collection.name, "r", None))
        .collect::<Vec<_>>();
    rows.extend(
        catalog
            .list_views()
            .into_iter()
            .filter(|view| {
                current_database
                    .is_none_or(|database| relation_belongs_to_database(&view.name, database))
            })
            .map(|view| class_row(catalog, &view.name, "v", None)),
    );
    rows.extend(
        catalog
            .list_sequences()
            .into_iter()
            .filter(|sequence| {
                current_database
                    .is_none_or(|database| relation_belongs_to_database(&sequence.name, database))
            })
            .map(|sequence| class_row(catalog, &sequence.name, "S", None)),
    );
    rows.extend(
        sorted_indexes(catalog)
            .into_iter()
            .filter(|index| {
                current_database.is_none_or(|database| {
                    relation_belongs_to_database(&index.collection, database)
                })
            })
            .map(|index| class_row(catalog, &index.name, "i", Some(&index))),
    );
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_attribute(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections_canonical() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        let Some(schema) = catalog.get_schema(&collection.name) else {
            continue;
        };
        let constraints = catalog.get_constraints(&collection.name);
        for (index, field) in schema.fields.iter().enumerate() {
            let constraint = constraint_for_field(&constraints, &field.name);
            rows.push(attribute_row(
                catalog,
                &schema.collection,
                &field.name,
                &field.data_type,
                index,
                constraint,
            ));
        }
    }
    for view in catalog.list_views() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&view.name, database))
        {
            continue;
        }
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(attribute_row(
                catalog,
                &view.name,
                &field.name,
                &field.data_type,
                index,
                None,
            ));
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_indexes(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    sorted_indexes(catalog)
        .into_iter()
        .filter(|index| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&index.collection, database))
        })
        .map(|index| {
            vec![
                string("schemaname", relation_schema_name(&index.collection)),
                string("tablename", local_name(&index.collection)),
                string("indexname", local_name(&index.name)),
                string("indexdef", index_definition(&index)),
            ]
        })
        .collect()
}

pub(super) fn pg_index(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    sorted_indexes(catalog)
        .into_iter()
        .filter(|index| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&index.collection, database))
        })
        .map(|index| {
            let primary = index_is_primary(catalog, &index);
            vec![
                string("indexrelid", local_name(&index.name)),
                string("indrelid", local_name(&index.collection)),
                int_value("indexrelid_oid", index_oid(&index)),
                int_value("indrelid_oid", relation_oid(catalog, &index.collection)),
                bool_value("indisunique", index.unique),
                bool_value("indisprimary", primary),
                bool_value("indnullsnotdistinct", index.nulls_not_distinct()),
                string("indkey", index_keys(catalog, &index)),
                int_value("indnatts", index_attribute_count(catalog, &index)),
                int_value("indnkeyatts", index_key_count(catalog, &index)),
                string("indoption", index_options(&index)),
                bool_value("indisvalid", true),
                bool_value("indisready", true),
                string("indpred", index.predicate.clone().unwrap_or_default()),
                string(
                    "indexprs",
                    index
                        .normalized_expressions()
                        .iter()
                        .map(|expression| format_stored_expression(expression))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            ]
        })
        .collect()
}

pub(super) fn pg_attrdef(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections_canonical() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        let Some(schema) = catalog.get_schema(&collection.name) else {
            continue;
        };
        let constraints = catalog.get_constraints(&collection.name);
        for (index, field) in schema.fields.iter().enumerate() {
            let Some(constraint) = constraint_for_field(&constraints, &field.name) else {
                continue;
            };
            let Some(default) = constraint_default_expression(constraint) else {
                continue;
            };
            rows.push(vec![
                string("adrelid", local_name(&schema.collection)),
                int_value("adnum", index + 1),
                string("adsrc", default),
            ]);
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_type(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = builtin_type_rows();
    let collection_types = catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&collection.name, database))
        })
        .filter_map(|collection| catalog.get_schema(&collection.name))
        .flat_map(|schema| schema.fields.into_iter().map(|field| field.data_type));
    let view_types = catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&view.name, database))
        })
        .flat_map(|view| view.schema.fields.into_iter().map(|field| field.data_type));
    let dimensions = collection_types
        .chain(view_types)
        .filter_map(|data_type| match data_type {
            DataType::Vector(dimensions)
                if crate::types::schema::vector_dimensions_for_oid(data_type.type_oid())
                    .is_some() =>
            {
                Some(dimensions)
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    rows.extend(
        dimensions
            .into_iter()
            .map(|dimensions| type_row(&DataType::Vector(dimensions), "pg_catalog")),
    );
    rows.sort_by_key(row_sort_key);
    rows
}

fn attribute_row(
    catalog: &Catalog,
    relation: &str,
    field_name: &str,
    data_type: &DataType,
    index: usize,
    constraint: Option<&FieldConstraint>,
) -> VirtualRow {
    vec![
        string("attrelid", local_name(relation)),
        int_value("attrelid_oid", relation_oid(catalog, relation)),
        string("attname", field_name),
        int_value("attnum", index + 1),
        int_value("atttypid", data_type.type_oid()),
        bool_value("attnotnull", is_not_null(constraint)),
        int_value("atttypmod", i64::from(data_type.atttypmod())),
        bool_value("atthasdef", constraint_has_default(constraint)),
        bool_value("attisdropped", false),
        string("attidentity", ""),
        string("attgenerated", ""),
        int_value("attcollation", 0),
    ]
}

fn namespace_row(catalog: &Catalog, canonical: &str, name: &str) -> VirtualRow {
    vec![
        int_value("oid", namespace_oid(catalog, canonical)),
        string("nspname", name),
        int_value("nspowner", postgres_role_oid()),
        string("nspacl", ""),
    ]
}

fn class_row(catalog: &Catalog, name: &str, kind: &str, index: Option<&IndexMeta>) -> VirtualRow {
    let relation_name = local_name(name);
    let schema_name = relation_schema_name(name);
    vec![
        int_value(
            "oid",
            index.map_or_else(|| relation_oid(catalog, name), index_oid),
        ),
        string("relname", relation_name),
        string("relkind", kind),
        string("relnamespace", &schema_name),
        int_value(
            "relnamespace_oid",
            namespace_oid(catalog, &relation_namespace(name)),
        ),
        int_value("relowner", postgres_role_oid()),
        bool_value("relhasindex", relation_has_indexes(catalog, name)),
        string(
            "relpersistence",
            if kind == "S" {
                catalog
                    .get_sequence(name)
                    .map_or("p", |sequence| sequence.persistence.code())
            } else {
                "p"
            },
        ),
        int_value("reltuples", 0),
        string("relacl", ""),
        index.and_then(index_reloptions).map_or_else(
            || ("reloptions".to_string(), Value::Null),
            |value| string("reloptions", value),
        ),
    ]
}

fn index_reloptions(index: &IndexMeta) -> Option<String> {
    let options = index
        .options
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "deduplicate_items" | "fillfactor"))
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>();
    (!options.is_empty()).then(|| format!("{{{}}}", options.join(",")))
}

fn relation_has_indexes(catalog: &Catalog, name: &str) -> bool {
    sorted_indexes(catalog)
        .into_iter()
        .any(|index| index.collection.eq_ignore_ascii_case(name))
}

fn sorted_indexes(catalog: &Catalog) -> Vec<IndexMeta> {
    let mut indexes = catalog.all_indexes_snapshot();
    indexes.sort_by_key(|index| {
        (
            index.collection.to_ascii_lowercase(),
            index.name.to_ascii_lowercase(),
        )
    });
    indexes
}

pub(super) fn index_definition_by_oid(catalog: &Catalog, oid: i64) -> Option<String> {
    sorted_indexes(catalog)
        .into_iter()
        .find(|index| index_oid(index) == oid)
        .map(|index| index_definition(&index))
}

pub(super) fn index_definition_by_name(catalog: &Catalog, name: &str) -> Option<String> {
    let matches = sorted_indexes(catalog)
        .into_iter()
        .filter(|index| crate::catalog::name_matches(&index.name, name))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Some(index_definition(index)),
        _ => None,
    }
}

pub(super) fn index_definition(index: &IndexMeta) -> String {
    index_definitions::index_definition(index)
}

pub(super) fn format_stored_expression(raw: &str) -> String {
    index_definitions::format_stored_expression(raw)
}

fn index_is_primary(catalog: &Catalog, index: &IndexMeta) -> bool {
    let primary_fields = catalog
        .get_constraints(&index.collection)
        .into_iter()
        .filter(|constraint| constraint.primary_key)
        .collect::<Vec<_>>();
    if primary_fields.is_empty() {
        return false;
    }

    if primary_fields.iter().any(|constraint| {
        constraint
            .primary_key_name
            .as_deref()
            .is_some_and(|name| name_matches(name, &index.name))
    }) {
        return true;
    }

    let mut ordered_primary_fields = primary_fields
        .into_iter()
        .map(|constraint| {
            (
                constraint.primary_key_ordinal.unwrap_or(u32::MAX),
                constraint.field,
            )
        })
        .collect::<Vec<_>>();
    ordered_primary_fields.sort_by_key(|(ordinal, field)| (*ordinal, field.to_ascii_lowercase()));
    let ordered_primary_fields = ordered_primary_fields
        .into_iter()
        .map(|(_, field)| field)
        .collect::<Vec<_>>();
    let index_fields = index.normalized_fields();

    ordered_primary_fields.len() == index_fields.len()
        && ordered_primary_fields
            .iter()
            .zip(index_fields.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn index_keys(catalog: &Catalog, index: &IndexMeta) -> String {
    let Some(schema) = catalog.get_schema(&index.collection) else {
        return String::new();
    };
    let keys = index
        .normalized_fields()
        .into_iter()
        .map(|field| {
            schema
                .fields
                .iter()
                .position(|candidate| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.name)
                        == crate::sql::ColumnIdentifierPath::stored_field_key(&field)
                })
                .map_or_else(|| "0".to_string(), |position| (position + 1).to_string())
        })
        .chain(
            index
                .normalized_expressions()
                .into_iter()
                .map(|_| "0".to_string()),
        )
        .chain(index.normalized_include_fields().into_iter().map(|field| {
            schema
                .fields
                .iter()
                .position(|candidate| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.name)
                        == crate::sql::ColumnIdentifierPath::stored_field_key(&field)
                })
                .map_or_else(|| "0".to_string(), |position| (position + 1).to_string())
        }))
        .collect::<Vec<_>>();
    keys.join(" ")
}

fn index_key_count(catalog: &Catalog, index: &IndexMeta) -> usize {
    let field_count = index.normalized_fields().len();
    let expression_count = index.normalized_expressions().len();
    if field_count + expression_count > 0 {
        field_count + expression_count
    } else {
        index_keys(catalog, index)
            .split_whitespace()
            .filter(|key| !key.is_empty())
            .count()
            .saturating_sub(index.normalized_include_fields().len())
    }
}

fn index_attribute_count(catalog: &Catalog, index: &IndexMeta) -> usize {
    index_key_count(catalog, index) + index.normalized_include_fields().len()
}

fn index_options(index: &IndexMeta) -> String {
    (0..index.normalized_fields().len() + index.normalized_expressions().len())
        .map(|position| {
            let descending = index
                .key_option(position, "order")
                .is_some_and(|order| order.eq_ignore_ascii_case("desc"));
            let nulls_first = index
                .key_option(position, "nulls")
                .map_or(descending, |nulls| nulls.eq_ignore_ascii_case("first"));
            u8::from(descending) | (u8::from(nulls_first) << 1)
        })
        .map(|option| option.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn constraint_for_field<'a>(
    constraints: &'a [FieldConstraint],
    field: &str,
) -> Option<&'a FieldConstraint> {
    constraints.iter().find(|constraint| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    })
}

fn is_not_null(constraint: Option<&FieldConstraint>) -> bool {
    constraint.is_some_and(|constraint| constraint.not_null || constraint.primary_key)
}

fn constraint_has_default(constraint: Option<&FieldConstraint>) -> bool {
    constraint.is_some_and(|constraint| {
        constraint.default_expression.is_some() || constraint.default_value.is_some()
    })
}

pub(super) fn constraint_default_expression(constraint: &FieldConstraint) -> Option<String> {
    if let Some(expression) = constraint.default_expression.as_ref() {
        return Some(expression.clone());
    }
    constraint.default_value.as_ref().map(default_expression)
}

fn default_expression(value: &serde_json::Value) -> String {
    if value.is_null() {
        return "NULL".to_string();
    }
    if let Some(value) = value.as_str() {
        return format!("'{}'", value.replace('\'', "''"));
    }
    value.to_string()
}

fn builtin_type_rows() -> Vec<VirtualRow> {
    let namespace = "pg_catalog";
    let types = crate::types::builtin::BUILTIN_TYPES;
    let mut rows: Vec<_> = types
        .iter()
        .map(|metadata| type_row(&metadata.kind.data_type(), namespace))
        .collect();
    rows.extend(
        types
            .iter()
            .filter(|metadata| metadata.kind != crate::types::builtin::BuiltinKind::Unknown)
            .map(|metadata| {
                type_row(
                    &DataType::Array(Box::new(metadata.kind.data_type())),
                    namespace,
                )
            }),
    );

    rows
}

fn type_row(data_type: &DataType, namespace: &str) -> VirtualRow {
    let typname = data_type.type_name();
    vec![
        int_value("oid", data_type.type_oid()),
        string("typname", typname),
        string("typnamespace", namespace),
        int_value("typlen", i64::from(data_type.typlen())),
        bool_value("typbyval", is_type_passed_by_value(data_type)),
        string("typtype", type_kind(data_type)),
        string("typcategory", type_category(data_type)),
        int_value("typelem", element_type_oid(data_type)),
    ]
}

fn is_type_passed_by_value(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Date
            | DataType::Time
    )
}

fn type_kind(data_type: &DataType) -> String {
    if matches!(data_type, DataType::Array(_)) {
        "a".to_string()
    } else {
        "b".to_string()
    }
}

fn type_category(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "Z".to_string(),
        DataType::Boolean => "B".to_string(),
        DataType::SmallInt
        | DataType::Int
        | DataType::BigInt
        | DataType::Float
        | DataType::ObjectId(_) => "N".to_string(),
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } | DataType::Json => {
            "S".to_string()
        }
        DataType::Bytea | DataType::Uuid => "U".to_string(),
        DataType::Date | DataType::Time | DataType::Timestamp => "D".to_string(),
        DataType::Vector(_) => "V".to_string(),
        DataType::Array(_) => "A".to_string(),
        DataType::Extended(kind) => kind.category().to_string(),
    }
}

fn element_type_oid(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Array(inner) => inner.type_oid(),
        _ => 0,
    }
}

pub(super) fn postgres_role_oid() -> i64 {
    10
}

/// `pg_namespace.oid`: the schema's stored identity. `schema` is canonical
/// (`database.schema`) or one of the database-independent system schemas.
pub(super) fn namespace_oid(catalog: &Catalog, schema: &str) -> i64 {
    crate::catalog::object_identity::stored_namespace_oid(catalog, schema).map_or_else(
        || stable_catalog_oid("namespace", schema, 50_000),
        i64::from,
    )
}

/// `pg_class.oid`: the relation's stored identity, which `regclass` shares.
pub(super) fn relation_oid(catalog: &Catalog, name: &str) -> i64 {
    crate::catalog::object_identity::stored_relation_oid(catalog, name)
        .map_or_else(|| stable_catalog_oid("relation", name, 100_000), i64::from)
}

/// The canonical schema a canonical relation name belongs to.
fn relation_namespace(relation: &str) -> String {
    crate::catalog::RelationId::parse_canonical(relation).map_or_else(
        || relation_schema_name(relation),
        |relation| crate::catalog::canonical_schema_name(&relation.database, &relation.schema),
    )
}

pub(super) fn database_oid(name: &str) -> i64 {
    stable_catalog_oid("relation", name, 100_000)
}

pub(super) fn index_oid(index: &IndexMeta) -> i64 {
    i64::from(crate::catalog::object_identity::catalog_index_oid(index))
}

pub(super) fn constraint_oid(collection: &str, constraint: &str) -> i64 {
    stable_catalog_oid("constraint", &format!("{collection}.{constraint}"), 300_000)
}

fn stable_catalog_oid(kind: &str, name: &str, base: i64) -> i64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in kind
        .bytes()
        .chain(*b":")
        .chain(name.to_ascii_lowercase().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    base + i64::try_from(hash % 800_000).unwrap_or(0)
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

fn bool(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Boolean)
}

fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_string(), Value::String(value.into()))
}

fn int_value<T>(name: &str, value: T) -> (String, Value)
where
    T: TryInto<i64>,
{
    (
        name.to_string(),
        Value::Int64(value.try_into().unwrap_or(i64::MAX)),
    )
}

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn row_sort_key(row: &VirtualRow) -> String {
    row.iter()
        .map(|(_, value)| match value {
            Value::String(value) => value.clone(),
            Value::Int64(value) => value.to_string(),
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Float64(value) => value.to_string(),
            Value::Vector(value) => format!("{:?}", value.values),
            Value::Json(value) => value.to_string(),
            Value::Typed(value) => value.text(),
        })
        .collect::<Vec<_>>()
        .join("\u{1f}")
}
