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
    canonical_relation_name, canonical_schema_name, local_name, relation_schema_name, Catalog,
    DEFAULT_SCHEMA,
};
use crate::sql::binder::BindingContext;
use crate::types::object_id::{parse_oid_text, FIRST_NORMAL_OBJECT_ID};
use crate::types::{ObjectIdKind, ObjectIdentifierError};

#[path = "object_identity/names.rs"]
mod names;
#[path = "object_identity/routines.rs"]
mod routines;

pub(crate) use names::quote_identifier;
use names::{qualified_name, Identifier};

/// The catalog and name-resolution context a lookup runs in.
pub struct ObjectScope<'a> {
    catalog: &'a Catalog,
    context: &'a BindingContext,
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
        Self { catalog, context }
    }

    /// Resolves `reg*` text input. `Ok(None)` means the object does not exist;
    /// malformed input is an error even for the `to_reg*` functions.
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
        match kind {
            ObjectIdKind::Oid => parse_oid_text(trimmed).map(Some),
            ObjectIdKind::RegClass => self.resolve_relation(trimmed),
            ObjectIdKind::RegType => names::builtin_type_oid(trimmed),
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
            ObjectIdKind::RegProc => routines::resolve_proc(trimmed),
            ObjectIdKind::RegProcedure => routines::resolve_procedure(trimmed),
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
                .map(|row| row.display.clone()),
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
            ObjectIdKind::RegProc => routines::proc_label(oid),
            ObjectIdKind::RegProcedure => routines::procedure_label(oid),
            ObjectIdKind::RegOper => routines::operator_label(oid),
            ObjectIdKind::RegOperator => routines::operator_signature_label(oid),
        };
        label.unwrap_or_else(|| oid.to_string())
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
        if let Some(oid) = stored_relation_oid(self.catalog, &canonical) {
            return Some(RelationIdentity {
                oid,
                schema: schema.to_string(),
                name: name.to_string(),
            });
        }
        self.index_in_schema(schema, name)
    }

    fn index_in_schema(&self, schema: &str, name: &str) -> Option<RelationIdentity> {
        self.catalog
            .logical_indexes_snapshot()
            .into_iter()
            .find_map(|index| {
                let relation = crate::catalog::RelationId::parse_canonical(&index.collection)?;
                if !relation
                    .database
                    .eq_ignore_ascii_case(&self.context.database)
                    || !relation.schema.eq_ignore_ascii_case(schema)
                    || !local_name(&index.name).eq_ignore_ascii_case(name)
                {
                    return None;
                }
                let oid = catalog_index_oid(&index);
                Some(RelationIdentity {
                    oid,
                    schema: relation_schema_name(&index.collection),
                    name: local_name(&index.name),
                })
            })
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
        if let Some(index) =
            self.catalog
                .logical_indexes_snapshot()
                .into_iter()
                .find(|index| {
                    catalog_index_oid(index) == oid
                        && crate::catalog::RelationId::parse_canonical(&index.collection)
                            .is_some_and(|relation| {
                                relation
                                    .database
                                    .eq_ignore_ascii_case(&self.context.database)
                            })
                })
        {
            let relation = crate::catalog::RelationId::parse_canonical(&index.collection)?;
            if !relation
                .database
                .eq_ignore_ascii_case(&self.context.database)
            {
                return None;
            }
            return Some(RelationIdentity {
                oid,
                schema: relation.schema,
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
        let relation = crate::catalog::RelationId::parse_canonical(&name)?;
        if !relation
            .database
            .eq_ignore_ascii_case(&self.context.database)
        {
            return None;
        }
        Some(RelationIdentity {
            oid,
            schema: relation.schema,
            name: relation.name,
        })
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

    fn role_name(&self, oid: u32) -> Option<String> {
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
