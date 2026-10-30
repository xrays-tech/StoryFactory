// src/extract.rs — axum 提取器包装：把原生 rejection 映射为统一错误信封（§5.3）
use axum::async_trait;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Json, Query};
use axum::http::request::Parts;
use axum::Json as JsonResponse;
use serde::de::DeserializeOwned;
use std::ops::Deref;

use crate::error::AppError;
use crate::models::common::validation_details;

pub struct AppJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let JsonResponse(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(json_rejection)?;
        Ok(AppJson(value))
    }
}

pub struct AppQuery<T>(pub T);

impl<T> Deref for AppQuery<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> Deref for AppJson<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[async_trait]
impl<T, S> FromRequestParts<S> for AppQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(query_rejection)?;
        Ok(AppQuery(value))
    }
}

fn json_rejection(e: JsonRejection) -> AppError {
    AppError::validation(
        "请求体解析失败",
        validation_details(vec![("body", e.body_text())]),
    )
}

fn query_rejection(e: QueryRejection) -> AppError {
    AppError::validation(
        "查询参数解析失败",
        validation_details(vec![("query", e.body_text())]),
    )
}
