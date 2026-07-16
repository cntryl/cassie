use super::{
    io, str, AsyncReadExt, BufReader, DescribeTarget, FrontendMessage, HandshakeError,
    StartupFrame, MAX_FRONTEND_MESSAGE_BYTES, MIN_STARTUP_MESSAGE_BYTES, PASSWORD_MESSAGE_TAG,
    PROTOCOL_VERSION_3, SSL_REQUEST_CODE,
};
use crate::pgwire::connection::CANCEL_REQUEST_CODE;
use std::collections::HashMap;

pub(super) async fn read_simple_query_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<String, HandshakeError> {
    let mut tag = [0u8; 1];
    reader.read_exact(&mut tag).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query tag".to_string())
        }
    })?;
    if tag[0] != b'Q' {
        return Err(HandshakeError::Invalid(
            "not a simple query message".to_string(),
        ));
    }

    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query length".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 4 {
        return Err(HandshakeError::Invalid(
            "invalid simple query frame length".to_string(),
        ));
    }
    let size = usize::try_from(size)
        .map_err(|_| HandshakeError::Invalid("invalid simple query frame length".to_string()))?;
    if size > MAX_FRONTEND_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "simple query frame exceeds supported bounds".to_string(),
        ));
    }
    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query payload".to_string())
        }
    })?;

    let mut cursor = 0usize;
    let sql = read_null_terminated(&payload, &mut cursor)?;
    if cursor != payload.len() {
        return Err(HandshakeError::Invalid(
            "invalid simple query payload".to_string(),
        ));
    }

    Ok(sql)
}

pub(super) async fn read_frontend_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<FrontendMessage, HandshakeError> {
    let (tag, payload) = read_frontend_frame(reader).await?;
    let payload_len = payload.len();
    let (message, cursor) = decode_frontend_message(tag, payload)?;
    if cursor != payload_len {
        return Err(HandshakeError::Invalid(
            "invalid frontend message payload".to_string(),
        ));
    }

    Ok(message)
}

async fn read_frontend_frame(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<(u8, Vec<u8>), HandshakeError> {
    let mut tag = [0u8; 1];
    reader.read_exact(&mut tag).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend message tag".to_string())
        }
    })?;

    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend message length".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 4 {
        return Err(HandshakeError::Invalid(
            "invalid frontend message length".to_string(),
        ));
    }

    let size = usize::try_from(size).map_err(|_| {
        HandshakeError::Invalid("frontend message length exceeds supported bounds".to_string())
    })?;
    if size > MAX_FRONTEND_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "frontend message exceeds supported bounds".to_string(),
        ));
    }

    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend payload".to_string())
        }
    })?;

    Ok((tag[0], payload))
}

pub(super) fn decode_frontend_message(
    tag: u8,
    payload: Vec<u8>,
) -> Result<(FrontendMessage, usize), HandshakeError> {
    let mut cursor = 0usize;
    let payload_len = payload.len();
    let message = match tag {
        b'P' => decode_parse_message(&payload, &mut cursor)?,
        b'B' => decode_bind_message(&payload, &mut cursor)?,
        b'D' => decode_describe_message(&payload, &mut cursor)?,
        b'E' => decode_execute_message(&payload, &mut cursor)?,
        b'S' => decode_empty_frontend_message(&payload, FrontendMessage::Sync, "sync")?,
        b'C' => decode_close_message(&payload, &mut cursor)?,
        b'd' => {
            cursor = payload_len;
            FrontendMessage::CopyData(payload)
        }
        b'c' => {
            cursor = payload_len;
            FrontendMessage::CopyDone
        }
        b'f' => {
            let message = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::CopyFail(message)
        }
        b'F' => {
            cursor = payload_len;
            FrontendMessage::FunctionCall
        }
        b'H' => decode_empty_frontend_message(&payload, FrontendMessage::Flush, "flush")?,
        b'X' => decode_empty_frontend_message(&payload, FrontendMessage::Terminate, "terminate")?,
        _ => {
            cursor = payload_len;
            FrontendMessage::Unknown
        }
    };
    Ok((message, cursor))
}

fn decode_parse_message(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<FrontendMessage, HandshakeError> {
    let name = read_null_terminated(payload, cursor)?;
    let query = read_null_terminated(payload, cursor)?;
    let parameter_count = read_frontend_i16(payload, cursor)?;
    let parameter_count = usize::try_from(parameter_count)
        .map_err(|_| HandshakeError::Invalid("invalid parse parameter count".to_string()))?;
    let mut parameter_type_oids = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        parameter_type_oids.push(read_frontend_i32(payload, cursor)?);
    }
    Ok(FrontendMessage::Parse {
        name,
        query,
        parameter_type_oids,
    })
}

fn decode_bind_message(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<FrontendMessage, HandshakeError> {
    let portal = read_null_terminated(payload, cursor)?;
    let statement = read_null_terminated(payload, cursor)?;
    let parameter_formats = read_bind_formats(payload, cursor, "invalid bind format count")?;
    let parameters = read_bind_parameters(payload, cursor)?;
    let result_formats = read_bind_formats(payload, cursor, "invalid result format count")?;
    Ok(FrontendMessage::Bind {
        portal,
        statement,
        parameter_formats,
        parameters,
        result_formats,
    })
}

fn read_bind_formats(
    payload: &[u8],
    cursor: &mut usize,
    count_error: &str,
) -> Result<Vec<i16>, HandshakeError> {
    let count = read_frontend_i16(payload, cursor)?;
    let count =
        usize::try_from(count).map_err(|_| HandshakeError::Invalid(count_error.to_string()))?;
    let mut formats = Vec::with_capacity(count);
    for _ in 0..count {
        formats.push(read_frontend_i16(payload, cursor)?);
    }
    Ok(formats)
}

fn read_bind_parameters(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<Vec<Option<Vec<u8>>>, HandshakeError> {
    let parameter_count = read_frontend_i16(payload, cursor)?;
    let parameter_count = usize::try_from(parameter_count)
        .map_err(|_| HandshakeError::Invalid("invalid bind parameter count".to_string()))?;
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value_len = read_frontend_i32(payload, cursor)?;
        if value_len == -1 {
            parameters.push(None);
            continue;
        }
        let value_len = usize::try_from(value_len)
            .map_err(|_| HandshakeError::Invalid("invalid bind parameter length".to_string()))?;
        let end = (*cursor)
            .checked_add(value_len)
            .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;
        let _ = payload
            .get(*cursor..end)
            .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;
        parameters.push(Some(payload[*cursor..end].to_vec()));
        *cursor = end;
    }
    Ok(parameters)
}

fn decode_describe_message(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<FrontendMessage, HandshakeError> {
    let target = read_describe_or_close_target(payload, cursor, "describe")?;
    let name = read_null_terminated(payload, cursor)?;
    Ok(FrontendMessage::Describe { target, name })
}

fn decode_execute_message(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<FrontendMessage, HandshakeError> {
    let portal = read_null_terminated(payload, cursor)?;
    let max_rows = read_frontend_i32(payload, cursor)?;
    if max_rows < 0 {
        return Err(HandshakeError::Invalid(
            "invalid execute row limit".to_string(),
        ));
    }
    Ok(FrontendMessage::Execute { portal, max_rows })
}

fn decode_close_message(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<FrontendMessage, HandshakeError> {
    let target = read_describe_or_close_target(payload, cursor, "close")?;
    let name = read_null_terminated(payload, cursor)?;
    Ok(FrontendMessage::Close { target, name })
}

fn read_describe_or_close_target(
    payload: &[u8],
    cursor: &mut usize,
    message: &str,
) -> Result<DescribeTarget, HandshakeError> {
    match payload.get(*cursor).copied() {
        Some(b'S') => {
            *cursor += 1;
            Ok(DescribeTarget::Statement)
        }
        Some(b'P') => {
            *cursor += 1;
            Ok(DescribeTarget::Portal)
        }
        Some(other) => Err(HandshakeError::Invalid(format!(
            "unsupported {message} target '{}'",
            char::from(other)
        ))),
        None => Err(HandshakeError::Invalid(format!("missing {message} target"))),
    }
}

fn decode_empty_frontend_message(
    payload: &[u8],
    message: FrontendMessage,
    name: &str,
) -> Result<FrontendMessage, HandshakeError> {
    if !payload.is_empty() {
        return Err(HandshakeError::Invalid(format!(
            "{name} message should not contain a payload"
        )));
    }
    Ok(message)
}

pub(super) fn read_frontend_i16(payload: &[u8], cursor: &mut usize) -> Result<i16, HandshakeError> {
    let end = cursor
        .checked_add(2)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    let bytes: [u8; 2] = payload
        .get(*cursor..end)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?
        .try_into()
        .map_err(|_| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    *cursor = end;
    Ok(i16::from_be_bytes(bytes))
}

pub(super) fn read_frontend_i32(payload: &[u8], cursor: &mut usize) -> Result<i32, HandshakeError> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    let bytes: [u8; 4] = payload
        .get(*cursor..end)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?
        .try_into()
        .map_err(|_| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    *cursor = end;
    Ok(i32::from_be_bytes(bytes))
}

pub(super) async fn read_startup_frame(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<StartupFrame, HandshakeError> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read startup frame".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 0 {
        return Err(HandshakeError::Invalid(
            "negative startup frame size".to_string(),
        ));
    }
    let size = usize::try_from(size).map_err(|_| {
        HandshakeError::Invalid("startup frame size exceeds supported bounds".to_string())
    })?;

    if size < MIN_STARTUP_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "startup frame too small".to_string(),
        ));
    }
    if size > MAX_FRONTEND_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "startup frame exceeds supported bounds".to_string(),
        ));
    }

    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read startup payload".to_string())
        }
    })?;

    let code = i32::from_be_bytes(
        *payload
            .first_chunk::<4>()
            .ok_or_else(|| HandshakeError::Invalid("malformed startup frame".to_string()))?,
    );

    if size == 8 && code == SSL_REQUEST_CODE {
        return Ok(StartupFrame::SslRequest);
    }

    if size == 16 && code == CANCEL_REQUEST_CODE {
        let process_id = i32::from_be_bytes(
            payload[4..8]
                .try_into()
                .map_err(|_| HandshakeError::Invalid("malformed cancel request".to_string()))?,
        );
        let secret_key = i32::from_be_bytes(
            payload[8..12]
                .try_into()
                .map_err(|_| HandshakeError::Invalid("malformed cancel request".to_string()))?,
        );
        return Ok(StartupFrame::CancelRequest {
            process_id,
            secret_key,
        });
    }

    if code != PROTOCOL_VERSION_3 {
        return Err(HandshakeError::Invalid(
            "unsupported protocol version".to_string(),
        ));
    }

    let parameters = parse_startup_payload(&payload[4..])?;
    Ok(StartupFrame::Startup(parameters))
}

pub(super) async fn read_password_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<String, HandshakeError> {
    let mut header = [0u8; 5];
    reader.read_exact(&mut header).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read password message header".to_string())
        }
    })?;
    if header[0] != PASSWORD_MESSAGE_TAG {
        return Err(HandshakeError::Invalid(
            "not a password message".to_string(),
        ));
    }

    let payload_len = i32::from_be_bytes(header[1..].try_into().unwrap_or([0u8; 4]));
    if payload_len < 4 {
        return Err(HandshakeError::Invalid(
            "invalid password frame length".to_string(),
        ));
    }
    let payload_len = usize::try_from(payload_len)
        .map_err(|_| HandshakeError::Invalid("invalid password frame length".to_string()))?;
    if payload_len > MAX_FRONTEND_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "password frame exceeds supported bounds".to_string(),
        ));
    }
    let payload_len = payload_len - 4;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read password payload".to_string())
        }
    })?;

    let mut cursor = 0usize;
    let value = read_null_terminated(&payload, &mut cursor)?;
    if cursor != payload.len() {
        return Err(HandshakeError::Invalid(
            "invalid password payload".to_string(),
        ));
    }
    Ok(value)
}

pub(super) fn parse_startup_payload(
    payload: &[u8],
) -> Result<HashMap<String, String>, HandshakeError> {
    let mut cursor = 0usize;
    let mut parameters = HashMap::new();
    while cursor < payload.len() {
        let key = read_null_terminated(payload, &mut cursor)?;
        if key.is_empty() {
            if cursor == payload.len() {
                break;
            }
            return Err(HandshakeError::Invalid(
                "malformed startup payload: unexpected trailing data".to_string(),
            ));
        }

        let value = read_null_terminated(payload, &mut cursor)?;
        parameters.insert(key, value);
    }

    Ok(parameters)
}

pub(super) fn read_null_terminated(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<String, HandshakeError> {
    let remaining = payload
        .get(*cursor..)
        .ok_or_else(|| HandshakeError::Invalid("invalid payload cursor".to_string()))?;

    let end = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| HandshakeError::Invalid("missing null terminator".to_string()))?;

    let decoded = str::from_utf8(&remaining[..end])
        .map_err(|_| HandshakeError::Invalid("invalid UTF-8 in startup option".to_string()))?;

    *cursor += end + 1;
    Ok(decoded.to_string())
}
