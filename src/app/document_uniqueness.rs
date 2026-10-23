//! Reconcile committed unique reservation owners with the session's staged view.
use super::{Cassie, CassieError, CassieSession, FieldConstraint, TransactionRowChange};
use crate::catalog::{IndexKind, IndexMeta};
use crate::midge::adapter::Midge;

impl Cassie {
    pub(super) fn validate_uniques(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Map<String, serde_json::Value>,
        constraints: &[FieldConstraint],
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        let schema = self.catalog.get_schema(collection);
        for constraint in constraints {
            if !crate::catalog::enforces_single_column_uniqueness(constraint, constraints) {
                continue;
            }
            let Some(value) = payload.get(&constraint.field) else {
                continue;
            };
            if constraint.unique_null_policy != crate::catalog::UniqueNullPolicy::NotDistinct
                && super::field_value_is_sql_null(Some(value), schema.as_ref(), &constraint.field)
            {
                continue;
            }
            // Only the materialized primary index proves an absent committed owner.
            // Plain UNIQUE constraints retain scan validation until #953 qualifies backing.
            let primary_index = constraint
                .primary_key
                .then(|| {
                    let name = format!("{}_pkey", crate::catalog::local_name(collection));
                    self.catalog
                        .list_indexes(collection)
                        .into_iter()
                        .find(|index| {
                            index.unique
                                && index.kind == IndexKind::Scalar
                                && index.predicate.is_none()
                                && index.normalized_expressions().is_empty()
                                && index.normalized_fields().as_slice()
                                    == std::slice::from_ref(&constraint.field)
                                && crate::catalog::local_name(&index.name) == name
                        })
                })
                .flatten();
            let conflict = if let Some(index) = primary_index {
                let owner = published_owner(self, collection, &index, std::slice::from_ref(value))?;
                has_conflicting_owner(self, session, collection, owner, exclude_id, |candidate| {
                    Ok(candidate.get(&constraint.field) == Some(value))
                })?
            } else {
                self.find_document_id_by_fields(
                    session,
                    collection,
                    &[(&constraint.field, value)],
                    exclude_id,
                )?
                .is_some()
            };
            if conflict {
                return Err(CassieError::UniqueViolation {
                    table: collection.to_owned(),
                    column: constraint.field.clone(),
                    constraint: constraint.unique_constraint_name(collection),
                });
            }
        }
        Ok(())
    }

    pub(super) fn validate_unique_indexes_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_owned())
        })?;
        let schema = self.catalog.get_schema(collection);
        let row_schema = self.midge.row_schema(collection)?;
        for index in self.catalog.list_indexes(collection) {
            if !index.unique || index.kind != IndexKind::Scalar {
                continue;
            }
            let fields = index.normalized_fields();
            let key_matches = |candidate: &serde_json::Value| -> Result<Option<Vec<serde_json::Value>>, CassieError> {
                if !Midge::payload_matches_scalar_index_predicate(&index, candidate, &row_schema)?
                    || (!index.nulls_not_distinct()
                        && fields.iter().any(|field| {
                            super::field_value_is_sql_null(
                                candidate.get(field),
                                schema.as_ref(),
                                field,
                            )
                        }))
                {
                    return Ok(None);
                }
                Midge::scalar_index_key_values(&index, candidate, &row_schema)
            };
            let Some(key) = key_matches(payload)? else {
                continue;
            };
            let owner = published_owner(self, collection, &index, &key)?;
            if has_conflicting_owner(self, session, collection, owner, exclude_id, |candidate| {
                Ok(key_matches(candidate)?.as_deref() == Some(key.as_slice()))
            })? {
                return Err(index.unique_violation(collection));
            }
        }
        Ok(())
    }
}

enum PublishedOwner {
    Unavailable,
    Ready(Option<String>),
}

fn published_owner(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    key: &[serde_json::Value],
) -> Result<PublishedOwner, CassieError> {
    let Some(stored) = cassie.midge.get_index(collection, &index.name)? else {
        return Ok(PublishedOwner::Unavailable);
    };
    if !stored.unique
        || stored.kind != IndexKind::Scalar
        || stored.nulls_not_distinct() != index.nulls_not_distinct()
        || stored.normalized_fields() != index.normalized_fields()
        || stored.normalized_expressions() != index.normalized_expressions()
        || stored.predicate != index.predicate
    {
        return Ok(PublishedOwner::Unavailable);
    }
    cassie
        .midge
        .unique_scalar_index_reservation_owner(collection, index.unique_reservation_name(), key)
        .map(PublishedOwner::Ready)
}

fn has_conflicting_owner(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    committed_owner: PublishedOwner,
    exclude_id: Option<&str>,
    mut matches: impl FnMut(&serde_json::Value) -> Result<bool, CassieError>,
) -> Result<bool, CassieError> {
    let PublishedOwner::Ready(committed_owner) = committed_owner else {
        for document in cassie
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id != Some(document.id.as_str()) && matches(&document.payload)? {
                return Ok(true);
            }
        }
        return Ok(false);
    };
    let snapshot = session.map(|session| session.staged_write_snapshot(collection));
    let changes = snapshot
        .as_ref()
        .map(super::StagedWriteSnapshot::ordered_changes);
    if let Some(owner) = committed_owner
        .as_deref()
        .filter(|owner| exclude_id != Some(*owner))
    {
        match changes.and_then(|changes| changes.get(owner)) {
            None => return Ok(true),
            Some(TransactionRowChange::Upsert(payload)) => {
                if matches(payload)? {
                    return Ok(true);
                }
            }
            Some(TransactionRowChange::Delete) => {}
        }
    }
    if let Some(changes) = changes {
        for (id, change) in changes {
            if exclude_id == Some(id.as_str()) {
                continue;
            }
            if let TransactionRowChange::Upsert(payload) = change {
                if matches(payload)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
