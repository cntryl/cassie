//! Names and persisted descriptors of the reserved graph schema.

use super::scope::{canonical_relation_name, canonical_schema_name, split_identifier_path};
use super::{CompositeTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION};
use crate::types::composite::{CompositeField, CompositeRef};
use crate::types::graph_identity::{GraphTypeKind, GRAPH_SCHEMA};
use crate::types::DataType;

/// `pg_roles.oid` of the bootstrap superuser that owns system objects.
const OWNER_OID: u32 = 10;
/// First revision of a newly created composite descriptor.
const INITIAL_REVISION: u64 = 1;

/// Whether `schema` names the reserved graph schema. A database-qualified
/// schema (`db.cassie_graph`) names it too, in whichever database.
#[must_use]
pub fn is_graph_schema(schema: &str) -> bool {
    split_identifier_path(schema)
        .ok()
        .and_then(|parts| parts.last().cloned())
        .is_some_and(|last| last.eq_ignore_ascii_case(GRAPH_SCHEMA))
}

/// Canonical name of the reserved schema in `database`.
#[must_use]
pub fn graph_schema_namespace(database: &str) -> String {
    canonical_schema_name(database, GRAPH_SCHEMA)
}

fn field(name: &str, data_type: DataType) -> CompositeField {
    CompositeField {
        name: name.to_owned(),
        data_type,
    }
}

fn array_of(descriptor: &CompositeRef) -> DataType {
    DataType::Array(Box::new(DataType::Composite(descriptor.clone())))
}

/// The three composite types of `database`, in dependency order. `allocate`
/// supplies the persistent object identities (type, array, relation per type).
///
/// # Errors
/// Returns the allocator's error.
pub fn build_graph_types<E>(
    database: &str,
    mut allocate: impl FnMut() -> Result<u64, E>,
) -> Result<Vec<CompositeTypeMeta>, E> {
    let mut built: Vec<CompositeTypeMeta> = Vec::with_capacity(3);
    for kind in GraphTypeKind::ALL {
        let fields = match kind {
            GraphTypeKind::KeyPart => vec![
                field("type_tag", DataType::SmallInt),
                field("integer_value", DataType::BigInt),
                field("text_value", DataType::Text),
                field("uuid_value", DataType::Uuid),
            ],
            GraphTypeKind::ElementRef => vec![
                field("database_id", DataType::Uuid),
                field("graph_id", DataType::Uuid),
                field("mapping_id", DataType::Uuid),
                field("element_kind", DataType::SmallInt),
                field("key_parts", array_of(&built[0].descriptor())),
            ],
            GraphTypeKind::Path => vec![
                field("vertices", array_of(&built[1].descriptor())),
                field("edges", array_of(&built[1].descriptor())),
                field("directions", DataType::Array(Box::new(DataType::SmallInt))),
            ],
        };
        let metadata = CompositeTypeMeta {
            format_version: COMPOSITE_TYPE_FORMAT_VERSION,
            database: database.to_owned(),
            name: canonical_relation_name(database, GRAPH_SCHEMA, kind.type_name()),
            object_id: allocate()?,
            array_object_id: allocate()?,
            relation_object_id: allocate()?,
            owner_oid: OWNER_OID,
            revision: INITIAL_REVISION,
            fields,
            dropped_positions: Vec::new(),
        };
        built.push(metadata);
    }
    Ok(built)
}
