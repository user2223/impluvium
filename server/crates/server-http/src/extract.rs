// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Request extractors that fail with the web error shape.
//!
//! They wrap the framework extractors and replace their rejection by an
//! [`HttpError`] with a fixed code. The rejection text of the framework is
//! dropped because it can quote the input.

use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::http::request::Parts;
use axum::response::IntoResponse;
use serde::de::DeserializeOwned;
use server_api_types::ErrorCode;

use crate::error::HttpError;

/// A JSON request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => {
                let code = match rejection.into_response().status().as_u16() {
                    413 => ErrorCode::PayloadTooLarge,
                    415 => ErrorCode::UnsupportedMediaType,
                    400 | 422 => ErrorCode::InvalidJsonBody,
                    500..=599 => ErrorCode::InternalError,
                    _ => ErrorCode::BadRequest,
                };
                Err(HttpError::new(code))
            }
        }
    }
}

/// The query string, deserialised into `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| HttpError::new(ErrorCode::InvalidQuery))
    }
}

/// The path parameters, deserialised into `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiPath<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|rejection| {
                // A missing parameter, a wrong parameter count or a type that
                // cannot come from a path means the route and its handler
                // disagree, which is a server fault; the framework answers
                // those with a 5xx status. Parse failures are the client's.
                let code = if rejection.into_response().status().is_server_error() {
                    ErrorCode::InternalError
                } else {
                    ErrorCode::InvalidPathParam
                };
                HttpError::new(code)
            })
    }
}
