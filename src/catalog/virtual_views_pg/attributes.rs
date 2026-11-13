//! `pg_attribute` row construction shared by relations and composite types.
use super::{
    bool_value, collation, constraint_has_default, int_value, is_not_null, relation_oid, string,
    VirtualRow,
};
use crate::catalog::{local_name, Catalog, FieldConstraint};
use crate::types::DataType;

/// One `pg_attribute` column described independently of its owning relation.
pub(super) struct AttributeSpec<'a> {
    pub(super) field_name: &'a str,
    pub(super) data_type: &'a DataType,
    pub(super) attnum: i64,
    pub(super) constraint: Option<&'a FieldConstraint>,
    pub(super) is_dropped: bool,
}

pub(super) fn attribute_row(
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
    let storage = constraint.and_then(|item| item.storage);
    let compression = if is_dropped {
        None
    } else {
        constraint.and_then(|item| item.compression)
    };
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
        string(
            "attstorage",
            storage
                .unwrap_or_else(|| crate::catalog::column_storage::default_storage(data_type))
                .code(),
        ),
        string(
            "attcompression",
            compression.map_or("", crate::catalog::ColumnCompression::code),
        ),
        int_value("attcollation", collation::type_oid(data_type)),
    ]
}
