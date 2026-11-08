//! Decoding and executing a single buffered COPY input.
use super::request::{CopyHeader, CopyLogVerbosity, CopyOnError, CopyWireFormat};
use super::{
    filter, input, payload_owner, request, validation, Cassie, CassieError, CassieSession,
    CopyRequest, CopyStatement,
};

pub(super) struct CopyCompletion {
    pub(super) command: String,
    pub(super) notices: Vec<String>,
}

pub(super) fn execute_copy_payload(
    cassie: &Cassie,
    session: &CassieSession,
    request: &CopyRequest,
    statement: &CopyStatement,
    payload: &[u8],
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    owner: Option<&crate::app::CopyCommandOwner>,
) -> Result<CopyCompletion, filter::CopyWhereError> {
    let controls = crate::midge::adapter::PublicationControlScope::current_query_controls()
        .ok_or_else(|| CassieError::Execution("COPY requires request controls".into()))?;
    let decoded_payload =
        payload_owner::decode_copy_payload(payload, request.encoding.as_deref(), &controls)?;
    let payload = decoded_payload
        .as_ref()
        .map_or(payload, payload_owner::DecodedPayload::as_slice);
    let target_columns = validation::copy_target_columns(cassie, statement)?;
    let force_null = request::CopyRequest::force_indices(&request.force_null, &target_columns);
    let force_not_null =
        request::CopyRequest::force_indices(&request.force_not_null, &target_columns);
    let _decoded_rows_memory;
    let _filter_rows_memory;
    let mut rows;
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
                &controls,
            )?;
            (rows, Some(memory))
        }
        CopyWireFormat::Text => {
            let (rows, memory) = input::parse_text_payload(
                payload,
                request.text_delimiter(),
                request.null_marker(),
                &controls,
            )?;
            (rows, Some(memory))
        }
        CopyWireFormat::Binary => {
            let (rows, memory) =
                input::parse_binary_payload(payload, cassie, statement, &controls)?;
            (rows, Some(memory))
        }
    };
    if request.header == CopyHeader::Match && !rows.is_empty() {
        let fields = cassie
            .catalog
            .get_schema(&statement.table)
            .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".to_string()))?;
        let columns = crate::app::copy_columns(statement, &fields.fields)?;
        let matches = rows[0].len() == columns.len()
            && rows[0].iter().zip(&columns).all(|(header, column)| {
                let expected = match column {
                    crate::app::CopyColumn::Field(field) => field.name.as_str(),
                    crate::app::CopyColumn::RowId => "id",
                };
                header.as_deref() == Some(expected)
            });
        if !matches {
            return Err(crate::app::DiagnosticError::new(
                "22P04",
                "COPY CSV header does not match the target column list",
            )
            .into());
        }
    }
    if request.header != CopyHeader::None && !rows.is_empty() {
        rows.remove(0);
    }
    if let Some(predicate) = request.where_clause.as_deref() {
        let (accepted, memory) =
            filter::filter_rows(cassie, session, statement, rows, predicate, &controls)?;
        rows = accepted;
        _filter_rows_memory = Some(memory);
    } else {
        _filter_rows_memory = None;
    }
    let result = cassie.ingest_copy_rows_with_default_marker(
        session,
        statement,
        rows,
        cancellation,
        crate::app::CopyIngestOptions {
            default_marker: request.default_marker.as_deref(),
            on_error_ignore: request.on_error == CopyOnError::Ignore,
            reject_limit: request.reject_limit,
        },
        owner,
    )?;
    let mut notices = Vec::new();
    if request.log_verbosity == CopyLogVerbosity::Verbose {
        for skipped in &result.skipped {
            notices.push(format!(
                "skipping row due to data type incompatibility at line {} for column \"{}\": \"{}\"",
                skipped.line, skipped.column, skipped.value
            ));
        }
        if !result.skipped.is_empty() {
            let count = result.skipped.len();
            let noun = if count == 1 { "row was" } else { "rows were" };
            notices.push(format!(
                "{count} {noun} skipped due to data type incompatibility"
            ));
        }
    }
    Ok(CopyCompletion {
        command: format!("COPY {}", result.count),
        notices,
    })
}
