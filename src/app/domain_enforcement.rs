//! Domain defaults and constraint enforcement for table writes (#828).
//!
//! A domain column stores its base type, so reads, operators and wire output
//! see the base type. The domain's identity rides on the column, and the live
//! descriptor in the catalog decides the default, NOT NULL flag and CHECK
//! constraints, so `ALTER DOMAIN` takes effect without rewriting a table.

use super::{field_value_is_sql_null, Cassie, CassieError, DiagnosticError};
use crate::catalog::{CollectionSchema, FieldMeta};
use crate::executor::filter::{evaluate_check, first_violation};
use crate::types::{DomainCheck, DomainRef, DomainType, Value};

const SCAN_BATCH_SIZE: usize = 1024;

/// The rule an existing row must satisfy before `ALTER DOMAIN` publishes it.
pub(crate) enum DomainRowRule<'a> {
    /// `SET NOT NULL`: no stored value may be SQL NULL.
    NotNull,
    /// `ADD CONSTRAINT` or `VALIDATE CONSTRAINT`: the check must hold.
    Check(&'a DomainCheck),
}

fn payload_value(field: &FieldMeta, json: Option<&serde_json::Value>) -> Value {
    json.filter(|value| !value.is_null())
        .map_or(Value::Null, |value| {
            crate::executor::scan::json_to_typed_value(value, &field.data_type)
        })
}

fn default_json(domain: &DomainType) -> Option<serde_json::Value> {
    let default = domain.default.as_ref()?;
    default
        .value
        .clone()
        .filter(|value| !value.is_null())
        .or_else(|| {
            default
                .expression
                .as_deref()
                .and_then(crate::catalog::evaluate_volatile_default)
        })
}

impl Cassie {
    /// The current descriptor of the domain `field` was declared with.
    fn live_domain(&self, field: &FieldMeta) -> Option<DomainRef> {
        let snapshot = field.domain.as_ref()?;
        Some(
            self.catalog
                .get_domain_type_by_oid(snapshot.oid)
                .map_or_else(|| snapshot.clone(), |meta| meta.descriptor()),
        )
    }

    /// Fills columns that are absent from `payload` and whose domain has a
    /// DEFAULT. A column's own DEFAULT takes precedence because it is applied
    /// first and leaves no key absent.
    pub(crate) fn apply_domain_defaults(
        &self,
        collection: &str,
        payload: &mut serde_json::Value,
    ) -> Result<(), CassieError> {
        let Some(schema) = self.catalog.get_schema(collection) else {
            return Ok(());
        };
        if schema.fields.iter().all(|field| field.domain.is_none()) {
            return Ok(());
        }
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;
        for field in &schema.fields {
            if object.contains_key(&field.name) {
                continue;
            }
            let Some(default) = self
                .live_domain(field)
                .and_then(|domain| default_json(&domain))
            else {
                continue;
            };
            object.insert(field.name.clone(), default);
        }
        Ok(())
    }

    /// The domain default of `field`, for `column = DEFAULT`.
    pub(crate) fn domain_default_json(
        &self,
        collection: &str,
        field: &str,
    ) -> Option<serde_json::Value> {
        let schema = self.catalog.get_schema(collection)?;
        let field = schema.fields.iter().find(|entry| entry.name == field)?;
        default_json(&self.live_domain(field)?)
    }

    /// Enforces NOT NULL and CHECK constraints of every domain column of the
    /// row `payload`. An update skips CHECK constraints added `NOT VALID`,
    /// because the unchanged legacy values they were not checked against stay
    /// updatable, as in PostgreSQL.
    pub(crate) fn validate_domain_columns(
        &self,
        schema: &CollectionSchema,
        payload: &serde_json::Value,
        is_update: bool,
    ) -> Result<(), CassieError> {
        if schema.fields.iter().all(|field| field.domain.is_none()) {
            return Ok(());
        }
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;
        for field in &schema.fields {
            let Some(mut domain) = self.live_domain(field).map(|domain| (*domain).clone()) else {
                continue;
            };
            if is_update {
                domain.checks.retain(|check| check.validated);
            }
            let value = payload_value(field, object.get(&field.name));
            if let Some(violation) = first_violation(&domain, &value).map_err(CassieError::from)? {
                return Err(violation.into_error(&domain).into());
            }
        }
        Ok(())
    }

    /// Checks every stored value of every column declared with the domain
    /// `oid` against `rule`; nothing is published by the caller on failure.
    pub(crate) fn validate_domain_rows(
        &self,
        oid: u32,
        rule: &DomainRowRule<'_>,
    ) -> Result<(), CassieError> {
        for (collection, column) in self.catalog.domain_columns(oid) {
            let Some(schema) = self.catalog.get_schema(&collection) else {
                continue;
            };
            let Some(field) = schema.fields.iter().find(|entry| entry.name == column) else {
                continue;
            };
            for document in self
                .scan_documents_batched_for_session(None, &collection, SCAN_BATCH_SIZE)?
                .into_iter()
                .flatten()
            {
                let object = document.payload.as_object().ok_or_else(|| {
                    CassieError::InvalidVector("document payload must be a JSON object".to_string())
                })?;
                let existing = object.get(&column);
                let table = crate::catalog::local_name(&collection);
                match rule {
                    DomainRowRule::NotNull => {
                        if field_value_is_sql_null(existing, Some(&schema), &column) {
                            return Err(DiagnosticError::new(
                                "23502",
                                format!(
                                    "column \"{column}\" of table \"{table}\" contains null values"
                                ),
                            )
                            .into());
                        }
                    }
                    DomainRowRule::Check(check) => {
                        let value = payload_value(field, existing);
                        if !evaluate_check(check, &value).map_err(CassieError::from)? {
                            return Err(DiagnosticError::new(
                                "23514",
                                format!(
                                    "column \"{column}\" of table \"{table}\" contains values that violate the new constraint"
                                ),
                            )
                            .into());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
