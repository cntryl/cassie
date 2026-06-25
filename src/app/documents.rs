use super::vector_helpers::project_payload_fields;
use super::*;

impl Cassie {
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
            self.apply_default_values(&mut payload, &constraints)?;
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
        self.refresh_document_write_metadata(collection, row_delta, &stats)?;
        Ok(row_id)
    }

    pub(crate) fn delete_document_for_session(
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
        self.refresh_document_write_metadata(collection, row_delta, &stats)?;
        Ok(removed)
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

    pub(crate) fn scan_documents_batched_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        let mut rows = self
            .midge
            .scan_documents(collection)?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();

        if let Some(session) = session {
            for (id, change) in session.collection_changes(collection) {
                match change {
                    TransactionRowChange::Upsert(payload) => {
                        rows.insert(id.clone(), DocumentRef { id, payload });
                    }
                    TransactionRowChange::Delete => {
                        rows.remove(&id);
                    }
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

        Ok(batches)
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
            .scan_rows_for_rebuild(collection, RowDecode::Projected(fields.to_vec()))?
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
            if matches!(expected, crate::types::DataType::Null) {
                return Ok(());
            }
            return Ok(());
        }

        match expected {
            crate::types::DataType::Null => Err(CassieError::InvalidVector(format!(
                "field '{field}' expects null"
            ))),
            crate::types::DataType::SmallInt => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .ok_or_else(|| {
                        CassieError::InvalidVector(format!("field '{field}' expects smallint"))
                    })?;

                if i16::try_from(number).is_ok() {
                    return Ok(());
                }

                Err(CassieError::InvalidVector(format!(
                    "field '{field}' expects smallint"
                )))
            }
            crate::types::DataType::Int => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .ok_or_else(|| {
                        CassieError::InvalidVector(format!("field '{field}' expects int"))
                    })?;

                if i32::try_from(number).is_ok() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects int"
                    )))
                }
            }
            crate::types::DataType::BigInt => {
                if value.is_i64() || value.as_u64().is_some() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects bigint"
                    )))
                }
            }
            crate::types::DataType::Float => {
                if value.is_number() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects float"
                    )))
                }
            }
            crate::types::DataType::Boolean => {
                if value.is_boolean() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects boolean"
                    )))
                }
            }
            crate::types::DataType::Text | crate::types::DataType::Uuid => {
                if !value.is_string() {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects {}",
                        expected.type_name()
                    )));
                }

                if let crate::types::DataType::Uuid = expected {
                    let value = value.as_str().unwrap_or_default();
                    if Uuid::parse_str(value).is_err() {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects UUID"
                        )));
                    }
                }

                Ok(())
            }
            crate::types::DataType::Char { length } => {
                let value = value.as_str().ok_or_else(|| {
                    CassieError::InvalidVector(format!("field '{field}' expects char"))
                })?;

                let max = length.unwrap_or(1) as usize;
                if value.chars().count() <= max {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects char({max})"
                    )))
                }
            }
            crate::types::DataType::Varchar { length } => {
                let value = value.as_str().ok_or_else(|| {
                    CassieError::InvalidVector(format!("field '{field}' expects varchar"))
                })?;

                if let Some(length) = length {
                    if value.chars().count() <= (*length as usize) {
                        Ok(())
                    } else {
                        Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects varchar({length})"
                        )))
                    }
                } else {
                    Ok(())
                }
            }
            crate::types::DataType::Bytea => {
                if !value.is_string() {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects bytea"
                    )));
                }

                Self::decode_bytea(value.as_str().unwrap_or_default())?;
                Ok(())
            }
            crate::types::DataType::Date
            | crate::types::DataType::Time
            | crate::types::DataType::Timestamp => {
                if value.is_string() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects {}",
                        expected.type_name()
                    )))
                }
            }
            crate::types::DataType::Json => {
                if value.is_object()
                    || value.is_array()
                    || value.is_string()
                    || value.is_number()
                    || value.is_boolean()
                    || value.is_null()
                {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects json"
                    )))
                }
            }
            crate::types::DataType::Vector(size) => {
                let Some(array) = value.as_array() else {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                };
                if array.len() != *size {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                }
                if array.iter().any(|value| value.as_f64().is_none()) {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                }
                Ok(())
            }
            crate::types::DataType::Array(inner) => {
                let Some(values) = value.as_array() else {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects array"
                    )));
                };

                for value in values {
                    Self::validate_value_against_data_type(field, inner, value)?;
                }

                Ok(())
            }
        }
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

    fn apply_default_values(
        &self,
        payload: &mut serde_json::Value,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for constraint in constraints {
            if object.contains_key(&constraint.field) {
                continue;
            }

            if let Some(default) = &constraint.default_value {
                object.insert(constraint.field.clone(), default.clone());
            }
        }

        Ok(())
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
                && (existing.is_none() || existing.is_some_and(|value| value.is_null()))
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
                if !self.satisfies_check_constraint(value, check) {
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

    fn satisfies_check_constraint(
        &self,
        value: &serde_json::Value,
        check: &ConstraintCheck,
    ) -> bool {
        match check.operator {
            ConstraintOperator::Eq => value == &check.value,
            ConstraintOperator::NotEq => value != &check.value,
            ConstraintOperator::Lt => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_lt()),
            ConstraintOperator::Lte => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_le()),
            ConstraintOperator::Gt => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_gt()),
            ConstraintOperator::Gte => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_ge()),
            ConstraintOperator::Like => {
                let Some(value) = value.as_str() else {
                    return false;
                };
                let Some(expected) = check.value.as_str() else {
                    return false;
                };
                self.string_like_match(expected, value)
            }
        }
    }

    fn compare_constraint_values(
        &self,
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

    fn string_like_match(&self, pattern: &str, value: &str) -> bool {
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
            self.validate_embedding_payload(index, &embedding)?;

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
