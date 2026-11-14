//! Domain type objects: durable identity, name resolution and dependencies (#828).
use serde::{Deserialize, Serialize};

use super::{
    canonical_relation_name, local_name, name_matches, parse_name, relation_database_name,
    relation_schema_name, Catalog, ParsedName,
};
use crate::types::{DataType, DomainCheck, DomainDefault, DomainRef, DomainType};

/// The persisted layout version of [`DomainTypeMeta`].
pub const DOMAIN_TYPE_FORMAT_VERSION: u32 = 1;

/// One `CREATE DOMAIN` object, scoped to its database.
///
/// `object_id` and `array_object_id` are drawn from the allocator that also
/// issues table and sequence identities, so they never collide with an object
/// of the same database. They back `pg_type.oid` and `pg_type.typarray`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainTypeMeta {
    pub format_version: u32,
    pub database: String,
    /// Canonical `database.schema.name`.
    pub name: String,
    pub object_id: u64,
    pub array_object_id: u64,
    pub owner_oid: u32,
    pub revision: u64,
    pub base: DataType,
    pub not_null: bool,
    pub default: Option<DomainDefault>,
    pub checks: Vec<DomainCheck>,
}

impl DomainTypeMeta {
    /// The immutable descriptor values and columns of this type carry.
    #[must_use]
    pub fn descriptor(&self) -> DomainRef {
        DomainRef::new(DomainType {
            oid: self.type_oid(),
            array_oid: self.array_oid(),
            qualified_name: self.name.clone(),
            name: local_name(&self.name),
            revision: self.revision,
            base: self.base.clone(),
            not_null: self.not_null,
            default: self.default.clone(),
            checks: self.checks.clone(),
        })
    }

    /// `pg_type.oid` of the domain.
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

/// The schema of a relation name stored bare (`public`), schema-qualified or
/// canonical.
fn stored_schema(name: &str) -> String {
    match parse_name(name) {
        Ok(
            ParsedName::SchemaQualified { schema, .. }
            | ParsedName::DatabaseQualified { schema, .. },
        ) => schema,
        _ => super::DEFAULT_SCHEMA.to_owned(),
    }
}

/// Whether two stored relation or type names denote the same object: equal
/// local names in equal schemas, with the database compared only when both
/// names are canonical. Handles bare, `schema.name` and canonical spellings.
#[must_use]
pub fn same_stored_relation(left: &str, right: &str) -> bool {
    local_name(left) == local_name(right)
        && stored_schema(left) == stored_schema(right)
        && match (relation_database_name(left), relation_database_name(right)) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
}

impl Catalog {
    pub fn register_domain_type(&self, metadata: DomainTypeMeta) {
        self.domain_types
            .write()
            .insert(metadata.name.clone(), metadata);
        self.bump_version();
    }

    pub fn unregister_domain_type(&self, name: &str) {
        let mut types = self.domain_types.write();
        let key = name.to_owned();
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
    pub fn get_domain_type(&self, name: &str) -> Option<DomainTypeMeta> {
        let types = self.domain_types.read();
        types.get(name).cloned().or_else(|| {
            types
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn has_domain_types(&self) -> bool {
        !self.domain_types.read().is_empty()
    }

    #[must_use]
    pub fn domain_type_exists(&self, name: &str) -> bool {
        self.get_domain_type(name).is_some()
    }

    /// The domain whose `pg_type.oid` is `oid`.
    #[must_use]
    pub fn get_domain_type_by_oid(&self, oid: u32) -> Option<DomainTypeMeta> {
        self.domain_types
            .read()
            .values()
            .find(|metadata| metadata.type_oid() == oid)
            .cloned()
    }

    /// The domain whose array type has `pg_type.oid` `oid`.
    #[must_use]
    pub fn get_domain_type_by_array_oid(&self, oid: u32) -> Option<DomainTypeMeta> {
        self.domain_types
            .read()
            .values()
            .find(|metadata| metadata.array_oid() == oid)
            .cloned()
    }

    #[must_use]
    pub fn list_domain_types(&self) -> Vec<DomainTypeMeta> {
        let mut out = self
            .domain_types
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn list_domain_types_for_database(&self, database: &str) -> Vec<DomainTypeMeta> {
        self.list_domain_types()
            .into_iter()
            .filter(|metadata| metadata.database.eq_ignore_ascii_case(database))
            .collect()
    }

    /// Resolves a domain name the way a relation name resolves: an unqualified
    /// name searches `search_order`, a qualified name is exact.
    #[must_use]
    pub fn resolve_domain_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<DomainTypeMeta> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.get_domain_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.get_domain_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.get_domain_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Whether a domain in the same schema (and database, when both names are
    /// canonical) as the relation `name` has its local name. Unlike
    /// [`Self::domain_type_exists`] this never matches a same-named domain of
    /// another schema through a bare, unscoped relation name.
    fn domain_owns_relation_name(&self, name: &str) -> bool {
        self.domain_types
            .read()
            .values()
            .any(|metadata| same_stored_relation(&metadata.name, name))
    }

    /// Rejects a relation name already taken by a domain (`42710`), because
    /// the relation's row type would collide with the domain, as in PostgreSQL.
    ///
    /// # Errors
    ///
    /// Returns the diagnostic when a domain owns `name`.
    pub fn reject_domain_relation_name(&self, name: &str) -> Result<(), crate::app::CassieError> {
        if self.domain_owns_relation_name(name) || self.enum_owns_relation_name(name) {
            return Err(crate::app::DiagnosticError::new(
                "42710",
                format!("type \"{}\" already exists", local_name(name)),
            )
            .into());
        }
        Ok(())
    }

    /// Rejects renaming or moving a table, view or materialized view onto a
    /// name its row type cannot take: a domain holds it (`42710`, the type
    /// namespace) or a composite type does (`42P07`, its `pg_class` row), as
    /// PostgreSQL 18 reports each. Sequences and indexes have no row type, so
    /// they never call this.
    ///
    /// # Errors
    ///
    /// Returns the diagnostic when a domain or composite type owns `name`.
    pub fn reject_relation_move_target(&self, name: &str) -> Result<(), crate::app::CassieError> {
        self.reject_domain_relation_name(name)?;
        self.reject_composite_relation_name(name)
    }

    /// Tables that keep the domain `oid` alive, phrased as PostgreSQL phrases
    /// the `DROP DOMAIN` dependency detail (`column c of table t`).
    #[must_use]
    pub fn domain_type_dependents(&self, oid: u32) -> Vec<String> {
        self.domain_type_dependents_with_owner(oid)
            .into_iter()
            .map(|(_, phrase)| phrase)
            .collect()
    }

    /// Like [`Self::domain_type_dependents`], pairing each phrase with the
    /// canonical name of the owning table.
    #[must_use]
    pub fn domain_type_dependents_with_owner(&self, oid: u32) -> Vec<(String, String)> {
        let mut dependents = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if field
                    .domain
                    .as_ref()
                    .is_some_and(|domain| domain.oid == oid)
                {
                    dependents.push((
                        schema.collection.clone(),
                        format!(
                            "column {} of table {}",
                            field.name,
                            local_name(&schema.collection)
                        ),
                    ));
                }
            }
        }
        // A routine taking or returning the domain keeps it alive as well.
        dependents.extend(self.routine_dependents_on_type(oid));
        dependents.sort();
        dependents
    }

    /// The `(collection, column)` pairs declared with the domain `oid`.
    #[must_use]
    pub fn domain_columns(&self, oid: u32) -> Vec<(String, String)> {
        let mut columns = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if field
                    .domain
                    .as_ref()
                    .is_some_and(|domain| domain.oid == oid)
                {
                    columns.push((schema.collection.clone(), field.name.clone()));
                }
            }
        }
        columns.sort();
        columns
    }

    /// Table columns whose stored value embeds the domain `oid` inside a row
    /// type, as `(collection, column)`: a column typed as a table's row type
    /// (directly, nested, or as an array element) when that table has a column
    /// of the domain. `ALTER DOMAIN` cannot validate values stored this way.
    #[must_use]
    pub fn domain_container_columns(&self, oid: u32) -> Vec<(String, String)> {
        let schemas = self.schemas.read().values().cloned().collect::<Vec<_>>();
        let mut columns = Vec::new();
        for schema in &schemas {
            for field in &schema.fields {
                if self.type_embeds_domain(&field.data_type, oid, &mut Vec::new()) {
                    columns.push((schema.collection.clone(), field.name.clone()));
                }
            }
        }
        columns.sort();
        columns
    }

    /// Whether a value of `data_type` holds a value of the domain `oid` in a
    /// row-type member. `visited` breaks cycles between row types.
    fn type_embeds_domain(&self, data_type: &DataType, oid: u32, visited: &mut Vec<u32>) -> bool {
        match data_type {
            DataType::Array(element) => self.type_embeds_domain(element, oid, visited),
            DataType::Composite(embedded) => {
                if visited.contains(&embedded.oid) {
                    return false;
                }
                visited.push(embedded.oid);
                // A column embeds a flattened copy of a table's row type; the
                // live row type still names the domains of its columns.
                let live = self
                    .relation_row_type_by_oid(embedded.oid)
                    .map(|(_, live, _)| live);
                let fields = live.as_ref().map_or(&embedded.fields, |live| &live.fields);
                fields.iter().any(|member| {
                    member
                        .data_type
                        .domain()
                        .is_some_and(|domain| domain.oid == oid)
                        || self.type_embeds_domain(&member.data_type, oid, visited)
                })
            }
            _ => false,
        }
    }

    /// Removes every domain of `database` from the catalog (database drop).
    pub fn unregister_domain_types_for_database(&self, database: &str) {
        self.domain_types
            .write()
            .retain(|_, metadata| !metadata.database.eq_ignore_ascii_case(database));
        self.bump_version();
    }

    /// Domains declared in `schema` of `database`, for `DROP SCHEMA` checks.
    #[must_use]
    pub fn domain_types_in_schema(&self, database: &str, schema: &str) -> Vec<DomainTypeMeta> {
        self.list_domain_types()
            .into_iter()
            .filter(|metadata| {
                relation_database_name(&metadata.name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    && metadata.schema_name().eq_ignore_ascii_case(schema)
            })
            .collect()
    }
}
