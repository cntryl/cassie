use super::foreign_key_checks::ForeignKeyReferences;
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

    /// Returns the collections to gate for a transaction commit, and whether any
    /// staged collection takes part in a FOREIGN KEY in either direction.
    ///
    /// `referential_write_collections` returns only the collection itself when it
    /// neither references nor is referenced by another table (a self-reference
    /// lists it twice), so a longer list means the commit needs the referential gate.
    pub(crate) fn transaction_commit_write_gates(
        &self,
        session: &CassieSession,
    ) -> (Vec<String>, bool) {
        let mut collections = Vec::new();
        let mut referential = false;
        for collection in session.transaction_write_collections() {
            let linked = self.referential_write_collections(&collection);
            referential |= linked.len() > 1;
            collections.extend(linked);
        }
        (collections, referential)
    }

    /// Re-checks FOREIGN KEY references for staged upserts at commit time.
    ///
    /// Statements inside an explicit transaction validate references when they
    /// stage rows, but a referenced row can be deleted by another session before
    /// COMMIT. Callers must hold the referential write gates for the staged
    /// collections so the referenced rows cannot change between this check and the
    /// commit. Each distinct referenced key is checked once per collection.
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
            let mut references = ForeignKeyReferences::default();
            for change in session.collection_changes(&collection).values() {
                if let TransactionRowChange::Upsert(payload) = change {
                    references.collect(&constraints, payload)?;
                }
            }
            self.validate_foreign_key_references(Some(session), &collection, &references)?;
        }
        Ok(())
    }
}
