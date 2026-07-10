use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use crate::types::Value;

pub(crate) type RowEntries = Vec<(String, Value)>;
pub(crate) type RowAliases = Vec<(String, usize)>;

#[derive(Debug, Clone)]
pub(crate) struct BatchRow {
    values: RowEntries,
    aliases: RowAliases,
    lookup: OnceLock<HashMap<String, usize>>,
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
        }
    }

    pub(crate) fn from_projected_values(values: RowEntries) -> Self {
        Self {
            values,
            aliases: Vec::new(),
            lookup: OnceLock::new(),
        }
    }

    pub(crate) fn entries(&self) -> &[(String, Value)] {
        &self.values
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        let lookup = self
            .lookup
            .get_or_init(|| build_lookup(self.values.as_slice(), self.aliases.as_slice()));
        let index = *lookup.get(name)?;
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

    pub(crate) fn into_parts(self) -> (RowEntries, RowAliases) {
        (self.values, self.aliases)
    }

    #[cfg(test)]
    pub(crate) fn lookup_initialized(&self) -> bool {
        self.lookup.get().is_some()
    }
}

pub(crate) trait RowAccess {
    fn get(&self, name: &str) -> Option<&Value>;
    fn entries(&self) -> &[(String, Value)];
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
    fn get(&self, name: &str) -> Option<&Value> {
        BatchRow::get(self, name)
    }

    fn entries(&self) -> &[(String, Value)] {
        BatchRow::entries(self)
    }
}

impl RowAccess for Vec<(String, Value)> {
    fn get(&self, name: &str) -> Option<&Value> {
        self.iter()
            .find(|(column, _)| column == name)
            .map(|(_, value)| value)
    }

    fn entries(&self) -> &[(String, Value)] {
        self.as_slice()
    }
}

impl RowAccess for [(String, Value)] {
    fn get(&self, name: &str) -> Option<&Value> {
        self.iter()
            .find(|(column, _)| column == name)
            .map(|(_, value)| value)
    }

    fn entries(&self) -> &[(String, Value)] {
        self
    }
}

pub(crate) const DEFAULT_BATCH_SIZE: usize = 1024;

pub(crate) type Batch = Vec<BatchRow>;

static BATCH_BUFFERS_BUILT: AtomicU64 = AtomicU64::new(0);
static ROW_TIE_KEYS_BUILT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn chunk_rows(rows: Vec<BatchRow>, batch_size: usize) -> Vec<Batch> {
    let batch_size = batch_size.max(1);
    if rows.is_empty() {
        return Vec::new();
    }

    let mut batches = Vec::with_capacity(rows.len().div_ceil(batch_size));
    let mut current = Vec::with_capacity(batch_size);
    for row in rows {
        current.push(row);
        if current.len() == batch_size {
            BATCH_BUFFERS_BUILT.fetch_add(1, Ordering::Relaxed);
            batches.push(current);
            current = Vec::with_capacity(batch_size);
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
