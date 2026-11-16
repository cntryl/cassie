//! The reserved `cassie_graph` schema and its composite types (#1268).
//!
//! The schema is system owned: it carries [`GRAPH_SCHEMA_OWNER`] in its
//! namespace metadata. A same-named schema without the marker is a user object
//! and is never taken over. The three types are ordinary named composite types
//! whose descriptors, array types and backing `relkind = 'c'` relations are
//! allocated and persisted like any other composite.

use super::{
    canonical_relation_name, canonical_schema_name, Catalog, CompositeTypeMeta,
    COMPOSITE_TYPE_FORMAT_VERSION,
};
use crate::app::CassieError;
use crate::types::composite::{CompositeField, CompositeRef};
use crate::types::graph::{GraphDescriptors, GraphTypeKind, GRAPH_SCHEMA, GRAPH_SCHEMA_OWNER};
use crate::types::DataType;

/// `pg_roles.oid` of the bootstrap superuser that owns system objects.
const OWNER_OID: u32 = 10;
/// First revision of a newly created composite descriptor.
const INITIAL_REVISION: u64 = 1;

/// What the catalog holds under the reserved schema name of one database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSchemaState {
    /// No schema of that name exists: bootstrap may create it.
    Absent,
    /// The system-owned schema exists.
    SystemOwned,
    /// A user-created schema holds the name: bootstrap must fail (`42710`).
    UserOwned,
}

/// Whether `schema` names the reserved graph schema. A database-qualified
/// schema (`db.cassie_graph`) names it too, in whichever database.
#[must_use]
pub fn is_graph_schema(schema: &str) -> bool {
    super::split_identifier_path(schema)
        .ok()
        .and_then(|parts| parts.last().cloned())
        .is_some_and(|last| last.eq_ignore_ascii_case(GRAPH_SCHEMA))
}

/// The error of user DDL that would create, alter, rename or drop an object of
/// the reserved schema. The system owns the schema: PostgreSQL reports a
/// system-owned schema as `42501`, and so does Cassie.
#[must_use]
pub fn reserved_schema_error(action: &str) -> crate::app::CassieError {
    crate::app::DiagnosticError::new(
        "42501",
        format!("permission denied to {action}: schema \"{GRAPH_SCHEMA}\" is reserved for Cassie graph types"),
    )
    .into()
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
pub fn build_graph_types(
    database: &str,
    mut allocate: impl FnMut() -> Result<u64, CassieError>,
) -> Result<Vec<CompositeTypeMeta>, CassieError> {
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

impl Catalog {
    /// Whether `schema` (local or qualified) of `database` is the system-owned
    /// reserved schema. A user-owned schema of the same name is not reserved:
    /// it is the operator's to rename or drop.
    #[must_use]
    pub fn is_reserved_graph_schema(&self, database: &str, schema: &str) -> bool {
        let (database, name) = match super::parse_name(schema) {
            Ok(super::ParsedName::Unqualified(name)) => (database.to_owned(), name),
            Ok(super::ParsedName::SchemaQualified { schema, name }) => (schema, name),
            Ok(super::ParsedName::DatabaseQualified {
                database, schema, ..
            }) => (database, schema),
            Err(_) => return false,
        };
        is_graph_schema(&name)
            && self.graph_schema_state(&database) == GraphSchemaState::SystemOwned
    }

    /// Who owns the reserved schema name in `database`.
    #[must_use]
    pub fn graph_schema_state(&self, database: &str) -> GraphSchemaState {
        match self.get_namespace(&graph_schema_namespace(database)) {
            None => GraphSchemaState::Absent,
            Some(namespace) if namespace.system_owner.as_deref() == Some(GRAPH_SCHEMA_OWNER) => {
                GraphSchemaState::SystemOwned
            }
            Some(_) => GraphSchemaState::UserOwned,
        }
    }

    /// The descriptors of the three reserved types of `database`, or `None`
    /// until the schema has been bootstrapped there.
    #[must_use]
    pub fn graph_descriptors(&self, database: &str) -> Option<GraphDescriptors> {
        if self.graph_schema_state(database) != GraphSchemaState::SystemOwned {
            return None;
        }
        let descriptor = |kind: GraphTypeKind| {
            self.get_composite_type(&canonical_relation_name(
                database,
                GRAPH_SCHEMA,
                kind.type_name(),
            ))
            .map(|metadata| metadata.descriptor())
        };
        Some(GraphDescriptors {
            key_part: descriptor(GraphTypeKind::KeyPart)?,
            element_ref: descriptor(GraphTypeKind::ElementRef)?,
            path: descriptor(GraphTypeKind::Path)?,
        })
    }
}
