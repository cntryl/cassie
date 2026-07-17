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
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::collections::create(&cassie, body.as_ref()),
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
                    crate::rest::search::vector_search(&cassie, &collection, body.as_ref())
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
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
    crate::rest::scope::resolve_collection(cassie, &request_context.session, requested)
        .map_err(|error| map_error(&error))
}
