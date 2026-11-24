//! Decoding and executing a single buffered COPY input.

#[cfg(test)]
use super::filter;
use super::request::{CopyHeader, CopyLogVerbosity, CopyOnError, CopyWireFormat};
use super::{input, payload_owner, request, CassieSession, CopyRequest};

pub(super) struct CopyCompletion {
    pub(super) command: String,
    pub(super) notices: Vec<String>,
}

/// Decodes the wire payload, then delegates row semantics and publication to
/// the engine's admitted typed COPY operation.
pub(super) fn execute_decoded_operation(
    session: &CassieSession,
    request: &CopyRequest,
    operation: &crate::app::transport::copy::CopyFromOperation,
    payload: &[u8],
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyCompletion, crate::app::transport::copy::CopyOperationError> {
    use crate::app::transport::copy::{CopyExecutionOptions, CopyHeaderMode};

    let decoded_payload =
        payload_owner::decode_copy_payload(payload, request.encoding.as_deref(), controls)?;
    let payload = decoded_payload
        .as_ref()
        .map_or(payload, payload_owner::DecodedPayload::as_slice);
    let force_null = operation.force_null_indices(&copy_force_selection(&request.force_null))?;
    let force_not_null =
        operation.force_not_null_indices(&copy_force_selection(&request.force_not_null))?;
    let _decoded_rows_memory;
    let rows;
    (rows, _decoded_rows_memory) = match request.format {
        CopyWireFormat::Csv => {
            let (rows, memory) = crate::app::parse_copy_csv_payload_controlled(
                payload,
                crate::app::CopyCsvOptions {
                    delimiter: request.csv_delimiter(),
                    quote: request.quote,
                    escape: request.escape,
                    null_marker: request.null_marker(),
                    force_null: &force_null,
                    force_not_null: &force_not_null,
                },
                request.null_marker().is_empty(),
                controls,
            )?;
            (rows, Some(memory))
        }
        CopyWireFormat::Text => {
            let (rows, memory) = input::parse_text_payload(
                payload,
                request.text_delimiter(),
                request.null_marker(),
                controls,
            )?;
            (rows, Some(memory))
        }
        CopyWireFormat::Binary => {
            let decoded = operation.with_binary_columns(controls, |columns| {
                input::parse_binary_payload_with_columns(payload, columns, controls)
            })?;
            let (rows, memory) = decoded?;
            (rows, Some(memory))
        }
    };
    let header = match request.header {
        CopyHeader::None => CopyHeaderMode::None,
        CopyHeader::Skip => CopyHeaderMode::Skip,
        CopyHeader::Match => CopyHeaderMode::Match,
    };
    let result = operation.execute_decoded_rows_with_controls(
        session,
        rows,
        CopyExecutionOptions {
            header,
            where_clause: request.where_clause.as_deref(),
            default_marker: request.default_marker.as_deref(),
            on_error_ignore: request.on_error == CopyOnError::Ignore,
            reject_limit: request.reject_limit,
        },
        cancellation,
        controls,
    )?;
    let notices = if request.log_verbosity == CopyLogVerbosity::Verbose {
        skipped_row_notices(result.skipped.iter().map(|skipped| {
            (
                skipped.line,
                skipped.column.as_str(),
                skipped.value.as_str(),
            )
        }))
    } else {
        Vec::new()
    };
    Ok(CopyCompletion {
        command: format!("COPY {}", result.count),
        notices,
    })
}

fn copy_force_selection(
    selection: &request::ColumnSelection,
) -> crate::app::transport::copy::CopyForceSelection {
    match selection {
        request::ColumnSelection::None => crate::app::transport::copy::CopyForceSelection::None,
        request::ColumnSelection::All => crate::app::transport::copy::CopyForceSelection::All,
        request::ColumnSelection::Columns(names) => {
            crate::app::transport::copy::CopyForceSelection::Columns(names.clone())
        }
    }
}

#[cfg(test)]
pub(super) fn execute_bound_copy_payload(
    binding: &super::binding::CopyFromBinding,
    session: &CassieSession,
    request: &CopyRequest,
    payload: &[u8],
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
) -> Result<CopyCompletion, filter::CopyWhereError> {
    binding.operation.with_publication_scope(|controls| {
        execute_decoded_operation(
            session,
            request,
            &binding.operation,
            payload,
            cancellation,
            controls,
        )
    })
}

fn skipped_row_notices<'a>(
    skipped: impl Iterator<Item = (usize, &'a str, &'a str)>,
) -> Vec<String> {
    let mut notices = Vec::new();
    for (line, column, value) in skipped {
        notices.push(format!(
            "skipping row due to data type incompatibility at line {line} for column \"{column}\": \"{value}\""
        ));
    }
    if !notices.is_empty() {
        let count = notices.len();
        let noun = if count == 1 { "row was" } else { "rows were" };
        notices.push(format!(
            "{count} {noun} skipped due to data type incompatibility"
        ));
    }
    notices
}
