// src/services/character.rs — Character CRUD + Relations（§6.2 / D-10）
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::models::common::{resolve_page, Page};
use crate::models::dto::{
    CharacterDto, CreateCharacterReq, PutRelationsReq, RelationDto, RelationInput,
    UpdateCharacterReq,
};
use crate::models::enums::RoleType;
use crate::services::search;
use crate::services::util::{db_err, invalid, not_found, novel_alive, push_len, push_required};

const COLS: &str = "id, novel_id, name, role_type, appearance, personality, hidden_personality, biography, current_goal, speaking_style, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct CharRow {
    id: String,
    novel_id: String,
    name: String,
    role_type: String,
    appearance: Option<String>,
    personality: Option<String>,
    hidden_personality: Option<String>,
    biography: Option<String>,
    current_goal: Option<String>,
    speaking_style: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn dto(row: CharRow) -> CharacterDto {
    CharacterDto {
        id: row.id,
        novel_id: row.novel_id,
        name: row.name,
        role_type: RoleType::from_db_or_default(row.role_type),
        appearance: row.appearance,
        personality: row.personality,
        hidden_personality: row.hidden_personality,
        biography: row.biography,
        current_goal: row.current_goal,
        speaking_style: row.speaking_style,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn fts_body(d: &CharacterDto) -> String {
    [
        d.personality.as_deref(),
        d.biography.as_deref(),
        d.current_goal.as_deref(),
    ]
    .iter()
    .flatten()
    .copied()
    .collect::<Vec<_>>()
    .join("\n")
}

#[allow(clippy::too_many_arguments)]
fn validate_fields(
    name: Option<&str>,
    require_name: bool,
    appearance: Option<&str>,
    personality: Option<&str>,
    hidden_personality: Option<&str>,
    biography: Option<&str>,
    current_goal: Option<&str>,
    speaking_style: Option<&str>,
) -> Result<(), AppError> {
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    if require_name || name.is_some() {
        match name {
            Some(v) => push_required(&mut errs, "name", v, 50),
            None => errs.push(("name", "name 不能为空".to_string())),
        }
    }
    push_len(&mut errs, "appearance", appearance, 20000);
    push_len(&mut errs, "personality", personality, 20000);
    push_len(&mut errs, "hidden_personality", hidden_personality, 20000);
    push_len(&mut errs, "biography", biography, 20000);
    push_len(&mut errs, "current_goal", current_goal, 20000);
    push_len(&mut errs, "speaking_style", speaking_style, 20000);
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
) -> Result<Page<CharacterDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let (p, ps, limit, offset) = resolve_page(page, page_size);
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM characters WHERE novel_id = ? AND deleted_at IS NULL",
    )
    .bind(novel_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let rows = sqlx::query_as::<_, CharRow>(&format!(
        "SELECT {COLS} FROM characters WHERE novel_id = ? AND deleted_at IS NULL ORDER BY created_at, id LIMIT ? OFFSET ?"
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
    req: CreateCharacterReq,
) -> Result<CharacterDto, AppError> {
    validate_fields(
        Some(&req.name),
        true,
        req.appearance.as_deref(),
        req.personality.as_deref(),
        req.hidden_personality.as_deref(),
        req.biography.as_deref(),
        req.current_goal.as_deref(),
        req.speaking_style.as_deref(),
    )?;
    novel_alive(pool, novel_id).await?;

    let role = req
        .role_type
        .map(RoleType::to_db)
        .unwrap_or_else(|| "support".to_string());
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO characters(novel_id, name, role_type, appearance, personality, hidden_personality, biography, current_goal, speaking_style) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(novel_id)
    .bind(&req.name)
    .bind(&role)
    .bind(&req.appearance)
    .bind(&req.personality)
    .bind(&req.hidden_personality)
    .bind(&req.biography)
    .bind(&req.current_goal)
    .bind(&req.speaking_style)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;

    let row = sqlx::query_as::<_, CharRow>(&format!("SELECT {COLS} FROM characters WHERE id = ?"))
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let d = dto(row);
    search::index_entity(
        &mut tx,
        "character",
        &d.id,
        novel_id,
        &d.name,
        &fts_body(&d),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn get(pool: &SqlitePool, novel_id: &str, cid: &str) -> Result<CharacterDto, AppError> {
    let row = sqlx::query_as::<_, CharRow>(&format!(
        "SELECT {COLS} FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
    ))
    .bind(cid)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(r) => Ok(dto(r)),
        None => Err(not_found(
            ErrorCode::CharacterNotFound,
            "角色不存在或已删除",
        )),
    }
}

pub async fn update(
    pool: &SqlitePool,
    novel_id: &str,
    cid: &str,
    req: UpdateCharacterReq,
) -> Result<CharacterDto, AppError> {
    validate_fields(
        req.name.as_deref(),
        false,
        req.appearance.as_deref(),
        req.personality.as_deref(),
        req.hidden_personality.as_deref(),
        req.biography.as_deref(),
        req.current_goal.as_deref(),
        req.speaking_style.as_deref(),
    )?;
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let current = sqlx::query_as::<_, CharRow>(&format!(
        "SELECT {COLS} FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL"
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
            return Err(not_found(
                ErrorCode::CharacterNotFound,
                "角色不存在或已删除",
            ));
        }
    };

    let name = req.name.unwrap_or(current.name);
    let role_type = req
        .role_type
        .map(RoleType::to_db)
        .unwrap_or(current.role_type);
    let appearance = req.appearance.or(current.appearance);
    let personality = req.personality.or(current.personality);
    let hidden_personality = req.hidden_personality.or(current.hidden_personality);
    let biography = req.biography.or(current.biography);
    let current_goal = req.current_goal.or(current.current_goal);
    let speaking_style = req.speaking_style.or(current.speaking_style);

    sqlx::query(
        "UPDATE characters SET name = ?, role_type = ?, appearance = ?, personality = ?, hidden_personality = ?, biography = ?, current_goal = ?, speaking_style = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&name)
    .bind(&role_type)
    .bind(&appearance)
    .bind(&personality)
    .bind(&hidden_personality)
    .bind(&biography)
    .bind(&current_goal)
    .bind(&speaking_style)
    .bind(cid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;

    let row = sqlx::query_as::<_, CharRow>(&format!("SELECT {COLS} FROM characters WHERE id = ?"))
        .bind(cid)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let d = dto(row);
    search::index_entity(
        &mut tx,
        "character",
        &d.id,
        novel_id,
        &d.name,
        &fts_body(&d),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(d)
}

pub async fn soft_delete(pool: &SqlitePool, novel_id: &str, cid: &str) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(cid)
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    if exists.is_none() {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(
            ErrorCode::CharacterNotFound,
            "角色不存在或已删除",
        ));
    }
    let batch = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "UPDATE characters SET deleted_at = strftime('%s','now'), deleted_batch = ? WHERE id = ?",
    )
    .bind(&batch)
    .bind(cid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    search::remove_entity(&mut tx, cid).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct RelationRow {
    char_a_id: String,
    char_b_id: String,
    affinity: i64,
    trust: i64,
    conflict_desc: Option<String>,
}

fn relation_dto(row: RelationRow) -> RelationDto {
    RelationDto {
        char_a_id: row.char_a_id,
        char_b_id: row.char_b_id,
        affinity: row.affinity,
        trust: row.trust,
        conflict_desc: row.conflict_desc,
    }
}

pub async fn get_relations(
    pool: &SqlitePool,
    novel_id: &str,
) -> Result<Vec<RelationDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let rows = sqlx::query_as::<_, RelationRow>(
        "SELECT r.char_a_id, r.char_b_id, r.affinity, r.trust, r.conflict_desc
         FROM character_relations r
         JOIN characters a ON a.id = r.char_a_id AND a.deleted_at IS NULL
         JOIN characters b ON b.id = r.char_b_id AND b.deleted_at IS NULL
         WHERE r.novel_id = ?
         ORDER BY r.char_a_id, r.char_b_id",
    )
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(relation_dto).collect())
}

pub async fn put_relations(
    pool: &SqlitePool,
    novel_id: &str,
    req: PutRelationsReq,
) -> Result<Vec<RelationDto>, AppError> {
    novel_alive(pool, novel_id).await?;
    let mut errs: Vec<(&'static str, String)> = Vec::new();
    let mut normalized: Vec<RelationInput> = Vec::new();

    for rel in req.relations {
        if rel.char_a_id == rel.char_b_id {
            errs.push(("relations", "不允许角色与自身建立羁绊".to_string()));
            continue;
        }
        let (a, b) = if rel.char_a_id < rel.char_b_id {
            (rel.char_a_id, rel.char_b_id)
        } else {
            (rel.char_b_id, rel.char_a_id)
        };
        let affinity = rel.affinity.unwrap_or(0);
        let trust = rel.trust.unwrap_or(0);
        if !(-100..=100).contains(&affinity) {
            errs.push(("affinity", "affinity 必须在 -100..100".to_string()));
        }
        if !(0..=100).contains(&trust) {
            errs.push(("trust", "trust 必须在 0..100".to_string()));
        }
        let item = RelationInput {
            char_a_id: a.clone(),
            char_b_id: b.clone(),
            affinity: Some(affinity),
            trust: Some(trust),
            conflict_desc: rel.conflict_desc,
        };
        if let Some(idx) = normalized
            .iter()
            .position(|x| x.char_a_id == a && x.char_b_id == b)
        {
            normalized[idx] = item;
        } else {
            normalized.push(item);
        }
    }
    if !errs.is_empty() {
        return Err(invalid(errs));
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    // 全量替换会抹掉沙盒在运行期累积的 affinity/trust；存在非终态会话时在写锁内拒绝，
    // 避免检查与删除之间的竞态。
    let active_session: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sandbox_sessions WHERE novel_id = ? AND status NOT IN ('completed','failed','aborted') LIMIT 1",
    )
    .bind(novel_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    if active_session.is_some() {
        tx.rollback().await.map_err(db_err)?;
        return Err(AppError::new(
            ErrorCode::SessionConflict,
            "存在活跃沙盒会话，禁止全量替换角色关系（会覆盖沙盒累积的 affinity/trust）",
        ));
    }
    for rel in &normalized {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM characters WHERE novel_id = ? AND deleted_at IS NULL AND id IN (?, ?)",
        )
        .bind(novel_id)
        .bind(&rel.char_a_id)
        .bind(&rel.char_b_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        if count != 2 {
            tx.rollback().await.map_err(db_err)?;
            return Err(invalid(vec![(
                "relations",
                "关系中的角色必须属于当前项目且未删除".to_string(),
            )]));
        }
    }

    sqlx::query("DELETE FROM character_relations WHERE novel_id = ?")
        .bind(novel_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for rel in &normalized {
        sqlx::query(
            "INSERT INTO character_relations(novel_id, char_a_id, char_b_id, affinity, trust, conflict_desc) VALUES(?, ?, ?, ?, ?, ?)",
        )
        .bind(novel_id)
        .bind(&rel.char_a_id)
        .bind(&rel.char_b_id)
        .bind(rel.affinity.unwrap_or(0))
        .bind(rel.trust.unwrap_or(0))
        .bind(&rel.conflict_desc)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    get_relations(pool, novel_id).await
}
