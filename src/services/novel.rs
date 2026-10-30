// src/services/novel.rs — Novel CRUD + 级联软删 + FTS（§6.2 / D-09 / D-11 / D-22）
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{CreateNovelReq, NovelDto, UpdateNovelReq};
use crate::models::enums::NovelStatus;
use crate::services::search;
use crate::services::util::{db_err, invalid, not_found, push_len, push_required};

const COLS: &str = "id, title, background, world_view, summary, status, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct NovelRow {
    id: String,
    title: String,
    background: Option<String>,
    world_view: Option<String>,
    summary: Option<String>,
    status: String,
    created_at: i64,
    updated_at: i64,
}

fn dto(row: NovelRow) -> NovelDto {
    NovelDto {
        id: row.id,
        title: row.title,
        background: row.background,
        world_view: row.world_view,
        summary: row.summary,
        status: NovelStatus::from_db_or_default(row.status),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn fts_body(d: &NovelDto) -> String {
    [
        d.summary.as_deref(),
        d.background.as_deref(),
        d.world_view.as_deref(),
    ]
    .iter()
    .flatten()
    .copied()
    .collect::<Vec<_>>()
    .join("\n")
}

fn validate(
    title: Option<&str>,
    background: Option<&str>,
    world_view: Option<&str>,
    summary: Option<&str>,
    require_title: bool,
) -> Result<(), AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    if require_title || title.is_some() {
        match title {
            Some(t) => push_required(&mut errs, "title", t, 200),
            None => errs.push(("title", "title 不能为空".to_string())),
        }
    }
    push_len(&mut errs, "background", background, 20000);
    push_len(&mut errs, "world_view", world_view, 20000);
    push_len(&mut errs, "summary", summary, 20000);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(invalid(errs))
    }
}

fn like_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

pub async fn list(
    pool: &SqlitePool,
    q: Option<&str>,
    status: Option<NovelStatus>,
    page: i64,
    page_size: i64,
) -> Result<Page<NovelDto>, AppError> {
    let (p, ps, limit, offset) = resolve_page(page, page_size);
    let mut where_sql = String::from("deleted_at IS NULL");
    let query = q.unwrap_or("").trim();
    if !query.is_empty() {
        where_sql
            .push_str(" AND (title LIKE ? ESCAPE '\\' OR IFNULL(summary,'') LIKE ? ESCAPE '\\')");
    }
    if status.is_some() {
        where_sql.push_str(" AND status = ?");
    }
    let like = format!("%{}%", like_escape(query));

    let count_sql = format!("SELECT COUNT(*) FROM novels WHERE {where_sql}");
    let mut count_stmt = sqlx::query_scalar::<_, i64>(&count_sql);
    if !query.is_empty() {
        count_stmt = count_stmt.bind(&like).bind(&like);
    }
    if let Some(s) = status {
        count_stmt = count_stmt.bind(NovelStatus::to_db(s));
    }
    let total = count_stmt.fetch_one(pool).await.map_err(db_err)?;

    let rows_sql = format!(
        "SELECT {COLS} FROM novels WHERE {where_sql} ORDER BY created_at DESC, id LIMIT ? OFFSET ?"
    );
    let mut rows_stmt = sqlx::query_as::<_, NovelRow>(&rows_sql);
    if !query.is_empty() {
        rows_stmt = rows_stmt.bind(&like).bind(&like);
    }
    if let Some(s) = status {
        rows_stmt = rows_stmt.bind(NovelStatus::to_db(s));
    }
    let rows = rows_stmt
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;

    Ok(Page {
        items: rows.into_iter().map(dto).collect(),
        total,
        page: p,
        page_size: ps,
    })
}

pub async fn create(pool: &SqlitePool, req: CreateNovelReq) -> Result<NovelDto, AppError> {
    validate(
        Some(&req.title),
        req.background.as_deref(),
        req.world_view.as_deref(),
        req.summary.as_deref(),
        true,
    )?;

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO novels(title, background, world_view, summary) VALUES(?, ?, ?, ?) RETURNING id",
    )
    .bind(&req.title)
    .bind(&req.background)
    .bind(&req.world_view)
    .bind(&req.summary)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;

    let row = sqlx::query_as::<_, NovelRow>(&format!("SELECT {COLS} FROM novels WHERE id = ?"))
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let d = dto(row);
    search::index_entity(&mut tx, "novel", &d.id, &d.id, &d.title, &fts_body(&d)).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<NovelDto, AppError> {
    let row = sqlx::query_as::<_, NovelRow>(&format!(
        "SELECT {COLS} FROM novels WHERE id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(r) => Ok(dto(r)),
        None => Err(not_found(ErrorCode::NovelNotFound, "小说不存在或已删除")),
    }
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    req: UpdateNovelReq,
) -> Result<NovelDto, AppError> {
    validate(
        req.title.as_deref(),
        req.background.as_deref(),
        req.world_view.as_deref(),
        req.summary.as_deref(),
        false,
    )?;
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current = sqlx::query_as::<_, NovelRow>(&format!(
        "SELECT {COLS} FROM novels WHERE id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let current = match current {
        Some(c) => c,
        None => {
            tx.rollback().await.map_err(db_err)?;
            return Err(not_found(ErrorCode::NovelNotFound, "小说不存在或已删除"));
        }
    };

    let title = req.title.unwrap_or(current.title);
    let background = req.background.or(current.background);
    let world_view = req.world_view.or(current.world_view);
    let summary = req.summary.or(current.summary);
    let status = req.status.map(NovelStatus::to_db).unwrap_or(current.status);

    sqlx::query(
        "UPDATE novels SET title = ?, background = ?, world_view = ?, summary = ?, status = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&title)
    .bind(&background)
    .bind(&world_view)
    .bind(&summary)
    .bind(&status)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;

    let row = sqlx::query_as::<_, NovelRow>(&format!("SELECT {COLS} FROM novels WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let d = dto(row);
    search::index_entity(&mut tx, "novel", &d.id, &d.id, &d.title, &fts_body(&d)).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

/// 软删：novel + 全部子实体写入同一 deleted_batch（D-11）。
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM novels WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    if exists.is_none() {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::NovelNotFound, "小说不存在或已删除"));
    }
    let batch = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "UPDATE novels SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE id = ?",
    )
    .bind(&batch)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    for table in ["volumes", "chapters", "characters", "items"] {
        let sql = format!(
            "UPDATE {table} SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE novel_id = ? AND deleted_at IS NULL"
        );
        sqlx::query(&sql)
            .bind(&batch)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    search::remove_novel_entities(&mut tx, id).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
