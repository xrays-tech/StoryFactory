// src/services/outline.rs — 卷/章 CRUD、排序、跨卷移动（§6.2 / D-11）
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{
    ChapterDto, CreateChapterReq, CreateVolumeReq, MoveReq, UpdateChapterReq, UpdateVolumeReq,
    VolumeDto,
};
use crate::models::enums::{ChapterStatus, MoveDirection};
use crate::services::search;
use crate::services::util::{
    db_err, invalid, next_chapter_index, next_volume_index, not_found, novel_alive, push_len,
};

/// 章节序号上界：与两阶段重排的偏移量（100000）拉开安全距离，避免加法溢出。
const MAX_CHAPTER_INDEX: i64 = 1_000_000;

const VOL_COLS: &str = "id, novel_id, volume_index, title, summary, created_at, updated_at";
const CH_COLS: &str = "id, novel_id, volume_id, chapter_index, title, scene_prompt, final_outline, status, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct VolumeRow {
    id: String,
    novel_id: String,
    volume_index: i64,
    title: String,
    summary: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct ChapterRow {
    id: String,
    novel_id: String,
    volume_id: String,
    chapter_index: i64,
    title: String,
    scene_prompt: Option<String>,
    final_outline: Option<String>,
    status: String,
    created_at: i64,
    updated_at: i64,
}

fn vol_dto(row: VolumeRow) -> VolumeDto {
    VolumeDto {
        id: row.id,
        novel_id: row.novel_id,
        volume_index: row.volume_index,
        title: row.title,
        summary: row.summary,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn ch_dto(row: ChapterRow) -> ChapterDto {
    ChapterDto {
        id: row.id,
        novel_id: row.novel_id,
        volume_id: row.volume_id,
        chapter_index: row.chapter_index,
        title: row.title,
        scene_prompt: row.scene_prompt,
        final_outline: row.final_outline,
        status: ChapterStatus::from_db_or_default(row.status),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn chapter_body(d: &ChapterDto) -> String {
    [d.scene_prompt.as_deref(), d.final_outline.as_deref()]
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// 在调用方事务内重建章节全文索引。
/// 供 finalize 等绕过 CRUD 直接改表的路径复用，确保标题/正文口径与大纲服务一致。
pub async fn reindex_chapter(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    novel_id: &str,
    chapter_id: &str,
) -> Result<(), AppError> {
    let row = sqlx::query_as::<_, ChapterRow>(&format!(
        "SELECT {CH_COLS} FROM chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(chapter_id)
    .bind(novel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some(row) = row else {
        return Ok(());
    };
    let d = ch_dto(row);
    search::index_entity(
        tx,
        "chapter",
        &d.id,
        &d.novel_id,
        &d.title,
        &chapter_body(&d),
    )
    .await
}

async fn volume_alive_in_novel(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    novel_id: &str,
    volume_id: &str,
) -> Result<bool, AppError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM volumes WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(volume_id)
    .bind(novel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(found.is_some())
}

pub async fn list_volumes(pool: &SqlitePool, novel_id: &str) -> Result<Vec<VolumeDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let rows = sqlx::query_as::<_, VolumeRow>(&format!(
        "SELECT {VOL_COLS} FROM volumes WHERE novel_id = ? AND deleted_at IS NULL ORDER BY volume_index"
    ))
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(vol_dto).collect())
}

pub async fn create_volume(
    pool: &SqlitePool,
    novel_id: &str,
    req: CreateVolumeReq,
) -> Result<VolumeDto, AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    let title = req.title.unwrap_or_default();
    push_len(&mut errs, "title", Some(&title), 200);
    push_len(&mut errs, "summary", req.summary.as_deref(), 20000);
    if !errs.is_empty() {
        return Err(invalid(errs));
    }
    novel_alive(pool, novel_id).await?;
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let index = next_volume_index(&mut tx, novel_id).await?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO volumes(novel_id, volume_index, title, summary) VALUES(?, ?, ?, ?) RETURNING id",
    )
    .bind(novel_id)
    .bind(index)
    .bind(&title)
    .bind(&req.summary)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let row =
        sqlx::query_as::<_, VolumeRow>(&format!("SELECT {VOL_COLS} FROM volumes WHERE id = ?"))
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let d = vol_dto(row);
    search::index_entity(
        &mut tx,
        "volume",
        &d.id,
        novel_id,
        &d.title,
        d.summary.as_deref().unwrap_or(""),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn get_volume(
    pool: &SqlitePool,
    novel_id: &str,
    vid: &str,
) -> Result<VolumeDto, AppError> {
    let row = sqlx::query_as::<_, VolumeRow>(&format!(
        "SELECT {VOL_COLS} FROM volumes WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(vid)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(r) => Ok(vol_dto(r)),
        None => Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除")),
    }
}

pub async fn update_volume(
    pool: &SqlitePool,
    novel_id: &str,
    vid: &str,
    req: UpdateVolumeReq,
) -> Result<VolumeDto, AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    push_len(&mut errs, "title", req.title.as_deref(), 200);
    push_len(&mut errs, "summary", req.summary.as_deref(), 20000);
    if !errs.is_empty() {
        return Err(invalid(errs));
    }
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current = sqlx::query_as::<_, VolumeRow>(&format!(
        "SELECT {VOL_COLS} FROM volumes WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(vid)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let current = match current {
        Some(c) => c,
        None => {
            tx.rollback().await.map_err(db_err)?;
            return Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除"));
        }
    };
    let title = req.title.unwrap_or(current.title);
    let summary = req.summary.or(current.summary);
    sqlx::query(
        "UPDATE volumes SET title = ?, summary = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&title)
    .bind(&summary)
    .bind(vid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let row =
        sqlx::query_as::<_, VolumeRow>(&format!("SELECT {VOL_COLS} FROM volumes WHERE id = ?"))
            .bind(vid)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let d = vol_dto(row);
    search::index_entity(
        &mut tx,
        "volume",
        &d.id,
        novel_id,
        &d.title,
        d.summary.as_deref().unwrap_or(""),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn move_volume(
    pool: &SqlitePool,
    novel_id: &str,
    vid: &str,
    req: MoveReq,
) -> Result<Vec<VolumeDto>, AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current: Option<(i64,)> = sqlx::query_as(
        "SELECT volume_index FROM volumes WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(vid)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let current_index = match current {
        Some((idx,)) => idx,
        None => {
            tx.rollback().await.map_err(db_err)?;
            return Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除"));
        }
    };

    let neighbor: Option<(String, i64)> = match req.direction {
        MoveDirection::Up => sqlx::query_as(
            "SELECT id, volume_index FROM volumes WHERE novel_id = ? AND deleted_at IS NULL AND volume_index < ? ORDER BY volume_index DESC LIMIT 1",
        )
        .bind(novel_id)
        .bind(current_index)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?,
        MoveDirection::Down => sqlx::query_as(
            "SELECT id, volume_index FROM volumes WHERE novel_id = ? AND deleted_at IS NULL AND volume_index > ? ORDER BY volume_index ASC LIMIT 1",
        )
        .bind(novel_id)
        .bind(current_index)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?,
    };

    if let Some((neighbor_id, neighbor_index)) = neighbor {
        let max: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(volume_index) FROM volumes WHERE novel_id = ? AND deleted_at IS NULL",
        )
        .bind(novel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        let temp = max.unwrap_or(0) + 1;
        sqlx::query("UPDATE volumes SET volume_index = ? WHERE id = ?")
            .bind(temp)
            .bind(vid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query("UPDATE volumes SET volume_index = ? WHERE id = ?")
            .bind(current_index)
            .bind(&neighbor_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query("UPDATE volumes SET volume_index = ? WHERE id = ?")
            .bind(neighbor_index)
            .bind(vid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    list_volumes(pool, novel_id).await
}

pub async fn soft_delete_volume(
    pool: &SqlitePool,
    novel_id: &str,
    vid: &str,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM volumes WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(vid)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    if exists.is_none() {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除"));
    }
    let batch = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "UPDATE volumes SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE id = ?",
    )
    .bind(&batch)
    .bind(vid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "UPDATE chapters SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE volume_id = ? AND deleted_at IS NULL",
    )
    .bind(&batch)
    .bind(vid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    search::remove_volume_entities(&mut tx, vid).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

pub async fn list_chapters(
    pool: &SqlitePool,
    novel_id: &str,
    volume_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Page<ChapterDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    if !volume_alive_in_novel(&mut tx, novel_id, volume_id).await? {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除"));
    }
    let (p, ps, limit, offset) = resolve_page(page, page_size);
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chapters WHERE volume_id = ? AND deleted_at IS NULL",
    )
    .bind(volume_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let rows = sqlx::query_as::<_, ChapterRow>(&format!(
        "SELECT {CH_COLS} FROM chapters WHERE volume_id = ? AND deleted_at IS NULL ORDER BY chapter_index LIMIT ? OFFSET ?"
    ))
    .bind(volume_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Page {
        items: rows.into_iter().map(ch_dto).collect(),
        total,
        page: p,
        page_size: ps,
    })
}

pub async fn create_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    volume_id: &str,
    req: CreateChapterReq,
) -> Result<ChapterDto, AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    let title = req.title.unwrap_or_default();
    push_len(&mut errs, "title", Some(&title), 200);
    push_len(&mut errs, "scene_prompt", req.scene_prompt.as_deref(), 4000);
    if !errs.is_empty() {
        return Err(invalid(errs));
    }
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    if !volume_alive_in_novel(&mut tx, novel_id, volume_id).await? {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::VolumeNotFound, "卷不存在或已删除"));
    }
    let index = next_chapter_index(&mut tx, volume_id).await?;
    let status = req
        .status
        .map(ChapterStatus::to_db)
        .unwrap_or_else(|| "draft".to_string());
    let id: String = sqlx::query_scalar(
        "INSERT INTO chapters(novel_id, volume_id, chapter_index, title, scene_prompt, status) VALUES(?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(novel_id)
    .bind(volume_id)
    .bind(index)
    .bind(&title)
    .bind(&req.scene_prompt)
    .bind(&status)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let row =
        sqlx::query_as::<_, ChapterRow>(&format!("SELECT {CH_COLS} FROM chapters WHERE id = ?"))
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let d = ch_dto(row);
    search::index_entity(
        &mut tx,
        "chapter",
        &d.id,
        novel_id,
        &d.title,
        &chapter_body(&d),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn get_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    cid: &str,
) -> Result<ChapterDto, AppError> {
    let row = sqlx::query_as::<_, ChapterRow>(&format!(
        "SELECT {CH_COLS} FROM chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(cid)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(r) => Ok(ch_dto(r)),
        None => Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除")),
    }
}

pub async fn update_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    cid: &str,
    req: UpdateChapterReq,
) -> Result<ChapterDto, AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    push_len(&mut errs, "title", req.title.as_deref(), 200);
    push_len(&mut errs, "scene_prompt", req.scene_prompt.as_deref(), 4000);
    push_len(
        &mut errs,
        "final_outline",
        req.final_outline.as_deref(),
        200000,
    );
    if let Some(v) = req.chapter_index {
        if !(1..=MAX_CHAPTER_INDEX).contains(&v) {
            errs.push((
                "chapter_index",
                format!("chapter_index 必须为 1..={MAX_CHAPTER_INDEX}"),
            ));
        }
    }
    if !errs.is_empty() {
        return Err(invalid(errs));
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current = sqlx::query_as::<_, ChapterRow>(&format!(
        "SELECT {CH_COLS} FROM chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(cid)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let current = match current {
        Some(c) => c,
        None => {
            tx.rollback().await.map_err(db_err)?;
            return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
        }
    };

    let target_volume = req.volume_id.unwrap_or(current.volume_id.clone());
    if !volume_alive_in_novel(&mut tx, novel_id, &target_volume).await? {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::VolumeNotFound, "目标卷不存在或已删除"));
    }
    let moved_volume = target_volume != current.volume_id;
    let target_index = match req.chapter_index {
        Some(v) => v,
        None if moved_volume => next_chapter_index(&mut tx, &target_volume).await?,
        None => current.chapter_index,
    };
    let moved_index = target_index != current.chapter_index || moved_volume;

    if moved_index {
        // 目标位置被占用时，后续章节整体后移一位。先把待移动章节停放到两卷最大序号之外：
        // 否则其旧位置会在第二阶段与后移章节撞唯一索引（同卷向下移动场景）。
        let occupied: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chapters WHERE volume_id = ? AND chapter_index = ? AND deleted_at IS NULL AND id <> ?",
        )
        .bind(&target_volume)
        .bind(target_index)
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        if occupied.is_some() {
            let max_index: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(chapter_index) FROM chapters WHERE deleted_at IS NULL AND volume_id IN (?, ?)",
            )
            .bind(&current.volume_id)
            .bind(&target_volume)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
            let parking_index = max_index.unwrap_or(0).saturating_add(1_000_000);
            sqlx::query("UPDATE chapters SET chapter_index = ? WHERE id = ?")
                .bind(parking_index)
                .bind(cid)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            sqlx::query(
                "UPDATE chapters SET chapter_index = chapter_index + 100000 WHERE volume_id = ? AND chapter_index >= ? AND deleted_at IS NULL AND id <> ?",
            )
            .bind(&target_volume)
            .bind(target_index)
            .bind(cid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            sqlx::query(
                "UPDATE chapters SET chapter_index = chapter_index - 99999 WHERE volume_id = ? AND chapter_index >= ? AND deleted_at IS NULL AND id <> ?",
            )
            .bind(&target_volume)
            .bind(target_index.saturating_add(100000))
            .bind(cid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    }

    let title = req.title.unwrap_or(current.title);
    let scene_prompt = req.scene_prompt.or(current.scene_prompt);
    let final_outline = req.final_outline.or(current.final_outline);
    let status = req
        .status
        .map(ChapterStatus::to_db)
        .unwrap_or(current.status);
    let index = if moved_index {
        target_index
    } else {
        current.chapter_index
    };

    sqlx::query(
        "UPDATE chapters SET volume_id = ?, chapter_index = ?, title = ?, scene_prompt = ?, final_outline = ?, status = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&target_volume)
    .bind(index)
    .bind(&title)
    .bind(&scene_prompt)
    .bind(&final_outline)
    .bind(&status)
    .bind(cid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;

    let row =
        sqlx::query_as::<_, ChapterRow>(&format!("SELECT {CH_COLS} FROM chapters WHERE id = ?"))
            .bind(cid)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let d = ch_dto(row);
    search::index_entity(
        &mut tx,
        "chapter",
        &d.id,
        novel_id,
        &d.title,
        &chapter_body(&d),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn soft_delete_chapter(
    pool: &SqlitePool,
    novel_id: &str,
    cid: &str,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let batch = uuid::Uuid::new_v4().to_string();
    let affected = sqlx::query(
        "UPDATE chapters SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(&batch)
    .bind(cid)
    .bind(novel_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if affected == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
    }
    search::remove_entity(&mut tx, cid).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
