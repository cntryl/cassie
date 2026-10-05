use std::collections::{HashSet, VecDeque};
use std::mem::size_of;

use crate::app::CassieError;
use crate::midge::row_blob::RowSchema;
use crate::runtime::accounted::json;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;

use super::DocumentRef;

/// One decoded storage row coupled to the query-memory reservation that owns it.
///
/// The wrapper is intentionally move-only. A merge cursor can retain a persisted lookahead row
/// without creating an accounting gap, then move both the row and its reservation downstream.
pub(crate) struct AccountedDocument {
    document: DocumentRef,
    reservation: QueryMemoryReservation,
}

impl AccountedDocument {
    pub(crate) fn try_build(
        controls: &QueryExecutionControls,
        retained_bytes: usize,
        build: impl FnOnce() -> Result<DocumentRef, CassieError>,
    ) -> Result<Self, CassieError> {
        Self::try_build_optional(controls, retained_bytes, || build().map(Some))?.ok_or_else(|| {
            CassieError::Execution("accounted document builder returned no document".to_owned())
        })
    }

    pub(super) fn try_build_optional(
        controls: &QueryExecutionControls,
        retained_bytes: usize,
        build: impl FnOnce() -> Result<Option<DocumentRef>, CassieError>,
    ) -> Result<Option<Self>, CassieError> {
        let reservation = controls.reserve_query_memory(retained_bytes)?;
        let Some(document) = build()? else {
            return Ok(None);
        };
        Self::from_reserved_document(document, reservation).map(Some)
    }

    pub(super) fn from_reserved_document(
        document: DocumentRef,
        reservation: QueryMemoryReservation,
    ) -> Result<Self, CassieError> {
        let actual_bytes = Self::estimated_retained_bytes(&document)?;
        Self::from_reserved_bytes(document, reservation, actual_bytes)
    }

    pub(crate) fn try_build_fresh_decode_optional(
        controls: &QueryExecutionControls,
        retained_bytes: usize,
        build: impl FnOnce() -> Result<Option<DocumentRef>, CassieError>,
    ) -> Result<Option<Self>, CassieError> {
        let reservation = controls.reserve_query_memory(retained_bytes)?;
        let Some(document) = build()? else {
            return Ok(None);
        };
        Self::from_reserved_decoded_document(document, reservation).map(Some)
    }

    pub(super) fn from_reserved_decoded_document(
        document: DocumentRef,
        reservation: QueryMemoryReservation,
    ) -> Result<Self, CassieError> {
        let actual_bytes = checked_sum(&[
            size_of::<AccountedDocument>(),
            document.id.len(),
            json::retained_decoded_bytes(&document.payload)?,
        ])?;
        Self::from_reserved_bytes(document, reservation, actual_bytes)
    }

    fn from_reserved_bytes(
        document: DocumentRef,
        mut reservation: QueryMemoryReservation,
        actual_bytes: usize,
    ) -> Result<Self, CassieError> {
        if actual_bytes > reservation.bytes() {
            reservation.try_grow(actual_bytes - reservation.bytes())?;
        } else {
            reservation.shrink_to(actual_bytes);
        }
        Ok(Self {
            document,
            reservation,
        })
    }

    #[must_use]
    pub(crate) const fn document(&self) -> &DocumentRef {
        &self.document
    }

    #[must_use]
    pub(crate) fn id(&self) -> &str {
        &self.document().id
    }

    #[must_use]
    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.reservation.bytes()
    }

    /// Estimates the bytes retained by a staged or already-decoded document.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when the retained-size calculation overflows.
    pub(crate) fn estimated_retained_bytes(document: &DocumentRef) -> Result<usize, CassieError> {
        document_retained_bytes(document)
    }

    #[must_use]
    pub(crate) fn into_parts(self) -> (DocumentRef, QueryMemoryReservation) {
        (self.document, self.reservation)
    }

    pub(super) fn into_unaccounted_document(self) -> DocumentRef {
        let (document, reservation) = self.into_parts();
        drop(reservation);
        document
    }
}

impl std::fmt::Debug for AccountedDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountedDocument")
            .field("document", &self.document)
            .field("reservation", &self.reservation)
            .field("accounted_bytes", &self.accounted_bytes())
            .finish()
    }
}

/// A bounded storage page whose rows retain their individual reservations when popped.
#[derive(Debug, Default)]
pub(crate) struct AccountedDocumentPage {
    documents: VecDeque<AccountedDocument>,
    has_more: bool,
}

impl AccountedDocumentPage {
    pub(super) fn from_documents(documents: Vec<AccountedDocument>, has_more: bool) -> Self {
        Self {
            documents: documents.into(),
            has_more,
        }
    }

    #[must_use]
    pub(crate) fn pop_document(&mut self) -> Option<AccountedDocument> {
        self.documents.pop_front()
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.documents.len()
    }

    #[must_use]
    pub(crate) const fn has_more(&self) -> bool {
        self.has_more
    }
}

pub(crate) fn provisional_document_bytes(
    row_schema: &RowSchema,
    projection: Option<&HashSet<String>>,
    include_historical_aliases: bool,
    raw_key_bytes: usize,
    raw_value_bytes: usize,
) -> Result<usize, CassieError> {
    let expansion_factor = row_schema
        .fields
        .iter()
        .map(|field| {
            let output_names =
                decoded_field_name_count(field, projection, include_historical_aliases);
            checked_mul(decode_expansion_factor(&field.data_type), output_names)
        })
        .try_fold(0usize, |largest, factor| {
            factor.map(|factor| largest.max(factor))
        })?;
    let decoded_payload = checked_mul(raw_value_bytes, expansion_factor)?;
    let field_overhead = row_schema.fields.iter().try_fold(0usize, |bytes, field| {
        let output_names = decoded_field_name_count(field, projection, include_historical_aliases);
        if output_names == 0 {
            return Ok(bytes);
        }
        let aliases = if include_historical_aliases {
            field
                .aliases
                .iter()
                .filter(|alias| {
                    projection.is_some_and(|fields| {
                        fields.contains(
                            &crate::sql::ColumnIdentifierPath::from_field_name(alias).lookup_key(),
                        )
                    })
                })
                .try_fold(0usize, |total, alias| checked_add(total, alias.len()))?
        } else {
            0
        };
        let entry_overhead = checked_mul(json::JSON_MAP_ENTRY_BYTES, output_names)?;
        checked_add(
            bytes,
            entry_overhead
                .saturating_add(field.name.len())
                .saturating_add(aliases),
        )
    })?;
    checked_sum(&[
        size_of::<AccountedDocument>(),
        raw_key_bytes,
        decoded_payload,
        size_of::<serde_json::Value>(),
        field_overhead,
    ])
}

fn decoded_field_name_count(
    field: &crate::midge::row_blob::RowFieldMeta,
    projection: Option<&HashSet<String>>,
    include_historical_aliases: bool,
) -> usize {
    let Some(projection) = projection else {
        return usize::from(!field.retired);
    };
    let current_key = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
    let current_name = usize::from(!field.retired && projection.contains(&current_key));
    let aliases = if include_historical_aliases {
        field
            .aliases
            .iter()
            .filter(|alias| {
                projection.contains(
                    &crate::sql::ColumnIdentifierPath::from_field_name(alias).lookup_key(),
                )
            })
            .count()
    } else {
        0
    };
    current_name.saturating_add(aliases)
}

pub(super) fn decode_expansion_factor(data_type: &DataType) -> usize {
    match data_type {
        DataType::Array(inner) => {
            let inner_factor = decode_expansion_factor(inner);
            if inner_factor > 40 {
                inner_factor
            } else {
                40
            }
        }
        DataType::Json => json::JSON_DECODE_EXPANSION_FACTOR,
        DataType::Vector(_) => 10,
        DataType::Bytea => 3,
        _ => 2,
    }
}

fn document_retained_bytes(document: &DocumentRef) -> Result<usize, CassieError> {
    checked_sum(&[
        size_of::<AccountedDocument>(),
        document.id.len(),
        json::retained_bytes(&document.payload)?,
    ])
}

fn checked_sum(values: &[usize]) -> Result<usize, CassieError> {
    values
        .iter()
        .try_fold(0usize, |total, value| checked_add(total, *value))
}

fn checked_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(accounting_overflow)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(accounting_overflow)
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("controlled storage row accounting overflow".to_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Instant;

    use serde_json::json;

    use crate::config::CassieRuntimeLimits;

    use super::{AccountedDocument, AccountedDocumentPage, DocumentRef, QueryExecutionControls};

    fn controls_with_budget(query_memory_budget_bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_reserve_before_decoding_a_storage_document() {
        // Arrange
        let controls = controls_with_budget(7);
        let decode_calls = Cell::new(0);

        // Act
        let result = AccountedDocument::try_build(&controls, 8, || {
            decode_calls.set(decode_calls.get() + 1);
            Ok(DocumentRef {
                id: "one".to_owned(),
                payload: json!({"value": 1}),
            })
        });

        // Assert
        assert!(result.is_err());
        assert_eq!(decode_calls.get(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_a_popped_document_accounted_until_its_owner_drops_it() {
        // Arrange
        let controls = controls_with_budget(4096);
        let document = AccountedDocument::try_build(&controls, 1024, || {
            Ok(DocumentRef {
                id: "one".to_owned(),
                payload: json!({"value": 1}),
            })
        })
        .expect("accounted document");
        let retained_bytes = document.accounted_bytes();
        let mut page = AccountedDocumentPage::from_documents(vec![document], false);

        // Act
        let popped = page.pop_document().expect("page document");

        // Assert
        assert!(page.is_empty());
        assert!(!page.has_more());
        assert_eq!(popped.id(), "one");
        assert_eq!(controls.current_query_memory_bytes(), retained_bytes);
        drop(popped);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
