// src/services/recycle.rs — 回收站列表/恢复/永久清除（§4.7 / D-11 / D-12）
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{RecycleItemDto, RestoreReq};
use crate::models::enums::RecycleEntity;
use crate::services::search;
use crate::services::util::{db_err, invalid, not_found};

type NovelText = (String, Option<String>, Option<String>, Option<String>);
type ChapterText = (String, String, Option<String>, Option<String>);
type CharacterText = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
type CharacterRestore = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(sqlx::FromRow)]
struct RecycleRow {
    entity_type: String,
    id: String,
    name: String,
    deleted_at: i64,
    novel_id: String,
    novel_title: String,
}

fn entity_of(raw: &str) -> RecycleEntity {
    RecycleEntity::from_db_or_default(raw.to_string())
}

pub async fn list(
    pool: &SqlitePool,
    entity_type: Option<RecycleEntity>,
    novel_id: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Page<RecycleItemDto>, AppError> {
    let (p, ps, limit, offset) = resolve_page(page, page_size);
    let base = "
        SELECT 'novel' AS entity_type, n.id AS id, n.title AS name, n.deleted_at AS deleted_at, n.id AS novel_id, n.title AS novel_title
          FROM novels n WHERE n.deleted_at IS NOT NULL
        UNION ALL
        SELECT 'volume', v.id, v.title, v.deleted_at, v.novel_id, n.title
          FROM volumes v JOIN novels n ON n.id = v.novel_id WHERE v.deleted_at IS NOT NULL
        UNION ALL
        SELECT 'chapter', c.id, c.title, c.deleted_at, c.novel_id, n.title
          FROM chapters c JOIN novels n ON n.id = c.novel_id WHERE c.deleted_at IS NOT NULL
        UNION ALL
        SELECT 'character', ch.id, ch.name, ch.deleted_at, ch.novel_id, n.title
          FROM characters ch JOIN novels n ON n.id = ch.novel_id WHERE ch.deleted_at IS NOT NULL
        UNION ALL
        SELECT 'item', i.id, i.name, i.deleted_at, i.novel_id, n.title
          FROM items i JOIN novels n ON n.id = i.novel_id WHERE i.deleted_at IS NOT NULL
    ";
    let mut where_sql = String::from(" WHERE 1=1");
    if entity_type.is_some() {
        where_sql.push_str(" AND entity_type = ?");
    }
    if novel_id.is_some() {
        where_sql.push_str(" AND novel_id = ?");
    }
    let type_value = entity_type.map(RecycleEntity::to_db);
    let count_sql = format!("SELECT COUNT(*) FROM ({base}){where_sql}");
    let mut count_stmt = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(v) = &type_value {
        count_stmt = count_stmt.bind(v);
    }
    if let Some(v) = novel_id {
        count_stmt = count_stmt.bind(v);
    }
    let total = count_stmt.fetch_one(pool).await.map_err(db_err)?;

    let rows_sql =
        format!("SELECT * FROM ({base}){where_sql} ORDER BY deleted_at DESC, id LIMIT ? OFFSET ?");
    let mut rows_stmt = sqlx::query_as::<_, RecycleRow>(&rows_sql);
    if let Some(v) = &type_value {
        rows_stmt = rows_stmt.bind(v);
    }
    if let Some(v) = novel_id {
        rows_stmt = rows_stmt.bind(v);
    }
    let rows = rows_stmt
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(Page {
        items: rows
            .into_iter()
            .map(|r| RecycleItemDto {
                entity_type: entity_of(&r.entity_type),
                id: r.id,
                name: r.name,
                deleted_at: r.deleted_at,
                novel_id: r.novel_id,
                novel_title: r.novel_title,
            })
            .collect(),
        total,
        page: p,
        page_size: ps,
    })
}

async fn batch_of(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    id: &str,
) -> Result<Option<String>, AppError> {
    let sql = format!("SELECT deleted_batch FROM {table} WHERE id = ? AND deleted_at IS NOT NULL");
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)
}

async fn unique_name(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    novel_id: &str,
    name: &str,
    suffix: &str,
) -> Result<String, AppError> {
    if name_free(tx, table, novel_id, name).await? {
        return Ok(name.to_string());
    }
    let mut candidate = format!("{name}（{suffix}）");
    for n in 2..=100 {
        if name_free(tx, table, novel_id, &candidate).await? {
            return Ok(candidate);
        }
        candidate = format!("{name}（{suffix}{n}）");
    }
    Err(invalid(vec![(
        "name",
        "恢复重命名次数超过上限".to_string(),
    )]))
}

async fn name_free(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    novel_id: &str,
    name: &str,
) -> Result<bool, AppError> {
    let sql = format!(
        "SELECT 1 FROM {table} WHERE novel_id = ? AND lower(name) = lower(?) AND deleted_at IS NULL"
    );
    let taken: Option<i64> = sqlx::query_scalar(&sql)
        .bind(novel_id)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(taken.is_none())
}

async fn unique_index(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    parent_col: &str,
    parent_id: &str,
    index_col: &str,
    desired: i64,
    id: &str,
) -> Result<i64, AppError> {
    let sql = format!(
        "SELECT 1 FROM {table} WHERE {parent_col} = ? AND {index_col} = ? AND deleted_at IS NULL AND id <> ?"
    );
    let taken: Option<i64> = sqlx::query_scalar(&sql)
        .bind(parent_id)
        .bind(desired)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    if taken.is_none() {
        return Ok(desired);
    }
    let max_sql = format!(
        "SELECT MAX({index_col}) FROM {table} WHERE {parent_col} = ? AND deleted_at IS NULL"
    );
    let max: Option<i64> = sqlx::query_scalar(&max_sql)
        .bind(parent_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(max.unwrap_or(0) + 1)
}

async fn reindex_novel(tx: &mut Transaction<'_, Sqlite>, novel_id: &str) -> Result<(), AppError> {
    let novel: Option<NovelText> = sqlx::query_as(
        "SELECT title, background, world_view, summary FROM novels WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    if let Some((title, background, world_view, summary)) = novel {
        let body = [summary, background, world_view]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        search::index_entity(tx, "novel", novel_id, novel_id, &title, &body).await?;
    }

    let volumes: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, title, summary FROM volumes WHERE novel_id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    for (id, title, summary) in volumes {
        search::index_entity(
            tx,
            "volume",
            &id,
            novel_id,
            &title,
            summary.as_deref().unwrap_or(""),
        )
        .await?;
    }

    let chapters: Vec<ChapterText> = sqlx::query_as(
        "SELECT id, title, scene_prompt, final_outline FROM chapters WHERE novel_id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    for (id, title, scene, outline) in chapters {
        let body = [scene, outline]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        search::index_entity(tx, "chapter", &id, novel_id, &title, &body).await?;
    }

    let characters: Vec<CharacterText> =
        sqlx::query_as(
            "SELECT id, name, personality, biography, current_goal FROM characters WHERE novel_id = ? AND deleted_at IS NULL",
        )
        .bind(novel_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
    for (id, name, personality, biography, goal) in characters {
        let body = [personality, biography, goal]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        search::index_entity(tx, "character", &id, novel_id, &name, &body).await?;
    }
    Ok(())
}

async fn restore_one(
    tx: &mut Transaction<'_, Sqlite>,
    entity_type: RecycleEntity,
    id: &str,
) -> Result<(), AppError> {
    match entity_type {
        RecycleEntity::Novel => {
            let batch = batch_of(tx, "novels", id)
                .await?
                .ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            sqlx::query("UPDATE novels SET deleted_at = NULL, deleted_batch = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            for table in ["volumes", "chapters", "characters", "items"] {
                let sql = format!(
                    "UPDATE {table} SET deleted_at = NULL, deleted_batch = NULL WHERE novel_id = ? AND deleted_batch = ?"
                );
                sqlx::query(&sql)
                    .bind(id)
                    .bind(&batch)
                    .execute(&mut **tx)
                    .await
                    .map_err(db_err)?;
            }
            reindex_novel(tx, id).await?;
        }
        RecycleEntity::Character => {
            let batch = batch_of(tx, "characters", id)
                .await?
                .ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let row: Option<CharacterRestore> = sqlx::query_as(
                "SELECT novel_id, name, personality, biography, current_goal FROM characters WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let (novel_id, name, personality, biography, current_goal) =
                row.ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let novel: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM novels WHERE id = ? AND deleted_at IS NULL")
                    .bind(&novel_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            if novel.is_none() {
                return Err(invalid(vec![(
                    "novel_id",
                    "所属小说已删除，无法单独恢复角色".to_string(),
                )]));
            }
            let new_name = unique_name(tx, "characters", &novel_id, &name, "恢复").await?;
            sqlx::query(
                "UPDATE characters SET deleted_at = NULL, deleted_batch = NULL, name = ? WHERE id = ? AND deleted_batch = ?",
            )
            .bind(&new_name)
            .bind(id)
            .bind(&batch)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
            let body = [personality, biography, current_goal]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            search::index_entity(tx, "character", id, &novel_id, &new_name, &body).await?;
        }
        RecycleEntity::Item => {
            let batch = batch_of(tx, "items", id)
                .await?
                .ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let row: Option<(String, String)> =
                sqlx::query_as("SELECT novel_id, name FROM items WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            let (novel_id, name) =
                row.ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let new_name = unique_name(tx, "items", &novel_id, &name, "恢复").await?;
            sqlx::query(
                "UPDATE items SET deleted_at = NULL, deleted_batch = NULL, name = ? WHERE id = ? AND deleted_batch = ?",
            )
            .bind(&new_name)
            .bind(id)
            .bind(&batch)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        }
        RecycleEntity::Volume => {
            let batch = batch_of(tx, "volumes", id)
                .await?
                .ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let row: Option<(String, String, i64)> =
                sqlx::query_as("SELECT novel_id, title, volume_index FROM volumes WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            let (novel_id, _title, desired) =
                row.ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let novel: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM novels WHERE id = ? AND deleted_at IS NULL")
                    .bind(&novel_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            if novel.is_none() {
                return Err(invalid(vec![(
                    "novel_id",
                    "所属小说已删除，无法单独恢复卷".to_string(),
                )]));
            }
            let index = unique_index(
                tx,
                "volumes",
                "novel_id",
                &novel_id,
                "volume_index",
                desired,
                id,
            )
            .await?;
            sqlx::query(
                "UPDATE volumes SET deleted_at = NULL, deleted_batch = NULL, volume_index = ? WHERE id = ? AND deleted_batch = ?",
            )
            .bind(index)
            .bind(id)
            .bind(&batch)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;

            let chapters: Vec<(String, i64)> = sqlx::query_as(
                "SELECT id, chapter_index FROM chapters WHERE volume_id = ? AND deleted_batch = ?",
            )
            .bind(id)
            .bind(&batch)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
            for (cid, desired_index) in chapters {
                let new_index = unique_index(
                    tx,
                    "chapters",
                    "volume_id",
                    id,
                    "chapter_index",
                    desired_index,
                    &cid,
                )
                .await?;
                sqlx::query(
                    "UPDATE chapters SET deleted_at = NULL, deleted_batch = NULL, chapter_index = ? WHERE id = ?",
                )
                .bind(new_index)
                .bind(&cid)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            }
            reindex_novel(tx, &novel_id).await?;
        }
        RecycleEntity::Chapter => {
            let batch = batch_of(tx, "chapters", id)
                .await?
                .ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let row: Option<(String, String, i64, i64)> = sqlx::query_as(
                "SELECT novel_id, volume_id, chapter_index, 1 FROM chapters WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let (novel_id, volume_id, desired, _) =
                row.ok_or_else(|| not_found(ErrorCode::RecycleItemNotFound, "回收站条目不存在"))?;
            let volume: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM volumes WHERE id = ? AND deleted_at IS NULL")
                    .bind(&volume_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            if volume.is_none() {
                return Err(invalid(vec![(
                    "volume_id",
                    "所属卷已删除，无法单独恢复章节".to_string(),
                )]));
            }
            let index = unique_index(
                tx,
                "chapters",
                "volume_id",
                &volume_id,
                "chapter_index",
                desired,
                id,
            )
            .await?;
            sqlx::query(
                "UPDATE chapters SET deleted_at = NULL, deleted_batch = NULL, chapter_index = ? WHERE id = ? AND deleted_batch = ?",
            )
            .bind(index)
            .bind(id)
            .bind(&batch)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
            reindex_novel(tx, &novel_id).await?;
        }
    }
    Ok(())
}

pub async fn restore(pool: &SqlitePool, req: RestoreReq) -> Result<Vec<String>, AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let mut restored = Vec::new();
    for id in req.ids {
        restore_one(&mut tx, req.entity_type, &id).await?;
        restored.push(id);
    }
    tx.commit().await.map_err(db_err)?;
    Ok(restored)
}

pub async fn purge(
    pool: &SqlitePool,
    entity_type: RecycleEntity,
    id: &str,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    match entity_type {
        RecycleEntity::Novel => {
            search::remove_novel_entities(&mut tx, id).await?;
            let affected =
                sqlx::query("DELETE FROM novels WHERE id = ? AND deleted_at IS NOT NULL")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?
                    .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                return Err(not_found(
                    ErrorCode::RecycleItemNotFound,
                    "回收站条目不存在",
                ));
            }
        }
        RecycleEntity::Volume => {
            search::remove_volume_entities(&mut tx, id).await?;
            let affected =
                sqlx::query("DELETE FROM volumes WHERE id = ? AND deleted_at IS NOT NULL")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?
                    .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                return Err(not_found(
                    ErrorCode::RecycleItemNotFound,
                    "回收站条目不存在",
                ));
            }
        }
        RecycleEntity::Chapter => {
            search::remove_entity(&mut tx, id).await?;
            let affected =
                sqlx::query("DELETE FROM chapters WHERE id = ? AND deleted_at IS NOT NULL")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?
                    .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                return Err(not_found(
                    ErrorCode::RecycleItemNotFound,
                    "回收站条目不存在",
                ));
            }
        }
        RecycleEntity::Character => {
            search::remove_entity(&mut tx, id).await?;
            let affected =
                sqlx::query("DELETE FROM characters WHERE id = ? AND deleted_at IS NOT NULL")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?
                    .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                return Err(not_found(
                    ErrorCode::RecycleItemNotFound,
                    "回收站条目不存在",
                ));
            }
        }
        RecycleEntity::Item => {
            let affected = sqlx::query("DELETE FROM items WHERE id = ? AND deleted_at IS NOT NULL")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?
                .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                return Err(not_found(
                    ErrorCode::RecycleItemNotFound,
                    "回收站条目不存在",
                ));
            }
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
