use std::sync::Arc;
use std::time::Instant;

use hyper::{Method, Response, StatusCode};

use crate::app::Cassie;

use super::{
    json_response, map_error, run_rest_blocking_route, run_rest_blocking_route_controlled,
    RestBody, RestBytes, RestRequestContext,
};

pub(super) async fn dispatch_collection_routes(
    method: &Method,
    segments: &[&str],
    cassie: Arc<Cassie>,
    request_context: &RestRequestContext,
    body: &RestBytes,
    path: &str,
    started_at: Instant,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    if let Some(response) = dispatch_document_routes(
        method,
        segments,
        Arc::clone(&cassie),
        request_context,
        body,
        path,
        started_at,
    )
    .await?
    {
        return Ok(Some(response));
    }

    match (method.as_str(), segments) {
        ("GET", ["api", "v1", "collections"]) => {
            dispatch_collection_list(cassie, method, path, started_at, request_context).await
        }
        ("POST", ["api", "v1", "collections"]) => {
            let body = body.clone();
            let session = request_context.session.clone();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| {
                    crate::rest::collections::create_for_session(&cassie, &session, body.as_ref())
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("POST", ["api", "v1", "collections", collection, "indexes"]) => {
            let body = body.clone();
            let collection = scoped_collection(&cassie, request_context, collection)?;
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::indexes::create(&cassie, &collection, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("POST", ["api", "v1", "collections", collection, "search"]) => {
            let body = body.clone();
            let collection = scoped_collection(&cassie, request_context, collection)?;
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_embedding_search",
                move |cassie| {
                    crate::rest::search::vector_search_converted(
                        &cassie,
                        &collection,
                        body.as_ref(),
                        |result, controls| {
                            super::query_response::try_query_json_response(
                                StatusCode::OK,
                                &result,
                                controls,
                            )
                        },
                    )
                },
            )
            .await
            .map(Some)
        }
        _ => Ok(None),
    }
}

async fn dispatch_document_routes(
    method: &Method,
    segments: &[&str],
    cassie: Arc<Cassie>,
    request_context: &RestRequestContext,
    body: &RestBytes,
    path: &str,
    started_at: Instant,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (method.as_str(), segments) {
        ("POST", ["api", "v1", "collections", collection, "documents"]) => {
            let body = body.clone();
            let collection = scoped_collection(&cassie, request_context, collection)?;
            run_rest_blocking_route_controlled(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie, cancellation| {
                    crate::rest::documents::create_with_cancellation(
                        &cassie,
                        &collection,
                        body.as_ref(),
                        cancellation,
                    )
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("GET", ["api", "v1", "collections", collection, "documents", id]) => {
            let collection = scoped_collection(&cassie, request_context, collection)?;
            let id = (*id).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::documents::get(&cassie, &collection, &id),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("DELETE", ["api", "v1", "collections", collection, "documents", id]) => {
            let collection = scoped_collection(&cassie, request_context, collection)?;
            let id = (*id).to_string();
            run_rest_blocking_route_controlled(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie, cancellation| {
                    crate::rest::documents::delete_with_cancellation(
                        &cassie,
                        &collection,
                        &id,
                        cancellation,
                    )
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        _ => Ok(None),
    }
}

async fn dispatch_collection_list(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    request_context: &RestRequestContext,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    let session = request_context.session.clone();
    run_rest_blocking_route(
        cassie,
        method,
        path,
        started_at,
        "rest_route",
        move |cassie| Ok(crate::rest::scope::list_collections(&cassie, &session)),
    )
    .await
    .map(|value| Some(json_response(StatusCode::OK, &value)))
}

fn scoped_collection(
    cassie: &Cassie,
    request_context: &RestRequestContext,
    requested: &str,
) -> Result<String, (StatusCode, String)> {
    let requested = decode_collection_segment(requested).map_err(|error| map_error(&error))?;
    crate::rest::scope::resolve_collection(cassie, &request_context.session, &requested)
        .map_err(|error| map_error(&error))
}

pub(super) fn decode_collection_segment(
    requested: &str,
) -> Result<String, crate::app::CassieError> {
    let raw = requested.as_bytes();
    let mut decoded = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            let high = raw
                .get(index + 1)
                .and_then(|byte| ascii_hex_value(*byte))
                .ok_or_else(|| invalid_encoded_collection(requested))?;
            let low = raw
                .get(index + 2)
                .and_then(|byte| ascii_hex_value(*byte))
                .ok_or_else(|| invalid_encoded_collection(requested))?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(raw[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| invalid_encoded_collection(requested))?;
    if decoded
        .chars()
        .any(|character| matches!(character, '/' | '\\' | '\0'))
        || matches!(decoded.as_str(), "." | "..")
    {
        return Err(invalid_encoded_collection(requested));
    }
    Ok(decoded)
}

fn ascii_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_encoded_collection(requested: &str) -> crate::app::CassieError {
    crate::app::CassieError::Parse(format!("invalid encoded REST collection name: {requested}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use bytes::Bytes;
    use http_body_util::BodyExt;
    use hyper::{Method, StatusCode};

    use super::dispatch_collection_routes;
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;
    use crate::rest::router::RestRequestContext;

    async fn dispatch_document_request(
        cassie: &Arc<Cassie>,
        context: &RestRequestContext,
        method: Method,
        collection: &str,
        id: Option<&str>,
        body: &str,
    ) -> Result<hyper::Response<crate::rest::body::RestBody>, (StatusCode, String)> {
        let mut segments = vec!["api", "v1", "collections", collection];
        if method == Method::POST {
            segments.push("documents");
        } else if let Some(id) = id {
            segments.extend(["documents", id]);
        }
        let path = format!("/{}", segments.join("/"));
        dispatch_collection_routes(
            &method,
            &segments,
            Arc::clone(cassie),
            context,
            &Bytes::from(body.to_owned()),
            &path,
            Instant::now(),
        )
        .await
        .map(|response| response.expect("document route response"))
    }

    async fn body_json(
        response: hyper::Response<crate::rest::body::RestBody>,
    ) -> serde_json::Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("REST response body")
            .to_bytes();
        serde_json::from_slice(&body).expect("REST JSON response")
    }

    fn rest_fixture() -> (Arc<Cassie>, RestRequestContext) {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("in-memory Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE \"Cities\" (id BIGINT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create mixed-case table");
        (
            Arc::new(cassie),
            RestRequestContext {
                session,
                role: None,
                token: None,
                external_user: None,
            },
        )
    }

    #[test]
    fn should_not_write_to_a_mixed_case_collection_given_a_folded_rest_name() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let (cassie, context) = rest_fixture();

        // Act
        let result = runtime.block_on(dispatch_document_request(
            &cassie,
            &context,
            Method::POST,
            "cities",
            None,
            r#"{"id":1,"name":"unexpected"}"#,
        ));

        // Assert
        assert!(matches!(result, Err((StatusCode::NOT_FOUND, _))));
        assert!(matches!(
            cassie
                .midge
                .scan_documents("postgres.public.\"Cities\"")
                .expect("scan mixed-case table")
                .as_slice(),
            []
        ));
    }

    #[test]
    fn should_resolve_case_distinct_rest_documents_by_exact_path_name() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let (cassie, context) = rest_fixture();
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cities (id BIGINT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create lowercase table");

        // Act
        let mixed_create = runtime.block_on(dispatch_document_request(
            &cassie,
            &context,
            Method::POST,
            "\"Cities\"",
            None,
            r#"{"id":1,"name":"mixed"}"#,
        ));
        let lower_create = runtime.block_on(dispatch_document_request(
            &cassie,
            &context,
            Method::POST,
            "cities",
            None,
            r#"{"id":1,"name":"lower"}"#,
        ));
        let mixed_id = runtime.block_on(body_json(mixed_create.expect("mixed-case create")));
        let lower_id = runtime.block_on(body_json(lower_create.expect("lowercase create")));
        let mixed_read = runtime.block_on(dispatch_document_request(
            &cassie,
            &context,
            Method::GET,
            "\"Cities\"",
            Some(mixed_id["id"].as_str().expect("mixed id")),
            "",
        ));
        let lower_read = runtime.block_on(dispatch_document_request(
            &cassie,
            &context,
            Method::GET,
            "cities",
            Some(lower_id["id"].as_str().expect("lower id")),
            "",
        ));

        // Assert
        let mixed_read = mixed_read.expect("mixed-case GET");
        let lower_read = lower_read.expect("lowercase GET");
        assert_eq!(mixed_read.status(), StatusCode::OK);
        assert_eq!(lower_read.status(), StatusCode::OK);
        let mixed_document = runtime.block_on(body_json(mixed_read));
        let lower_document = runtime.block_on(body_json(lower_read));
        assert_eq!(mixed_document["name"], "mixed");
        assert_eq!(lower_document["name"], "lower");
    }
}
