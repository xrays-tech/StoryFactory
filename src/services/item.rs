// src/services/item.rs — Item CRUD（§6.2；item 不进入 FTS，D-22）
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{CreateItemReq, ItemDto, UpdateItemReq};
use crate::services::util::{db_err, invalid, not_found, novel_alive, push_len, push_required};

const COLS: &str =
    "id, novel_id, name, features, description, owner_id, importance, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct ItemRow {
    id: String,
    novel_id: String,
    name: String,
    features: Option<String>,
    description: Option<String>,
    owner_id: Option<String>,
    importance: i64,
    created_at: i64,
    updated_at: i64,
}

fn dto(row: ItemRow) -> ItemDto {
    ItemDto {
        id: row.id,
        novel_id: row.novel_id,
        name: row.name,
        features: row.features,
        description: row.description,
        owner_id: row.owner_id,
        importance: row.importance,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn owner_valid(pool: &SqlitePool, novel_id: &str, owner_id: &str) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(owner_id)
    .bind(novel_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok(count == 1)
}

fn validate(
    name: Option<&str>,
    require_name: bool,
    features: Option<&str>,
    description: Option<&str>,
    importance: Option<i64>,
) -> Result<(), AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    if require_name || name.is_some() {
        match name {
            Some(v) => push_required(&mut errs, "name", v, 100),
            None => errs.push(("name", "name 不能为空".to_string())),
        }
    }
    push_len(&mut errs, "features", features, 20000);
    push_len(&mut errs, "description", description, 20000);
    if let Some(v) = importance {
        if !(0..=5).contains(&v) {
            errs.push(("importance", "importance 必须在 0..5".to_string()));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(invalid(errs))
    }
}

pub async fn list(
    pool: &SqlitePool,
    novel_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Page<ItemDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let (p, ps, limit, offset) = resolve_page(page, page_size);
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE novel_id = ? AND deleted_at IS NULL")
            .bind(novel_id)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
    let rows = sqlx::query_as::<_, ItemRow>(&format!(
        "SELECT {COLS} FROM items WHERE novel_id = ? AND deleted_at IS NULL ORDER BY created_at, id LIMIT ? OFFSET ?"
    ))
    .bind(novel_id)
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

pub async fn create(
    pool: &SqlitePool,
    novel_id: &str,
    req: CreateItemReq,
) -> Result<ItemDto, AppError> {
    validate(
        Some(&req.name),
        true,
        req.features.as_deref(),
        req.description.as_deref(),
        req.importance,
    )?;
    novel_alive(pool, novel_id).await?;
    if let Some(owner) = req.owner_id.as_deref() {
        if !owner_valid(pool, novel_id, owner).await? {
            return Err(invalid(vec![(
                "owner_id",
                "owner 必须是当前项目未删除的角色".to_string(),
            )]));
        }
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO items(novel_id, name, features, description, owner_id, importance) VALUES(?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(novel_id)
    .bind(&req.name)
    .bind(&req.features)
    .bind(&req.description)
    .bind(&req.owner_id)
    .bind(req.importance.unwrap_or(0))
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let row = sqlx::query_as::<_, ItemRow>(&format!("SELECT {COLS} FROM items WHERE id = ?"))
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(dto(row))
}

pub async fn get(pool: &SqlitePool, novel_id: &str, id: &str) -> Result<ItemDto, AppError> {
    let row = sqlx::query_as::<_, ItemRow>(&format!(
        "SELECT {COLS} FROM items WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(r) => Ok(dto(r)),
        None => Err(not_found(ErrorCode::ItemNotFound, "物品不存在或已删除")),
    }
}

pub async fn update(
    pool: &SqlitePool,
    novel_id: &str,
    id: &str,
    req: UpdateItemReq,
) -> Result<ItemDto, AppError> {
    validate(
        req.name.as_deref(),
        false,
        req.features.as_deref(),
        req.description.as_deref(),
        req.importance,
    )?;
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current = sqlx::query_as::<_, ItemRow>(&format!(
        "SELECT {COLS} FROM items WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let current = match current {
        Some(c) => c,
        None => {
            tx.rollback().await.map_err(db_err)?;
            return Err(not_found(ErrorCode::ItemNotFound, "物品不存在或已删除"));
        }
    };

    let owner = match req.owner_id {
        Some(v) => v,
        None => current.owner_id,
    };
    if let Some(owner_id) = owner.as_deref() {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
        )
        .bind(owner_id)
        .bind(novel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        if count != 1 {
            tx.rollback().await.map_err(db_err)?;
            return Err(invalid(vec![(
                "owner_id",
                "owner 必须是当前项目未删除的角色".to_string(),
            )]));
        }
    }

    let name = req.name.unwrap_or(current.name);
    let features = req.features.or(current.features);
    let description = req.description.or(current.description);
    let importance = req.importance.unwrap_or(current.importance);

    sqlx::query(
        "UPDATE items SET name = ?, features = ?, description = ?, owner_id = ?, importance = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&name)
    .bind(&features)
    .bind(&description)
    .bind(&owner)
    .bind(importance)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;

    let row = sqlx::query_as::<_, ItemRow>(&format!("SELECT {COLS} FROM items WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(dto(row))
}

pub async fn soft_delete(pool: &SqlitePool, novel_id: &str, id: &str) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let batch = uuid::Uuid::new_v4().to_string();
    let affected = sqlx::query(
        "UPDATE items SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(&batch)
    .bind(id)
    .bind(novel_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if affected == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::ItemNotFound, "物品不存在或已删除"));
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
