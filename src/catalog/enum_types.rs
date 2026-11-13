//! Named enum type objects: durable identity, name resolution and lookup (#827).
//!
//! The design authority is the "Selected v0.4.0 named enum contract" section of
//! `docs/type-contract.md`.
use serde::{Deserialize, Serialize};

use super::domain_types::same_stored_relation;
use super::{
    canonical_relation_name, local_name, name_matches, parse_name, relation_database_name,
    relation_schema_name, Catalog, ParsedName,
};
use crate::types::enum_type::{EnumMember, EnumRef, EnumType};
use crate::types::DataType;

/// The persisted layout version of [`EnumTypeMeta`].
pub const ENUM_TYPE_FORMAT_VERSION: u32 = 1;

/// One `CREATE TYPE ... AS ENUM` object, scoped to its database.
///
/// `object_id`, `array_object_id` and every `members[].member_id` come from
/// the persistent object allocator, which commits independently of the DDL
/// overlay: an identity allocated by a rolled-back statement is burned and
/// never reused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumTypeMeta {
    pub format_version: u32,
    pub database: String,
    /// Canonical `database.schema.name`.
    pub name: String,
    pub object_id: u64,
    pub array_object_id: u64,
    pub owner_oid: u32,
    pub revision: u64,
    /// Members in sort order.
    pub members: Vec<EnumMember>,
}

impl EnumTypeMeta {
    /// The immutable descriptor values of this type carry.
    #[must_use]
    pub fn descriptor(&self) -> EnumRef {
        EnumRef::new(EnumType {
            oid: self.type_oid(),
            array_oid: self.array_oid(),
            qualified_name: self.name.clone(),
            name: local_name(&self.name),
            revision: self.revision,
            members: self.members.clone(),
        })
    }

    /// `pg_type.oid` of the enum type.
    #[must_use]
    pub fn type_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.object_id).unwrap_or(0)
    }

    /// `pg_type.oid` of the matching `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.array_object_id).unwrap_or(0)
    }

    #[must_use]
    pub fn schema_name(&self) -> String {
        relation_schema_name(&self.name)
    }

    #[must_use]
    pub fn local_name(&self) -> String {
        local_name(&self.name)
    }
}

/// One object that keeps an enum type alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDependent {
    /// Canonical name of the owning table.
    pub owner: String,
    /// PostgreSQL's phrase, `column c of table t`.
    pub phrase: String,
    /// The column is an array of the enum (`depends on type x[]`).
    pub array: bool,
}

impl EnumDependent {
    /// The `DROP TYPE` detail line for the enum named `type_name`.
    #[must_use]
    pub fn detail(&self, type_name: &str) -> String {
        let suffix = if self.array { "[]" } else { "" };
        format!("{} depends on type {type_name}{suffix}", self.phrase)
    }
}

/// Prefix of the internal table that stores one version of a materialized
/// view's output (`__cassie_projection_<view>_<version>`).
const PROJECTION_OUTPUT_PREFIX: &str = "__cassie_projection_";

/// PostgreSQL's name for the relation `collection` in a dependency detail:
/// `table t`, or `materialized view v` for a materialized view's output table,
/// whose internal name is never shown.
fn dependent_relation(collection: &str) -> String {
    let local = local_name(collection);
    match local
        .strip_prefix(PROJECTION_OUTPUT_PREFIX)
        .and_then(|versioned| versioned.rsplit_once('_'))
    {
        Some((view, _version)) if !view.is_empty() => format!("materialized view {view}"),
        _ => format!("table {local}"),
    }
}

/// The canonical output-table name of a materialized view without its
/// version suffix, or `None` for any other relation.
fn projection_output_stem(collection: &str) -> Option<&str> {
    let (stem, _version) = collection.rsplit_once('_')?;
    stem.rsplit('.')
        .next()
        .is_some_and(|local| local.starts_with(PROJECTION_OUTPUT_PREFIX))
        .then_some(stem)
}

/// PostgreSQL's rendering of an enum column default, `'label'::type`, from
/// the stored member form. `None` for any other column type or value.
#[must_use]
pub fn default_expression(value: &serde_json::Value, data_type: &DataType) -> Option<String> {
    let DataType::Enum(descriptor) = data_type.flattened() else {
        return None;
    };
    let member = descriptor.member(crate::types::EnumValue::storage_member_id(value)?)?;
    Some(format!(
        "'{}'::{}",
        member.label.replace('\'', "''"),
        descriptor.name
    ))
}

/// Whether `data_type` is, or arrays, the enum type `oid`.
#[must_use]
pub fn data_type_depends_on_enum(data_type: &DataType, oid: u32) -> bool {
    match data_type {
        DataType::Enum(descriptor) => descriptor.oid == oid,
        DataType::Array(inner) => data_type_depends_on_enum(inner, oid),
        _ => false,
    }
}

impl Catalog {
    pub fn register_enum_type(&self, metadata: EnumTypeMeta) {
        self.enum_types
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_enum_type(&self, name: &str) {
        let mut types = self.enum_types.write();
        let key = name.to_ascii_lowercase();
        if types.remove(&key).is_none() {
            let matching = types
                .iter()
                .find(|(_, metadata)| name_matches(&metadata.name, name))
                .map(|(stored, _)| stored.clone());
            if let Some(stored) = matching {
                types.remove(&stored);
            }
        }
        drop(types);
        self.bump_version();
    }

    #[must_use]
    pub fn get_enum_type(&self, name: &str) -> Option<EnumTypeMeta> {
        let types = self.enum_types.read();
        types.get(&name.to_ascii_lowercase()).cloned().or_else(|| {
            types
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn has_enum_types(&self) -> bool {
        !self.enum_types.read().is_empty()
    }

    #[must_use]
    pub fn enum_type_exists(&self, name: &str) -> bool {
        self.get_enum_type(name).is_some()
    }

    /// The type whose `pg_type.oid` is `oid`.
    #[must_use]
    pub fn get_enum_type_by_oid(&self, oid: u32) -> Option<EnumTypeMeta> {
        self.enum_types
            .read()
            .values()
            .find(|metadata| metadata.type_oid() == oid)
            .cloned()
    }

    /// The type whose array type has `pg_type.oid` `oid`.
    #[must_use]
    pub fn get_enum_type_by_array_oid(&self, oid: u32) -> Option<EnumTypeMeta> {
        self.enum_types
            .read()
            .values()
            .find(|metadata| metadata.array_oid() == oid)
            .cloned()
    }

    #[must_use]
    pub fn list_enum_types(&self) -> Vec<EnumTypeMeta> {
        let mut out = self.enum_types.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn list_enum_types_for_database(&self, database: &str) -> Vec<EnumTypeMeta> {
        self.list_enum_types()
            .into_iter()
            .filter(|metadata| metadata.database.eq_ignore_ascii_case(database))
            .collect()
    }

    /// The enum type, or enum array type, a parameter OID names in `database`.
    #[must_use]
    pub fn enum_parameter_type(&self, oid: i32, database: &str) -> Option<DataType> {
        let oid = u32::try_from(oid).ok()?;
        let in_database =
            |metadata: &EnumTypeMeta| metadata.database.eq_ignore_ascii_case(database);
        if let Some(metadata) = self.get_enum_type_by_oid(oid).filter(in_database) {
            return Some(DataType::Enum(metadata.descriptor()));
        }
        self.get_enum_type_by_array_oid(oid)
            .filter(in_database)
            .map(|metadata| DataType::Array(Box::new(DataType::Enum(metadata.descriptor()))))
    }

    /// Resolves a type name the way a relation name resolves: an unqualified
    /// name searches `search_order`, a qualified name is exact.
    #[must_use]
    pub fn resolve_enum_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<EnumTypeMeta> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.get_enum_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.get_enum_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.get_enum_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Whether an enum type in the same schema (and database, when both names
    /// are canonical) as the relation `name` has its local name. A table, view
    /// or materialized view row type would collide with it (`42710`).
    pub(super) fn enum_owns_relation_name(&self, name: &str) -> bool {
        self.enum_types
            .read()
            .values()
            .any(|metadata| same_stored_relation(&metadata.name, name))
    }

    /// Table columns, materialized views and views that keep the enum type
    /// `oid` alive. Composite fields and domain bases cannot hold an enum in
    /// v0.4. A view counts when one of its output columns has the type; a view
    /// that only casts to it internally is found from its definition by the
    /// `DROP TYPE` command.
    #[must_use]
    pub fn enum_type_dependents(&self, oid: u32) -> Vec<EnumDependent> {
        let mut dependents = Vec::new();
        for schema in self.schemas.read().values() {
            let relation = dependent_relation(&schema.collection);
            for field in &schema.fields {
                if data_type_depends_on_enum(&field.data_type, oid) {
                    dependents.push(EnumDependent {
                        owner: schema.collection.clone(),
                        phrase: format!("column {} of {relation}", field.name),
                        array: matches!(field.data_type, DataType::Array(_)),
                    });
                }
            }
        }
        for view in self.list_views() {
            if let Some(field) = view
                .schema
                .fields
                .iter()
                .find(|field| data_type_depends_on_enum(&field.data_type, oid))
            {
                dependents.push(EnumDependent {
                    owner: view.name.clone(),
                    phrase: format!("view {}", local_name(&view.name)),
                    array: matches!(field.data_type, DataType::Array(_)),
                });
            }
        }
        // Functions and procedures with an argument or result of the type.
        dependents.extend(self.routine_dependents_on_type(oid).into_iter().map(
            |(owner, phrase)| EnumDependent {
                owner,
                phrase,
                array: false,
            },
        ));
        dependents
            .sort_by(|left, right| (&left.owner, &left.phrase).cmp(&(&right.owner, &right.phrase)));
        // Every version of a materialized view's output names the same object.
        dependents.dedup_by(|next, kept| {
            next.phrase == kept.phrase
                && projection_output_stem(&next.owner)
                    .is_some_and(|stem| projection_output_stem(&kept.owner) == Some(stem))
        });
        dependents
    }

    /// Removes every enum type of `database` from the catalog (database drop).
    pub fn unregister_enum_types_for_database(&self, database: &str) {
        self.enum_types
            .write()
            .retain(|_, metadata| !metadata.database.eq_ignore_ascii_case(database));
        self.bump_version();
    }

    /// Enum types declared in `schema` of `database`, for `DROP SCHEMA`
    /// dependency checks.
    #[must_use]
    pub fn enum_types_in_schema(&self, database: &str, schema: &str) -> Vec<EnumTypeMeta> {
        self.list_enum_types()
            .into_iter()
            .filter(|metadata| {
                relation_database_name(&metadata.name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    && metadata.schema_name().eq_ignore_ascii_case(schema)
            })
            .collect()
    }

    /// Clears the in-memory `uncommitted` flag of every member and of every
    /// column and view copy of a descriptor. Transaction publication calls
    /// this after it copies the maps: committed labels are usable by everyone
    /// (`55P04` only guards the open transaction).
    pub fn clear_uncommitted_enum_labels(&self) {
        {
            let mut types = self.enum_types.write();
            for metadata in types.values_mut() {
                for member in &mut metadata.members {
                    member.uncommitted = false;
                }
            }
        }
        let _refreshed = self.reconcile_embedded_enums();
    }
}

#[cfg(test)]
#[path = "enum_types_tests.rs"]
mod tests;
