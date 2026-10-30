// src/services/search.rs — FTS5 同步与检索（D-05 / D-22）
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::SearchItemDto;
use crate::services::util::db_err;

fn fallback_rowid(entity_id: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    entity_id.hash(&mut hasher);
    (hasher.finish() & 0x7fff_ffff_ffff_ffff) as i64
}

fn base_rowid(entity_id: &str) -> i64 {
    let prefix: String = entity_id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(16)
        .collect();
    u64::from_str_radix(&prefix, 16)
        .map(|v| v as i64)
        .unwrap_or_else(|_| fallback_rowid(entity_id))
}

async fn allocate_rowid(
    tx: &mut Transaction<'_, Sqlite>,
    entity_id: &str,
) -> Result<i64, AppError> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT rowid FROM search_docs WHERE entity_id = ?")
            .bind(entity_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    if let Some(rowid) = existing {
        return Ok(rowid);
    }

    let mut candidate = base_rowid(entity_id);
    for _ in 0..8 {
        let occupied: Option<String> =
            sqlx::query_scalar("SELECT entity_id FROM search_docs WHERE rowid = ?")
                .bind(candidate)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        if occupied.is_none() {
            return Ok(candidate);
        }
        candidate = fallback_rowid(&format!("{entity_id}:{candidate}"));
    }
    Err(AppError::new(
        ErrorCode::Internal,
        "FTS rowid 分配失败（连续冲突）",
    ))
}

/// 同事务写入/更新索引。空 title/body 也会被索引，便于后续检索。
pub async fn index_entity(
    tx: &mut Transaction<'_, Sqlite>,
    entity_type: &str,
    entity_id: &str,
    novel_id: &str,
    title: &str,
    body: &str,
) -> Result<(), AppError> {
    let rowid = allocate_rowid(tx, entity_id).await?;
    sqlx::query(
        "INSERT INTO search_docs(entity_id, rowid) VALUES(?, ?) ON CONFLICT(entity_id) DO UPDATE SET rowid = excluded.rowid",
    )
    .bind(entity_id)
    .bind(rowid)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "INSERT OR REPLACE INTO search_index(rowid, title, body, entity_type, entity_id, novel_id) VALUES(?, ?, ?, ?, ?, ?)",
    )
    .bind(rowid)
    .bind(title)
    .bind(body)
    .bind(entity_type)
    .bind(entity_id)
    .bind(novel_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn remove_entity(
    tx: &mut Transaction<'_, Sqlite>,
    entity_id: &str,
) -> Result<(), AppError> {
    let rowid: Option<i64> =
        sqlx::query_scalar("SELECT rowid FROM search_docs WHERE entity_id = ?")
            .bind(entity_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    if let Some(rowid) = rowid {
        sqlx::query("DELETE FROM search_index WHERE rowid = ?")
            .bind(rowid)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM search_docs WHERE entity_id = ?")
            .bind(entity_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

pub async fn remove_novel_entities(
    tx: &mut Transaction<'_, Sqlite>,
    novel_id: &str,
) -> Result<(), AppError> {
    let rowids: Vec<i64> = sqlx::query_scalar("SELECT rowid FROM search_index WHERE novel_id = ?")
        .bind(novel_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
    for rowid in rowids {
        sqlx::query("DELETE FROM search_index WHERE rowid = ?")
            .bind(rowid)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM search_docs WHERE rowid = ?")
            .bind(rowid)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

pub async fn remove_volume_entities(
    tx: &mut Transaction<'_, Sqlite>,
    volume_id: &str,
) -> Result<(), AppError> {
    let chapter_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM chapters WHERE volume_id = ?")
            .bind(volume_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
    for id in chapter_ids {
        remove_entity(tx, &id).await?;
    }
    remove_entity(tx, volume_id).await
}

fn escape_like(raw: &str) -> String {
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

fn snippet_of(title: &str, body: &str, q: &str) -> String {
    let lower_q = q.to_lowercase();
    if title.to_lowercase().contains(&lower_q) {
        return title.to_string();
    }
    let lower = body.to_lowercase();
    if let Some(pos) = lower.find(&lower_q) {
        let start = pos.saturating_sub(30);
        let end = (pos + q.len() + 60).min(body.len());
        // 对齐 UTF-8 边界
        let mut s = start;
        while s > 0 && !body.is_char_boundary(s) {
            s -= 1;
        }
        let mut e = end;
        while e < body.len() && !body.is_char_boundary(e) {
            e += 1;
        }
        return format!("…{}…", &body[s..e]);
    }
    body.chars().take(120).collect()
}

#[derive(sqlx::FromRow)]
struct HitRow {
    entity_type: String,
    entity_id: String,
    title: String,
    body: String,
}

pub async fn search(
    pool: &SqlitePool,
    q: &str,
    entity_type: Option<&str>,
    novel_id: Option<&str>,
    page: i64,
    page_size: i64,
    min_len: usize,
) -> Result<Page<SearchItemDto>, AppError> {
    let query = q.trim();
    if query.chars().count() < min_len {
        return Err(AppError::validation(
            "搜索词过短",
            crate::models::common::validation_details(vec![(
                "q",
                format!("至少 {min_len} 个字符"),
            )]),
        ));
    }
    let (p, ps, limit, offset) = resolve_page(page, page_size);

    let mut filters = String::new();
    if entity_type.is_some() {
        filters.push_str(" AND entity_type = ?");
    }
    if novel_id.is_some() {
        filters.push_str(" AND novel_id = ?");
    }

    let use_match = query.chars().count() >= 3;
    let like_pattern = format!("%{}%", escape_like(query));
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));

    let total: i64 = if use_match {
        let sql = format!("SELECT COUNT(*) FROM search_index WHERE search_index MATCH ?{filters}");
        let mut stmt = sqlx::query_scalar::<_, i64>(&sql).bind(&phrase);
        if let Some(v) = entity_type {
            stmt = stmt.bind(v);
        }
        if let Some(v) = novel_id {
            stmt = stmt.bind(v);
        }
        stmt.fetch_one(pool).await.map_err(db_err)?
    } else {
        let sql = format!(
            "SELECT COUNT(*) FROM search_index WHERE (title LIKE ? ESCAPE '\\' OR body LIKE ? ESCAPE '\\'){filters}"
        );
        let mut stmt = sqlx::query_scalar::<_, i64>(&sql)
            .bind(&like_pattern)
            .bind(&like_pattern);
        if let Some(v) = entity_type {
            stmt = stmt.bind(v);
        }
        if let Some(v) = novel_id {
            stmt = stmt.bind(v);
        }
        stmt.fetch_one(pool).await.map_err(db_err)?
    };

    let rows: Vec<HitRow> = if use_match {
        let sql = format!(
            "SELECT entity_type, entity_id, title, body FROM search_index WHERE search_index MATCH ?{filters} ORDER BY bm25(search_index) LIMIT ? OFFSET ?"
        );
        let mut stmt = sqlx::query_as::<_, HitRow>(&sql).bind(&phrase);
        if let Some(v) = entity_type {
            stmt = stmt.bind(v);
        }
        if let Some(v) = novel_id {
            stmt = stmt.bind(v);
        }
        stmt.bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(db_err)?
    } else {
        let sql = format!(
            "SELECT entity_type, entity_id, title, body FROM search_index WHERE (title LIKE ? ESCAPE '\\' OR body LIKE ? ESCAPE '\\'){filters} ORDER BY title LIMIT ? OFFSET ?"
        );
        let mut stmt = sqlx::query_as::<_, HitRow>(&sql)
            .bind(&like_pattern)
            .bind(&like_pattern);
        if let Some(v) = entity_type {
            stmt = stmt.bind(v);
        }
        if let Some(v) = novel_id {
            stmt = stmt.bind(v);
        }
        stmt.bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(db_err)?
    };

    let items = rows
        .into_iter()
        .map(|row| SearchItemDto {
            snippet: snippet_of(&row.title, &row.body, query),
            entity_type: row.entity_type,
            entity_id: row.entity_id,
            title: row.title,
        })
        .collect();
    Ok(Page {
        items,
        total,
        page: p,
        page_size: ps,
    })
}
