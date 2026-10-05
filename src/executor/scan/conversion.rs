use super::{
    document_to_row, projected_document_matches, projected_document_to_row, Cassie,
    CollectionSchema, ProjectedDocumentFilter,
};
use crate::app::CassieError;
use crate::executor::batch::Batch;
use crate::executor::QueryError;
use crate::midge::adapter::{AccountedDocument, DocumentRef};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

mod accounting;

pub(super) enum InputBatch {
    Accounted(Vec<AccountedDocument>),
    Owned(Vec<DocumentRef>),
}

impl InputBatch {
    fn len(&self) -> usize {
        match self {
            Self::Accounted(documents) => documents.len(),
            Self::Owned(documents) => documents.len(),
        }
    }

    fn document(&self, index: usize) -> &DocumentRef {
        match self {
            Self::Accounted(documents) => documents[index].document(),
            Self::Owned(documents) => &documents[index],
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Shape<'a> {
    Full,
    Projected {
        fields: &'a [String],
        filter: Option<&'a ProjectedDocumentFilter>,
    },
}

pub(super) struct Request<'a> {
    pub(super) schema: Option<&'a CollectionSchema>,
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) shape: Shape<'a>,
    pub(super) parallel: bool,
    #[cfg(test)]
    pub(super) after_row: Option<&'a (dyn Fn() + Sync)>,
}

pub(super) struct ConvertedBatches {
    pub(super) batches: Vec<Batch>,
    pub(super) memory: QueryMemoryReservation,
}

pub(super) fn projected_row_bytes(
    document: &DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> Result<usize, CassieError> {
    accounting::projected_row_bytes(document, fields, schema)
}

pub(super) fn projected_value_heap(
    document: &DocumentRef,
    field: &str,
    schema: Option<&CollectionSchema>,
) -> Result<usize, CassieError> {
    if crate::types::row_identity::is_identity_reference(
        field,
        schema.is_some_and(CollectionSchema::declares_id),
    ) {
        return Ok(document.id.len());
    }
    let value = document
        .payload
        .as_object()
        .and_then(|object| super::projected_field_value(object, field));
    value.map_or(Ok(0), |value| {
        accounting::value_heap(value, super::field_data_type(schema, field))
    })
}

pub(super) fn projected_lookup_bytes(
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> Result<usize, CassieError> {
    let schema_has_id = schema.is_some_and(CollectionSchema::declares_id);
    let mut count = 1;
    let mut names = crate::types::row_identity::ROW_IDENTITY_COLUMN.len();
    for field in fields {
        if !crate::types::row_identity::is_identity_reference(field, schema_has_id) {
            count = accounting::add(count, 1)?;
            names = accounting::add(names, field.len())?;
        }
    }
    lookup_bytes(count, names)
}

pub(super) fn lookup_bytes(entries: usize, names_bytes: usize) -> Result<usize, CassieError> {
    accounting::add(
        accounting::hash_table_bytes::<(String, usize)>(entries)?,
        names_bytes,
    )
}

pub(super) fn check_controls(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled.into());
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded.into());
    }
    Ok(())
}

pub(super) fn convert(
    cassie: &Cassie,
    inputs: Vec<InputBatch>,
    request: &Request<'_>,
) -> Result<ConvertedBatches, QueryError> {
    check_controls(request.controls)?;
    // Reserve identifier resolution scratch before the estimator or row builder parses names.
    let worker_limit = if request.parallel {
        cassie.runtime.limits().parallel_scan_workers.max(1)
    } else {
        1
    };
    let workers = worker_limit.min(inputs.len());
    let scratch = accounting::identifier_scratch(&inputs, request, workers)?;
    let mut memory = request.controls.reserve_query_memory(scratch)?;
    memory.try_grow(accounting::conversion_bytes(&inputs, request, workers)?)?;
    check_controls(request.controls)?;
    let batches = if workers < 2 {
        let batches = convert_serial(inputs, request)?;
        check_controls(request.controls)?;
        cassie.runtime.record_parallel_scan_fallback();
        batches
    } else if let Some(worker_guard) = cassie.runtime.try_acquire_operator_workers(worker_limit) {
        let workers = worker_guard.workers().min(inputs.len());
        let batches = convert_parallel(inputs, request, workers)?;
        check_controls(request.controls)?;
        let rows = batches.iter().map(Vec::len).sum::<usize>();
        cassie
            .runtime
            .record_parallel_scan(workers, batches.len(), rows);
        batches
    } else {
        let batches = convert_serial(inputs, request)?;
        check_controls(request.controls)?;
        cassie.runtime.record_parallel_scan_fallback();
        batches
    };
    Ok(ConvertedBatches { batches, memory })
}

fn convert_serial(
    inputs: Vec<InputBatch>,
    request: &Request<'_>,
) -> Result<Vec<Batch>, QueryError> {
    let mut output = Vec::with_capacity(inputs.len());
    for input in inputs {
        let batch = convert_batch(&input, request)?;
        if !batch.is_empty() {
            output.push(batch);
        }
    }
    Ok(output)
}

fn convert_batch(input: &InputBatch, request: &Request<'_>) -> Result<Batch, QueryError> {
    let mut rows = Vec::with_capacity(input.len());
    for index in 0..input.len() {
        check_controls(request.controls)?;
        let document = input.document(index);
        let row = match request.shape {
            Shape::Full => Some(document_to_row(document, request.schema)),
            Shape::Projected { fields, filter } => filter
                .is_none_or(|filter| projected_document_matches(&document.payload, filter))
                .then(|| projected_document_to_row(document, fields, request.schema)),
        };
        if let Some(row) = row {
            rows.push(row);
        }
        #[cfg(test)]
        if let Some(after_row) = request.after_row {
            after_row();
        }
        check_controls(request.controls)?;
    }
    Ok(rows)
}

fn convert_parallel(
    inputs: Vec<InputBatch>,
    request: &Request<'_>,
    workers: usize,
) -> Result<Vec<Batch>, QueryError> {
    let input_count = inputs.len();
    let mut partitions = Vec::with_capacity(workers);
    for worker in 0..workers {
        let count = input_count / workers + usize::from(worker < input_count % workers);
        partitions.push(Vec::with_capacity(count));
    }
    for (index, input) in inputs.into_iter().enumerate() {
        partitions[index % workers].push((index, input));
    }
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for partition in partitions {
            handles.push(scope.spawn(move || {
                let mut output = Vec::with_capacity(partition.len());
                for (index, input) in partition {
                    output.push((index, convert_batch(&input, request)?));
                }
                Ok::<_, QueryError>(output)
            }));
        }
        let mut joined = Vec::with_capacity(input_count);
        let mut failure = None;
        // Join every admitted worker before dropping the shared output reservation on error.
        for handle in handles {
            match super::super::worker::join_scoped_worker(handle, "parallel scan worker panicked")
            {
                Ok(Ok(output)) => joined.extend(output),
                Ok(Err(error)) | Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }
        failure.map_or(Ok(joined), Err)
    })?;
    check_controls(request.controls)?;
    indexed.sort_unstable_by_key(|(index, _)| *index);
    let mut batches = Vec::with_capacity(input_count);
    for (_, batch) in indexed {
        if !batch.is_empty() {
            batches.push(batch);
        }
    }
    Ok(batches)
}

pub(super) fn convert_owned(
    cassie: &Cassie,
    documents: Vec<Vec<DocumentRef>>,
    request: &Request<'_>,
) -> Result<ConvertedBatches, QueryError> {
    let _input_memory = request.controls.reserve_query_memory(accounting::mul(
        documents.len(),
        std::mem::size_of::<InputBatch>(),
    )?)?;
    let inputs = documents.into_iter().map(InputBatch::Owned).collect();
    convert(cassie, inputs, request)
}

#[cfg(test)]
mod tests;
