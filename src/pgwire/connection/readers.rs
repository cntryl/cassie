use super::{BufReader, HandshakeError, AsyncReadExt, io, TryFrom, MAX_FRONTEND_MESSAGE_BYTES, FrontendMessage, DescribeTarget, CloseTarget, StartupFrame, MIN_STARTUP_MESSAGE_BYTES, SSL_REQUEST_CODE, CANCEL_REQUEST_CODE, PROTOCOL_VERSION_3, PASSWORD_MESSAGE_TAG, HashMap, Value, str, query};

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

    let mut cursor = 0usize;
    let payload_len = payload.len();
    let message = match tag[0] {
        b'P' => {
            let name = read_null_terminated(&payload, &mut cursor)?;
            let query = read_null_terminated(&payload, &mut cursor)?;
            let parameter_count = read_frontend_i16(&payload, &mut cursor)?;
            let parameter_count = usize::try_from(parameter_count).map_err(|_| {
                HandshakeError::Invalid("invalid parse parameter count".to_string())
            })?;
            let mut parameter_types = Vec::with_capacity(parameter_count);
            for _ in 0..parameter_count {
                parameter_types.push(read_frontend_i32(&payload, &mut cursor)?);
            }
            FrontendMessage::Parse {
                name,
                query,
                parameter_types,
            }
        }
        b'B' => {
            let portal_name = read_null_terminated(&payload, &mut cursor)?;
            let statement_name = read_null_terminated(&payload, &mut cursor)?;
            let format_count = read_frontend_i16(&payload, &mut cursor)?;
            let format_count = usize::try_from(format_count)
                .map_err(|_| HandshakeError::Invalid("invalid bind format count".to_string()))?;
            let mut format_codes = Vec::with_capacity(format_count);
            for _ in 0..format_count {
                format_codes.push(read_frontend_i16(&payload, &mut cursor)?);
            }

            let parameter_count = read_frontend_i16(&payload, &mut cursor)?;
            let parameter_count = usize::try_from(parameter_count)
                .map_err(|_| HandshakeError::Invalid("invalid bind parameter count".to_string()))?;

            let mut params = Vec::with_capacity(parameter_count);
            for _ in 0..parameter_count {
                let value_len = read_frontend_i32(&payload, &mut cursor)?;
                if value_len == -1 {
                    params.push(None);
                    continue;
                }
                let value_len = usize::try_from(value_len).map_err(|_| {
                    HandshakeError::Invalid("invalid bind parameter length".to_string())
                })?;
                let end = cursor
                    .checked_add(value_len)
                    .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;
                let value = payload
                    .get(cursor..end)
                    .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;
                params.push(Some(value.to_vec()));
                cursor = end;
            }

            let result_format_count = read_frontend_i16(&payload, &mut cursor)?;
            let result_format_count = usize::try_from(result_format_count)
                .map_err(|_| HandshakeError::Invalid("invalid result format count".to_string()))?;
            let mut result_formats = Vec::with_capacity(result_format_count);
            for _ in 0..result_format_count {
                result_formats.push(read_frontend_i16(&payload, &mut cursor)?);
            }

            FrontendMessage::Bind {
                portal_name,
                statement_name,
                parameter_formats: format_codes,
                params,
                result_formats,
            }
        }
        b'D' => {
            let target = match payload.get(cursor).copied() {
                Some(b'S') => {
                    cursor += 1;
                    DescribeTarget::Statement
                }
                Some(b'P') => {
                    cursor += 1;
                    DescribeTarget::Portal
                }
                Some(other) => {
                    return Err(HandshakeError::Invalid(format!(
                        "unsupported describe target '{}'",
                        char::from(other)
                    )))
                }
                None => {
                    return Err(HandshakeError::Invalid(
                        "missing describe target".to_string(),
                    ))
                }
            };
            let name = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::Describe { target, name }
        }
        b'E' => {
            let portal_name = read_null_terminated(&payload, &mut cursor)?;
            let limit = read_frontend_i32(&payload, &mut cursor)?;
            let limit = match limit.cmp(&0) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Less => {
                    return Err(HandshakeError::Invalid(
                        "invalid execute row limit".to_string(),
                    ))
                }
                std::cmp::Ordering::Greater => Some(i64::from(limit)),
            };
            FrontendMessage::Execute { portal_name, limit }
        }
        b'S' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "sync message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Sync
        }
        b'C' => {
            let target = match payload.get(cursor).copied() {
                Some(b'S') => {
                    cursor += 1;
                    CloseTarget::Statement
                }
                Some(b'P') => {
                    cursor += 1;
                    CloseTarget::Portal
                }
                Some(other) => {
                    return Err(HandshakeError::Invalid(format!(
                        "unsupported close target '{}'",
                        char::from(other)
                    )))
                }
                None => return Err(HandshakeError::Invalid("missing close target".to_string())),
            };
            let name = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::Close { target, name }
        }
        b'd' => {
            cursor = payload.len();
            FrontendMessage::CopyData(payload)
        }
        b'c' => {
            cursor = payload.len();
            FrontendMessage::CopyDone
        }
        b'f' => {
            let message = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::CopyFail(message)
        }
        b'F' => {
            cursor = payload.len();
            FrontendMessage::FunctionCall
        }
        b'H' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "flush message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Flush
        }
        b'X' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "terminate message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Terminate
        }
        other => FrontendMessage::Unknown(other),
    };

    if cursor != payload_len {
        return Err(HandshakeError::Invalid(
            "invalid frontend message payload".to_string(),
        ));
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
        return Ok(StartupFrame::CancelRequest);
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

pub(super) fn parse_bind_param_value(
    value: Option<&[u8]>,
    format_code: i16,
    type_oid: i32,
) -> Result<Value, HandshakeError> {
    let Some(value) = value else {
        return Ok(Value::Null);
    };
    match format_code {
        0 => {
            let text = str::from_utf8(value).map_err(|_| {
                HandshakeError::Invalid("invalid UTF-8 in bind parameter".to_string())
            })?;
            Ok(query::parse_bind_param(text))
        }
        1 => parse_binary_bind_param(value, type_oid),
        _ => Err(HandshakeError::Invalid(
            "unsupported bind format code".to_string(),
        )),
    }
}

fn parse_binary_bind_param(value: &[u8], type_oid: i32) -> Result<Value, HandshakeError> {
    match type_oid {
        16 => {
            if value.len() != 1 {
                return Err(HandshakeError::Invalid(
                    "invalid bool bind parameter".to_string(),
                ));
            }
            return Ok(Value::Bool(value[0] != 0));
        }
        17 => return Ok(Value::String(format_bytea_hex(value))),
        20 => {
            return value
                .try_into()
                .map(i64::from_be_bytes)
                .map(Value::Int64)
                .map_err(|_| HandshakeError::Invalid("invalid int8 bind parameter".to_string()));
        }
        21 => {
            return value
                .try_into()
                .map(i16::from_be_bytes)
                .map(|value| Value::Int64(i64::from(value)))
                .map_err(|_| HandshakeError::Invalid("invalid int2 bind parameter".to_string()));
        }
        23 => {
            return value
                .try_into()
                .map(i32::from_be_bytes)
                .map(|value| Value::Int64(i64::from(value)))
                .map_err(|_| HandshakeError::Invalid("invalid int4 bind parameter".to_string()));
        }
        701 => {
            return value
                .try_into()
                .map(f64::from_be_bytes)
                .map(Value::Float64)
                .map_err(|_| HandshakeError::Invalid("invalid float8 bind parameter".to_string()));
        }
        25 | 1042 | 1043 | 705 => {
            let text = str::from_utf8(value).map_err(|_| {
                HandshakeError::Invalid("invalid UTF-8 in binary text parameter".to_string())
            })?;
            return Ok(Value::String(text.to_string()));
        }
        114 => {
            let text = str::from_utf8(value).map_err(|_| {
                HandshakeError::Invalid("invalid UTF-8 in binary json parameter".to_string())
            })?;
            let json = serde_json::from_str(text)
                .map_err(|_| HandshakeError::Invalid("invalid json bind parameter".to_string()))?;
            return Ok(Value::Json(json));
        }
        3802 => {
            let json = value.strip_prefix(&[1]).unwrap_or(value);
            let text = str::from_utf8(json).map_err(|_| {
                HandshakeError::Invalid("invalid UTF-8 in binary jsonb parameter".to_string())
            })?;
            let json = serde_json::from_str(text)
                .map_err(|_| HandshakeError::Invalid("invalid jsonb bind parameter".to_string()))?;
            return Ok(Value::Json(json));
        }
        2950 => {
            if value.len() != 16 {
                return Err(HandshakeError::Invalid(
                    "invalid uuid bind parameter".to_string(),
                ));
            }
            return Ok(Value::String(format_uuid(value)));
        }
        _ => {}
    }

    if let Ok(text) = str::from_utf8(value) {
        if text.chars().all(|character| !character.is_control()) {
            return Ok(query::parse_bind_param(text));
        }
    }

    match value.len() {
        2 => Ok(Value::Int64(i64::from(i16::from_be_bytes(
            value
                .try_into()
                .map_err(|_| HandshakeError::Invalid("invalid int2 bind parameter".to_string()))?,
        )))),
        4 => Ok(Value::Int64(i64::from(i32::from_be_bytes(
            value
                .try_into()
                .map_err(|_| HandshakeError::Invalid("invalid int4 bind parameter".to_string()))?,
        )))),
        8 => Ok(Value::Int64(i64::from_be_bytes(value.try_into().map_err(
            |_| HandshakeError::Invalid("invalid int8 bind parameter".to_string()),
        )?))),
        _ => {
            let text = str::from_utf8(value).map_err(|_| {
                HandshakeError::Invalid("unsupported binary bind parameter".to_string())
            })?;
            Ok(query::parse_bind_param(text))
        }
    }
}

fn format_bytea_hex(value: &[u8]) -> String {
    let mut out = String::with_capacity(2 + value.len() * 2);
    out.push_str("\\x");
    for byte in value {
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }
    out
}

fn format_uuid(value: &[u8]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in value.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }
    out
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

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
