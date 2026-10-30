// src/handlers/export_import.rs — 导出/导入端点（附录 C）
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::AppQuery;
use crate::services::export;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportQ {
    pub include_sessions: Option<bool>,
    pub include_deleted: Option<bool>,
}

pub async fn export_novel(
    state: State<AppState>,
    path: Path<String>,
    query: AppQuery<ExportQ>,
) -> Result<Response, AppError> {
    let (filename, bytes) = export::export_novel(
        &state.pool,
        &path.0,
        query.include_sessions.unwrap_or(true),
        query.include_deleted.unwrap_or(false),
    )
    .await?;
    let mut response = Response::new(axum::body::Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

pub async fn import_package(state: State<AppState>, body: Bytes) -> Result<Response, AppError> {
    let report = export::import_package(&state.pool, &body).await?;
    Ok((StatusCode::CREATED, Json(report)).into_response())
}
