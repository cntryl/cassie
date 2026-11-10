use std::future::pending;
#[cfg(test)]
use std::future::Future;
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncReadExt};

#[cfg(test)]
use super::super::read_frontend_message_after_tag;
use super::super::{read_frontend_tag, FrontendMessage, HandshakeError};
use crate::app::CassieError;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "deadline_tests.rs"]
mod deadline_tests;

pub(super) enum CopyReadOutcome {
    Canceled,
    DeadlineIdle,
    DeadlinePartial,
    FramingInvalid,
    Resource(CassieError),
    AdmittedMessage(Result<(FrontendMessage, QueryMemoryReservation), HandshakeError>),
    #[cfg(test)]
    Message(Result<FrontendMessage, HandshakeError>),
}

async fn canceled(cancellation: Option<&QueryCancellationHandle>) {
    if let Some(cancellation) = cancellation {
        cancellation.wait_canceled().await;
    } else {
        pending::<()>().await;
    }
}

async fn deadline_elapsed(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    } else {
        pending::<()>().await;
    }
}

#[cfg(test)]
pub(super) async fn read_copy_frontend_message_with_deadline(
    cancellation: Option<&QueryCancellationHandle>,
    deadline: Option<Instant>,
    reader: &mut (impl AsyncRead + Unpin),
) -> CopyReadOutcome {
    let tag = tokio::select! {
        biased;
        () = canceled(cancellation) => return CopyReadOutcome::Canceled,
        tag = read_frontend_tag(reader) => match tag {
            Ok(tag) => tag,
            Err(error) => return CopyReadOutcome::Message(Err(error)),
        },
        () = deadline_elapsed(deadline) => return CopyReadOutcome::DeadlineIdle,
    };
    // The tag commits the reader to the frame: cancellation waits for it,
    // while an expired deadline closes the connection rather than lose alignment.
    let message = tokio::select! {
        biased;
        message = read_frontend_message_after_tag(reader, tag) => message,
        () = deadline_elapsed(deadline) => return CopyReadOutcome::DeadlinePartial,
    };
    if cancellation.is_some_and(QueryCancellationHandle::is_canceled) {
        CopyReadOutcome::Canceled
    } else {
        CopyReadOutcome::Message(message)
    }
}

#[cfg(test)]
pub(super) async fn read_copy_frontend_message(
    cancellation: &QueryCancellationHandle,
    reader: &mut (impl AsyncRead + Unpin),
) -> CopyReadOutcome {
    read_copy_frontend_message_with_deadline(Some(cancellation), None, reader).await
}

#[cfg(test)]
pub(super) async fn select_copy_read(
    cancellation: &QueryCancellationHandle,
    message: impl Future<Output = Result<FrontendMessage, HandshakeError>>,
) -> CopyReadOutcome {
    tokio::select! {
        biased;
        () = cancellation.wait_canceled() => CopyReadOutcome::Canceled,
        message = message => CopyReadOutcome::Message(message),
    }
}

pub(super) async fn read_admitted_copy_message(
    cancellation: Option<&QueryCancellationHandle>,
    deadline: Option<Instant>,
    controls: &QueryExecutionControls,
    reader: &mut (impl AsyncRead + Unpin),
) -> CopyReadOutcome {
    let tag = tokio::select! {
        biased;
        () = canceled(cancellation) => return CopyReadOutcome::Canceled,
        tag = read_frontend_tag(reader) => match tag {
            Ok(tag) => tag,
            Err(error) => return CopyReadOutcome::AdmittedMessage(Err(error)),
        },
        () = deadline_elapsed(deadline) => return CopyReadOutcome::DeadlineIdle,
    };
    let message = tokio::select! {
        biased;
        message = read_admitted_body(reader, tag, controls) => message,
        () = deadline_elapsed(deadline) => return CopyReadOutcome::DeadlinePartial,
    };
    if matches!(&message, Err(AdmittedBodyError::FramingInvalid)) {
        return CopyReadOutcome::FramingInvalid;
    }
    if cancellation.is_some_and(QueryCancellationHandle::is_canceled) {
        return CopyReadOutcome::Canceled;
    }
    let decoded = match message {
        Ok((payload, memory)) => decode_admitted_frame(tag, memory, payload, controls).await,
        Err(error) => Err(error),
    };
    if cancellation.is_some_and(QueryCancellationHandle::is_canceled) {
        return CopyReadOutcome::Canceled;
    }
    match decoded {
        Ok(message) => CopyReadOutcome::AdmittedMessage(Ok(message)),
        Err(AdmittedBodyError::Wire(error)) => CopyReadOutcome::AdmittedMessage(Err(error)),
        Err(AdmittedBodyError::Resource(error)) => CopyReadOutcome::Resource(error),
        Err(AdmittedBodyError::FramingInvalid) => CopyReadOutcome::FramingInvalid,
    }
}

enum AdmittedBodyError {
    FramingInvalid,
    Wire(HandshakeError),
    Resource(CassieError),
}

fn frame_read_error(error: &std::io::Error, part: &str) -> HandshakeError {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        HandshakeError::Closed
    } else {
        HandshakeError::Invalid(format!("failed to read frontend message {part}"))
    }
}

async fn drain_body(
    reader: &mut (impl AsyncRead + Unpin),
    mut remaining: usize,
) -> Result<(), AdmittedBodyError> {
    let mut scratch = [0_u8; 4096];
    while remaining != 0 {
        let size = remaining.min(scratch.len());
        reader
            .read_exact(&mut scratch[..size])
            .await
            .map_err(|error| AdmittedBodyError::Wire(frame_read_error(&error, "payload")))?;
        remaining -= size;
    }
    Ok(())
}

async fn read_admitted_body(
    reader: &mut (impl AsyncRead + Unpin),
    tag: u8,
    controls: &QueryExecutionControls,
) -> Result<(Vec<u8>, QueryMemoryReservation), AdmittedBodyError> {
    let mut length = [0_u8; 4];
    reader
        .read_exact(&mut length)
        .await
        .map_err(|error| AdmittedBodyError::Wire(frame_read_error(&error, "length")))?;
    let size = i32::from_be_bytes(length);
    if size < 4 {
        return Err(AdmittedBodyError::FramingInvalid);
    }
    let size = usize::try_from(size).map_err(|_| AdmittedBodyError::FramingInvalid)?;
    if size > super::super::MAX_FRONTEND_MESSAGE_BYTES {
        return Err(AdmittedBodyError::FramingInvalid);
    }
    let bytes = size - 4;
    // CopyFail decoding constructs a string before the input body is released.
    let admission = if tag == b'f' {
        bytes.checked_mul(2)
    } else {
        Some(bytes)
    }
    .ok_or_else(|| {
        AdmittedBodyError::Resource(CassieError::ResourceLimit(
            "COPY frame capacity overflow".into(),
        ))
    })?;
    let memory = match controls.reserve_query_memory(admission) {
        Ok(memory) => memory,
        Err(error) => {
            drain_body(reader, bytes).await?;
            return Err(AdmittedBodyError::Resource(error));
        }
    };
    let mut payload = Vec::new();
    if let Err(error) = payload.try_reserve_exact(bytes) {
        let error = crate::runtime::owned_memory::allocation_error(&error);
        drain_body(reader, bytes).await?;
        return Err(AdmittedBodyError::Resource(error));
    }
    if payload.capacity() > bytes {
        drain_body(reader, bytes).await?;
        return Err(AdmittedBodyError::Resource(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY frame capacity".into(),
        )));
    }
    let mut scratch = [0_u8; 4096];
    while payload.len() < bytes {
        let size = (bytes - payload.len()).min(scratch.len());
        super::super::readers::read_frontend_payload_bytes(reader, tag, &mut scratch[..size])
            .await
            .map_err(|error| AdmittedBodyError::Wire(frame_read_error(&error, "payload")))?;
        payload.extend_from_slice(&scratch[..size]);
    }
    Ok((payload, memory))
}

async fn decode_admitted_frame(
    tag: u8,
    memory: QueryMemoryReservation,
    payload: Vec<u8>,
    controls: &QueryExecutionControls,
) -> Result<(FrontendMessage, QueryMemoryReservation), AdmittedBodyError> {
    if !matches!(tag, b'd' | b'c' | b'f' | b'H' | b'S' | b'X') {
        return Ok((FrontendMessage::Unknown, memory));
    }
    if tag == b'f' {
        let mut end = None;
        for (index, chunk) in payload.chunks(4096).enumerate() {
            controls
                .check_interruption()
                .map_err(AdmittedBodyError::Resource)?;
            if let Some(offset) = chunk.iter().position(|byte| *byte == 0) {
                end = Some(index * 4096 + offset);
                break;
            }
            tokio::task::yield_now().await;
        }
        let end = end.ok_or_else(|| {
            AdmittedBodyError::Wire(HandshakeError::Invalid("missing null terminator".into()))
        })?;
        let mut text = String::new();
        text.try_reserve_exact(end).map_err(|error| {
            AdmittedBodyError::Resource(crate::runtime::owned_memory::allocation_error(&error))
        })?;
        if text.capacity() > end {
            return Err(AdmittedBodyError::Resource(CassieError::ResourceLimit(
                "allocator exceeded admitted COPY failure capacity".into(),
            )));
        }
        let mut start = 0;
        while start < end {
            controls
                .check_interruption()
                .map_err(AdmittedBodyError::Resource)?;
            let mut next = (start + 4096).min(end);
            while next > start && next < end && (payload[next] & 0xc0) == 0x80 {
                next -= 1;
            }
            if next == start {
                return Err(AdmittedBodyError::Wire(HandshakeError::Invalid(
                    "invalid UTF-8 in startup option".into(),
                )));
            }
            let chunk = std::str::from_utf8(&payload[start..next]).map_err(|_| {
                AdmittedBodyError::Wire(HandshakeError::Invalid(
                    "invalid UTF-8 in startup option".into(),
                ))
            })?;
            text.push_str(chunk);
            start = next;
            tokio::task::yield_now().await;
        }
        if end + 1 != payload.len() {
            return Err(AdmittedBodyError::Wire(HandshakeError::Invalid(
                "invalid frontend message payload".into(),
            )));
        }
        return Ok((FrontendMessage::CopyFail(text), memory));
    }
    let payload_len = payload.len();
    let (message, cursor) = super::super::readers::decode_frontend_message(tag, payload)
        .map_err(AdmittedBodyError::Wire)?;
    if cursor != payload_len {
        return Err(AdmittedBodyError::Wire(HandshakeError::Invalid(
            "invalid frontend message payload".into(),
        )));
    }
    Ok((message, memory))
}
