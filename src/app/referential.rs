use super::{Cassie, CassieError, CassieSession, TransactionRowChange};

impl Cassie {
    pub(crate) fn referential_write_collections(&self, collection: &str) -> Vec<String> {
        let canonical_name = |name: &str| {
            self.catalog
                .get_schema(name)
                .map_or_else(|| name.to_string(), |schema| schema.collection)
        };
        let collection = canonical_name(collection);
        let mut collections = vec![collection.clone()];
        for constraint in self.catalog.get_constraints(&collection) {
            if let Some(referenced_table) = constraint.references_table {
                collections.push(canonical_name(&referenced_table));
            }
        }
        for candidate in self.catalog.list_collections_canonical() {
            if self
                .catalog
                .get_constraints(&candidate.name)
                .iter()
                .any(|constraint| {
                    constraint
                        .references_table
                        .as_deref()
                        .is_some_and(|referenced| {
                            canonical_name(referenced).eq_ignore_ascii_case(&collection)
                        })
                })
            {
                collections.push(candidate.name);
            }
        }
        collections
    }

    /// Returns the gated collections for every collection staged in a session transaction.
    pub(crate) fn transaction_referential_write_collections(
        &self,
        session: &CassieSession,
    ) -> Vec<String> {
        session
            .transaction_write_collections()
            .iter()
            .flat_map(|collection| self.referential_write_collections(collection))
            .collect()
    }

    /// Re-checks FOREIGN KEY references for staged upserts at commit time.
    ///
    /// Statements inside an explicit transaction validate references when they
    /// stage rows, but a referenced row can be deleted by another session before
    /// COMMIT. Callers must hold the referential write gates for the staged
    /// collections so the referenced rows cannot change between this check and the
    /// commit.
    pub(crate) fn validate_staged_foreign_keys(
        &self,
        session: &CassieSession,
    ) -> Result<(), CassieError> {
        for collection in session.transaction_write_collections() {
            let constraints = self.catalog.get_constraints(&collection);
            if constraints
                .iter()
                .all(|constraint| constraint.references_table.is_none())
            {
                continue;
            }
            for change in session.collection_changes(&collection).into_values() {
                if let TransactionRowChange::Upsert(payload) = change {
                    self.validate_foreign_keys_for_session(
                        Some(session),
                        &collection,
                        &payload,
                        &constraints,
                    )?;
                }
            }
        }
        Ok(())
    }
}
