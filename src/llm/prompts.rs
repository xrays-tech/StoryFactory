// src/llm/prompts.rs — Prompt 模板渲染/加载/seed（§7.7 / D-19）
use minijinja::{Environment, UndefinedBehavior};
use sqlx::sqlite::SqlitePool;

use crate::error::{AppError, ErrorCode};
use crate::services::util::db_err;

pub const ACTOR_STEP: &str = include_str!("../../assets/prompts/actor_step.txt");
pub const ACTOR_STEP_USER: &str = include_str!("../../assets/prompts/actor_step_user.txt");
pub const DIRECTOR_DECIDE: &str = include_str!("../../assets/prompts/director_decide.txt");
pub const ROLLING_SUMMARY: &str = include_str!("../../assets/prompts/rolling_summary.txt");
pub const OUTLINE_FINALIZE: &str = include_str!("../../assets/prompts/outline_finalize.txt");
pub const NOVEL_ANALYZE_CHUNK: &str = include_str!("../../assets/prompts/novel_analyze_chunk.txt");
pub const NOVEL_ANALYZE_STRUCTURE: &str =
    include_str!("../../assets/prompts/novel_analyze_structure.txt");
pub const CHAPTER_PROSE_SYSTEM: &str =
    include_str!("../../assets/prompts/chapter_prose_system.txt");
pub const CHAPTER_PROSE_SCENE: &str = include_str!("../../assets/prompts/chapter_prose_scene.txt");

pub const TEMPLATES: [(&str, &str); 9] = [
    ("actor_step", ACTOR_STEP),
    ("actor_step_user", ACTOR_STEP_USER),
    ("director_decide", DIRECTOR_DECIDE),
    ("rolling_summary", ROLLING_SUMMARY),
    ("outline_finalize", OUTLINE_FINALIZE),
    ("novel_analyze_chunk", NOVEL_ANALYZE_CHUNK),
    ("novel_analyze_structure", NOVEL_ANALYZE_STRUCTURE),
    ("chapter_prose_system", CHAPTER_PROSE_SYSTEM),
    ("chapter_prose_scene", CHAPTER_PROSE_SCENE),
];

pub fn render(body: &str, ctx: &serde_json::Value) -> Result<String, AppError> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let template = env
        .template_from_str(body)
        .map_err(|e| AppError::internal(format!("模板语法错误: {e}")))?;
    template
        .render(ctx)
        .map_err(|e| AppError::internal(format!("模板渲染失败: {e}")))
}

pub async fn load_active(pool: &SqlitePool, name: &str) -> Result<(i64, String), AppError> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT version, body FROM prompt_templates WHERE name = ? AND is_active = 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    row.ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            format!("缺少激活的 Prompt 模板: {name}"),
        )
    })
}

pub async fn seed_defaults(pool: &SqlitePool) -> Result<(), AppError> {
    for (name, body) in TEMPLATES {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM prompt_templates WHERE name = ? AND is_active = 1")
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(db_err)?;
        if exists.is_none() {
            sqlx::query(
                "INSERT INTO prompt_templates(name, version, body, is_active) VALUES(?, 1, ?, 1)",
            )
            .bind(name)
            .bind(body)
            .execute(pool)
            .await
            .map_err(db_err)?;
        }
    }
    Ok(())
}

/// 会话启动时快照模板版本（JSON: {name: version}）。
pub async fn snapshot_versions(pool: &SqlitePool) -> Result<serde_json::Value, AppError> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT name, version FROM prompt_templates WHERE is_active = 1")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let mut map = serde_json::Map::new();
    for (name, version) in rows {
        map.insert(name, serde_json::json!(version));
    }
    Ok(serde_json::Value::Object(map))
}
