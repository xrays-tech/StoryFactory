// src/handlers/recycle::rs — /api/v1/recycle 端点（§4.7 list / restore / purge）
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::dto::RestoreReq;
use crate::models::enums::RecycleEntity;
use crate::services::recycle;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RecycleQ {
    #[serde(rename = "type", alias = "entity_type")]
    pub entity_type: Option<RecycleEntity>,
    pub novel_id: Option<String>,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn list_recycle(
    state: State<AppState>,
    params: AppQuery<RecycleQ>,
) -> Result<Response, AppError> {
    let r = recycle::list(
        &state.pool,
        params.entity_type,
        params.novel_id.as_deref(),
        params.page,
        params.page_size,
    )
    .await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn restore_recycle(
    state: State<AppState>,
    body: AppJson<RestoreReq>,
) -> Result<Response, AppError> {
    let r = recycle::restore(&state.pool, body.0).await?;
    Ok((StatusCode::OK, Json(r)).into_response())
}

pub async fn purge_recycle(
    state: State<AppState>,
    path: Path<(String, String)>,
) -> Result<Response, AppError> {
    let (etype, id) = path.0;
    let entity = RecycleEntity::parse(&etype).ok_or_else(|| {
        AppError::validation(
            "非法回收站实体类型",
            crate::models::common::validation_details(vec![(
                "type",
                "必须是 novel/volume/chapter/character/item",
            )]),
        )
    })?;
    recycle::purge(&state.pool, entity, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
