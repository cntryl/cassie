pub use cassie_core::catalog::composite_type::{data_type_depends_on, CompositeTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION};
use super::{name_matches, Catalog};
impl Catalog {
    pub fn register_composite_type(&self, metadata: CompositeTypeMeta) {
        self.composite_types
            .write()
            .insert(metadata.name.clone(), metadata);
        self.bump_version();
    }

    pub fn unregister_composite_type(&self, name: &str) {
        let mut types = self.composite_types.write();
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
    pub fn get_composite_type(&self, name: &str) -> Option<CompositeTypeMeta> {
        let types = self.composite_types.read();
        types.get(name).cloned().or_else(|| {
            types
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn has_composite_types(&self) -> bool {
        !self.composite_types.read().is_empty()
    }

    /// A composite type owns a `pg_class` row, so creating a view, sequence,
    /// index or table of the same name is `42P07`, as in PostgreSQL.
    ///
    /// # Errors
    /// Returns `42P07` when a composite type holds `name`.
    pub fn reject_composite_relation_name(
        &self,
        name: &str,
    ) -> Result<(), crate::app::CassieError> {
        if self.composite_type_exists(name) {
            return Err(crate::app::DiagnosticError::new(
                "42P07",
                format!("relation \"{}\" already exists", local_name(name)),
            )
            .into());
        }
        Ok(())
    }

    #[must_use]
    pub fn composite_type_exists(&self, name: &str) -> bool {
        self.get_composite_type(name).is_some()
    }

    /// The type whose `pg_type.oid` is `oid`.
    #[must_use]
    pub fn get_composite_type_by_oid(&self, oid: u32) -> Option<CompositeTypeMeta> {
        self.composite_types
            .read()
            .values()
            .find(|metadata| metadata.type_oid() == oid)
            .cloned()
    }

    /// The type whose array type has `pg_type.oid` `oid`.
    #[must_use]
    pub fn get_composite_type_by_array_oid(&self, oid: u32) -> Option<CompositeTypeMeta> {
        self.composite_types
            .read()
            .values()
            .find(|metadata| metadata.array_oid() == oid)
            .cloned()
    }

    #[must_use]
    pub fn list_composite_types(&self) -> Vec<CompositeTypeMeta> {
        let mut out = self
            .composite_types
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn list_composite_types_for_database(&self, database: &str) -> Vec<CompositeTypeMeta> {
        self.list_composite_types()
            .into_iter()
            .filter(|metadata| metadata.database.eq_ignore_ascii_case(database))
            .collect()
    }

    /// Resolves a type name the way a relation name resolves: an unqualified
    /// name searches `search_order`, a qualified name is exact.
    #[must_use]
    pub fn resolve_composite_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<CompositeTypeMeta> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.get_composite_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.get_composite_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.get_composite_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Objects that keep the type `oid` alive, phrased as PostgreSQL phrases the
    /// `DROP TYPE` dependency detail (`column c of table t`).
    #[must_use]
    pub fn composite_type_dependents(&self, oid: u32) -> Vec<String> {
        self.composite_type_dependents_with_owner(oid)
            .into_iter()
            .map(|(_, phrase)| phrase)
            .collect()
    }

    /// Like [`Self::composite_type_dependents`], pairing each phrase with the
    /// canonical name of the owning table or type.
    #[must_use]
    pub fn composite_type_dependents_with_owner(&self, oid: u32) -> Vec<(String, String)> {
        let mut dependents = Vec::new();
        for metadata in self.composite_types.read().values() {
            if metadata.type_oid() == oid {
                continue;
            }
            for field in &metadata.fields {
                if data_type_depends_on(&field.data_type, oid) {
                    dependents.push((
                        metadata.name.clone(),
                        format!(
                            "column {} of composite type {}",
                            field.name,
                            metadata.local_name()
                        ),
                    ));
                }
            }
        }
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if data_type_depends_on(&field.data_type, oid) {
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
        // A routine taking or returning the type keeps it alive as well.
        dependents.extend(self.routine_dependents_on_type(oid));
        dependents.sort();
        dependents
    }

    /// Removes every type of `database` from the catalog (database drop).
    pub fn unregister_composite_types_for_database(&self, database: &str) {
        self.composite_types
            .write()
            .retain(|_, metadata| !metadata.database.eq_ignore_ascii_case(database));
        self.bump_version();
    }

    /// Types declared in `schema` of `database`, for `DROP SCHEMA` dependency checks.
    #[must_use]
    pub fn composite_types_in_schema(
        &self,
        database: &str,
        schema: &str,
    ) -> Vec<CompositeTypeMeta> {
        self.list_composite_types()
            .into_iter()
            .filter(|metadata| {
                relation_database_name(&metadata.name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    && metadata.schema_name().eq_ignore_ascii_case(schema)
            })
            .collect()
    }
}
