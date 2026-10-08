#[path = "batch/operator_memory.rs"]
mod operator_memory;
pub(crate) use operator_memory::OperatorMemory;

#[cfg(test)]
#[path = "batch/operator_memory_tests.rs"]
mod operator_memory_tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::app::CassieError;
use crate::executor::retained_memory::data_type_clone_bytes;
use crate::types::{DataType, Value};

pub(crate) type RowEntries = Vec<(String, Value)>;
pub(crate) type RowAliases = Vec<(String, usize)>;

#[derive(Debug, Clone)]
pub(crate) struct BatchRow {
    operator_memory: Option<Arc<operator_memory::OperatorMemory>>,
    values: RowEntries,
    aliases: RowAliases,
    lookup: OnceLock<HashMap<String, usize>>,
    outer_scope: Option<Arc<Self>>,
    data_types: Option<Arc<Vec<DataType>>>,
    // A clone keeps its origin alive; this lease never admits the clone's new allocations.
    query_memory: Option<Arc<crate::runtime::QueryMemoryReservation>>,
}

impl BatchRow {
    pub(crate) fn new(values: RowEntries) -> Self {
        Self::with_aliases(values, Vec::new())
    }

    pub(crate) fn with_aliases(values: RowEntries, aliases: RowAliases) -> Self {
        let lookup = OnceLock::new();
        let _ = lookup.set(build_lookup(values.as_slice(), aliases.as_slice()));

        Self {
            values,
            aliases,
            lookup,
            outer_scope: None,
            data_types: None,
            query_memory: None,
            operator_memory: None,
        }
    }

    pub(crate) fn from_projected_values(values: RowEntries) -> Self {
        Self {
            values,
            aliases: Vec::new(),
            lookup: OnceLock::new(),
            outer_scope: None,
            data_types: None,
            query_memory: None,
            operator_memory: None,
        }
    }

    pub(crate) fn entries(&self) -> &[(String, Value)] {
        &self.values
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        let lookup = self
            .lookup
            .get_or_init(|| build_lookup(self.values.as_slice(), self.aliases.as_slice()));
        let column = crate::sql::ColumnIdentifierPath::parse(name).ok();
        let index = lookup.get(name).copied().or_else(|| {
            column.as_ref().and_then(|column| {
                let candidates = column.row_lookup_candidates();
                let candidate_count = candidates.len().saturating_sub(usize::from(
                    self.outer_scope.is_some() && column.is_qualified(),
                ));
                candidates
                    .iter()
                    .take(candidate_count)
                    .find_map(|candidate| lookup.get(candidate).copied())
            })
        });
        let Some(index) = index else {
            return self.outer_scope.as_ref()?.get(name);
        };
        let entry = &self.values[index];
        Some(&entry.1)
    }

    pub(crate) fn into_entries(self) -> RowEntries {
        self.values
    }

    pub(crate) fn into_values(self) -> Vec<Value> {
        self.values.into_iter().map(|(_, value)| value).collect()
    }

    pub(crate) fn aliases(&self) -> &[(String, usize)] {
        self.aliases.as_slice()
    }

    /// Existing buffers survive a move; clone estimates cover only populated slots and names.
    pub(crate) fn retained_buffer_spare_bytes(&self) -> Result<usize, CassieError> {
        use std::mem::size_of;

        use crate::executor::retained_memory::{add, mul};

        let bytes = add(
            mul(
                self.values.capacity() - self.values.len(),
                size_of::<(String, Value)>(),
            )?,
            mul(
                self.aliases.capacity() - self.aliases.len(),
                size_of::<(String, usize)>(),
            )?,
        )?;
        self.values
            .iter()
            .map(|(name, _)| name)
            .chain(self.aliases.iter().map(|(name, _)| name))
            .try_fold(bytes, |bytes, name| {
                add(bytes, name.capacity() - name.len())
            })
    }

    pub(crate) fn with_outer_scope(mut self, outer_scope: Arc<Self>) -> Self {
        self.outer_scope = Some(outer_scope);
        self
    }

    pub(crate) fn has_outer_scope(&self) -> bool {
        self.outer_scope.is_some()
    }

    pub(crate) fn into_parts(self) -> (RowEntries, RowAliases) {
        (self.values, self.aliases)
    }

    pub(crate) fn set_data_types(&mut self, data_types: Arc<Vec<DataType>>) {
        self.data_types = (!data_types.is_empty()).then_some(data_types);
    }

    pub(crate) fn data_types(&self) -> &[DataType] {
        self.data_types
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn shared_data_types(&self) -> Option<Arc<Vec<DataType>>> {
        self.data_types.clone()
    }

    pub(crate) fn query_memory(&self) -> Option<Arc<crate::runtime::QueryMemoryReservation>> {
        self.query_memory.clone()
    }

    pub(crate) fn with_query_memory(
        mut self,
        query_memory: Option<Arc<crate::runtime::QueryMemoryReservation>>,
    ) -> Self {
        self.query_memory = query_memory;
        self
    }

    pub(crate) fn with_optional_data_types(
        mut self,
        data_types: Option<Arc<Vec<DataType>>>,
    ) -> Self {
        self.data_types = data_types;
        self
    }

    pub(crate) fn append_value(&mut self, name: String, value: Value) {
        self.values.push((name, value));
        self.lookup = OnceLock::new();
    }

    fn column_type(&self, name: &str) -> Option<&DataType> {
        let lookup = self
            .lookup
            .get_or_init(|| build_lookup(&self.values, &self.aliases));
        let column = crate::sql::ColumnIdentifierPath::parse(name).ok();
        let index = lookup.get(name).copied().or_else(|| {
            column.as_ref().and_then(|column| {
                let candidates = column.row_lookup_candidates();
                let candidate_count = candidates.len().saturating_sub(usize::from(
                    self.outer_scope.is_some() && column.is_qualified(),
                ));
                candidates
                    .iter()
                    .take(candidate_count)
                    .find_map(|candidate| lookup.get(candidate).copied())
            })
        });
        if let Some(index) = index {
            self.data_types().get(index)
        } else {
            self.outer_scope.as_ref()?.column_type(name)
        }
    }

    pub(crate) fn lookup_initialized(&self) -> bool {
        self.lookup.get().is_some()
    }
}

pub(crate) trait RowAccess {
    fn operator_memory(&self) -> Option<Arc<OperatorMemory>> {
        None
    }
    fn query_memory(&self) -> Option<Arc<crate::runtime::QueryMemoryReservation>> {
        None
    }
    fn get(&self, name: &str) -> Option<&Value>;
    fn entries(&self) -> &[(String, Value)];
    fn column_type(&self, _name: &str) -> Option<&DataType> {
        None
    }
    fn has_array_types(&self) -> bool {
        false
    }
    fn entry_type(&self, _index: usize) -> Option<&DataType> {
        None
    }
    fn maximum_type_heap_bytes(&self) -> Result<usize, CassieError> {
        (0..self.entries().len()).try_fold(0, |maximum, index| {
            self.entry_type(index).map_or(Ok(maximum), |data_type| {
                Ok(maximum.max(data_type_clone_bytes(data_type)?))
            })
        })
    }
}

fn build_lookup(values: &[(String, Value)], aliases: &[(String, usize)]) -> HashMap<String, usize> {
    let mut lookup = HashMap::with_capacity(values.len() + aliases.len());
    for (index, (name, _)) in values.iter().enumerate() {
        lookup.entry(name.clone()).or_insert(index);
    }
    for (name, index) in aliases {
        lookup.entry(name.clone()).or_insert(*index);
    }
    lookup
}

impl RowAccess for BatchRow {
    fn operator_memory(&self) -> Option<Arc<OperatorMemory>> {
        BatchRow::operator_memory(self)
    }
    fn query_memory(&self) -> Option<Arc<crate::runtime::QueryMemoryReservation>> {
        BatchRow::query_memory(self)
    }
    fn maximum_type_heap_bytes(&self) -> Result<usize, CassieError> {
        let own = self.data_types().iter().try_fold(0, |maximum, data_type| {
            Ok::<_, CassieError>(maximum.max(data_type_clone_bytes(data_type)?))
        })?;
        self.outer_scope.as_ref().map_or(Ok(own), |outer| {
            Ok(own.max(outer.maximum_type_heap_bytes()?))
        })
    }

    fn entry_type(&self, index: usize) -> Option<&DataType> {
        self.data_types().get(index)
    }
    fn has_array_types(&self) -> bool {
        self.data_types()
            .iter()
            .any(|data_type| matches!(data_type, DataType::Array(_)))
            || self
                .outer_scope
                .as_ref()
                .is_some_and(|row| row.has_array_types())
    }
    fn column_type(&self, name: &str) -> Option<&DataType> {
        BatchRow::column_type(self, name)
    }

    fn get(&self, name: &str) -> Option<&Value> {
        BatchRow::get(self, name)
    }

    fn entries(&self) -> &[(String, Value)] {
        BatchRow::entries(self)
    }
}

impl RowAccess for Vec<(String, Value)> {
    fn get(&self, name: &str) -> Option<&Value> {
        entry_value(self, name)
    }

    fn entries(&self) -> &[(String, Value)] {
        self.as_slice()
    }
}

impl RowAccess for [(String, Value)] {
    fn get(&self, name: &str) -> Option<&Value> {
        entry_value(self, name)
    }

    fn entries(&self) -> &[(String, Value)] {
        self
    }
}

/// Looks a canonical SQL column reference up in row entries.
fn entry_value<'a>(entries: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    let column = crate::sql::ColumnIdentifierPath::parse(name).ok();
    let exact = entries
        .iter()
        .find(|(column, _)| column == name)
        .or_else(|| {
            column.as_ref().and_then(|column| {
                column
                    .row_lookup_candidates()
                    .iter()
                    .find_map(|candidate| entries.iter().find(|(entry, _)| entry == candidate))
            })
        });
    exact.map(|(_, value)| value)
}

/// Shared declared types are accounted once across rows retaining the same
/// allocation. ARRAY element types own their boxed schema metadata too.
pub(crate) fn row_type_bytes<'a>(rows: impl IntoIterator<Item = &'a BatchRow>) -> usize {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter_map(|row| row.data_types.as_ref())
        .filter(|types| seen.insert(types.as_ptr()))
        .fold(0_usize, |bytes, types| {
            let retained = types
                .capacity()
                .saturating_mul(std::mem::size_of::<DataType>())
                .saturating_add(
                    types
                        .iter()
                        .map(|data_type| match data_type {
                            DataType::Array(element) => data_type_bytes(element),
                            _ => 0,
                        })
                        .sum::<usize>(),
                )
                .saturating_add(std::mem::size_of::<Vec<DataType>>())
                .saturating_add(2 * std::mem::size_of::<usize>());
            bytes.saturating_add(retained)
        })
}

fn data_type_bytes(data_type: &DataType) -> usize {
    std::mem::size_of::<DataType>().saturating_add(match data_type {
        DataType::Array(element) => data_type_bytes(element),
        _ => 0,
    })
}

pub(crate) const DEFAULT_BATCH_SIZE: usize = 1024;

pub(crate) type Batch = Vec<BatchRow>;

pub(crate) trait BatchStream {
    fn next_batch(&mut self) -> Result<Option<Batch>, crate::executor::QueryError>;
}

pub(crate) struct VecBatchStream {
    batches: std::vec::IntoIter<Batch>,
}

impl VecBatchStream {
    pub(crate) fn new(batches: Vec<Batch>) -> Self {
        Self {
            batches: batches.into_iter(),
        }
    }
}

impl BatchStream for VecBatchStream {
    fn next_batch(&mut self) -> Result<Option<Batch>, crate::executor::QueryError> {
        Ok(self.batches.next())
    }
}

static BATCH_BUFFERS_BUILT: AtomicU64 = AtomicU64::new(0);
static ROW_TIE_KEYS_BUILT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn chunk_rows(rows: Vec<BatchRow>, batch_size: usize) -> Vec<Batch> {
    let batch_size = batch_size.max(1);
    if rows.is_empty() {
        return Vec::new();
    }

    let mut batches = Vec::with_capacity(rows.len().div_ceil(batch_size));
    let mut remaining = rows.len();
    let mut current = Vec::with_capacity(batch_size.min(remaining));
    for row in rows {
        current.push(row);
        remaining -= 1;
        if current.len() == batch_size {
            BATCH_BUFFERS_BUILT.fetch_add(1, Ordering::Relaxed);
            batches.push(current);
            current = Vec::with_capacity(batch_size.min(remaining));
        }
    }
    if !current.is_empty() {
        BATCH_BUFFERS_BUILT.fetch_add(1, Ordering::Relaxed);
        batches.push(current);
    }
    batches
}

pub(crate) fn flatten_batches(batches: Vec<Batch>) -> Vec<BatchRow> {
    batches.into_iter().flatten().collect()
}

pub(crate) fn try_flatten_batches(
    batches: Vec<Batch>,
) -> Result<Vec<BatchRow>, crate::executor::QueryError> {
    let mut stream = VecBatchStream::new(batches);
    collect_batch_stream(&mut stream)
}

pub(crate) fn collect_batch_stream(
    stream: &mut dyn BatchStream,
) -> Result<Vec<BatchRow>, crate::executor::QueryError> {
    let mut rows = Vec::new();
    while let Some(batch) = stream.next_batch()? {
        rows.extend(batch);
    }
    Ok(rows)
}

pub(crate) fn collect_batch_stream_accounted(
    stream: &mut dyn BatchStream,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(Vec<BatchRow>, Vec<crate::runtime::QueryMemoryReservation>), crate::executor::QueryError>
{
    let mut rows = Vec::new();
    let mut reservations = Vec::new();
    while let Some(batch) = stream.next_batch()? {
        let bytes = batch
            .iter()
            .map(|row| serde_json::to_vec(row.entries()).map_or(0, |bytes| bytes.len()))
            .sum::<usize>()
            .saturating_add(row_type_bytes(&batch));
        reservations.push(controls.reserve_query_memory(bytes)?);
        rows.extend(batch);
    }
    Ok((rows, reservations))
}

pub(crate) fn slice_batches(
    batches: Vec<Batch>,
    offset: usize,
    limit: Option<usize>,
) -> Vec<Batch> {
    if batches.is_empty() {
        return batches;
    }

    let mut remaining_offset = offset;
    let mut remaining_limit = limit;
    let mut out = Vec::new();

    for batch in batches {
        if remaining_limit == Some(0) {
            break;
        }

        let mut rows = Vec::new();
        for row in batch {
            if remaining_offset > 0 {
                remaining_offset -= 1;
                continue;
            }

            if let Some(limit) = remaining_limit.as_mut() {
                if *limit == 0 {
                    break;
                }
                *limit -= 1;
            }

            rows.push(row);
        }

        if !rows.is_empty() {
            out.push(rows);
        }
    }

    out
}

pub(crate) fn row_tie_key(row: &impl RowAccess) -> String {
    ROW_TIE_KEYS_BUILT.fetch_add(1, Ordering::Relaxed);
    let entries = row.entries();
    let mut out = String::new();
    for (index, (_, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push('|');
        }
        push_value_key(&mut out, value);
    }
    out
}

fn push_value_key(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("<null>"),
        Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Value::Int64(v) => out.push_str(&v.to_string()),
        Value::Float64(v) => out.push_str(&v.to_string()),
        Value::String(v) => out.push_str(v),
        Value::Vector(v) => {
            for (index, value) in v.values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&value.to_string());
            }
        }
        Value::Json(v) => out.push_str(&v.to_string()),
    }
}

#[cfg(test)]
pub(crate) fn batch_buffers_built_for_tests() -> u64 {
    BATCH_BUFFERS_BUILT.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn row_tie_keys_built_for_tests() -> u64 {
    ROW_TIE_KEYS_BUILT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_pull_batches_until_stream_is_exhausted() {
        // Arrange
        let first = vec![BatchRow::new(vec![("id".to_string(), Value::Int64(1))])];
        let second = vec![BatchRow::new(vec![("id".to_string(), Value::Int64(2))])];
        let mut stream = VecBatchStream::new(vec![first, second]);

        // Act
        let first = stream.next_batch().expect("first batch");
        let second = stream.next_batch().expect("second batch");
        let exhausted = stream.next_batch().expect("exhausted stream");

        // Assert
        assert_eq!(
            first.expect("first rows")[0].get("id"),
            Some(&Value::Int64(1))
        );
        assert_eq!(
            second.expect("second rows")[0].get("id"),
            Some(&Value::Int64(2))
        );
        assert!(exhausted.is_none());
    }

    #[test]
    fn should_propagate_fallible_batch_stream_error() {
        struct FailingStream;

        impl BatchStream for FailingStream {
            fn next_batch(&mut self) -> Result<Option<Batch>, crate::executor::QueryError> {
                Err(crate::executor::QueryError::General(
                    "stream failed".to_string(),
                ))
            }
        }

        // Arrange
        let mut stream = FailingStream;

        // Act
        let error = collect_batch_stream(&mut stream).expect_err("stream should fail");

        // Assert
        assert_eq!(error.to_string(), "stream failed");
    }

    #[test]
    fn should_chunk_rows_into_batches() {
        // Arrange
        let rows = vec![
            BatchRow::new(vec![("id".to_string(), Value::String("a".to_string()))]),
            BatchRow::new(vec![("id".to_string(), Value::String("b".to_string()))]),
            BatchRow::new(vec![("id".to_string(), Value::String("c".to_string()))]),
            BatchRow::new(vec![("id".to_string(), Value::String("d".to_string()))]),
            BatchRow::new(vec![("id".to_string(), Value::String("e".to_string()))]),
        ];

        // Act
        let batches = chunk_rows(rows, 2);

        // Assert
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 2);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn should_retain_only_actual_row_slots_in_partial_batches() {
        // Arrange
        let row_counts = [
            0,
            1,
            DEFAULT_BATCH_SIZE,
            DEFAULT_BATCH_SIZE + 1,
            2 * DEFAULT_BATCH_SIZE,
        ];
        let cases: Vec<_> = [0, DEFAULT_BATCH_SIZE]
            .into_iter()
            .flat_map(|batch_size| row_counts.map(|count| (batch_size, count)))
            .map(|(batch_size, count)| {
                let rows = (0..count)
                    .map(|index| {
                        BatchRow::new(vec![(
                            "id".to_owned(),
                            Value::Int64(i64::try_from(index).expect("fixture row id")),
                        )])
                    })
                    .collect();
                (batch_size, count, rows)
            })
            .collect();

        // Act
        let results: Vec<_> = cases
            .into_iter()
            .map(|(batch_size, count, rows)| {
                let before = batch_buffers_built_for_tests();
                let chunks = chunk_rows(rows, batch_size);
                let built = batch_buffers_built_for_tests() - before;
                (batch_size, count, chunks, built)
            })
            .collect();

        // Assert
        for (batch_size, count, chunks, built) in results {
            let expected_chunks = count.div_ceil(batch_size.max(1));
            assert_eq!(chunks.len(), expected_chunks);
            assert!(built >= u64::try_from(expected_chunks).expect("fixture chunk count"));
            assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), count);
            for chunk in &chunks {
                assert!(!chunk.is_empty());
                assert_eq!(chunk.capacity(), chunk.len());
                assert!(chunk.len() <= batch_size.max(1));
            }
            for (index, row) in chunks.iter().flatten().enumerate() {
                assert_eq!(
                    row.get("id"),
                    Some(&Value::Int64(i64::try_from(index).expect("fixture row id")))
                );
            }
            eprintln!("partial batch probe: size={batch_size}, rows={count}, expected_buffers={expected_chunks}, observed_buffers={built}");
        }
    }

    #[test]
    fn should_move_rows_into_batch_buffers_without_stale_values() {
        // Arrange
        let before = batch_buffers_built_for_tests();
        let rows = vec![
            BatchRow::new(vec![(
                "title".to_string(),
                Value::String("alpha".to_string()),
            )]),
            BatchRow::new(vec![(
                "title".to_string(),
                Value::String("beta".to_string()),
            )]),
            BatchRow::new(vec![(
                "title".to_string(),
                Value::String("gamma".to_string()),
            )]),
        ];

        // Act
        let batches = chunk_rows(rows, 2);
        let after = batch_buffers_built_for_tests();

        // Assert
        assert_eq!(batches.len(), 2);
        assert_eq!(
            batches[0][0].get("title"),
            Some(&Value::String("alpha".to_string()))
        );
        assert_eq!(
            batches[0][1].get("title"),
            Some(&Value::String("beta".to_string()))
        );
        assert_eq!(
            batches[1][0].get("title"),
            Some(&Value::String("gamma".to_string()))
        );
        assert!(after >= before + 2);
    }

    #[test]
    fn should_build_row_tie_key_without_temporary_key_vector() {
        // Arrange
        let before = row_tie_keys_built_for_tests();
        let row = BatchRow::new(vec![
            ("title".to_string(), Value::String("alpha".to_string())),
            ("score".to_string(), Value::Int64(7)),
            (
                "embedding".to_string(),
                Value::Vector(crate::types::Vector::new(vec![1.0, 2.0])),
            ),
        ]);

        // Act
        let key = row_tie_key(&row);
        let after = row_tie_keys_built_for_tests();

        // Assert
        assert_eq!(key, "alpha|7|1,2");
        assert!(after > before);
    }

    #[test]
    fn should_resolve_row_values_by_name_without_scanning_entire_row() {
        // Arrange
        let row = BatchRow::new(vec![
            ("id".to_string(), Value::String("doc-1".to_string())),
            ("title".to_string(), Value::String("alpha".to_string())),
        ]);

        // Act
        let title = row.get("title");

        // Assert
        assert_eq!(title, Some(&Value::String("alpha".to_string())));
        assert_eq!(row.entries()[0].0, "id");
        assert_eq!(row.entries()[1].0, "title");
    }

    #[test]
    fn should_resolve_projected_row_values_after_lazy_lookup_build() {
        // Arrange
        let row = BatchRow::from_projected_values(vec![
            ("id".to_string(), Value::String("doc-1".to_string())),
            ("title".to_string(), Value::String("alpha".to_string())),
        ]);

        // Act
        let title = row.get("title");

        // Assert
        assert_eq!(title, Some(&Value::String("alpha".to_string())));
    }
}
