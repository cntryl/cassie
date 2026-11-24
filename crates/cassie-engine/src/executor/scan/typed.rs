//! Controlled Midge-to-typed transport. JSON decoding remains a storage boundary.
use super::{
    controlled_storage_error, conversion, field_data_type, is_identity_reference,
    is_row_identity_column, projected_field_value, try_json_to_typed_value, Cassie, CassieSession,
    DataType, QueryError, QueryExecutionControls, QueryMemoryReservation, RowDecode,
    SessionRowCursor, Value, DEFAULT_BATCH_SIZE,
};
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::executor::typed_batch::TypedBatch;
use std::mem::size_of;

mod authoritative;
pub(crate) use authoritative::{PartitionBatches, PartitionedScan, Partitioning};

pub(crate) struct TypedScanStream<'a> {
    cassie: &'a Cassie,
    cursor: Option<SessionRowCursor>,
    authoritative: Option<authoritative::AuthoritativeSource>,
    encoded: Option<crate::midge::adapter::EncodedSource>,
    encoded_position: usize,
    schema: Vec<(String, DataType)>,
    controls: &'a QueryExecutionControls,
    _schema_memory: QueryMemoryReservation,
}

impl<'a> TypedScanStream<'a> {
    /// Lets a column-table source skip segments that a conjunctive filter
    /// excludes. The caller still applies the filter to every row it receives.
    pub(crate) fn set_column_pruning(
        &mut self,
        filter: &crate::midge::adapter::ColumnBatchScanFilter,
    ) {
        if let Some(cursor) = self.cursor.as_mut() {
            cursor.set_column_pruning(&self.cassie.midge, filter);
        }
        if let Some(source) = self.authoritative.as_mut() {
            source.set_pruning(self.cassie, filter, self.controls);
        }
    }

    /// Chooses between a serial read and waves of up to `requested` workers
    /// over id-range partitions. `None` when the source is not an
    /// authoritative column table, whose other readers have no partitions.
    ///
    /// # Errors
    ///
    /// Returns storage, resource-limit and cancellation errors.
    pub(crate) fn partitioning(
        &mut self,
        requested: usize,
    ) -> Result<Option<Partitioning<'_>>, QueryError> {
        let Some(source) = self.authoritative.as_mut() else {
            return Ok(None);
        };
        source
            .partitioning(self.cassie, &self.schema, requested, self.controls)
            .map(Some)
    }

    #[cfg(test)]
    fn is_encoded(&self) -> bool {
        self.encoded.is_some()
    }
    pub(crate) fn open(
        cassie: &'a Cassie,
        session: Option<&CassieSession>,
        collection: &str,
        fields: &[String],
        controls: &'a QueryExecutionControls,
    ) -> Result<Option<Self>, QueryError> {
        Self::open_with_traversal(
            cassie,
            session,
            collection,
            fields,
            crate::midge::adapter::RowTraversal::CompleteSource,
            controls,
        )
    }

    pub(crate) fn open_with_traversal(
        cassie: &'a Cassie,
        session: Option<&CassieSession>,
        collection: &str,
        fields: &[String],
        traversal: crate::midge::adapter::RowTraversal,
        controls: &'a QueryExecutionControls,
    ) -> Result<Option<Self>, QueryError> {
        conversion::check_controls(controls)?;
        let (catalog_schema, _catalog_memory) = cassie
            .catalog
            .clone_schema_with_controls(collection, controls)?;
        let Some(catalog_schema) = catalog_schema else {
            return Ok(None);
        };
        let bytes = fields.iter().try_fold(512, |bytes, field| {
            add(
                bytes,
                add(mul(field.len(), 64)?, size_of::<(String, DataType)>())?,
            )
        })?;
        let mut memory = controls.reserve_query_memory(bytes)?;
        let mut schema = Vec::with_capacity(fields.len());
        for field in fields {
            let data_type = if is_identity_reference(field, catalog_schema.declares_id()) {
                DataType::Text
            } else {
                let Some(data_type) = field_data_type(Some(&catalog_schema), field) else {
                    return Ok(None);
                };
                memory.try_grow(data_type_clone_bytes(data_type)?)?;
                data_type.clone()
            };
            schema.push((field.clone(), data_type));
        }
        if !session.is_some_and(|session| session.has_collection_changes(collection))
            && fields
                .iter()
                .all(|field| !is_identity_reference(field, catalog_schema.declares_id()))
            && super::has_covering_column_index(cassie, collection, fields)
        {
            match cassie
                .midge
                .controlled_encoded_source(collection, fields, &controls.storage_request())
            {
                Ok(crate::midge::adapter::EncodedSourceDecision::Hit(source)) => {
                    cassie.runtime.record_column_batch_scan(&source.metrics);
                    cassie.runtime.record_storage_access("data", false, true);
                    return Ok(Some(Self {
                        cassie,
                        cursor: None,
                        authoritative: None,
                        encoded: Some(source),
                        encoded_position: 0,
                        schema,
                        controls,
                        _schema_memory: memory,
                    }));
                }
                Ok(crate::midge::adapter::EncodedSourceDecision::Fallback(reason)) => {
                    cassie.runtime.record_column_batch_fallback(reason.as_str());
                }
                Ok(crate::midge::adapter::EncodedSourceDecision::TransportBudget) => {
                    return Ok(None);
                }
                Err(error) => return Err(controlled_storage_error(cassie, error.into())),
            }
        }
        if let Some(source) = authoritative::open(
            cassie, session, collection, fields, &schema, traversal, controls,
        )? {
            return Ok(Some(Self {
                cassie,
                cursor: None,
                authoritative: Some(source),
                encoded: None,
                encoded_position: 0,
                schema,
                controls,
                _schema_memory: memory,
            }));
        }
        let Some(cursor) = cassie
            .open_session_row_cursor_with_traversal(
                session,
                collection,
                RowDecode::ProjectedHistorical(fields.to_vec()),
                traversal,
                controls,
            )
            .map_err(|error| controlled_storage_error(cassie, error))?
        else {
            return Ok(None);
        };
        cassie.runtime.record_parallel_scan_fallback();
        Ok(Some(Self {
            cassie,
            cursor: Some(cursor),
            authoritative: None,
            encoded: None,
            encoded_position: 0,
            schema,
            controls,
            _schema_memory: memory,
        }))
    }

    #[cfg(test)]
    pub(crate) fn next_batch(&mut self) -> Result<Option<TypedBatch>, QueryError> {
        self.next_batch_bounded(DEFAULT_BATCH_SIZE)
    }

    pub(crate) fn next_batch_bounded(
        &mut self,
        max_rows: usize,
    ) -> Result<Option<TypedBatch>, QueryError> {
        conversion::check_controls(self.controls)?;
        if let Some(source) = &self.encoded {
            let Some(segment) = source.segments.get(self.encoded_position) else {
                return Ok(None);
            };
            let _memory = self.controls.reserve_query_memory(mul(
                self.schema.len(),
                size_of::<crate::executor::typed_batch::Column>(),
            )?)?;
            let mut columns = Vec::with_capacity(self.schema.len());
            let mut lanes = ProfileLanes::default();
            for (name, data_type) in &self.schema {
                let field = segment
                    .fields
                    .iter()
                    .find(|field| {
                        crate::sql::ColumnIdentifierPath::matches_stored_field(
                            name,
                            field.get().name(),
                        )
                    })
                    .ok_or_else(|| {
                        QueryError::General("validated encoded field missing".to_owned())
                    })?;
                if field.get().boolean_values().is_some() {
                    lanes.boolean = lanes.boolean.saturating_add(segment.row_ids.len());
                } else if field.get().numeric_values().is_some() {
                    lanes.numeric = lanes.numeric.saturating_add(segment.row_ids.len());
                } else {
                    lanes.json = lanes.json.saturating_add(field.get().values().len());
                    lanes.values = lanes.values.saturating_add(field.get().values().len());
                }
                columns.push(crate::executor::typed_batch::Column::from_encoded(
                    self.controls,
                    data_type,
                    field,
                )?);
            }
            let batch = TypedBatch::from_views(
                self.controls,
                &self.schema,
                &columns,
                segment.row_ids.len(),
                None,
            )?;
            self.encoded_position += 1;
            profile_batch("encoded", &batch, lanes);
            return Ok(Some(batch));
        }
        if let Some(source) = self.authoritative.as_mut() {
            return source.next_batch(self.cassie, &self.schema, max_rows, self.controls);
        }
        self.next_document_batch(max_rows)
    }

    fn next_document_batch(&mut self, max_rows: usize) -> Result<Option<TypedBatch>, QueryError> {
        let documents = self
            .cursor
            .as_mut()
            .expect("row source exists without encoded source")
            .next_accounted_documents(
                &self.cassie.midge,
                DEFAULT_BATCH_SIZE.min(max_rows),
                self.controls,
            )
            .map_err(|error| controlled_storage_error(self.cassie, error))?;
        if documents.is_empty() {
            return Ok(None);
        }
        let bytes = mul(
            self.schema.len(),
            add(
                size_of::<Vec<Value>>(),
                mul(documents.len(), size_of::<Value>())?,
            )?,
        )?;
        let mut memory = self.controls.reserve_query_memory(bytes)?;
        let mut columns = Vec::with_capacity(self.schema.len());
        let mut lanes = ProfileLanes::default();
        for (name, data_type) in &self.schema {
            // Classify the same branch as the per-lane conversion below, once per column.
            if is_row_identity_column(name) {
                lanes.identity = lanes.identity.saturating_add(documents.len());
            } else {
                lanes.documents = lanes.documents.saturating_add(documents.len());
            }
            lanes.values = lanes.values.saturating_add(documents.len());
            let mut values = Vec::with_capacity(documents.len());
            for source in &documents {
                conversion::check_controls(self.controls)?;
                let document = source.document();
                let mut value = if is_row_identity_column(name) {
                    memory.try_grow(document.id.len())?;
                    Value::String(document.id.clone())
                } else {
                    document
                        .payload
                        .as_object()
                        .and_then(|object| projected_field_value(object, name))
                        .map_or(Ok(Value::Null), |value| {
                            // Admit the decoded carrier while the original source guard remains live.
                            let heap = match value {
                                serde_json::Value::String(text) => text.len(),
                                _ => crate::runtime::accounted::json::retained_bytes(value)?,
                            };
                            memory.try_grow(heap)?;
                            try_json_to_typed_value(value, data_type)
                        })?
                };
                super::hydrate_character_array_value(
                    &mut value,
                    data_type,
                    self.controls,
                    &mut memory,
                )?;
                values.push(value);
            }
            columns.push(values);
        }
        let batch =
            TypedBatch::from_columns(self.controls, &self.schema, &columns, documents.len(), None)?;
        self.cassie
            .runtime
            .record_storage_access("data", false, true);
        profile_batch("documents", &batch, lanes);
        Ok(Some(batch))
    }
}

#[derive(Clone, Copy, Default)]
struct ProfileLanes {
    documents: usize,
    identity: usize,
    values: usize,
    numeric: usize,
    boolean: usize,
    json: usize,
}

fn profile_batch(source: &'static str, batch: &TypedBatch, lanes: ProfileLanes) {
    // Completed-batch observations, not allocation counts or whole-query success.
    tracing::trace!(target: "cassie::typed_profile",
        event = "typed_transport_batch", source, rows = batch.len(), fields = batch.schema().len(),
        document_field_lanes = lanes.documents, document_identity_lanes = lanes.identity,
        value_carrier_slots = lanes.values,
        value_carrier_bytes = lanes.values.saturating_mul(size_of::<crate::types::Value>()),
        encoded_numeric_lanes = lanes.numeric, encoded_boolean_lanes = lanes.boolean,
        encoded_json_lanes = lanes.json,
    );
}

#[cfg(test)]
#[path = "typed/profile_tests.rs"]
mod profile_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldSchema, Schema};

    #[test]
    fn should_retain_validated_encoded_backing_after_advancing_the_scan() {
        // Arrange
        let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
        let path =
            std::env::temp_dir().join(format!("cassie-typed-encoded-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE typed_encoded (n BIGINT)",
            "INSERT INTO typed_encoded (n) VALUES (9007199254740993), (NULL), (7)",
            "CREATE INDEX typed_encoded_idx ON typed_encoded USING column (n) WITH (segment_size = 1)",
        ] { cassie.execute_sql(&session, sql, vec![]).expect("setup"); }
        let controls = QueryExecutionControls::from_limits(
            &cassie.runtime.limits(),
            std::time::Instant::now(),
        );

        // Act
        let reads_before = cassie.runtime.snapshot().storage.data.reads;
        let mut stream = TypedScanStream::open(
            &cassie,
            Some(&session),
            "typed_encoded",
            &["n".to_owned()],
            &controls,
        )
        .expect("open")
        .expect("typed stream");
        let encoded = stream.is_encoded();
        let object_lanes = stream
            .encoded
            .as_ref()
            .expect("encoded source")
            .segments
            .iter()
            .flat_map(|segment| &segment.fields)
            .map(|field| field.get().values().len())
            .sum::<usize>();
        let source_owner = std::sync::Arc::downgrade(
            &stream.encoded.as_ref().expect("encoded source").segments[0].fields[0],
        );
        let first = stream
            .next_batch()
            .expect("first read")
            .expect("first batch");
        let value = first.value(0, 0).expect("first value");
        let retained_slice = first.slice(&controls, 0, 1).expect("derived slice");
        drop(first);
        while stream.next_batch().expect("advance").is_some() {}
        drop(stream);

        // Assert
        assert!(encoded);
        assert_eq!(
            object_lanes, 0,
            "numeric encoded owners do not materialize JSON lanes"
        );
        assert!(
            cassie.runtime.snapshot().storage.data.reads > reads_before,
            "encoded reads remain observable"
        );
        assert!(
            source_owner.upgrade().is_some(),
            "derived view retains the encoded source lease"
        );
        assert_eq!(
            retained_slice.value(0, 0).expect("retained first value"),
            value
        );
        assert!(controls.current_query_memory_bytes() > 0);
        drop(retained_slice);
        assert!(
            source_owner.upgrade().is_none(),
            "final view releases the source lease"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn should_read_controlled_midge_documents_directly_into_typed_columns() {
        // Arrange
        let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
        let path = std::env::temp_dir().join(format!("cassie-typed-scan-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_owned(),
                data_type: DataType::BigInt,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("typed", schema.clone())
            .expect("collection");
        cassie.register_collection("typed", schema);
        cassie
            .midge
            .put_document(
                "typed",
                Some("a".to_owned()),
                serde_json::json!({"n": 9_007_199_254_740_993_i64}),
            )
            .expect("first");
        cassie
            .midge
            .put_document(
                "typed",
                Some("b".to_owned()),
                serde_json::json!({"n": null}),
            )
            .expect("second");
        let controls = QueryExecutionControls::from_limits(
            &cassie.runtime.limits(),
            std::time::Instant::now(),
        );

        // Act
        let mut stream =
            TypedScanStream::open(&cassie, None, "typed", &["n".to_owned()], &controls)
                .expect("open")
                .expect("typed path");
        let batch = stream.next_batch().expect("read").expect("batch");

        // Assert
        assert_eq!(batch.schema(), &[("n".to_owned(), DataType::BigInt)]);
        assert_eq!(
            batch.value(0, 0).expect("exact integer"),
            Value::Int64(9_007_199_254_740_993)
        );
        assert_eq!(batch.value(0, 1).expect("SQL NULL"), Value::Null);
        assert!(stream.next_batch().expect("terminal read").is_none());
        drop((batch, stream));
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}
