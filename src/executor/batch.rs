use std::collections::HashMap;
use std::sync::OnceLock;

use crate::types::Value;

#[derive(Debug, Clone)]
pub(crate) struct BatchRow {
    values: Vec<(String, Value)>,
    lookup: OnceLock<HashMap<String, usize>>,
}

impl BatchRow {
    pub(crate) fn new(values: Vec<(String, Value)>) -> Self {
        let lookup = OnceLock::new();
        let _ = lookup.set(build_lookup(values.as_slice()));

        Self { values, lookup }
    }

    pub(crate) fn from_projected_values(values: Vec<(String, Value)>) -> Self {
        Self {
            values,
            lookup: OnceLock::new(),
        }
    }

    pub(crate) fn entries(&self) -> &[(String, Value)] {
        &self.values
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        let lookup = self
            .lookup
            .get_or_init(|| build_lookup(self.values.as_slice()));
        let index = *lookup.get(name)?;
        let entry = &self.values[index];
        Some(&entry.1)
    }

    pub(crate) fn into_entries(self) -> Vec<(String, Value)> {
        self.values
    }

    pub(crate) fn into_values(self) -> Vec<Value> {
        self.values.into_iter().map(|(_, value)| value).collect()
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

fn build_lookup(values: &[(String, Value)]) -> HashMap<String, usize> {
    let mut lookup = HashMap::with_capacity(values.len());
    for (index, (name, _)) in values.iter().enumerate() {
        lookup.entry(name.clone()).or_insert(index);
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

pub(crate) fn chunk_rows(rows: Vec<BatchRow>, batch_size: usize) -> Vec<Batch> {
    let batch_size = batch_size.max(1);
    if rows.is_empty() {
        return Vec::new();
    }

    rows.chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
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
    row.entries()
        .iter()
        .map(|(_, value)| value_to_key(value))
        .collect::<Vec<_>>()
        .join("|")
}

fn value_to_key(value: &Value) -> String {
    match value {
        Value::Null => String::from("<null>"),
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Vector(v) => v
            .values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Json(v) => v.to_string(),
    }
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
