use std::collections::{BTreeMap, BTreeSet};

use super::foreign_key_checks::{foreign_key_value_key, ForeignKeyReferences};
use super::{Cassie, CassieError, CassieSession, FieldConstraint, TransactionRowChange};

const REFERENCE_SCAN_BATCH_SIZE: usize = 1024;

/// Referenced key values a transaction removes from one parent collection,
/// grouped by the child constraint that could still reference them.
type RemovedParentKeys = BTreeMap<
    (String, String),
    (
        FieldConstraint,
        BTreeSet<crate::types::semantic::SemanticKey>,
    ),
>;

impl Cassie {
    fn canonical_referential_name(&self, name: &str) -> String {
        self.catalog
            .get_schema(name)
            .map_or_else(|| name.to_string(), |schema| schema.collection)
    }

    pub(crate) fn referential_write_collections(&self, collection: &str) -> Vec<String> {
        let canonical_name = |name: &str| self.canonical_referential_name(name);
        let collection = canonical_name(collection);
        let mut collections = vec![collection.clone()];
        let mut pending = vec![collection];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            for constraint in self.catalog.get_constraints(&current) {
                if let Some(referenced_table) = constraint.references_table {
                    let referenced_table = canonical_name(&referenced_table);
                    collections.push(referenced_table.clone());
                    pending.push(referenced_table);
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
                                canonical_name(referenced).eq_ignore_ascii_case(&current)
                            })
                    })
                {
                    collections.push(candidate.name.clone());
                    pending.push(candidate.name);
                }
            }
        }
        collections
    }

    /// Returns the full FOREIGN KEY-connected collection set for a transaction
    /// commit, and whether any staged collection takes part in a FOREIGN KEY.
    ///
    /// `referential_write_collections` returns only the collection itself when it
    /// has no foreign keys. A self-reference also adds the collection again, so a
    /// longer list means the commit needs the referential gate.
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
            let schema = self.catalog.get_schema(&collection);
            for change in session.collection_changes(&collection).values() {
                if let TransactionRowChange::Upsert(payload) = change {
                    references.collect(&collection, &constraints, payload, schema.as_ref())?;
                }
            }
            self.validate_foreign_key_references(Some(session), &collection, &references)?;
        }
        Ok(())
    }

    /// Re-checks staged parent deletes and referenced-key updates at commit time.
    ///
    /// A statement inside an explicit transaction checks for referencing child
    /// rows when it stages a parent delete or key change, but another session can
    /// commit a new child for that key before COMMIT. Callers must hold the
    /// referential write gates so no child can be written between this check and
    /// the commit. A removed key that the transaction also restores (for example
    /// by re-inserting the parent) is not a violation.
    pub(crate) fn validate_staged_parent_references(
        &self,
        session: &CassieSession,
    ) -> Result<(), CassieError> {
        for collection in session.transaction_write_collections() {
            let removed = self.removed_parent_keys(session, &collection)?;
            if removed.is_empty() {
                continue;
            }
            let remaining = self.session_parent_keys(session, &collection, &removed)?;
            for ((child_table, _), (constraint, values)) in removed {
                let Some(referenced_column) = constraint.references_field.as_deref() else {
                    continue;
                };
                let orphaned = values
                    .iter()
                    .filter(|value| {
                        !remaining.contains(&(referenced_column.to_string(), (*value).clone()))
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if orphaned.is_empty() {
                    continue;
                }
                if self.child_references_any(session, &child_table, &constraint.field, &orphaned)? {
                    return Err(CassieError::ForeignKeyViolation {
                        constraint: constraint.foreign_key_constraint_name(&child_table),
                        table: child_table,
                        column: constraint.field.clone(),
                        referenced_table: collection,
                        referenced_column: referenced_column.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn referencing_constraints(&self, collection: &str) -> Vec<(String, FieldConstraint)> {
        let mut out = Vec::new();
        for candidate in self.catalog.list_collections_canonical() {
            for constraint in self.catalog.get_constraints(&candidate.name) {
                let references_collection =
                    constraint
                        .references_table
                        .as_deref()
                        .is_some_and(|referenced| {
                            self.canonical_referential_name(referenced)
                                .eq_ignore_ascii_case(collection)
                        });
                if references_collection && constraint.references_field.is_some() {
                    out.push((candidate.name.clone(), constraint));
                }
            }
        }
        out
    }

    /// Collects the committed referenced-key values that staged deletes and
    /// key-changing updates remove from `collection`.
    fn removed_parent_keys(
        &self,
        session: &CassieSession,
        collection: &str,
    ) -> Result<RemovedParentKeys, CassieError> {
        let referencing = self.referencing_constraints(collection);
        let schema = self.catalog.get_schema(collection);
        let mut removed = RemovedParentKeys::new();
        if referencing.is_empty() {
            return Ok(removed);
        }
        for (row_id, change) in session.collection_changes(collection) {
            let Some(committed) = self.midge.get_document(collection, &row_id)? else {
                continue;
            };
            for (child_table, constraint) in &referencing {
                let Some(referenced_column) = constraint.references_field.as_deref() else {
                    continue;
                };
                let Some(old_value) = committed.payload.get(referenced_column) else {
                    continue;
                };
                if super::field_value_is_sql_null(
                    Some(old_value),
                    schema.as_ref(),
                    referenced_column,
                ) {
                    continue;
                }
                let key_removed = match &change {
                    TransactionRowChange::Delete => true,
                    TransactionRowChange::Upsert(payload) => {
                        payload.get(referenced_column).is_none_or(|value| {
                            foreign_key_value_key(value) != foreign_key_value_key(old_value)
                        })
                    }
                };
                if key_removed {
                    removed
                        .entry((child_table.clone(), constraint.field.clone()))
                        .or_insert_with(|| (constraint.clone(), BTreeSet::new()))
                        .1
                        .insert(foreign_key_value_key(old_value));
                }
            }
        }
        Ok(removed)
    }

    /// Returns the (column, value) pairs among the removed keys that the
    /// transaction's own view of the parent collection still contains.
    fn session_parent_keys(
        &self,
        session: &CassieSession,
        collection: &str,
        removed: &RemovedParentKeys,
    ) -> Result<BTreeSet<(String, crate::types::semantic::SemanticKey)>, CassieError> {
        let columns = removed
            .values()
            .filter_map(|(constraint, _)| constraint.references_field.clone())
            .collect::<BTreeSet<_>>();
        let mut remaining = BTreeSet::new();
        let batches = self.scan_documents_batched_for_session(
            Some(session),
            collection,
            REFERENCE_SCAN_BATCH_SIZE,
        )?;
        for document in batches.into_iter().flatten() {
            for column in &columns {
                if let Some(value) = document.payload.get(column) {
                    remaining.insert((column.clone(), foreign_key_value_key(value)));
                }
            }
        }
        Ok(remaining)
    }

    fn child_references_any(
        &self,
        session: &CassieSession,
        child_table: &str,
        child_field: &str,
        values: &BTreeSet<crate::types::semantic::SemanticKey>,
    ) -> Result<bool, CassieError> {
        let batches = self.scan_documents_batched_for_session(
            Some(session),
            child_table,
            REFERENCE_SCAN_BATCH_SIZE,
        )?;
        Ok(batches.into_iter().flatten().any(|document| {
            document
                .payload
                .get(child_field)
                .is_some_and(|value| values.contains(&foreign_key_value_key(value)))
        }))
    }
}
