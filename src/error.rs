// src/error.rs — AppError / ErrorCode 全局错误处理（§5.3）
// 响应形状: { "error": { "code", "message", "details?", "request_id" } }
use serde::Serialize;
use thiserror::Error;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // 资源
    NovelNotFound,
    CharacterNotFound,
    ItemNotFound,
    VolumeNotFound,
    ChapterNotFound,
    SessionNotFound,
    RecycleItemNotFound,
    ImportNotFound,
    RouteNotFound,
    // 状态冲突
    SessionConflict,
    IdempotencyConflict,
    NameConflict,
    ImportNotReady,
    ImportConflict,
    // 请求
    ValidationFailed,
    PayloadTooLarge,
    RateLimited,
    HostForbidden,
    OriginForbidden,
    TokenRequired,
    TokenInvalid,
    UnsupportedImportVersion,
    // LLM
    LlmUnavailable,
    LlmRateLimited,
    LlmConfigMissing,
    // 系统
    ConfigInvalid,
    CryptoError,
    Internal,
}

pub fn status_of(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::NovelNotFound => 404,
        ErrorCode::CharacterNotFound => 404,
        ErrorCode::ItemNotFound => 404,
        ErrorCode::VolumeNotFound => 404,
        ErrorCode::ChapterNotFound => 404,
        ErrorCode::SessionNotFound => 404,
        ErrorCode::RecycleItemNotFound => 404,
        ErrorCode::ImportNotFound => 404,
        ErrorCode::RouteNotFound => 404,
        ErrorCode::SessionConflict => 409,
        ErrorCode::IdempotencyConflict => 409,
        ErrorCode::NameConflict => 409,
        ErrorCode::ImportNotReady => 409,
        ErrorCode::ImportConflict => 409,
        ErrorCode::ValidationFailed => 422,
        ErrorCode::PayloadTooLarge => 413,
        ErrorCode::RateLimited => 429,
        ErrorCode::HostForbidden => 403,
        ErrorCode::OriginForbidden => 403,
        ErrorCode::TokenRequired => 401,
        ErrorCode::TokenInvalid => 401,
        ErrorCode::UnsupportedImportVersion => 422,
        ErrorCode::LlmUnavailable => 503,
        ErrorCode::LlmRateLimited => 429,
        ErrorCode::LlmConfigMissing => 503,
        ErrorCode::ConfigInvalid => 500,
        ErrorCode::CryptoError => 500,
        ErrorCode::Internal => 500,
    }
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub request_id: String,
    pub status: u16,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> AppError {
        let status = status_of(code.clone());
        AppError {
            code,
            message: message.into(),
            details: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            status,
        }
    }

    pub fn with_details(
        code: ErrorCode,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> AppError {
        let mut e = AppError::new(code, message);
        e.details = Some(details);
        e
    }

    pub fn validation(message: impl Into<String>, details: serde_json::Value) -> AppError {
        AppError::with_details(ErrorCode::ValidationFailed, message, details)
    }

    pub fn too_large() -> AppError {
        AppError::new(ErrorCode::PayloadTooLarge, "请求体超过字节限额".to_string())
    }

    pub fn rate_limited() -> AppError {
        AppError::new(
            ErrorCode::RateLimited,
            "并发或预算已达上限，请稍后重试".to_string(),
        )
    }

    pub fn host_forbidden() -> AppError {
        AppError::new(ErrorCode::HostForbidden, "Host 头不在允许列表".to_string())
    }

    pub fn origin_forbidden() -> AppError {
        AppError::new(
            ErrorCode::OriginForbidden,
            "Origin 头不在允许列表".to_string(),
        )
    }

    pub fn token_required() -> AppError {
        AppError::new(
            ErrorCode::TokenRequired,
            "缺少访问令牌（?token= 或 X-App-Token）".to_string(),
        )
    }

    pub fn token_invalid() -> AppError {
        AppError::new(ErrorCode::TokenInvalid, "访问令牌无效".to_string())
    }

    pub fn route_not_found() -> AppError {
        AppError::new(ErrorCode::RouteNotFound, "未知 API 路由".to_string())
    }

    pub fn internal(message: impl Into<String>) -> AppError {
        AppError::new(ErrorCode::Internal, message)
    }

    pub fn sqlx_to_app_error(
        e: sqlx::Error,
        fallback: ErrorCode,
        message: impl Into<String>,
    ) -> AppError {
        // UNIQUE 约束 -> NameConflict（业务唯一冲突）; 其余 -> fallback
        let is_unique = e
            .as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false);
        if is_unique {
            return AppError::new(
                ErrorCode::NameConflict,
                "名称/唯一字段冲突: 已存在".to_string(),
            );
        }
        AppError::new(fallback, message)
    }
}

#[derive(Serialize)]
struct ErrorInner {
    code: ErrorCode,
    message: String,
    details: Option<serde_json::Value>,
    request_id: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorInner,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let inner = ErrorInner {
            code: self.code,
            message: self.message,
            details: self.details,
            request_id: self.request_id,
        };
        let body = ErrorBody { error: inner };
        let mut res = Json(body).into_response();
        *res.status_mut() =
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        res
    }
}
