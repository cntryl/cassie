use std::io::{self, Write};
use std::mem::size_of;

use bytes::Bytes;
use hyper::{header::CONTENT_TYPE, Response, StatusCode};

use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::rest::body::{full_body, RestBody};
use crate::rest::query::{RestQueryExplainResponse, RestQueryResult};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

trait QueryJson: serde::Serialize {
    fn admission(&self) -> &QueryMemoryReservation;
}

impl QueryJson for RestQueryResult {
    fn admission(&self) -> &QueryMemoryReservation {
        self.admission()
    }
}

impl QueryJson for RestQueryExplainResponse {
    fn admission(&self) -> &QueryMemoryReservation {
        self.admission()
    }
}

pub(crate) fn try_query_json_response(
    status: StatusCode,
    value: &RestQueryResult,
    controls: &QueryExecutionControls,
) -> Result<Response<RestBody>, CassieError> {
    try_json_response(status, value, controls)
}

pub(crate) fn try_explain_json_response(
    status: StatusCode,
    value: &RestQueryExplainResponse,
    controls: &QueryExecutionControls,
) -> Result<Response<RestBody>, CassieError> {
    try_json_response(status, value, controls)
}

fn try_json_response<T: QueryJson>(
    status: StatusCode,
    value: &T,
    controls: &QueryExecutionControls,
) -> Result<Response<RestBody>, CassieError> {
    controls.check_interruption()?;
    let mut counter = JsonWriter {
        controls,
        bytes: None,
        length: 0,
        error: None,
    };
    if let Err(error) = serde_json::to_writer(&mut counter, value) {
        return Err(counter
            .error
            .take()
            .unwrap_or_else(|| serialization_error(error)));
    }
    // bytes 1.12's from_owner allocates one AtomicUsize plus the owner.
    // Include its alignment and the boxed Full/MapErr body before either allocation.
    let owner = add(size_of::<EncodedQuery>(), 2 * size_of::<usize>())?;
    let body = add(size_of::<http_body_util::Full<Bytes>>(), size_of::<usize>())?;
    let reservation = value
        .admission()
        .try_sibling(add(counter.length, add(owner, body)?)?)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(counter.length).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate REST query encoding: {error}"))
    })?;
    if bytes.capacity() > counter.length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted REST encoding capacity".to_owned(),
        ));
    }
    let mut writer = JsonWriter {
        controls,
        bytes: Some(&mut bytes),
        length: 0,
        error: None,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        return Err(writer
            .error
            .take()
            .unwrap_or_else(|| serialization_error(error)));
    }
    if writer.length != counter.length {
        return Err(CassieError::Execution(
            "REST query serialization changed between passes".to_owned(),
        ));
    }
    controls.check_interruption()?;
    let bytes = Bytes::from_owner(EncodedQuery {
        bytes,
        _reservation: reservation,
    });
    let mut response = Response::new(full_body(bytes));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

struct EncodedQuery {
    bytes: Vec<u8>,
    _reservation: QueryMemoryReservation,
}

impl AsRef<[u8]> for EncodedQuery {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

struct JsonWriter<'a> {
    controls: &'a QueryExecutionControls,
    bytes: Option<&'a mut Vec<u8>>,
    length: usize,
    error: Option<CassieError>,
}

impl Write for JsonWriter<'_> {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        let result = self
            .controls
            .check_interruption()
            .and_then(|()| add(self.length, value.len()));
        let length = match result {
            Ok(length) => length,
            Err(error) => {
                self.error = Some(error);
                return Err(io::Error::other("query encoding interrupted"));
            }
        };
        if let Some(bytes) = &mut self.bytes {
            if length > bytes.capacity() {
                self.error = Some(CassieError::Execution(
                    "REST encoding exceeded its admitted length".to_owned(),
                ));
                return Err(io::Error::other("query encoding length changed"));
            }
            for chunk in value.chunks(4096) {
                if let Err(error) = self.controls.check_interruption() {
                    self.error = Some(error);
                    return Err(io::Error::other("query encoding interrupted"));
                }
                bytes.extend_from_slice(chunk);
            }
        }
        self.length = length;
        Ok(value.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialization_error(error: serde_json::Error) -> CassieError {
    CassieError::Execution(format!("unable to encode REST query result: {error}"))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use http_body_util::BodyExt;

    use crate::config::CassieRuntimeLimits;
    use crate::executor::{ColumnMeta, QueryResult};
    use crate::types::Value;

    use super::{
        try_query_json_response, CassieError, QueryExecutionControls, RestQueryResult, StatusCode,
    };

    const BUDGET: usize = 128 * 1024;

    fn controls() -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: BUDGET,
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    fn result(controls: &QueryExecutionControls) -> RestQueryResult {
        let mut builder = QueryResult::try_builder(controls).expect("source admission");
        builder
            .try_push_column(&ColumnMeta::text("message"))
            .expect("column");
        builder
            .try_push_row(&[Value::String("frame payload".repeat(128))])
            .expect("row");
        builder.try_set_command("SELECT 1").expect("command");
        RestQueryResult::try_from(builder.finish().expect("source result")).expect("REST result")
    }

    #[test]
    fn should_keep_encoded_query_bytes_reserved_until_the_last_frame_drops() {
        // Arrange
        let controls = controls();
        let value = result(&controls);
        let expected = serde_json::to_vec(&value).expect("literal reference");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current thread runtime");

        // Act
        let response =
            try_query_json_response(StatusCode::OK, &value, &controls).expect("response");
        let status = response.status();
        let mut body = response.into_body();
        let bytes = runtime.block_on(async {
            body.frame()
                .await
                .expect("response frame")
                .expect("frame result")
                .into_data()
                .expect("data frame")
        });
        let alias = bytes.clone();
        let literal_matches = bytes.as_ref() == expected;
        drop(body);
        drop(value);
        drop(bytes);
        let frame_live = controls.current_query_memory_bytes();
        drop(alias);
        let released = controls.current_query_memory_bytes();

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert!(literal_matches);
        assert!(
            frame_live > 0,
            "a retained frame must retain its encoded-byte reservation"
        );
        assert_eq!(released, 0);
    }

    #[test]
    fn should_reject_query_encoding_before_publishing_an_overbudget_response() {
        // Arrange
        let controls = controls();
        let value = result(&controls);
        let expected = serde_json::to_value(&value).expect("literal reference");
        let blocker = controls
            .reserve_query_memory(BUDGET - controls.current_query_memory_bytes())
            .expect("occupy remaining budget");

        // Act
        let response = try_query_json_response(StatusCode::OK, &value, &controls);
        let rejected = matches!(response, Err(CassieError::ResourceLimit(_)));
        drop(response);
        let unchanged = serde_json::to_value(&value).expect("preserved result") == expected;
        drop(blocker);
        drop(value);
        let released = controls.current_query_memory_bytes();

        // Assert
        assert!(
            rejected,
            "encoding must reserve before returning success headers or bytes"
        );
        assert!(unchanged);
        assert_eq!(released, 0);
    }
}
