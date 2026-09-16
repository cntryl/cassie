use std::collections::{BTreeMap, BTreeSet};

use super::{Cassie, CassieError, CassieSession, FieldConstraint};

const REFERENCE_SCAN_BATCH_SIZE: usize = 1024;

#[derive(Debug, Clone)]
struct ForeignKeyReference {
    column: String,
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
    seen: BTreeSet<(String, String, String)>,
    references: Vec<ForeignKeyReference>,
}

impl ForeignKeyReferences {
    pub(crate) fn collect(
        &mut self,
        constraints: &[FieldConstraint],
        payload: &serde_json::Value,
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
            if value.is_null() {
                continue;
            }
            let key = (table.to_string(), field.to_string(), value.to_string());
            if self.seen.insert(key) {
                self.references.push(ForeignKeyReference {
                    column: constraint.field.clone(),
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
    /// Checks that every collected reference has a matching referenced row.
    ///
    /// Each referenced table is scanned at most once, and the scan stops as soon
    /// as all of its pending references are found. The first missing reference,
    /// in collection order, is reported.
    pub(crate) fn validate_foreign_key_references(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        references: &ForeignKeyReferences,
    ) -> Result<(), CassieError> {
        if references.is_empty() {
            return Ok(());
        }
        let mut pending_by_table = BTreeMap::<&str, Vec<usize>>::new();
        for (index, reference) in references.references.iter().enumerate() {
            pending_by_table
                .entry(reference.referenced_table.as_str())
                .or_default()
                .push(index);
        }

        let mut found = vec![false; references.len()];
        for (table, mut pending) in pending_by_table {
            let batches =
                self.scan_documents_batched_for_session(session, table, REFERENCE_SCAN_BATCH_SIZE)?;
            for document in batches.into_iter().flatten() {
                pending.retain(|&index| {
                    let reference = &references.references[index];
                    let matched = document.payload.get(&reference.referenced_column)
                        == Some(&reference.value);
                    if matched {
                        found[index] = true;
                    }
                    !matched
                });
                if pending.is_empty() {
                    break;
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
            constraint: crate::catalog::generated_constraint_name(
                collection,
                &missing.column,
                "FOREIGN KEY",
            ),
            referenced_table: missing.referenced_table.clone(),
            referenced_column: missing.referenced_column.clone(),
        })
    }
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
                .collect(&constraints, payload)
                .expect("collect references");
        }

        // Assert
        assert_eq!(references.len(), 2);
    }
}
