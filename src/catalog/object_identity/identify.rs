//! `pg_identify_object`: the type, schema, name and identity text of a catalog
//! object named by class, OID and sub-object number.
//!
//! Only the classes Cassie models are identified: relations, types, schemas,
//! routines, extensions and roles. A class outside that set is an error; an
//! object that does not exist yields PostgreSQL's all-`NULL` shape.
use super::names::quote_identifier;
use super::{catalog_index_oid, ObjectScope, RelationIdentity};
use crate::catalog::builtin_identity::{builtin_identities, BuiltinTypeRow};
use crate::catalog::canonical_relation_name;
use crate::types::object_id::FIRST_NORMAL_OBJECT_ID;

const RELATION_CLASS: u32 = 1259;
const TYPE_CLASS: u32 = 1247;
const PROC_CLASS: u32 = 1255;
const NAMESPACE_CLASS: u32 = 2615;
const EXTENSION_CLASS: u32 = 3079;
const ROLE_CLASS: u32 = 1260;

/// Types `format_type` spells with an SQL keyword instead of a qualified name.
const KEYWORD_TYPE_OIDS: [u32; 16] = [
    16, 21, 23, 20, 700, 701, 1700, 1042, 1043, 1083, 1114, 1184, 1186, 1266, 1560, 1562,
];

/// The sequence relation's fixed columns, in attribute order.
const SEQUENCE_COLUMNS: [&str; 3] = ["last_value", "log_cnt", "is_called"];

/// The four columns `pg_identify_object` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub object_type: Option<String>,
    pub schema: Option<String>,
    pub name: Option<String>,
    pub identity: Option<String>,
}

impl ObjectIdentity {
    /// The shape of an object that does not exist: only its class description.
    fn missing(object_type: &str) -> Self {
        Self {
            object_type: Some(object_type.to_string()),
            schema: None,
            name: None,
            identity: None,
        }
    }

    /// The row a `NULL` argument returns.
    #[must_use]
    pub fn null() -> Self {
        Self {
            object_type: None,
            schema: None,
            name: None,
            identity: None,
        }
    }

    fn unqualified(object_type: &str, name: &str) -> Self {
        let quoted = quote_identifier(name);
        Self {
            object_type: Some(object_type.to_string()),
            schema: None,
            name: Some(quoted.clone()),
            identity: Some(quoted),
        }
    }

    fn qualified(object_type: &str, schema: &str, name: &str) -> Self {
        let schema = quote_identifier(schema);
        let name = quote_identifier(name);
        Self {
            object_type: Some(object_type.to_string()),
            identity: Some(format!("{schema}.{name}")),
            schema: Some(schema),
            name: Some(name),
        }
    }
}

/// The catalog class OID names no object class `pg_identify_object` models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedObjectClass(pub u32);

/// A type's schema, name column and `format_type_be_qualified` text.
struct TypeIdentity {
    schema: String,
    name: String,
    identity: String,
}

impl ObjectScope<'_> {
    /// Identifies the object `(class_id, object_id, sub_id)`. `table_column`
    /// names the column with attribute number `sub_id` of a stored table.
    ///
    /// # Errors
    /// Returns the class OID when it is not one Cassie models.
    pub fn identify_object(
        &self,
        class_id: u32,
        object_id: u32,
        sub_id: i32,
        table_column: &dyn Fn(&str, i32) -> Option<String>,
    ) -> Result<ObjectIdentity, UnsupportedObjectClass> {
        match class_id {
            RELATION_CLASS => Ok(self.identify_relation(object_id, sub_id, table_column)),
            TYPE_CLASS => Ok(self.identify_type(object_id)),
            PROC_CLASS => Ok(self.identify_routine(object_id)),
            NAMESPACE_CLASS => Ok(self.namespace_name(object_id).map_or_else(
                || ObjectIdentity::missing("schema"),
                |name| ObjectIdentity::unqualified("schema", &name),
            )),
            EXTENSION_CLASS => Ok(self.identify_extension(object_id)),
            ROLE_CLASS => Ok(self.role_name(object_id).map_or_else(
                || ObjectIdentity::missing("role"),
                |name| ObjectIdentity::unqualified("role", &name),
            )),
            other => Err(UnsupportedObjectClass(other)),
        }
    }

    fn identify_relation(
        &self,
        oid: u32,
        sub_id: i32,
        table_column: &dyn Fn(&str, i32) -> Option<String>,
    ) -> ObjectIdentity {
        let Some(relation) = self.relation_by_oid(oid) else {
            return ObjectIdentity::missing("relation");
        };
        let column_suffix = if sub_id == 0 { "" } else { " column" };
        let kind = self.relation_kind(&relation);
        let object_type = format!("{}{column_suffix}", relation_description(kind));
        if sub_id == 0 {
            return ObjectIdentity::qualified(&object_type, &relation.schema, &relation.name);
        }
        let Some(column) = self.relation_column_name(&relation, kind, sub_id, table_column) else {
            return ObjectIdentity::missing(&object_type);
        };
        let mut identity =
            ObjectIdentity::qualified(&object_type, &relation.schema, &relation.name);
        identity.identity = identity
            .identity
            .map(|relation| format!("{relation}.{}", quote_identifier(&column)));
        identity
    }

    /// The `relkind` of a relation `relation_by_oid` found.
    fn relation_kind(&self, relation: &RelationIdentity) -> char {
        let oid = relation.oid;
        if oid < FIRST_NORMAL_OBJECT_ID {
            return builtin_identities()
                .relations
                .iter()
                .find(|row| row.oid == oid)
                .map_or('r', |row| row.kind);
        }
        let database = &self.context.database;
        if self
            .catalog
            .list_composite_types_for_database(database)
            .iter()
            .any(|metadata| metadata.relation_oid() == oid)
        {
            return 'c';
        }
        if self.catalog.logical_indexes_snapshot().iter().any(|index| {
            catalog_index_oid(index) == oid
                && self
                    .catalog
                    .is_published_in_postgresql_catalogs(index, Some(database))
        }) {
            return 'i';
        }
        let canonical = canonical_relation_name(database, &relation.schema, &relation.name);
        if self.catalog.get_collection(&canonical).is_some() {
            'r'
        } else if self.catalog.get_view(&canonical).is_some() {
            'v'
        } else if self.catalog.get_sequence(&canonical).is_some() {
            'S'
        } else {
            'm'
        }
    }

    /// The name of column `sub_id` of a relation, for the relation kinds whose
    /// columns the catalog records. Other kinds name no column.
    fn relation_column_name(
        &self,
        relation: &RelationIdentity,
        kind: char,
        sub_id: i32,
        table_column: &dyn Fn(&str, i32) -> Option<String>,
    ) -> Option<String> {
        let position = usize::try_from(sub_id).ok()?.checked_sub(1)?;
        let database = &self.context.database;
        let canonical = canonical_relation_name(database, &relation.schema, &relation.name);
        match kind {
            'r' => table_column(&canonical, sub_id),
            'v' => self
                .catalog
                .get_view(&canonical)?
                .schema
                .fields
                .get(position)
                .map(|field| field.name.clone()),
            'S' => SEQUENCE_COLUMNS
                .get(position)
                .map(|name| (*name).to_string()),
            'c' => self
                .catalog
                .list_composite_types_for_database(database)
                .into_iter()
                .find(|metadata| metadata.relation_oid() == relation.oid)?
                .fields
                .get(position)
                .map(|field| field.name.clone()),
            _ => None,
        }
    }

    fn identify_type(&self, oid: u32) -> ObjectIdentity {
        self.type_identity(oid).map_or_else(
            || ObjectIdentity::missing("type"),
            |found| ObjectIdentity {
                object_type: Some("type".to_string()),
                schema: Some(quote_identifier(&found.schema)),
                name: Some(quote_identifier(&found.name)),
                identity: Some(found.identity),
            },
        )
    }

    fn identify_routine(&self, oid: u32) -> ObjectIdentity {
        let Some(routine) = self.routine_by_oid(oid) else {
            return ObjectIdentity::missing("routine");
        };
        let description = match routine.kind {
            'p' => "procedure",
            'a' => "aggregate",
            _ => "function",
        };
        let arguments = routine
            .arguments
            .iter()
            .map(|argument| self.type_identity_text(*argument))
            .collect::<Vec<_>>()
            .join(",");
        ObjectIdentity {
            object_type: Some(description.to_string()),
            identity: Some(format!(
                "{}.{}({arguments})",
                quote_identifier(&routine.schema),
                quote_identifier(&routine.name)
            )),
            schema: Some(quote_identifier(&routine.schema)),
            name: None,
        }
    }

    fn identify_extension(&self, oid: u32) -> ObjectIdentity {
        self.catalog
            .list_extensions_for_database(&self.context.database)
            .into_iter()
            .find(|extension| super::normal_oid(extension.object_id) == Some(oid))
            .map_or_else(
                || ObjectIdentity::missing("extension"),
                |extension| ObjectIdentity::unqualified("extension", &extension.name),
            )
    }

    /// `format_type_be_qualified` text, or the bare OID for an unknown type.
    fn type_identity_text(&self, oid: u32) -> String {
        self.type_identity(oid)
            .map_or_else(|| oid.to_string(), |found| found.identity)
    }

    /// A built-in or user-defined type, or its array type.
    fn type_identity(&self, oid: u32) -> Option<TypeIdentity> {
        if let Some(row) = builtin_identities().type_by_oid(oid) {
            return Some(TypeIdentity {
                schema: "pg_catalog".to_string(),
                name: row.name.clone(),
                identity: builtin_type_text(row),
            });
        }
        let (schema, name, is_array) = self.user_type_parts(oid)?;
        let qualified = format!("{}.{}", quote_identifier(&schema), quote_identifier(&name));
        Some(if is_array {
            TypeIdentity {
                schema,
                name: format!("_{name}"),
                identity: format!("{qualified}[]"),
            }
        } else {
            TypeIdentity {
                schema,
                name,
                identity: qualified,
            }
        })
    }

    /// Schema, name and array-ness of a user-defined type. Every user type
    /// family registers here, so a new family is identified by adding it.
    fn user_type_parts(&self, oid: u32) -> Option<(String, String, bool)> {
        if let Some(metadata) = self.catalog.get_composite_type_by_oid(oid) {
            return Some((metadata.schema_name(), metadata.local_name(), false));
        }
        if let Some(metadata) = self.catalog.get_composite_type_by_array_oid(oid) {
            return Some((metadata.schema_name(), metadata.local_name(), true));
        }
        if let Some(metadata) = self.catalog.get_domain_type_by_oid(oid) {
            return Some((metadata.schema_name(), metadata.local_name(), false));
        }
        if let Some(metadata) = self.catalog.get_domain_type_by_array_oid(oid) {
            return Some((metadata.schema_name(), metadata.local_name(), true));
        }
        // Every table owns a row type named like the table.
        let (table, _, is_array) = self.catalog.relation_row_type_by_oid(oid)?;
        Some((
            crate::catalog::relation_schema_name(&table),
            crate::catalog::local_name(&table),
            is_array,
        ))
    }
}

/// `format_type_be_qualified` for a built-in type row.
fn builtin_type_text(row: &BuiltinTypeRow) -> String {
    if row.category == 'A' && row.element != 0 {
        if let Some(element) = builtin_identities().type_by_oid(row.element) {
            return format!("{}[]", builtin_type_text(element));
        }
    }
    if KEYWORD_TYPE_OIDS.contains(&row.oid) {
        row.display.clone()
    } else {
        format!("pg_catalog.{}", quote_identifier(&row.name))
    }
}

/// `getObjectTypeDescription` for a `pg_class.relkind`.
const fn relation_description(kind: char) -> &'static str {
    match kind {
        'r' | 'p' => "table",
        'i' | 'I' => "index",
        'S' => "sequence",
        'v' => "view",
        'm' => "materialized view",
        'c' => "composite type",
        'f' => "foreign table",
        't' => "toast table",
        _ => "relation",
    }
}
