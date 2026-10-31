// src/services/novel.rs — Novel CRUD + 级联软删 + FTS（§6.2 / D-09 / D-11 / D-22）
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{CreateNovelReq, NovelDto, UpdateNovelReq};
use crate::models::enums::NovelStatus;
use crate::services::search;
use crate::services::util::{db_err, invalid, not_found, push_len, push_required};

const COLS: &str = "id, title, background, world_view, summary, style_name, style_sample, \
     style_instruction, pov, status, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct NovelRow {
    id: String,
    title: String,
    background: Option<String>,
    world_view: Option<String>,
    summary: Option<String>,
    style_name: Option<String>,
    style_sample: Option<String>,
    style_instruction: Option<String>,
    pov: Option<String>,
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
        style_name: row.style_name,
        style_sample: row.style_sample,
        style_instruction: row.style_instruction,
        pov: row.pov,
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

/// 一次请求里参与校验的小说字段（None = 未出现，不校验）。
struct NovelFields<'a> {
    title: Option<&'a str>,
    background: Option<&'a str>,
    world_view: Option<&'a str>,
    summary: Option<&'a str>,
    style_name: Option<&'a str>,
    style_sample: Option<&'a str>,
    style_instruction: Option<&'a str>,
    pov: Option<&'a str>,
    require_title: bool,
}

fn validate(fields: NovelFields<'_>) -> Result<(), AppError> {
    let NovelFields {
        title,
        background,
        world_view,
        summary,
        style_name,
        style_sample,
        style_instruction,
        pov,
        require_title,
    } = fields;
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
    push_len(&mut errs, "style_name", style_name.map(str::trim), 40);
    push_len(&mut errs, "style_sample", style_sample.map(str::trim), 6000);
    push_len(
        &mut errs,
        "style_instruction",
        style_instruction.map(str::trim),
        1000,
    );
    if let Some(value) = pov {
        let normalized = value.trim();
        if !normalized.is_empty() && !matches!(normalized, "第一人称" | "第三人称") {
            errs.push(("pov", "pov 仅支持「第一人称」或「第三人称」".to_string()));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(invalid(errs))
    }
}

/// 部分更新中的可清空文本：全空白视为清空（NULL），否则 trim 后覆盖。
fn clearable(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
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
    validate(NovelFields {
        title: Some(&req.title),
        background: req.background.as_deref(),
        world_view: req.world_view.as_deref(),
        summary: req.summary.as_deref(),
        style_name: None,
        style_sample: None,
        style_instruction: None,
        pov: None,
        require_title: true,
    })?;

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
    validate(NovelFields {
        title: req.title.as_deref(),
        background: req.background.as_deref(),
        world_view: req.world_view.as_deref(),
        summary: req.summary.as_deref(),
        style_name: req.style_name.as_deref(),
        style_sample: req.style_sample.as_deref(),
        style_instruction: req.style_instruction.as_deref(),
        pov: req.pov.as_deref(),
        require_title: false,
    })?;
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

    // 部分更新：仅把请求里出现的字段拼进 SET；None = 不改。
    let mut sets: Vec<&'static str> = Vec::new();
    let mut values: Vec<Option<String>> = Vec::new();
    if let Some(title) = req.title {
        sets.push("title = ?");
        values.push(Some(title));
    }
    if let Some(background) = req.background {
        sets.push("background = ?");
        values.push(Some(background));
    }
    if let Some(world_view) = req.world_view {
        sets.push("world_view = ?");
        values.push(Some(world_view));
    }
    if let Some(summary) = req.summary {
        sets.push("summary = ?");
        values.push(Some(summary));
    }
    if let Some(style_name) = req.style_name {
        sets.push("style_name = ?");
        values.push(clearable(style_name));
    }
    if let Some(style_sample) = req.style_sample {
        sets.push("style_sample = ?");
        values.push(clearable(style_sample));
    }
    if let Some(style_instruction) = req.style_instruction {
        sets.push("style_instruction = ?");
        values.push(clearable(style_instruction));
    }
    if let Some(pov) = req.pov {
        sets.push("pov = ?");
        values.push(clearable(pov));
    }
    if let Some(status) = req.status {
        sets.push("status = ?");
        values.push(Some(NovelStatus::to_db(status)));
    }
    sets.push("updated_at = strftime('%s','now')");

    let sql = format!("UPDATE novels SET {} WHERE id = ?", sets.join(", "));
    let mut update = sqlx::query(&sql);
    for value in &values {
        update = update.bind(value);
    }
    update.bind(id).execute(&mut *tx).await.map_err(db_err)?;

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
