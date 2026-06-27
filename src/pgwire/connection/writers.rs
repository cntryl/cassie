use super::codecs::*;
use super::errors::PgWireError;
use super::*;

pub(super) async fn write_auth_ok(write_half: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let mut frame = Vec::new();
    frame.push(b'R');
    frame.extend_from_slice(&8_i32.to_be_bytes());
    frame.extend_from_slice(&0_i32.to_be_bytes());
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) async fn write_auth_cleartext(
    write_half: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
    let mut frame = Vec::new();
    frame.push(b'R');
    frame.extend_from_slice(&8_i32.to_be_bytes());
    frame.extend_from_slice(&3_i32.to_be_bytes());
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) async fn write_parameter_statuses(
    write_half: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
    for (key, value) in [
        ("server_version", "16.0"),
        ("server_encoding", "UTF8"),
        ("client_encoding", "UTF8"),
        ("DateStyle", "ISO, MDY"),
        ("integer_datetimes", "on"),
        ("TimeZone", "UTC"),
        ("standard_conforming_strings", "on"),
    ] {
        let mut payload = Vec::new();
        payload.extend_from_slice(key.as_bytes());
        payload.push(0);
        payload.extend_from_slice(value.as_bytes());
        payload.push(0);
        write_backend_frame(write_half, b'S', &payload).await?;
    }
    Ok(())
}

pub(super) async fn write_ssl_not_supported(
    write_half: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
    write_half.write_all(b"N").await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) async fn write_error_response(
    write_half: &mut (impl AsyncWrite + Unpin),
    error: &PgWireError,
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"S");
    payload.extend_from_slice(error.severity.as_str().as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"C");
    payload.extend_from_slice(error.code.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"M");
    payload.extend_from_slice(error.message.as_bytes());
    payload.push(0);
    append_error_field(&mut payload, b'D', error.detail.as_deref());
    append_error_field(&mut payload, b'H', error.hint.as_deref());
    append_error_field(&mut payload, b's', error.schema.as_deref());
    append_error_field(&mut payload, b't', error.table.as_deref());
    append_error_field(&mut payload, b'c', error.column.as_deref());
    append_error_field(&mut payload, b'n', error.constraint.as_deref());
    payload.push(0);

    let mut frame = vec![b'E'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);

    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

fn append_error_field(payload: &mut Vec<u8>, tag: u8, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    payload.push(tag);
    payload.extend_from_slice(value.as_bytes());
    payload.push(0);
}

pub(super) async fn write_simple_query_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    result: crate::executor::QueryResult,
) -> io::Result<()> {
    let crate::executor::QueryResult {
        columns,
        rows,
        command,
    } = result;

    if !columns.is_empty() {
        let mut frames = Vec::new();
        append_row_description_frame(&mut frames, &columns, &[])?;
        for row in rows {
            append_data_row_frame(&mut frames, row, &columns, &[])?;
        }
        append_command_complete_frame(&mut frames, &command)?;
        write_half.write_all(&frames).await?;
        write_half.flush().await?;
        return Ok(());
    }

    write_command_complete(write_half, &command).await?;
    Ok(())
}

pub(super) async fn write_row_description(
    write_half: &mut (impl AsyncWrite + Unpin),
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_row_description_frame(&mut frame, columns, result_formats)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) fn append_row_description_frame(
    frame: &mut Vec<u8>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    validate_result_formats(result_formats, columns.len())?;
    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(columns.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many columns"))?
            .to_be_bytes(),
    );

    for (index, column) in columns.iter().enumerate() {
        payload.extend_from_slice(column.name.as_bytes());
        payload.push(0);
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i16.to_be_bytes());
        payload.extend_from_slice(
            &i32::try_from(column.type_oid)
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "column oid out of range")
                })?
                .to_be_bytes(),
        );
        payload.extend_from_slice(&column.typlen.to_be_bytes());
        payload.extend_from_slice(&column.atttypmod.to_be_bytes());
        let format_code = result_format_for_index(result_formats, index);
        payload.extend_from_slice(&format_code.to_be_bytes());
    }

    append_backend_frame(frame, b'T', &payload)
}

pub(super) async fn write_parameter_description(
    write_half: &mut (impl AsyncWrite + Unpin),
    parameter_types: &[i32],
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(parameter_types.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many parameters"))?
            .to_be_bytes(),
    );
    for oid in parameter_types {
        payload.extend_from_slice(&oid.to_be_bytes());
    }

    write_backend_frame(write_half, b't', &payload).await
}

pub(super) async fn write_data_row(
    write_half: &mut (impl AsyncWrite + Unpin),
    row: Vec<Value>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_data_row_frame(&mut frame, row, columns, result_formats)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) fn append_data_row_frame(
    frame: &mut Vec<u8>,
    row: Vec<Value>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    validate_result_formats(result_formats, columns.len())?;

    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(row.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many row values"))?
            .to_be_bytes(),
    );

    for (index, value) in row.into_iter().enumerate() {
        match value {
            Value::Null => payload.extend_from_slice(&(-1_i32).to_be_bytes()),
            other => {
                let format_code = match result_formats.len() {
                    0 => 0,
                    1 => result_formats[0],
                    _ => result_formats[index],
                };
                let bytes = if format_code == 0 {
                    value_to_text(other).into_bytes()
                } else if format_code == 1 {
                    value_to_binary(other, columns[index].type_oid)?
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported result format",
                    ));
                };
                payload.extend_from_slice(
                    &i32::try_from(bytes.len())
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidInput, "value too large")
                        })?
                        .to_be_bytes(),
                );
                payload.extend_from_slice(&bytes);
            }
        }
    }

    append_backend_frame(frame, b'D', &payload)
}

pub(super) async fn write_command_complete(
    write_half: &mut (impl AsyncWrite + Unpin),
    command: &str,
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_command_complete_frame(&mut frame, command)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) async fn write_portal_suspended(
    write_half: &mut (impl AsyncWrite + Unpin),
) -> io::Result<()> {
    write_backend_frame(write_half, b's', &[]).await
}

pub(super) async fn write_copy_in_response(
    write_half: &mut (impl AsyncWrite + Unpin),
    column_count: usize,
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.push(0);
    payload.extend_from_slice(
        &i16::try_from(column_count)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many columns"))?
            .to_be_bytes(),
    );
    for _ in 0..column_count {
        payload.extend_from_slice(&0_i16.to_be_bytes());
    }
    write_backend_frame(write_half, b'G', &payload).await
}

pub(super) fn append_command_complete_frame(frame: &mut Vec<u8>, command: &str) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(command.as_bytes());
    payload.push(0);
    append_backend_frame(frame, b'C', &payload)
}

pub(super) async fn write_ready_for_query(
    write_half: &mut (impl AsyncWrite + Unpin),
    session: &CassieSession,
) -> io::Result<()> {
    let status = if session.is_transaction_failed() {
        b'E'
    } else if session.is_transaction_active() {
        b'T'
    } else {
        b'I'
    };
    write_backend_frame(write_half, b'Z', &[status]).await
}

pub(super) async fn write_backend_frame(
    write_half: &mut (impl AsyncWrite + Unpin),
    tag: u8,
    payload: &[u8],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_backend_frame(&mut frame, tag, payload)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

pub(super) fn append_backend_frame(frame: &mut Vec<u8>, tag: u8, payload: &[u8]) -> io::Result<()> {
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    Ok(())
}

pub(super) fn validate_result_formats(
    result_formats: &[i16],
    column_count: usize,
) -> io::Result<()> {
    if !result_formats.iter().all(|format| matches!(*format, 0 | 1)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format",
        ));
    }
    if !result_formats.is_empty()
        && result_formats.len() != 1
        && result_formats.len() != column_count
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format count",
        ));
    }
    Ok(())
}

fn result_format_for_index(result_formats: &[i16], index: usize) -> i16 {
    match result_formats.len() {
        0 => 0,
        1 => result_formats[0],
        _ => result_formats[index],
    }
}
