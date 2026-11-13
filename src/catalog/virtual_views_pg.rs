use crate::catalog::{
    local_name, name_matches, relation_belongs_to_database, relation_schema_name,
    schema_belongs_to_database, Catalog, FieldConstraint, IndexMeta,
};
use crate::midge::adapter::Midge;
use crate::types::{DataType, ObjectIdKind, Value};

use super::VirtualRow;

#[path = "virtual_views_pg/acl.rs"]
mod acl;
#[path = "virtual_views_pg/collation.rs"]
pub(super) mod collation;
#[path = "virtual_views_pg/composite_catalogs.rs"]
mod composite_catalogs;
#[path = "virtual_views_pg/descriptions.rs"]
pub(super) mod descriptions;
#[path = "virtual_views_pg/domain_catalogs.rs"]
mod domain_catalogs;
#[path = "virtual_views_pg/index_definitions.rs"]
mod index_definitions;
#[path = "virtual_views_pg/type_identity.rs"]
mod type_identity;
#[path = "virtual_views_pg/vector_catalogs.rs"]
pub(super) mod vector_catalogs;

pub(super) fn namespace_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        ("nspname".to_owned(), DataType::Name),
        int("nspowner"),
        text("nspacl"),
    ]
}

pub(super) fn class_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        text("relname"),
        text("relkind"),
        oid("relam"),
        text("relnamespace"),
        int("relnamespace_oid"),
        oid("reltype"),
        int("relowner"),
        bool("relhasindex"),
        text("relpersistence"),
        int("reltuples"),
        text("relacl"),
        text("reloptions"),
    ]
}

pub(super) fn access_method_schema() -> Vec<(String, DataType)> {
    vec![oid("oid"), text("amname")]
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
        oid("oid"),
        text("typname"),
        oid("typnamespace"),
        int("typlen"),
        bool("typbyval"),
        text("typtype"),
        text("typcategory"),
        oid("typelem"),
        oid("typarray"),
        oid("typbasetype"),
        bool("typnotnull"),
        int("typtypmod"),
        oid("typrelid"),
    ]
}

pub(super) fn pg_namespace(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = vec![
        namespace_row(
            catalog,
            "information_schema",
            "information_schema",
            current_database,
        ),
        namespace_row(catalog, "pg_catalog", "pg_catalog", current_database),
        namespace_row(catalog, "public", "public", current_database),
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .into_iter()
            .filter(|namespace| {
                current_database
                    .is_none_or(|database| schema_belongs_to_database(&namespace.name, database))
            })
            .map(|namespace| {
                namespace_row(
                    catalog,
                    &namespace.name,
                    &local_name(&namespace.name),
                    current_database,
                )
            }),
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
        .map(|collection| class_row(catalog, &collection.name, "r", None, current_database))
        .collect::<Vec<_>>();
    rows.extend(
        catalog
            .list_views()
            .into_iter()
            .filter(|view| {
                current_database
                    .is_none_or(|database| relation_belongs_to_database(&view.name, database))
            })
            .map(|view| class_row(catalog, &view.name, "v", None, current_database)),
    );
    rows.extend(
        catalog
            .list_projection_metadata()
            .into_iter()
            .filter(|projection| {
                projection.kind == crate::catalog::ProjectionKind::Materialized
                    && projection.materialized.as_ref().is_some_and(
                        crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                    )
                    && current_database.is_none_or(|database| {
                        relation_belongs_to_database(&projection.collection, database)
                    })
            })
            .map(|projection| {
                class_row(catalog, &projection.collection, "m", None, current_database)
            }),
    );
    rows.extend(
        catalog
            .list_sequences()
            .into_iter()
            .filter(|sequence| {
                current_database
                    .is_none_or(|database| relation_belongs_to_database(&sequence.name, database))
            })
            .map(|sequence| class_row(catalog, &sequence.name, "S", None, current_database)),
    );
    rows.extend(
        sorted_indexes(catalog)
            .into_iter()
            .filter(|index| {
                catalog.is_published_in_postgresql_catalogs(index, current_database)
                    && current_database.is_none_or(|database| {
                        relation_belongs_to_database(&index.collection, database)
                    })
            })
            .map(|index| class_row(catalog, &index.name, "i", Some(&index), current_database)),
    );
    rows.extend(composite_catalogs::class_rows(catalog, current_database));
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_attribute(
    catalog: &Catalog,
    midge: &Midge,
    current_database: Option<&str>,
) -> Result<Vec<VirtualRow>, crate::app::CassieError> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections_canonical() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        let row_schema = midge.row_schema(&collection.name)?;
        let constraints = catalog.get_constraints(&collection.name);
        // Midge field IDs are allocated monotonically and retained on drop, so
        // they provide the stable positive attribute number for table columns.
        for field in &row_schema.fields {
            let constraint = if field.retired {
                None
            } else {
                constraint_for_field(&constraints, &field.name)
            };
            let field_name = if field.retired {
                dropped_attribute_name(field.field_id)
            } else {
                field.name.clone()
            };
            let mut row = attribute_row(
                catalog,
                &collection.name,
                &AttributeSpec {
                    field_name: &field_name,
                    data_type: &field.data_type,
                    attnum: i64::from(field.field_id),
                    constraint,
                    is_dropped: field.retired,
                },
                current_database,
            );
            if let Some(domain) = field.data_type.domain().filter(|_| !field.retired) {
                domain_catalogs::mark_domain_column(&mut row, domain);
            }
            rows.push(row);
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
                &AttributeSpec {
                    field_name: &field.name,
                    data_type: &field.data_type,
                    attnum: i64::try_from(index + 1).unwrap_or(i64::MAX),
                    constraint: None,
                    is_dropped: false,
                },
                current_database,
            ));
        }
    }
    for projection in catalog.list_projection_metadata() {
        if projection.kind != crate::catalog::ProjectionKind::Materialized
            || current_database.is_some_and(|database| {
                !relation_belongs_to_database(&projection.collection, database)
            })
        {
            continue;
        }
        let Some(materialized) = projection
            .materialized
            .filter(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
        else {
            continue;
        };
        for (index, field) in materialized.output_schema.fields.iter().enumerate() {
            rows.push(attribute_row(
                catalog,
                &projection.collection,
                &AttributeSpec {
                    field_name: &field.name,
                    data_type: &field.data_type,
                    attnum: i64::try_from(index + 1).unwrap_or(i64::MAX),
                    constraint: None,
                    is_dropped: false,
                },
                current_database,
            ));
        }
    }
    rows.extend(composite_catalogs::attribute_rows(
        catalog,
        current_database,
    ));
    rows.sort_by_key(row_sort_key);
    Ok(rows)
}

pub(super) fn pg_indexes(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    sorted_indexes(catalog)
        .into_iter()
        .filter(|index| {
            catalog.is_published_in_postgresql_catalogs(index, current_database)
                && current_database.is_none_or(|database| {
                    relation_belongs_to_database(&index.collection, database)
                })
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
            catalog.is_published_in_postgresql_catalogs(index, current_database)
                && current_database.is_none_or(|database| {
                    relation_belongs_to_database(&index.collection, database)
                })
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
                (
                    "indpred".to_string(),
                    index
                        .predicate
                        .as_ref()
                        .map_or(Value::Null, |predicate| Value::String(predicate.clone())),
                ),
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
            let Some(default) = constraint_default_expression(constraint, &field.data_type) else {
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
    let mut rows = builtin_type_rows(catalog);
    let extensions = current_database.map_or_else(
        || catalog.list_extensions(),
        |database| catalog.list_extensions_for_database(database),
    );
    rows.extend(extensions.into_iter().filter_map(|extension| {
        if !extension.name.eq_ignore_ascii_case("vector") {
            return None;
        }
        let type_oid = extension
            .vector_type_object_id
            .and_then(crate::catalog::object_identity::normal_oid)?;
        Some(type_row_with_oid(
            catalog,
            &DataType::Vector(None),
            &extension.schema,
            i64::from(type_oid),
        ))
    }));
    rows.extend(composite_catalogs::type_rows(catalog, current_database));
    rows.extend(domain_catalogs::type_rows(catalog, current_database));
    rows.sort_by_key(row_sort_key);
    rows
}

/// One `pg_attribute` column described independently of its owning relation.
struct AttributeSpec<'a> {
    field_name: &'a str,
    data_type: &'a DataType,
    attnum: i64,
    constraint: Option<&'a FieldConstraint>,
    is_dropped: bool,
}

fn attribute_row(
    catalog: &Catalog,
    relation: &str,
    attribute: &AttributeSpec<'_>,
    current_database: Option<&str>,
) -> VirtualRow {
    let &AttributeSpec {
        field_name,
        data_type,
        attnum,
        constraint,
        is_dropped,
    } = attribute;
    let identity = if is_dropped {
        None
    } else {
        constraint.and_then(|item| item.identity)
    };
    vec![
        string("attrelid", local_name(relation)),
        int_value("attrelid_oid", relation_oid(catalog, relation)),
        string("attname", field_name),
        int_value("attnum", attnum),
        int_value(
            "atttypid",
            if is_dropped {
                0
            } else {
                current_database
                    .and_then(|database| {
                        crate::catalog::object_identity::type_oid_for_database(
                            catalog, database, data_type,
                        )
                    })
                    .unwrap_or_else(|| {
                        if matches!(data_type, DataType::Vector(_)) {
                            0
                        } else {
                            data_type.type_oid()
                        }
                    })
            },
        ),
        bool_value("attnotnull", !is_dropped && is_not_null(constraint)),
        int_value("atttypmod", i64::from(data_type.atttypmod())),
        bool_value(
            "atthasdef",
            !is_dropped && constraint_has_default(constraint),
        ),
        bool_value("attisdropped", is_dropped),
        string(
            "attidentity",
            match identity {
                Some(crate::catalog::IdentityGeneration::Always) => "a",
                Some(crate::catalog::IdentityGeneration::ByDefault) => "d",
                None => "",
            },
        ),
        string("attgenerated", ""),
        int_value("attcollation", collation::type_oid(data_type)),
    ]
}

fn dropped_attribute_name(field_id: u32) -> String {
    format!("........pg.dropped.{field_id}........")
}

fn namespace_row(
    catalog: &Catalog,
    canonical: &str,
    name: &str,
    current_database: Option<&str>,
) -> VirtualRow {
    vec![
        int_value("oid", namespace_oid(catalog, canonical)),
        string("nspname", name),
        int_value("nspowner", postgres_role_oid()),
        (
            "nspacl".to_string(),
            acl::schema_acl(catalog, current_database, canonical),
        ),
    ]
}

fn class_row(
    catalog: &Catalog,
    name: &str,
    kind: &str,
    index: Option<&IndexMeta>,
    current_database: Option<&str>,
) -> VirtualRow {
    let relation_name = local_name(name);
    let schema_name = index.map_or_else(
        || relation_schema_name(name),
        |index| relation_schema_name(&index.collection),
    );
    let namespace = index.map_or_else(
        || relation_namespace(name),
        |index| relation_namespace(&index.collection),
    );
    vec![
        int_value(
            "oid",
            index.map_or_else(|| relation_oid(catalog, name), index_oid),
        ),
        string("relname", relation_name),
        string("relkind", kind),
        int_value(
            "relam",
            relation_access_method_oid(catalog, current_database, kind, index),
        ),
        string("relnamespace", &schema_name),
        int_value("relnamespace_oid", namespace_oid(catalog, &namespace)),
        // Tables own a derived row type; views, sequences and indexes do not.
        int_value(
            "reltype",
            composite_catalogs::relation_reltype(catalog, kind, name),
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
        (
            "relacl".to_string(),
            acl::table_acl(catalog, current_database, name, kind),
        ),
        index.and_then(index_reloptions).map_or_else(
            || ("reloptions".to_string(), Value::Null),
            |value| string("reloptions", value),
        ),
    ]
}

fn relation_access_method_oid(
    catalog: &Catalog,
    current_database: Option<&str>,
    kind: &str,
    index: Option<&IndexMeta>,
) -> i64 {
    match kind {
        "r" | "m" => 2,
        "i" => index.map_or(0, |index| match index.access_method() {
            "btree" => 403,
            "hash" => 405,
            "gin" => 2742,
            "hnsw" | "ivfflat" => current_database
                .and_then(|database| catalog.get_extension(database, "vector"))
                .and_then(|extension| extension.vector_catalog_identities)
                .map(|identities| match index.access_method() {
                    "hnsw" => identities.hnsw.access_method,
                    "ivfflat" => identities.ivfflat.access_method,
                    _ => 0,
                })
                .and_then(crate::catalog::object_identity::normal_oid)
                .map_or(0, i64::from),
            _ => 0,
        }),
        _ => 0,
    }
}

fn index_reloptions(index: &IndexMeta) -> Option<String> {
    let names = match index.access_method() {
        "hnsw" => &["m", "ef_construction"][..],
        "ivfflat" => &["lists"][..],
        _ => &["deduplicate_items", "fillfactor"][..],
    };
    let options = names
        .iter()
        .filter_map(|name| {
            index
                .options
                .get(*name)
                .map(|value| format!("{name}={value}"))
        })
        .collect::<Vec<_>>();
    (!options.is_empty()).then(|| format!("{{{}}}", options.join(",")))
}

fn relation_has_indexes(catalog: &Catalog, name: &str) -> bool {
    catalog.has_relation_index_history(name)
}

fn sorted_indexes(catalog: &Catalog) -> Vec<IndexMeta> {
    let mut indexes = catalog.logical_indexes_snapshot();
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

pub(super) fn constraint_default_expression(
    constraint: &FieldConstraint,
    data_type: &crate::types::DataType,
) -> Option<String> {
    if let Some(expression) = constraint.default_expression.as_ref() {
        return Some(expression.clone());
    }
    constraint
        .default_value
        .as_ref()
        .map(|value| default_expression(value, data_type))
}

fn default_expression(value: &serde_json::Value, data_type: &crate::types::DataType) -> String {
    if value.is_null() {
        return "NULL".to_string();
    }
    if let Some(value) = value.as_str() {
        if matches!(
            data_type,
            crate::types::DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
        ) {
            return value.to_owned();
        }
        return format!("'{}'", value.replace('\'', "''"));
    }
    value.to_string()
}

fn builtin_type_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    let namespace = "pg_catalog";
    let types = crate::types::builtin::BUILTIN_TYPES;
    let mut rows: Vec<_> = types
        .iter()
        .map(|metadata| type_row(catalog, &metadata.kind.data_type(), namespace))
        .collect();
    rows.extend(
        types
            .iter()
            .filter(|metadata| metadata.kind != crate::types::builtin::BuiltinKind::Unknown)
            .map(|metadata| {
                type_row(
                    catalog,
                    &DataType::Array(Box::new(metadata.kind.data_type())),
                    namespace,
                )
            }),
    );

    rows
}

fn type_row(catalog: &Catalog, data_type: &DataType, namespace: &str) -> VirtualRow {
    type_row_with_oid(catalog, data_type, namespace, data_type.type_oid())
}

fn type_row_with_oid(
    catalog: &Catalog,
    data_type: &DataType,
    namespace: &str,
    type_oid: i64,
) -> VirtualRow {
    let builtin_type = u32::try_from(type_oid)
        .ok()
        .and_then(|oid| crate::catalog::builtin_identity::builtin_identities().type_by_oid(oid));
    let typname =
        builtin_type.map_or_else(|| data_type.type_name(), |metadata| metadata.name.clone());
    vec![
        int_value("oid", type_oid),
        string("typname", typname),
        int_value("typnamespace", namespace_oid(catalog, namespace)),
        int_value("typlen", i64::from(data_type.typlen())),
        bool_value("typbyval", is_type_passed_by_value(data_type)),
        string("typtype", type_identity::kind(data_type, builtin_type)),
        string(
            "typcategory",
            type_identity::category(data_type, builtin_type),
        ),
        int_value(
            "typelem",
            builtin_type.map_or_else(
                || element_type_oid(data_type),
                |metadata| i64::from(metadata.element),
            ),
        ),
        int_value(
            "typarray",
            builtin_type.map_or_else(
                || array_type_oid(data_type),
                |metadata| i64::from(metadata.array),
            ),
        ),
        int_value("typbasetype", 0),
        bool_value("typnotnull", false),
        int_value("typtypmod", -1),
        int_value("typrelid", 0),
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
            | DataType::Timestamp
            | DataType::ObjectId(_)
            | DataType::Extended(
                crate::types::extended::ExtendedType::Real
                    | crate::types::extended::ExtendedType::PgLsn
                    | crate::types::extended::ExtendedType::Date
                    | crate::types::extended::ExtendedType::Time { .. }
                    | crate::types::extended::ExtendedType::Timestamp { .. }
                    | crate::types::extended::ExtendedType::TimestampTz { .. }
            )
    )
}

fn element_type_oid(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Array(inner) => inner.type_oid(),
        _ => 0,
    }
}

fn array_type_oid(data_type: &DataType) -> i64 {
    if matches!(data_type, DataType::Array(_)) {
        return 0;
    }

    u32::try_from(data_type.type_oid())
        .ok()
        .and_then(crate::catalog::builtin_identity::builtin_array_oid)
        .map_or(0, i64::from)
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

fn oid(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::ObjectId(ObjectIdKind::Oid))
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
            Value::Array(value) => value.to_storage_json().to_string(),
            Value::Typed(value) => value.text(),
        })
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::IndexKind;

    #[test]
    fn should_leave_unselected_native_index_methods_without_catalog_oids() {
        // Arrange
        let catalog = Catalog::new();
        let indexes =
            [IndexKind::Vector, IndexKind::Column, IndexKind::TimeSeries].map(|kind| IndexMeta {
                definition_semantics_version: 0,
                collection: "items".to_string(),
                name: "items_idx".to_string(),
                field: "value".to_string(),
                fields: vec!["value".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind,
                unique: false,
                options: std::collections::BTreeMap::new(),
            });

        // Act
        let access_method_oids = indexes
            .iter()
            .map(|index| relation_access_method_oid(&catalog, None, "i", Some(index)))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(access_method_oids, vec![0, 0, 0]);
    }
}
