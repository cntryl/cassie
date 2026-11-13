//! Text input and output for the `reg*` object identifier types.
//!
//! [`ObjectScope`] resolves names to OIDs against the session's database and
//! `search_path`, and renders OIDs back to names, qualifying a name only when
//! the search path would not find the same object, as PostgreSQL does.
use crate::catalog::builtin_identity::{
    builtin_identities, BOOTSTRAP_ROLE_OID, INFORMATION_SCHEMA_NAMESPACE_OID,
    PG_CATALOG_NAMESPACE_OID, PG_TOAST_NAMESPACE_OID, PUBLIC_NAMESPACE_OID,
};
use crate::catalog::{
    canonical_identifier_component, canonical_relation_name, canonical_schema_name, local_name,
    Catalog, DEFAULT_SCHEMA,
};
use crate::sql::binder::BindingContext;
use crate::types::object_id::{parse_oid_text, FIRST_NORMAL_OBJECT_ID};
use crate::types::{ObjectIdKind, ObjectIdentifierError};

#[path = "object_identity/identify.rs"]
mod identify;
#[path = "object_identity/names.rs"]
mod names;
#[path = "object_identity/routines.rs"]
mod routines;
#[path = "object_identity/user_routines.rs"]
mod user_routines;

pub use identify::{ObjectIdentity, UnsupportedObjectClass};

pub(crate) use names::quote_identifier;
use names::{qualified_name, Identifier};

/// The catalog and name-resolution context a lookup runs in.
pub struct ObjectScope<'a> {
    catalog: &'a Catalog,
    context: &'a BindingContext,
    /// Lowercase names of schemas the role has no `USAGE` on; a qualified
    /// `reg*` name in one of them is `42501`, as in PostgreSQL.
    unusable_schemas: std::collections::HashSet<String>,
}

/// A relation's place in the catalog.
struct RelationIdentity {
    oid: u32,
    schema: String,
    name: String,
}

impl<'a> ObjectScope<'a> {
    #[must_use]
    pub fn new(catalog: &'a Catalog, context: &'a BindingContext) -> Self {
        Self {
            catalog,
            context,
            unusable_schemas: std::collections::HashSet::new(),
        }
    }

    /// Makes qualified names in `schemas` fail with `42501`.
    #[must_use]
    pub fn with_unusable_schemas(mut self, schemas: std::collections::HashSet<String>) -> Self {
        self.unusable_schemas = schemas;
        self
    }

    /// `42501` when `text` qualifies a name with a schema the role cannot use.
    fn check_schema_usage(
        &self,
        kind: ObjectIdKind,
        text: &str,
    ) -> Result<(), ObjectIdentifierError> {
        if self.unusable_schemas.is_empty()
            || !matches!(
                kind,
                ObjectIdKind::RegClass
                    | ObjectIdKind::RegType
                    | ObjectIdKind::RegProc
                    | ObjectIdKind::RegProcedure
            )
        {
            return Ok(());
        }
        let mut quoted = false;
        let end = text
            .char_indices()
            .find(|&(_, character)| {
                if character == '"' {
                    quoted = !quoted;
                }
                !quoted && matches!(character, '(' | '[' | ' ')
            })
            .map_or(text.len(), |(index, _)| index);
        let Ok(parts) = qualified_name(&text[..end]) else {
            return Ok(());
        };
        let Some(schema) = parts
            .len()
            .checked_sub(2)
            .and_then(|index| parts.get(index))
        else {
            return Ok(());
        };
        if self
            .unusable_schemas
            .contains(&schema.value.to_ascii_lowercase())
        {
            return Err(ObjectIdentifierError::PermissionDenied {
                schema: quote_identifier(&schema.value),
            });
        }
        Ok(())
    }

    /// Resolves `reg*` text input. `Ok(None)` means the object does not exist;
    /// malformed input is an error at this resolver boundary. Nullable
    /// `to_reg*` callers apply their documented per-kind error behavior.
    ///
    /// # Errors
    /// Returns PostgreSQL's error for malformed or ambiguous input.
    pub fn resolve(
        &self,
        kind: ObjectIdKind,
        text: &str,
    ) -> Result<Option<u32>, ObjectIdentifierError> {
        let trimmed = text.trim();
        if trimmed == "-" {
            return Ok(Some(0));
        }
        if trimmed
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            return parse_oid_text(trimmed).map(Some);
        }
        self.check_schema_usage(kind, trimmed)?;
        match kind {
            ObjectIdKind::Oid => parse_oid_text(trimmed).map(Some),
            ObjectIdKind::RegClass => self.resolve_relation(trimmed),
            ObjectIdKind::RegType => match names::builtin_type_oid(trimmed)? {
                Some(oid) => Ok(Some(oid)),
                None => Ok(self.resolve_user_type(trimmed)),
            },
            ObjectIdKind::RegNamespace => self.resolve_namespace_text(trimmed),
            ObjectIdKind::RegRole => self.resolve_role(trimmed),
            ObjectIdKind::RegCollation => Ok(resolve_builtin_name(
                trimmed,
                &builtin_identities().collations,
            )?),
            ObjectIdKind::RegConfig => Ok(resolve_builtin_name(
                trimmed,
                &builtin_identities().text_search_configs,
            )?),
            ObjectIdKind::RegDictionary => Ok(resolve_builtin_name(
                trimmed,
                &builtin_identities().text_search_dictionaries,
            )?),
            ObjectIdKind::RegProc => self.resolve_proc(trimmed),
            ObjectIdKind::RegProcedure => self.resolve_procedure(trimmed),
            ObjectIdKind::RegOper => routines::resolve_operator(trimmed),
            ObjectIdKind::RegOperator => routines::resolve_operator_signature(trimmed),
        }
    }

    /// Resolves input for a cast, where a missing object is an error.
    ///
    /// # Errors
    /// Returns PostgreSQL's error for malformed, ambiguous or missing objects.
    pub fn resolve_required(
        &self,
        kind: ObjectIdKind,
        text: &str,
    ) -> Result<u32, ObjectIdentifierError> {
        self.resolve(kind, text)?
            .ok_or_else(|| ObjectIdentifierError::Undefined {
                kind,
                name: text.trim().to_string(),
            })
    }

    /// Renders `reg*` output for `oid`.
    #[must_use]
    pub fn label(&self, kind: ObjectIdKind, oid: u32) -> String {
        if oid == 0 && kind.is_alias() {
            return "-".to_string();
        }
        let label = match kind {
            ObjectIdKind::Oid => None,
            ObjectIdKind::RegClass => self.relation_label(oid),
            ObjectIdKind::RegType => builtin_identities()
                .type_by_oid(oid)
                .map(|row| row.display.clone())
                .or_else(|| self.user_type_label(oid))
                .or_else(|| self.domain_type_label(oid)),
            ObjectIdKind::RegNamespace => {
                self.namespace_name(oid).map(|name| quote_identifier(&name))
            }
            ObjectIdKind::RegRole => self.role_name(oid).map(|name| quote_identifier(&name)),
            ObjectIdKind::RegCollation => builtin_label(oid, &builtin_identities().collations),
            ObjectIdKind::RegConfig => {
                builtin_label(oid, &builtin_identities().text_search_configs)
            }
            ObjectIdKind::RegDictionary => {
                builtin_label(oid, &builtin_identities().text_search_dictionaries)
            }
            ObjectIdKind::RegProc => self.proc_label(oid),
            ObjectIdKind::RegProcedure => self.procedure_label(oid),
            ObjectIdKind::RegOper => routines::operator_label(oid),
            ObjectIdKind::RegOperator => routines::operator_signature_label(oid),
        };
        label.unwrap_or_else(|| oid.to_string())
    }

    /// A user composite type named by `text`, or its `[]` array type.
    fn resolve_user_type(&self, text: &str) -> Option<u32> {
        let search_order = self.context.relation_search_order();
        let resolve = |name: &str| {
            self.catalog
                .resolve_composite_type(&self.context.database, &search_order, name)
        };
        let resolve_row = |name: &str| {
            self.catalog
                .resolve_relation_row_type(&self.context.database, &search_order, name)
        };
        let resolve_domain = |name: &str| {
            self.catalog
                .resolve_domain_type(&self.context.database, &search_order, name)
        };
        if let Some(base) = text.trim_end().strip_suffix("[]") {
            let base = base.trim_end();
            return resolve(base)
                .map(|metadata| metadata.array_oid())
                .or_else(|| resolve_row(base).map(|row| row.array_oid))
                .or_else(|| resolve_domain(base).map(|metadata| metadata.array_oid()));
        }
        resolve(text)
            .map(|metadata| metadata.type_oid())
            .or_else(|| resolve_row(text).map(|row| row.oid))
            .or_else(|| resolve_domain(text).map(|metadata| metadata.type_oid()))
    }

    /// `regtype` output for a user domain type or its array type.
    fn domain_type_label(&self, oid: u32) -> Option<String> {
        let (metadata, suffix) = self.catalog.get_domain_type_by_oid(oid).map_or_else(
            || {
                self.catalog
                    .get_domain_type_by_array_oid(oid)
                    .map(|metadata| (metadata, "[]"))
            },
            |metadata| Some((metadata, "")),
        )?;
        let search_order = self.context.relation_search_order();
        // Like PostgreSQL's TypeIsVisible, the first schema on the search path
        // that holds any type of this name (domain, composite or table row
        // type) decides: the domain is visible only when that schema is its own.
        let local = metadata.local_name();
        let visible = search_order
            .iter()
            .find(|schema| {
                let canonical = canonical_relation_name(&self.context.database, schema, &local);
                self.catalog.domain_type_exists(&canonical)
                    || self.catalog.composite_type_exists(&canonical)
                    || self.catalog.relation_row_type(&canonical).is_some()
            })
            .is_some_and(|schema| schema.eq_ignore_ascii_case(&metadata.schema_name()));
        let name = if visible {
            quote_identifier(&metadata.local_name())
        } else {
            format!(
                "{}.{}",
                quote_identifier(&metadata.schema_name()),
                quote_identifier(&metadata.local_name())
            )
        };
        Some(format!("{name}{suffix}"))
    }

    /// `regtype` output for a user composite type or its array type.
    fn user_type_label(&self, oid: u32) -> Option<String> {
        if let Some(label) = self.relation_row_type_label(oid) {
            return Some(label);
        }
        let (metadata, suffix) = self.catalog.get_composite_type_by_oid(oid).map_or_else(
            || {
                self.catalog
                    .get_composite_type_by_array_oid(oid)
                    .map(|metadata| (metadata, "[]"))
            },
            |metadata| Some((metadata, "")),
        )?;
        let search_order = self.context.relation_search_order();
        let visible = self
            .catalog
            .resolve_composite_type(
                &self.context.database,
                &search_order,
                &quote_identifier(&metadata.local_name()),
            )
            .is_some_and(|found| found.type_oid() == metadata.type_oid());
        let name = if visible {
            quote_identifier(&metadata.local_name())
        } else {
            format!(
                "{}.{}",
                quote_identifier(&metadata.schema_name()),
                quote_identifier(&metadata.local_name())
            )
        };
        Some(format!("{name}{suffix}"))
    }

    /// `regtype` output for a table's row type or its array type.
    fn relation_row_type_label(&self, oid: u32) -> Option<String> {
        let (table, _, is_array) = self.catalog.relation_row_type_by_oid(oid)?;
        let (schema, local) = self.stored_relation_parts(&table)?;
        let search_order = self.context.relation_search_order();
        let visible = self
            .catalog
            .resolve_relation_row_type(
                &self.context.database,
                &search_order,
                &quote_identifier(&local),
            )
            .is_some_and(|found| found.oid == oid || found.array_oid == oid);
        let name = if visible {
            quote_identifier(&local)
        } else {
            format!("{}.{}", quote_identifier(&schema), quote_identifier(&local))
        };
        Some(format!("{name}{}", if is_array { "[]" } else { "" }))
    }

    fn resolve_relation(&self, text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
        let parts = qualified_name(text)?;
        let found = match parts.as_slice() {
            [name] => self
                .context
                .relation_search_order()
                .into_iter()
                .find_map(|schema| self.relation_in_schema(schema, &name.value)),
            [schema, name] => self.relation_in_schema(&schema.value, &name.value),
            [database, schema, name]
                if database.value.eq_ignore_ascii_case(&self.context.database) =>
            {
                self.relation_in_schema(&schema.value, &name.value)
            }
            _ => None,
        };
        Ok(found.map(|relation| relation.oid))
    }

    fn relation_in_schema(&self, schema: &str, name: &str) -> Option<RelationIdentity> {
        if crate::catalog::is_system_schema(schema) {
            let schema = schema.to_ascii_lowercase();
            return builtin_identities()
                .relations
                .iter()
                .find(|relation| relation.namespace == schema && relation.name == name)
                .map(|relation| RelationIdentity {
                    oid: relation.oid,
                    schema: relation.namespace.clone(),
                    name: relation.name.clone(),
                });
        }
        let canonical = canonical_relation_name(&self.context.database, schema, name);
        let stored = stored_relation_oid(self.catalog, &canonical).or_else(|| {
            // An instance without a database catalog stores relations under
            // their unscoped names, whatever the session's binding scope.
            // With databases registered, a bare name could match a relation
            // in another database, so that fallback stays disabled.
            if !self.catalog.list_databases().is_empty() {
                return None;
            }
            let unscoped = Self::unscoped_relation_name(&canonical, schema, name);
            stored_relation_oid(self.catalog, &unscoped)
        });
        if let Some(oid) = stored {
            return Some(RelationIdentity {
                oid,
                schema: schema.to_string(),
                name: name.to_string(),
            });
        }
        self.index_in_schema(schema, name)
            .or_else(|| self.composite_relation_in_schema(schema, name))
    }

    /// The `relkind = 'c'` `pg_class` row a composite type owns.
    fn composite_relation_in_schema(&self, schema: &str, name: &str) -> Option<RelationIdentity> {
        let metadata = self.catalog.get_composite_type(&canonical_relation_name(
            &self.context.database,
            schema,
            name,
        ))?;
        Some(RelationIdentity {
            oid: metadata.relation_oid(),
            schema: metadata.schema_name(),
            name: metadata.local_name(),
        })
    }

    /// The name a table, view or sequence is stored under: the database
    /// canonical name, or the unscoped name of an instance without databases.
    fn stored_relation_name(&self, schema: &str, name: &str) -> String {
        let canonical = canonical_relation_name(&self.context.database, schema, name);
        let exists = |stored: &str| {
            self.catalog.get_collection(stored).is_some()
                || self.catalog.get_view(stored).is_some()
                || self.catalog.get_sequence(stored).is_some()
        };
        if exists(&canonical) || !self.catalog.list_databases().is_empty() {
            return canonical;
        }
        let unscoped = Self::unscoped_relation_name(&canonical, schema, name);
        if exists(&unscoped) {
            unscoped
        } else {
            canonical
        }
    }

    /// The name an unscoped catalog stores a relation under: bare for
    /// `public`, `schema.name` elsewhere, and database-qualified when a
    /// component needs escaping.
    fn unscoped_relation_name(canonical: &str, schema: &str, name: &str) -> String {
        let escaped = |component: &str| canonical_identifier_component(component) != component;
        if escaped(schema) || escaped(name) {
            canonical.to_string()
        } else if schema.eq_ignore_ascii_case(DEFAULT_SCHEMA) {
            name.to_string()
        } else {
            canonical_schema_name(schema, name)
        }
    }

    fn index_in_schema(&self, schema: &str, name: &str) -> Option<RelationIdentity> {
        self.catalog
            .logical_indexes_snapshot()
            .into_iter()
            .find_map(|index| {
                if !self
                    .catalog
                    .is_published_in_postgresql_catalogs(&index, Some(&self.context.database))
                {
                    return None;
                }
                let (index_schema, _) = self.stored_relation_parts(&index.collection)?;
                if !index_schema.eq_ignore_ascii_case(schema)
                    || !local_name(&index.name).eq_ignore_ascii_case(name)
                {
                    return None;
                }
                Some(RelationIdentity {
                    oid: catalog_index_oid(&index),
                    schema: index_schema,
                    name: local_name(&index.name),
                })
            })
    }

    /// The schema and relation name of a stored relation name that belongs to
    /// the session database. Names scoped to another database yield `None`; an
    /// instance without databases stores unscoped `schema.name` or bare names.
    fn stored_relation_parts(&self, stored: &str) -> Option<(String, String)> {
        match crate::catalog::parse_name(stored).ok()? {
            crate::catalog::ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => database
                .eq_ignore_ascii_case(&self.context.database)
                .then_some((schema, name)),
            crate::catalog::ParsedName::SchemaQualified { schema, name }
                if self.catalog.list_databases().is_empty() =>
            {
                Some((schema, name))
            }
            crate::catalog::ParsedName::Unqualified(name)
                if self.catalog.list_databases().is_empty() =>
            {
                Some((DEFAULT_SCHEMA.to_string(), name))
            }
            _ => None,
        }
    }

    fn relation_by_oid(&self, oid: u32) -> Option<RelationIdentity> {
        if oid < FIRST_NORMAL_OBJECT_ID {
            return builtin_identities()
                .relations
                .iter()
                .find(|relation| relation.oid == oid)
                .map(|relation| RelationIdentity {
                    oid,
                    schema: relation.namespace.clone(),
                    name: relation.name.clone(),
                });
        }
        let id = u64::from(oid - FIRST_NORMAL_OBJECT_ID);
        if let Some(metadata) = self
            .catalog
            .list_composite_types_for_database(&self.context.database)
            .into_iter()
            .find(|metadata| metadata.relation_oid() == oid)
        {
            return Some(RelationIdentity {
                oid,
                schema: metadata.schema_name(),
                name: metadata.local_name(),
            });
        }
        if let Some(index) = self
            .catalog
            .logical_indexes_snapshot()
            .into_iter()
            .find(|index| {
                self.catalog
                    .is_published_in_postgresql_catalogs(index, Some(&self.context.database))
                    && catalog_index_oid(index) == oid
                    && self.stored_relation_parts(&index.collection).is_some()
            })
        {
            let (schema, _) = self.stored_relation_parts(&index.collection)?;
            return Some(RelationIdentity {
                oid,
                schema,
                name: local_name(&index.name),
            });
        }
        let name = self
            .catalog
            .list_collections_canonical()
            .into_iter()
            .find(|collection| collection.storage_id == id)
            .map(|collection| collection.name)
            .or_else(|| {
                self.catalog
                    .list_views()
                    .into_iter()
                    .find(|view| view.object_id == id)
                    .map(|view| view.name)
            })
            .or_else(|| {
                self.catalog
                    .list_sequences()
                    .into_iter()
                    .find(|sequence| sequence.object_id == id)
                    .map(|sequence| sequence.name)
            })
            .or_else(|| {
                self.catalog
                    .list_projection_metadata()
                    .into_iter()
                    .find(|projection| {
                        projection.kind == crate::catalog::ProjectionKind::Materialized
                            && projection.materialized.as_ref().is_some_and(
                                crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                            )
                            && stable_materialized_view_oid(projection.projection_id()) == oid
                    })
                    .map(|projection| projection.collection)
            })?;
        let (schema, name) = self.stored_relation_parts(&name)?;
        Some(RelationIdentity { oid, schema, name })
    }

    /// The canonical catalog name of the user relation with `oid`.
    #[must_use]
    pub fn relation_canonical_name(&self, oid: u32) -> Option<String> {
        let relation = self.relation_by_oid(oid)?;
        Some(canonical_relation_name(
            &self.context.database,
            &relation.schema,
            &relation.name,
        ))
    }

    fn relation_label(&self, oid: u32) -> Option<String> {
        let relation = self.relation_by_oid(oid)?;
        let visible = self
            .context
            .relation_search_order()
            .into_iter()
            .find_map(|schema| self.relation_in_schema(schema, &relation.name))
            .is_some_and(|found| found.oid == oid);
        Some(if visible {
            quote_identifier(&relation.name)
        } else {
            format!(
                "{}.{}",
                quote_identifier(&relation.schema),
                quote_identifier(&relation.name)
            )
        })
    }

    fn resolve_namespace_text(&self, text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
        let parts = qualified_name(text)?;
        let [name] = parts.as_slice() else {
            return Err(ObjectIdentifierError::InvalidName(text.to_string()));
        };
        Ok(self.namespace_oid(&name.value))
    }

    /// The OID of schema `name` in the current database.
    #[must_use]
    pub fn namespace_oid(&self, name: &str) -> Option<u32> {
        match name {
            "pg_catalog" => Some(PG_CATALOG_NAMESPACE_OID),
            "pg_toast" => Some(PG_TOAST_NAMESPACE_OID),
            "information_schema" => Some(INFORMATION_SCHEMA_NAMESPACE_OID),
            _ if name.eq_ignore_ascii_case(DEFAULT_SCHEMA) => Some(PUBLIC_NAMESPACE_OID),
            _ => stored_namespace_oid(
                self.catalog,
                &canonical_schema_name(&self.context.database, name),
            ),
        }
    }

    fn namespace_name(&self, oid: u32) -> Option<String> {
        match oid {
            PG_CATALOG_NAMESPACE_OID => Some("pg_catalog".to_string()),
            PG_TOAST_NAMESPACE_OID => Some("pg_toast".to_string()),
            INFORMATION_SCHEMA_NAMESPACE_OID => Some("information_schema".to_string()),
            PUBLIC_NAMESPACE_OID => Some(DEFAULT_SCHEMA.to_string()),
            oid if oid >= FIRST_NORMAL_OBJECT_ID => {
                let id = u64::from(oid - FIRST_NORMAL_OBJECT_ID);
                let namespace = self
                    .catalog
                    .list_namespaces()
                    .into_iter()
                    .find(|namespace| namespace.object_id == id)?;
                let (database, schema) = namespace.name.split_once('.')?;
                database
                    .eq_ignore_ascii_case(&self.context.database)
                    .then(|| schema.trim_matches('"').to_string())
            }
            _ => None,
        }
    }

    fn resolve_role(&self, text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
        let parts = qualified_name(text)?;
        let [name] = parts.as_slice() else {
            return Err(ObjectIdentifierError::InvalidName(text.to_string()));
        };
        Ok(self.role_oid(&name.value))
    }

    /// The OID of role `name`; the bootstrap administrator is OID 10.
    #[must_use]
    pub fn role_oid(&self, name: &str) -> Option<u32> {
        let role = self.catalog.get_role(name)?;
        if role.is_admin && role.object_id == 0 {
            return Some(BOOTSTRAP_ROLE_OID);
        }
        normal_oid(role.object_id)
    }

    pub(crate) fn role_name(&self, oid: u32) -> Option<String> {
        self.catalog
            .list_roles()
            .into_iter()
            .find(|role| {
                if oid == BOOTSTRAP_ROLE_OID {
                    role.is_admin && role.object_id == 0
                } else {
                    normal_oid(role.object_id) == Some(oid)
                }
            })
            .map(|role| role.name)
    }
}

/// The OID of a user table, view or sequence by canonical name.
#[must_use]
pub fn stored_relation_oid(catalog: &Catalog, canonical: &str) -> Option<u32> {
    if let Some(collection) = catalog.get_collection(canonical) {
        normal_oid(collection.storage_id)
    } else if let Some(view) = catalog.get_view(canonical) {
        normal_oid(view.object_id)
    } else if let Some(projection) =
        catalog
            .get_materialized_projection(canonical)
            .filter(|projection| {
                projection.materialized.as_ref().is_some_and(
                    crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                )
            })
    {
        Some(stable_materialized_view_oid(projection.projection_id()))
    } else {
        normal_oid(catalog.get_sequence(canonical)?.object_id)
    }
}

/// Stable catalog OID for a SQL materialized view, keyed by its persisted
/// projection identity so refresh and same-schema rename keep the relation OID.
#[must_use]
fn stable_materialized_view_oid(projection_id: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in "materialized-view:".bytes().chain(projection_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let oid_range = u64::from(u32::MAX - 0x8000_0000);
    u32::try_from(hash % oid_range).unwrap_or(0) | 0x8000_0000
}

/// The stable catalog OID used for an index relation.
#[must_use]
pub(crate) fn stable_index_oid(name: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in "index"
        .bytes()
        .chain(*b":")
        .chain(name.to_ascii_lowercase().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    u32::try_from(200_000_u64 + hash % 800_000).unwrap_or(0)
}

/// The catalog OID of an index, preserving the assigned identity across a rename.
#[must_use]
pub(crate) fn catalog_index_oid(index: &crate::catalog::IndexMeta) -> u32 {
    index
        .catalog_oid()
        .unwrap_or_else(|| stable_index_oid(&index.name))
}

/// The OID of a schema by canonical name, including the fixed system schemas.
#[must_use]
pub fn stored_namespace_oid(catalog: &Catalog, canonical: &str) -> Option<u32> {
    let local = crate::catalog::SchemaId::parse_canonical(canonical)
        .map_or_else(|| canonical.to_string(), |schema| schema.schema);
    match local.as_str() {
        "pg_catalog" => Some(PG_CATALOG_NAMESPACE_OID),
        "pg_toast" => Some(PG_TOAST_NAMESPACE_OID),
        "information_schema" => Some(INFORMATION_SCHEMA_NAMESPACE_OID),
        _ if local.eq_ignore_ascii_case(DEFAULT_SCHEMA) => Some(PUBLIC_NAMESPACE_OID),
        _ => normal_oid(catalog.get_namespace(canonical)?.object_id),
    }
}

/// Object identities are allocated from one counter, offset into
/// PostgreSQL's user OID range.
#[must_use]
pub fn normal_oid(object_id: u64) -> Option<u32> {
    if object_id == 0 {
        return None;
    }
    u32::try_from(object_id)
        .ok()
        .and_then(|id| id.checked_add(FIRST_NORMAL_OBJECT_ID))
}

/// Resolve a SQL type OID using the active database for database-owned types.
#[must_use]
pub(crate) fn type_oid_for_database(
    catalog: &crate::catalog::Catalog,
    database: &str,
    data_type: &crate::types::DataType,
) -> Option<i64> {
    match data_type {
        crate::types::DataType::Vector(_) => catalog
            .get_extension(database, "vector")?
            .vector_type_object_id
            .and_then(normal_oid)
            .map(i64::from),
        _ => Some(data_type.type_oid()),
    }
}

fn resolve_builtin_name(
    text: &str,
    rows: &[crate::catalog::builtin_identity::BuiltinNamedRow],
) -> Result<Option<u32>, ObjectIdentifierError> {
    let parts = qualified_name(text)?;
    let name = match parts.as_slice() {
        [name] => name,
        [Identifier { value, .. }, name] if value == "pg_catalog" => name,
        [_, _] => return Ok(None),
        _ => return Err(ObjectIdentifierError::InvalidName(text.to_string())),
    };
    Ok(rows
        .iter()
        .find(|row| row.name == name.value)
        .map(|row| row.oid))
}

fn builtin_label(
    oid: u32,
    rows: &[crate::catalog::builtin_identity::BuiltinNamedRow],
) -> Option<String> {
    rows.iter()
        .find(|row| row.oid == oid)
        .map(|row| quote_identifier(&row.name))
}

#[cfg(test)]
mod tests {
    use super::{catalog_index_oid, ObjectScope};
    use crate::catalog::{
        canonical_relation_name, Catalog, ExtensionMeta, IndexKind, IndexMeta,
        VectorAnnCatalogIdentityIds, VectorCatalogIdentitySet, VectorDistanceOperatorIds,
    };
    use crate::sql::binder::BindingContext;
    use crate::types::ObjectIdKind;

    fn index(kind: IndexKind, name: &str) -> IndexMeta {
        IndexMeta {
            definition_semantics_version: 0,
            collection: canonical_relation_name("object_identity_test", "public", "events"),
            name: name.to_string(),
            field: "event_at".to_string(),
            fields: vec!["event_at".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind,
            unique: false,
            options: std::collections::BTreeMap::new(),
        }
    }

    fn vector_extension() -> ExtensionMeta {
        let ann_identities = |access_method| VectorAnnCatalogIdentityIds {
            access_method,
            l2_operator_family: access_method + 1,
            inner_product_operator_family: access_method + 2,
            cosine_operator_family: access_method + 3,
            l2_operator_class: access_method + 4,
            inner_product_operator_class: access_method + 5,
            cosine_operator_class: access_method + 6,
        };
        ExtensionMeta {
            format_version: 2,
            database: "object_identity_test".to_string(),
            name: "vector".to_string(),
            version: "0.8.7".to_string(),
            schema: "object_identity_test.public".to_string(),
            owner_oid: 10,
            object_id: 1,
            digest_text_object_id: None,
            digest_bytea_object_id: None,
            vector_type_object_id: Some(2),
            vector_catalog_identities: Some(VectorCatalogIdentitySet {
                operators: VectorDistanceOperatorIds {
                    l2: 3,
                    inner_product: 4,
                    cosine: 5,
                },
                hnsw: ann_identities(10),
                ivfflat: ann_identities(20),
            }),
        }
    }

    #[test]
    fn should_resolve_only_index_relations_published_in_postgresql_catalogs() {
        // Arrange
        let catalog = Catalog::new();
        let column_index = index(IndexKind::Column, "events_column_idx");
        let time_series_index = index(IndexKind::TimeSeries, "events_time_series_idx");
        let scalar_index = index(IndexKind::Scalar, "events_btree_idx");
        let legacy_vector_index = index(IndexKind::Vector, "events_legacy_vector_idx");
        let mut hnsw_index = index(IndexKind::Vector, "events_hnsw_idx");
        hnsw_index.options.insert(
            IndexMeta::ACCESS_METHOD_OPTION.to_string(),
            "hnsw".to_string(),
        );
        catalog.register_index(column_index.clone());
        catalog.register_index(time_series_index.clone());
        catalog.register_index(scalar_index.clone());
        catalog.register_index(legacy_vector_index.clone());
        catalog.register_index(hnsw_index.clone());
        let context = BindingContext::new("object_identity_test", vec!["public".to_string()]);
        let scope = ObjectScope::new(&catalog, &context);

        // Act
        let column_name = scope
            .resolve(ObjectIdKind::RegClass, "events_column_idx")
            .expect("parse Column index identity");
        let time_series_name = scope
            .resolve(ObjectIdKind::RegClass, "events_time_series_idx")
            .expect("parse TimeSeries index identity");
        let scalar_name = scope
            .resolve(ObjectIdKind::RegClass, "events_btree_idx")
            .expect("parse scalar index identity");
        let legacy_vector_name = scope
            .resolve(ObjectIdKind::RegClass, "events_legacy_vector_idx")
            .expect("parse legacy Vector index identity");
        let hnsw_name_before_install = scope
            .resolve(ObjectIdKind::RegClass, "events_hnsw_idx")
            .expect("parse HNSW index identity before extension install");
        let column_label = scope.label(ObjectIdKind::RegClass, catalog_index_oid(&column_index));
        let time_series_label = scope.label(
            ObjectIdKind::RegClass,
            catalog_index_oid(&time_series_index),
        );
        let legacy_vector_label = scope.label(
            ObjectIdKind::RegClass,
            catalog_index_oid(&legacy_vector_index),
        );

        // Install extension identities before resolving the selected pgvector method.
        catalog.register_extension(vector_extension());
        let hnsw_name_after_install = scope
            .resolve(ObjectIdKind::RegClass, "events_hnsw_idx")
            .expect("resolve HNSW only after vector identities are installed");
        let hnsw_label = scope.label(ObjectIdKind::RegClass, catalog_index_oid(&hnsw_index));

        // Assert
        assert_eq!(column_name, None);
        assert_eq!(time_series_name, None);
        assert!(scalar_name.is_some());
        assert_eq!(legacy_vector_name, None);
        assert_eq!(hnsw_name_before_install, None);
        assert_eq!(
            hnsw_name_after_install,
            Some(catalog_index_oid(&hnsw_index))
        );
        assert_eq!(column_label, catalog_index_oid(&column_index).to_string());
        assert_eq!(
            time_series_label,
            catalog_index_oid(&time_series_index).to_string()
        );
        assert_eq!(
            legacy_vector_label,
            catalog_index_oid(&legacy_vector_index).to_string()
        );
        assert_eq!(hnsw_label, "events_hnsw_idx");
    }
}
