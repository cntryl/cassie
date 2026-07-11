use super::vector_helpers::project_payload_fields;
use super::{
    BTreeMap, Cassie, CassieError, CassieSession, ConstraintCheck, ConstraintOperator, DocumentRef,
    FieldConstraint, Instant, MidgeScanTimings, RowDecode, RowFilter, TransactionRowChange, Uuid,
    VectorIndexRecord,
};

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn ingest_document(
        &self,
        collection: &str,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        self.write_document(collection, None, payload, true, None)
    }

    pub(crate) fn write_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        self.write_document_for_session(None, collection, id, payload, apply_defaults, exclude_id)
    }

    pub(crate) fn write_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        let collections = self.referential_write_collections(collection);
        self.midge.with_collection_write_gates(&collections, || {
            self.write_document_for_session_with_held_collection_gates(
                session,
                collection,
                id,
                payload,
                apply_defaults,
                exclude_id,
            )
        })
    }

    fn write_document_for_session_with_held_collection_gates(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        let payload = self.prepare_document_write_for_session(
            session,
            collection,
            payload,
            apply_defaults,
            exclude_id,
        )?;

        if let Some(session) = session {
            if session.is_transaction_active() {
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                session.stage_document_write(collection, id.clone(), payload);
                return Ok(id);
            }
        }

        let (row_id, stats, row_delta) = self
            .midge
            .put_document_with_stats(collection, id, payload)?;
        self.runtime
            .record_projection_write_batch(collection.to_string(), &stats);
        self.refresh_runtime_data_epoch()?;
        self.refresh_document_write_metadata(collection, row_delta, &stats)?;
        Ok(row_id)
    }

    pub(crate) fn prepare_document_write_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        mut payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<serde_json::Value, CassieError> {
        let constraints = self.catalog.get_constraints(collection);
        if apply_defaults && !constraints.is_empty() {
            super::defaults::apply_default_values(self, &mut payload, &constraints)?;
        }

        self.validate_payload_schema(collection, &payload)?;

        let indexes = self.catalog.list_vector_indexes(collection);
        if !indexes.is_empty() {
            self.apply_vector_indexes(collection, &mut payload, indexes.as_slice())?;
        }

        self.validate_constraints_for_session(
            session,
            collection,
            &payload,
            &constraints,
            exclude_id,
        )?;
        self.validate_unique_indexes_for_session(session, collection, &payload, exclude_id)?;
        self.validate_foreign_keys_for_session(session, collection, &payload, &constraints)?;

        Ok(payload)
    }

    pub(crate) fn put_prepared_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: String,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        if let Some(session) = session {
            if session.is_transaction_active() {
                session.stage_document_write(collection, id.clone(), payload);
                return Ok(id);
            }
        }

        let (row_id, stats, row_delta) =
            self.midge
                .put_document_with_stats(collection, Some(id), payload)?;
        self.runtime
            .record_projection_write_batch(collection.to_string(), &stats);
        self.refresh_runtime_data_epoch()?;
        self.refresh_document_write_metadata(collection, row_delta, &stats)?;
        Ok(row_id)
    }

    pub(crate) fn delete_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: &str,
    ) -> Result<bool, CassieError> {
        let collections = self.referential_write_collections(collection);
        self.midge.with_collection_write_gates(&collections, || {
            self.delete_document_for_session_with_held_collection_gates(session, collection, id)
        })
    }

    fn delete_document_for_session_with_held_collection_gates(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: &str,
    ) -> Result<bool, CassieError> {
        if let Some(session) = session {
            if session.is_transaction_active() {
                let existed = self
                    .get_document_for_session(Some(session), collection, id)?
                    .is_some();
                session.stage_document_delete(collection, id.to_string());
                return Ok(existed);
            }
        }

        let (removed, stats, row_delta) = self.midge.delete_document_with_stats(collection, id)?;
        self.runtime
            .record_projection_write_batch(collection.to_string(), &stats);
        self.refresh_runtime_data_epoch()?;
        self.refresh_document_write_metadata(collection, row_delta, &stats)?;
        Ok(removed)
    }

    fn refresh_runtime_data_epoch(&self) -> Result<(), CassieError> {
        self.runtime.set_data_epoch(self.midge.data_epoch()?);
        Ok(())
    }

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

    pub(crate) fn get_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        if let Some(session) = session {
            if let Some(change) = session.document_change(collection, id) {
                return Ok(match change {
                    TransactionRowChange::Upsert(payload) => Some(DocumentRef {
                        id: id.to_string(),
                        payload,
                    }),
                    TransactionRowChange::Delete => None,
                });
            }
        }

        self.midge.get_document(collection, id)
    }

    pub(crate) fn scan_projected_documents_batched_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_projected_documents_batched_for_session_with_timings(
            session, collection, batch_size, fields, limit,
        )
        .map(|(batches, _)| batches)
    }

    pub(crate) fn scan_projected_documents_batched_for_session_with_timings(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_projected_documents_batched_for_session_with_filter_and_timings(
            session, collection, batch_size, fields, None, limit,
        )
    }

    pub(crate) fn scan_projected_documents_batched_for_session_with_filter_and_timings(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let started = Instant::now();
        let mut timings = MidgeScanTimings::default();
        let collection_changes = if let Some(session) = session {
            session.collection_changes(collection)
        } else {
            BTreeMap::new()
        };
        if collection_changes.is_empty() {
            let (batches, scan_timings) = self
                .midge
                .scan_projected_rows_batched_filter_limit_with_timings(
                    collection,
                    batch_size,
                    fields.to_vec(),
                    filter,
                    limit,
                )?;
            let measured = scan_timings.scan.saturating_add(scan_timings.row_decode);
            timings = scan_timings;
            timings.scan = timings
                .scan
                .saturating_add(started.elapsed().saturating_sub(measured));
            return Ok((batches, timings));
        }

        let mut rows = self
            .midge
            .scan_rows_for_rebuild(collection, RowDecode::ProjectedHistorical(fields.to_vec()))?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();

        for (id, change) in collection_changes {
            match change {
                TransactionRowChange::Upsert(payload) => {
                    rows.insert(
                        id.clone(),
                        DocumentRef {
                            id,
                            payload: project_payload_fields(&payload, fields),
                        },
                    );
                }
                TransactionRowChange::Delete => {
                    rows.remove(&id);
                }
            }
        }

        let batch_size = batch_size.max(1);
        let mut batches = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        for document in rows.into_values() {
            current.push(document);
            if current.len() >= batch_size {
                batches.push(current);
                current = Vec::with_capacity(batch_size);
            }
        }
        if !current.is_empty() {
            batches.push(current);
        }

        let measured = timings.scan.saturating_add(timings.row_decode);
        timings.scan = timings
            .scan
            .saturating_add(started.elapsed().saturating_sub(measured));

        Ok((batches, timings))
    }

    fn validate_payload_schema(
        &self,
        collection: &str,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let schema = self
            .catalog
            .get_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;

        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for (field, value) in object {
            let expected = schema
                .fields
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(field))
                .ok_or_else(|| {
                    CassieError::InvalidVector(format!(
                        "field '{field}' is not defined on collection '{collection}'"
                    ))
                })?
                .data_type
                .clone();
            Self::validate_value_against_data_type(field, &expected, value)?;
        }

        Ok(())
    }

    fn validate_value_against_data_type(
        field: &str,
        expected: &crate::types::DataType,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        if value.is_null() {
            return Ok(());
        }

        match expected {
            crate::types::DataType::Null => Self::invalid_vector(field, "null"),
            crate::types::DataType::SmallInt => {
                Self::validate_integer_range(field, value, "smallint", |number| {
                    i16::try_from(number).is_ok()
                })
            }
            crate::types::DataType::Int => {
                Self::validate_integer_range(field, value, "int", |number| {
                    i32::try_from(number).is_ok()
                })
            }
            crate::types::DataType::BigInt => Self::validate_bigint(field, value),
            crate::types::DataType::Float => Self::validate_float(field, value),
            crate::types::DataType::Boolean => Self::validate_boolean(field, value),
            crate::types::DataType::Text | crate::types::DataType::Uuid => {
                Self::validate_text_or_uuid(field, expected, value)
            }
            crate::types::DataType::Char { length } => Self::validate_char(field, *length, value),
            crate::types::DataType::Varchar { length } => {
                Self::validate_varchar(field, *length, value)
            }
            crate::types::DataType::Bytea => Self::validate_bytea(field, value),
            crate::types::DataType::Date
            | crate::types::DataType::Time
            | crate::types::DataType::Timestamp => {
                Self::validate_string_only(field, expected, value)
            }
            crate::types::DataType::Json => Self::validate_json(field, value),
            crate::types::DataType::Vector(size) => Self::validate_vector(field, *size, value),
            crate::types::DataType::Array(inner) => Self::validate_array(field, inner, value),
        }
    }

    fn invalid_vector<T>(field: &str, expected: T) -> Result<(), CassieError>
    where
        T: std::fmt::Display,
    {
        Err(CassieError::InvalidVector(format!(
            "field '{field}' expects {expected}"
        )))
    }

    fn validate_integer_range<F>(
        field: &str,
        value: &serde_json::Value,
        type_name: &str,
        in_range: F,
    ) -> Result<(), CassieError>
    where
        F: FnOnce(i64) -> bool,
    {
        let number = value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .ok_or_else(|| {
                CassieError::InvalidVector(format!("field '{field}' expects {type_name}"))
            })?;

        if in_range(number) {
            Ok(())
        } else {
            Self::invalid_vector(field, type_name)
        }
    }

    fn validate_bigint(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
        if value.is_i64() || value.as_u64().is_some() {
            Ok(())
        } else {
            Self::invalid_vector(field, "bigint")
        }
    }

    fn validate_float(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
        if value.is_number() {
            Ok(())
        } else {
            Self::invalid_vector(field, "float")
        }
    }

    fn validate_boolean(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
        if value.is_boolean() {
            Ok(())
        } else {
            Self::invalid_vector(field, "boolean")
        }
    }

    fn validate_text_or_uuid(
        field: &str,
        expected: &crate::types::DataType,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        if !value.is_string() {
            return Self::invalid_vector(field, expected.type_name());
        }

        if let crate::types::DataType::Uuid = expected {
            let value = value.as_str().unwrap_or_default();
            if Uuid::parse_str(value).is_err() {
                return Self::invalid_vector(field, "UUID");
            }
        }

        Ok(())
    }

    fn validate_char(
        field: &str,
        length: Option<u32>,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let value = value
            .as_str()
            .ok_or_else(|| CassieError::InvalidVector(format!("field '{field}' expects char")))?;

        let max = length.unwrap_or(1) as usize;
        if value.chars().count() <= max {
            Ok(())
        } else {
            Self::invalid_vector(field, format!("char({max})"))
        }
    }

    fn validate_varchar(
        field: &str,
        length: Option<u32>,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let value = value.as_str().ok_or_else(|| {
            CassieError::InvalidVector(format!("field '{field}' expects varchar"))
        })?;

        if let Some(length) = length {
            if value.chars().count() <= (length as usize) {
                Ok(())
            } else {
                Self::invalid_vector(field, format!("varchar({length})"))
            }
        } else {
            Ok(())
        }
    }

    fn validate_bytea(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
        if !value.is_string() {
            return Self::invalid_vector(field, "bytea");
        }

        Self::decode_bytea(value.as_str().unwrap_or_default())?;
        Ok(())
    }

    fn validate_string_only(
        field: &str,
        expected: &crate::types::DataType,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        if value.is_string() {
            Ok(())
        } else {
            Self::invalid_vector(field, expected.type_name())
        }
    }

    fn validate_json(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
        if value.is_object()
            || value.is_array()
            || value.is_string()
            || value.is_number()
            || value.is_boolean()
            || value.is_null()
        {
            Ok(())
        } else {
            Self::invalid_vector(field, "json")
        }
    }

    fn validate_vector(
        field: &str,
        size: usize,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let Some(array) = value.as_array() else {
            return Self::invalid_vector(field, format!("vector({size})"));
        };
        if array.len() != size || array.iter().any(|value| value.as_f64().is_none()) {
            Self::invalid_vector(field, format!("vector({size})"))
        } else {
            Ok(())
        }
    }

    fn validate_array(
        field: &str,
        inner: &crate::types::DataType,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let Some(values) = value.as_array() else {
            return Self::invalid_vector(field, "array");
        };

        for value in values {
            Self::validate_value_against_data_type(field, inner, value)?;
        }

        Ok(())
    }

    fn decode_bytea(value: &str) -> Result<Vec<u8>, CassieError> {
        if !value.starts_with("\\x") {
            return Err(CassieError::InvalidVector(
                "bytea expects hex format '\\x'".to_string(),
            ));
        }

        if value.len() == 2 {
            return Ok(Vec::new());
        }

        if (value.len() - 2).rem_euclid(2) != 0 {
            return Err(CassieError::InvalidVector(
                "bytea expects an even number of hex digits".to_string(),
            ));
        }

        let raw = value.as_bytes();
        let mut out = Vec::with_capacity((value.len() - 2) / 2);
        let mut index = 2;
        while index < value.len() {
            let high = Self::decode_hex_digit(raw[index])
                .ok_or_else(|| CassieError::InvalidVector("invalid bytea hex digit".to_string()))?;
            let low = Self::decode_hex_digit(raw[index + 1])
                .ok_or_else(|| CassieError::InvalidVector("invalid bytea hex digit".to_string()))?;
            out.push(high << 4 | low);
            index += 2;
        }

        Ok(out)
    }

    fn decode_hex_digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    fn validate_constraints_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        constraints: &[FieldConstraint],
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for constraint in constraints {
            let existing = object.get(&constraint.field);

            if (constraint.not_null || constraint.primary_key)
                && existing.is_none_or(serde_json::Value::is_null)
            {
                let constraint_name = if constraint.primary_key {
                    Some(crate::catalog::generated_constraint_name(
                        collection,
                        &constraint.field,
                        "PRIMARY KEY",
                    ))
                } else if constraint.not_null {
                    Some(crate::catalog::generated_constraint_name(
                        collection,
                        &constraint.field,
                        "NOT NULL",
                    ))
                } else {
                    None
                };
                return Err(CassieError::NotNullViolation {
                    table: collection.to_string(),
                    column: constraint.field.clone(),
                    constraint: constraint_name,
                });
            }

            if let Some(check) = &constraint.check {
                let Some(value) = existing else {
                    continue;
                };
                if !Self::satisfies_check_constraint(value, check) {
                    return Err(CassieError::CheckViolation {
                        table: collection.to_string(),
                        column: check.field.clone(),
                        constraint: crate::catalog::generated_constraint_name(
                            collection,
                            &check.field,
                            "CHECK",
                        ),
                    });
                }
            }
        }

        self.validate_uniques(session, collection, object, constraints, exclude_id)
    }

    fn satisfies_check_constraint(value: &serde_json::Value, check: &ConstraintCheck) -> bool {
        match check.operator {
            ConstraintOperator::Eq => value == &check.value,
            ConstraintOperator::NotEq => value != &check.value,
            ConstraintOperator::Lt => Self::compare_constraint_values(value, &check.value)
                .is_some_and(std::cmp::Ordering::is_lt),
            ConstraintOperator::Lte => Self::compare_constraint_values(value, &check.value)
                .is_some_and(std::cmp::Ordering::is_le),
            ConstraintOperator::Gt => Self::compare_constraint_values(value, &check.value)
                .is_some_and(std::cmp::Ordering::is_gt),
            ConstraintOperator::Gte => Self::compare_constraint_values(value, &check.value)
                .is_some_and(std::cmp::Ordering::is_ge),
            ConstraintOperator::Like => {
                let Some(value) = value.as_str() else {
                    return false;
                };
                let Some(expected) = check.value.as_str() else {
                    return false;
                };
                Self::string_like_match(expected, value)
            }
        }
    }

    fn compare_constraint_values(
        left: &serde_json::Value,
        right: &serde_json::Value,
    ) -> Option<std::cmp::Ordering> {
        match (left, right) {
            (serde_json::Value::Number(left), serde_json::Value::Number(right)) => left
                .as_f64()
                .and_then(|left| right.as_f64().map(|right| left.partial_cmp(&right)))
                .flatten(),
            (serde_json::Value::String(left), serde_json::Value::String(right)) => {
                Some(left.cmp(right))
            }
            (serde_json::Value::Bool(left), serde_json::Value::Bool(right)) => {
                Some(left.cmp(right))
            }
            _ => None,
        }
    }

    fn string_like_match(pattern: &str, value: &str) -> bool {
        if pattern == "%" {
            return true;
        }

        let starts_with_wildcard = pattern.starts_with('%');
        let ends_with_wildcard = pattern.ends_with('%');
        let normalized = pattern.trim_matches('%');

        if starts_with_wildcard && ends_with_wildcard {
            value.contains(normalized)
        } else if starts_with_wildcard {
            value.ends_with(normalized)
        } else if ends_with_wildcard {
            value.starts_with(normalized)
        } else {
            value == pattern
        }
    }

    fn validate_uniques(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Map<String, serde_json::Value>,
        constraints: &[FieldConstraint],
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        for constraint in constraints {
            if !(constraint.unique || constraint.primary_key) {
                continue;
            }

            let Some(value) = payload.get(&constraint.field) else {
                continue;
            };
            if value.is_null() {
                continue;
            }

            if self.value_exists_for_collection_field(
                session,
                collection,
                &constraint.field,
                value,
                exclude_id,
            )? {
                let kind = if constraint.primary_key {
                    "PRIMARY KEY"
                } else {
                    "UNIQUE"
                };
                return Err(CassieError::UniqueViolation {
                    table: collection.to_string(),
                    column: constraint.field.clone(),
                    constraint: crate::catalog::generated_constraint_name(
                        collection,
                        &constraint.field,
                        kind,
                    ),
                });
            }
        }

        Ok(())
    }

    fn validate_foreign_keys_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        constraints: &[FieldConstraint],
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

            if self
                .find_document_id_by_fields(session, table, &[(field, value)], None)?
                .is_none()
            {
                return Err(CassieError::ForeignKeyViolation {
                    table: collection.to_string(),
                    column: constraint.field.clone(),
                    constraint: crate::catalog::generated_constraint_name(
                        collection,
                        &constraint.field,
                        "FOREIGN KEY",
                    ),
                    referenced_table: table.to_string(),
                    referenced_column: field.to_string(),
                });
            }
        }

        Ok(())
    }

    fn validate_unique_indexes_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for index in self.catalog.list_indexes(collection) {
            if !index.unique || index.kind != crate::catalog::IndexKind::Scalar {
                continue;
            }

            let fields = index.normalized_fields();
            let mut values = Vec::with_capacity(fields.len());
            for field in &fields {
                let Some(value) = object.get(field) else {
                    values.clear();
                    break;
                };
                if value.is_null() {
                    values.clear();
                    break;
                }
                values.push((field.as_str(), value));
            }

            if values.is_empty() {
                continue;
            }

            if self.values_exist_for_collection_fields(session, collection, &values, exclude_id)? {
                return Err(CassieError::InvalidVector(format!(
                    "unique index '{}' failed",
                    index.name
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn value_exists_for_collection_field(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        field: &str,
        value: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<bool, CassieError> {
        for document in self
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if document.payload.get(field) == Some(value) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(crate) fn values_exist_for_collection_fields(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        values: &[(&str, &serde_json::Value)],
        exclude_id: Option<&str>,
    ) -> Result<bool, CassieError> {
        for document in self
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if values
                .iter()
                .all(|(field, value)| document.payload.get(*field) == Some(*value))
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(crate) fn find_document_id_by_fields(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        values: &[(&str, &serde_json::Value)],
        exclude_id: Option<&str>,
    ) -> Result<Option<String>, CassieError> {
        for document in self
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if values
                .iter()
                .all(|(field, value)| document.payload.get(*field) == Some(*value))
            {
                return Ok(Some(document.id));
            }
        }

        Ok(None)
    }

    fn apply_vector_indexes(
        &self,
        _collection: &str,
        payload: &mut serde_json::Value,
        indexes: &[VectorIndexRecord],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidEmbedding("document payload must be a JSON object".to_string())
        })?;

        for index in indexes {
            self.validate_embedding_compatibility(index, None)?;

            let source_value = object.get(&index.source_field).ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "missing source field '{}' for vector index '{}' on collection '{}'",
                    index.source_field, index.field, index.collection
                ))
            })?;

            let source = if let Some(value) = source_value.as_str() {
                value.to_string()
            } else {
                source_value.to_string()
            };

            let embedding = self
                .embedding_provider
                .embed_query(&source)
                .map_err(CassieError::from)?;
            Self::validate_embedding_payload(index, &embedding)?;

            object.insert(
                index.field.clone(),
                serde_json::Value::Array(
                    embedding
                        .values
                        .into_iter()
                        .map(serde_json::Value::from)
                        .collect(),
                ),
            );
        }

        Ok(())
    }

    pub(crate) fn refresh_projection_metadata(&self, collection: &str) -> Result<(), CassieError> {
        if let Some(metadata) = self.midge.projection_metadata(collection)? {
            self.catalog.register_projection_metadata(metadata);
        }
        Ok(())
    }
}
