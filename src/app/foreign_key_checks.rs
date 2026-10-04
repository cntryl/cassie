use std::collections::{BTreeMap, BTreeSet};

use super::{Cassie, CassieError, CassieSession, FieldConstraint};

const REFERENCE_SCAN_BATCH_SIZE: usize = 1024;

#[derive(Debug, Clone)]
struct ForeignKeyReference {
    column: String,
    constraint_name: Option<String>,
    referenced_table: String,
    referenced_column: String,
    value: serde_json::Value,
}

/// Distinct FOREIGN KEY references gathered from one or more child payloads.
///
/// Each (referenced table, referenced column, value) is kept once, in the order
/// it was first seen, so a batch of child rows checks every parent key only once.
#[derive(Debug, Default)]
pub(crate) struct ForeignKeyReferences {
    seen: BTreeSet<(String, String, crate::types::semantic::SemanticKey)>,
    references: Vec<ForeignKeyReference>,
}

impl ForeignKeyReferences {
    /// Collects the references `payload`, a row of `collection`, makes. A
    /// self-referencing key equal to the row's own referenced value is
    /// satisfied by the row itself and is not collected.
    pub(crate) fn collect(
        &mut self,
        collection: &str,
        constraints: &[FieldConstraint],
        payload: &serde_json::Value,
        schema: Option<&crate::catalog::CollectionSchema>,
    ) -> Result<(), CassieError> {
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;
        for constraint in constraints {
            let (Some(table), Some(field)) = (
                constraint.references_table.as_deref(),
                constraint.references_field.as_deref(),
            ) else {
                continue;
            };
            let Some(value) = object.get(&constraint.field) else {
                continue;
            };
            if super::field_value_is_sql_null(Some(value), schema, &constraint.field) {
                continue;
            }
            let value_key = foreign_key_value_key(value);
            if crate::catalog::name_matches(table, collection)
                && object
                    .get(field)
                    .is_some_and(|own| foreign_key_value_key(own) == value_key)
            {
                continue;
            }
            let key = (table.to_string(), field.to_string(), value_key);
            if self.seen.insert(key) {
                self.references.push(ForeignKeyReference {
                    column: constraint.field.clone(),
                    constraint_name: constraint.foreign_key_name.clone(),
                    referenced_table: table.to_string(),
                    referenced_column: field.to_string(),
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }

    fn len(&self) -> usize {
        self.references.len()
    }

    fn is_empty(&self) -> bool {
        self.references.is_empty()
    }
}

impl Cassie {
    /// Validates existing rows against newly added FOREIGN KEY constraints.
    pub(crate) fn validate_existing_foreign_key_rows(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        if constraints
            .iter()
            .all(|constraint| constraint.references_table.is_none())
        {
            return Ok(());
        }

        let mut references = ForeignKeyReferences::default();
        let schema = self.catalog.get_schema(collection);
        for document in self
            .scan_documents_batched_for_session(None, collection, REFERENCE_SCAN_BATCH_SIZE)?
            .into_iter()
            .flatten()
        {
            references.collect(collection, constraints, &document.payload, schema.as_ref())?;
        }
        self.validate_foreign_key_references(None, collection, &references)
    }

    /// The child's value in the stored form of the referenced column, so it
    /// compares equal to parent keys the row encoder canonicalized on write
    /// (for example TIMESTAMP text or a whole-number FLOAT).
    fn canonical_referenced_value(&self, reference: &ForeignKeyReference) -> serde_json::Value {
        self.catalog
            .get_schema(&reference.referenced_table)
            .and_then(|schema| {
                schema
                    .fields
                    .into_iter()
                    .find(|field| field.name == reference.referenced_column)
            })
            .and_then(|field| {
                crate::midge::adapter::canonical_field_value(&field.data_type, &reference.value)
            })
            .unwrap_or_else(|| reference.value.clone())
    }

    /// Checks that every collected reference has a matching referenced row.
    ///
    /// Pending references are grouped by referenced table and keyed by
    /// (referenced column, value), so each parent row costs one lookup per
    /// distinct referenced column. Each referenced table is scanned at most once.
    /// The scan loads the whole table, so stopping once every key is found only
    /// saves comparisons, not I/O. The first missing reference, in collection
    /// order, is reported.
    pub(crate) fn validate_foreign_key_references(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        references: &ForeignKeyReferences,
    ) -> Result<(), CassieError> {
        if references.is_empty() {
            return Ok(());
        }
        let mut pending_by_table = BTreeMap::<
            &str,
            BTreeMap<(&str, crate::types::semantic::SemanticKey), Vec<usize>>,
        >::new();
        for (index, reference) in references.references.iter().enumerate() {
            let value = self.canonical_referenced_value(reference);
            pending_by_table
                .entry(reference.referenced_table.as_str())
                .or_default()
                .entry((
                    reference.referenced_column.as_str(),
                    foreign_key_value_key(&value),
                ))
                .or_default()
                .push(index);
        }

        let mut found = vec![false; references.len()];
        for (table, mut pending) in pending_by_table {
            let columns = pending
                .keys()
                .map(|(column, _)| *column)
                .collect::<BTreeSet<_>>();
            let batches =
                self.scan_documents_batched_for_session(session, table, REFERENCE_SCAN_BATCH_SIZE)?;
            'documents: for document in batches.into_iter().flatten() {
                for column in &columns {
                    let Some(value) = document.payload.get(*column) else {
                        continue;
                    };
                    if let Some(hits) = pending.remove(&(*column, foreign_key_value_key(value))) {
                        for index in hits {
                            found[index] = true;
                        }
                        if pending.is_empty() {
                            break 'documents;
                        }
                    }
                }
            }
        }

        let Some(missing) = references
            .references
            .iter()
            .zip(&found)
            .find_map(|(reference, found)| (!found).then_some(reference))
        else {
            return Ok(());
        };
        Err(CassieError::ForeignKeyViolation {
            table: collection.to_string(),
            column: missing.column.clone(),
            constraint: missing.constraint_name.clone().unwrap_or_else(|| {
                crate::catalog::generated_constraint_name(
                    collection,
                    &missing.column,
                    "FOREIGN KEY",
                )
            }),
            referenced_table: missing.referenced_table.clone(),
            referenced_column: missing.referenced_column.clone(),
        })
    }
}

pub(super) fn foreign_key_value_key(
    value: &serde_json::Value,
) -> crate::types::semantic::SemanticKey {
    let value = match value {
        serde_json::Value::Null => crate::types::Value::Null,
        serde_json::Value::Bool(value) => crate::types::Value::Bool(*value),
        serde_json::Value::Number(value) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .map_or_else(
                || {
                    value.as_f64().map_or_else(
                        || crate::types::Value::Json(serde_json::Value::Number(value.clone())),
                        crate::types::Value::Float64,
                    )
                },
                crate::types::Value::Int64,
            ),
        serde_json::Value::String(value) => crate::types::Value::String(value.clone()),
        value => crate::types::Value::Json(value.clone()),
    };
    crate::types::semantic::SemanticKey::single(&value)
}

#[cfg(test)]
mod tests {
    use super::ForeignKeyReferences;
    use crate::catalog::FieldConstraint;

    fn reference_constraint() -> FieldConstraint {
        serde_json::from_value(serde_json::json!({
            "field": "parent_pid",
            "references_table": "parents",
            "references_field": "pid"
        }))
        .expect("foreign key constraint")
    }

    #[test]
    fn should_collect_each_referenced_parent_key_once() {
        // Arrange
        let constraints = vec![reference_constraint()];
        let payloads = [
            serde_json::json!({"parent_pid": 1}),
            serde_json::json!({"parent_pid": 1}),
            serde_json::json!({"parent_pid": 2}),
            serde_json::json!({"parent_pid": null}),
        ];
        let mut references = ForeignKeyReferences::default();

        // Act
        for payload in &payloads {
            references
                .collect("children", &constraints, payload, None)
                .expect("collect references");
        }

        // Assert
        assert_eq!(references.len(), 2);
    }
}
