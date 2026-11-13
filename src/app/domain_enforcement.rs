//! Domain defaults and constraint enforcement for table writes (#828).
//!
//! A domain column stores its base type, so reads, operators and wire output
//! see the base type. The domain's identity rides on the column, and the live
//! descriptor in the catalog decides the default, NOT NULL flag and CHECK
//! constraints, so `ALTER DOMAIN` takes effect without rewriting a table.

use super::{field_value_is_sql_null, Cassie, CassieError, CassieSession, DiagnosticError};
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
        let domain = self.live_domain(field)?;
        default_json(&domain)
    }

    /// Enforces NOT NULL and CHECK constraints of every domain column of the
    /// row `payload`. When `exclude_id` names the row being replaced, CHECK
    /// constraints added `NOT VALID` are skipped for columns whose value did
    /// not change, because legacy values they were never checked against stay
    /// updatable; newly assigned values are always checked, as in PostgreSQL.
    pub(crate) fn validate_domain_columns(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        schema: &CollectionSchema,
        payload: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        if schema.fields.iter().all(|field| {
            field.domain.is_none()
                && !matches!(field.data_type, crate::types::DataType::Composite(_))
        }) {
            return Ok(());
        }
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;
        self.validate_row_type_domain_fields(schema, object)?;
        let mut before: Option<Option<serde_json::Value>> = None;
        for field in &schema.fields {
            let Some(mut domain) = self.live_domain(field).map(|domain| (*domain).clone()) else {
                continue;
            };
            let has_unvalidated = domain.checks.iter().any(|check| !check.validated);
            if let (true, Some(id)) = (has_unvalidated, exclude_id) {
                if before.is_none() {
                    before = Some(
                        self.get_document_for_session(session, collection, id)?
                            .map(|document| document.payload),
                    );
                }
                let unchanged = before
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|old| {
                        let old = old.get(&field.name).filter(|value| !value.is_null());
                        let new = object.get(&field.name).filter(|value| !value.is_null());
                        old == new
                    });
                if unchanged {
                    domain.checks.retain(|check| check.validated);
                }
            }
            let value = payload_value(field, object.get(&field.name));
            if let Some(violation) = first_violation(&domain, &value).map_err(CassieError::from)? {
                return Err(violation.into_error(&domain).into());
            }
        }
        Ok(())
    }

    /// Enforces the live domains of columns whose type is another table's row
    /// type. The column embeds a flattened descriptor, so the domains come from
    /// that table's current row type, resolved by OID.
    fn validate_row_type_domain_fields(
        &self,
        schema: &CollectionSchema,
        object: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), CassieError> {
        for field in &schema.fields {
            let crate::types::DataType::Composite(embedded) = &field.data_type else {
                continue;
            };
            let Some((_, live, false)) = self.catalog.relation_row_type_by_oid(embedded.oid) else {
                continue;
            };
            if live.fields.iter().all(|f| f.data_type.domain().is_none()) {
                continue;
            }
            let Value::Typed(crate::types::typed::TypedValue::Record(record)) =
                payload_value(field, object.get(&field.name))
            else {
                continue;
            };
            for (value, member) in record.fields().iter().zip(&live.fields) {
                let Some(domain) = member.data_type.domain() else {
                    continue;
                };
                if let Some(violation) =
                    first_violation(domain, value).map_err(CassieError::from)?
                {
                    return Err(violation.into_error(domain).into());
                }
            }
        }
        Ok(())
    }

    /// The domain each `CREATE TABLE AS` output column inherits: a query over
    /// one table keeps the domain of every column it selects by name (or
    /// through `*`), as PostgreSQL keeps the column's declared type. Computed
    /// outputs and other query shapes yield their base type.
    pub(crate) fn ctas_output_domains(
        &self,
        query: &crate::sql::ast::QueryStatement,
        output_names: &[String],
    ) -> Vec<Option<DomainRef>> {
        use crate::sql::ast::{QuerySource, QueryStatement, SelectItem};
        use crate::sql::ColumnIdentifierPath;

        let none = || vec![None; output_names.len()];
        let QueryStatement::Select(select) = query else {
            return none();
        };
        let table = match &select.source {
            QuerySource::Collection(table) => table,
            QuerySource::Aliased {
                source,
                column_aliases,
                ..
            } if column_aliases.is_empty() => match &**source {
                QuerySource::Collection(table) => table,
                _ => return none(),
            },
            _ => return none(),
        };
        if select.set.is_some() || !select.group_by.is_empty() {
            return none();
        }
        let Some(schema) = self.catalog.get_schema(table.as_str()) else {
            return none();
        };
        let domain_of = |column: &str| {
            schema
                .fields
                .iter()
                .find(|field| {
                    ColumnIdentifierPath::from_field_name(&field.name).lookup_key() == column
                })
                .and_then(|field| self.live_domain(field))
        };
        if matches!(select.projection.as_slice(), [SelectItem::Wildcard]) {
            return output_names
                .iter()
                .map(|name| domain_of(&ColumnIdentifierPath::from_field_name(name).lookup_key()))
                .collect();
        }
        if select.projection.len() != output_names.len() {
            return none();
        }
        select
            .projection
            .iter()
            .map(|item| match item {
                SelectItem::Column { name, .. } => ColumnIdentifierPath::parse(name)
                    .ok()
                    .and_then(|path| domain_of(&path.field_lookup_key())),
                _ => None,
            })
            .collect()
    }

    /// The default backfilled into existing rows when a column of type
    /// `data_type` is added to `table`, after checking it against the domain.
    ///
    /// The column's own default wins; otherwise the domain default applies. A
    /// populated table must satisfy the domain's NOT NULL and CHECK constraints
    /// with the backfilled value, as in PostgreSQL (`23502` / `23514`).
    pub(crate) fn domain_add_column_default(
        &self,
        session: Option<&CassieSession>,
        table: &str,
        data_type: &crate::types::DataType,
        column_default: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        let Some(snapshot) = data_type.domain() else {
            return Ok(column_default);
        };
        let domain = self
            .catalog
            .get_domain_type_by_oid(snapshot.oid)
            .map_or_else(|| snapshot.clone(), |meta| meta.descriptor());
        let populated = self
            .scan_documents_batched_for_session(session, table, 1)?
            .into_iter()
            .flatten()
            .next()
            .is_some();
        // gen_random_uuid() yields a distinct value per row in PostgreSQL, and
        // a single backfilled value would not; now()/CURRENT_* are statement
        // stable and may be backfilled once.
        let per_row_volatile = column_default.is_none()
            && domain.default.as_ref().is_some_and(|default| {
                default
                    .expression
                    .as_deref()
                    .is_some_and(|sql| sql.to_ascii_lowercase().contains("gen_random_uuid"))
            });
        if populated && per_row_volatile {
            return Err(DiagnosticError::new(
                "0A000",
                "adding a column whose domain default is a per-row volatile function to a populated table is not supported".to_string(),
            )
            .into());
        }
        let default = column_default.or_else(|| default_json(&domain));
        if populated {
            let value = default
                .as_ref()
                .filter(|value| !value.is_null())
                .map_or(Value::Null, |value| {
                    crate::executor::scan::json_to_typed_value(value, data_type.flattened())
                });
            if let Some(violation) = first_violation(&domain, &value).map_err(CassieError::from)? {
                return Err(violation.into_error(&domain).into());
            }
        }
        Ok(default)
    }

    /// Checks every stored value of every column declared with the domain
    /// `oid` against `rule`; nothing is published by the caller on failure.
    pub(crate) fn validate_domain_rows(
        &self,
        session: Option<&CassieSession>,
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
                .scan_documents_batched_for_session(session, &collection, SCAN_BATCH_SIZE)?
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
