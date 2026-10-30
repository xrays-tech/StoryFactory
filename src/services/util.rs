// src/services/util.rs — 服务层共用校验/错误辅助
use sqlx::sqlite::SqlitePool;

use crate::error::{AppError, ErrorCode};
use crate::models::common::validation_details;

pub fn db_err(e: sqlx::Error) -> AppError {
    tracing::error!(error = %e, "database error");
    AppError::sqlx_to_app_error(e, ErrorCode::Internal, "数据库错误")
}

pub fn not_found(code: ErrorCode, message: &str) -> AppError {
    AppError::new(code, message)
}

pub fn invalid(errs: Vec<(&'static str, String)>) -> AppError {
    AppError::validation("请求参数校验失败", validation_details(errs))
}

pub fn push_len(
    errs: &mut Vec<(&'static str, String)>,
    field: &'static str,
    value: Option<&str>,
    max: usize,
) {
    if let Some(v) = value {
        let n = v.chars().count();
        if n > max {
            errs.push((
                field,
                format!("{field} 长度不能超过 {max} 个字符（当前 {n}）"),
            ));
        }
    }
}

pub fn push_required(
    errs: &mut Vec<(&'static str, String)>,
    field: &'static str,
    value: &str,
    max: usize,
) {
    let n = value.chars().count();
    if n == 0 {
        errs.push((field, format!("{field} 不能为空")));
    } else if n > max {
        errs.push((field, format!("{field} 长度不能超过 {max} 个字符")));
    }
}

pub async fn novel_alive(pool: &SqlitePool, novel_id: &str) -> Result<(), AppError> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM novels WHERE id = ? AND deleted_at IS NULL")
            .bind(novel_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    if found.is_none() {
        return Err(not_found(ErrorCode::NovelNotFound, "小说不存在或已删除"));
    }
    Ok(())
}

pub async fn novel_alive_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    novel_id: &str,
) -> Result<(), AppError> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM novels WHERE id = ? AND deleted_at IS NULL")
            .bind(novel_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    if found.is_none() {
        return Err(not_found(ErrorCode::NovelNotFound, "小说不存在或已删除"));
    }
    Ok(())
}

pub async fn next_volume_index(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    novel_id: &str,
) -> Result<i64, AppError> {
    let max: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(volume_index) FROM volumes WHERE novel_id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(max.unwrap_or(0) + 1)
}

pub async fn next_chapter_index(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    volume_id: &str,
) -> Result<i64, AppError> {
    let max: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(chapter_index) FROM chapters WHERE volume_id = ? AND deleted_at IS NULL",
    )
    .bind(volume_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(max.unwrap_or(0) + 1)
}
