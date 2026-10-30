// src/services/export.rs — 项目导出/导入（附录 C / D-23 / D-27）
use std::io::{Cursor, Read, Write};

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePool;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::services::search;
use crate::services::util::{db_err, novel_alive};

pub const FORMAT_VERSION: u32 = 2;

#[derive(Serialize, sqlx::FromRow)]
struct NovelRow {
    id: String,
    title: String,
    background: Option<String>,
    world_view: Option<String>,
    summary: Option<String>,
    status: String,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
    deleted_batch: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct VolumeRow {
    id: String,
    novel_id: String,
    volume_index: i64,
    title: String,
    summary: Option<String>,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
    deleted_batch: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ChapterRow {
    id: String,
    novel_id: String,
    volume_id: String,
    chapter_index: i64,
    title: String,
    scene_prompt: Option<String>,
    sandbox_log: Option<String>,
    final_outline: Option<String>,
    status: String,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
    deleted_batch: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct CharacterRow {
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
    deleted_at: Option<i64>,
    deleted_batch: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct RelationRow {
    novel_id: String,
    char_a_id: String,
    char_b_id: String,
    affinity: i64,
    trust: i64,
    conflict_desc: Option<String>,
    updated_at: i64,
}

#[derive(Serialize, sqlx::FromRow)]
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
    deleted_at: Option<i64>,
    deleted_batch: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct SessionRow {
    id: String,
    novel_id: String,
    chapter_id: Option<String>,
    scene_prompt: String,
    actor_ids: String,
    mode: String,
    status: String,
    step_limit: i64,
    current_step: i64,
    last_event_seq: i64,
    summary_until_seq: i64,
    rolling_summary: Option<String>,
    director_config: Option<String>,
    template_versions: Option<String>,
    session_deadline: Option<i64>,
    summary_text: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    pending_directive: Option<String>,
    user_directive: Option<String>,
    created_at: i64,
    updated_at: i64,
    finished_at: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct EventRow {
    seq: i64,
    session_id: String,
    event_type: String,
    payload: String,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub novel_id: String,
    pub imported: Value,
    pub renamed: Vec<String>,
    pub skipped: Vec<String>,
}

pub async fn export_novel(
    pool: &SqlitePool,
    novel_id: &str,
    include_sessions: bool,
    include_deleted: bool,
) -> Result<(String, Vec<u8>), AppError> {
    novel_alive(pool, novel_id).await?;
    let filter = if include_deleted {
        ""
    } else {
        " AND deleted_at IS NULL"
    };
    let novel = sqlx::query_as::<_, NovelRow>("SELECT * FROM novels WHERE id = ?")
        .bind(novel_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let volumes = sqlx::query_as::<_, VolumeRow>(&format!(
        "SELECT * FROM volumes WHERE novel_id = ?{filter} ORDER BY volume_index"
    ))
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let chapters = sqlx::query_as::<_, ChapterRow>(&format!(
        "SELECT * FROM chapters WHERE novel_id = ?{filter} ORDER BY volume_id, chapter_index"
    ))
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let characters = sqlx::query_as::<_, CharacterRow>(&format!(
        "SELECT * FROM characters WHERE novel_id = ?{filter} ORDER BY created_at"
    ))
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let relations = sqlx::query_as::<_, RelationRow>(
        "SELECT * FROM character_relations WHERE novel_id = ? ORDER BY char_a_id, char_b_id",
    )
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let items =
        sqlx::query_as::<_, ItemRow>(&format!("SELECT * FROM items WHERE novel_id = ?{filter}"))
            .bind(novel_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;

    let (sessions, events) = if include_sessions {
        let sessions = sqlx::query_as::<_, SessionRow>(
            "SELECT * FROM sandbox_sessions WHERE novel_id = ? ORDER BY created_at",
        )
        .bind(novel_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let events = sqlx::query_as::<_, EventRow>(
            "SELECT e.seq, e.session_id, e.event_type, e.payload, e.created_at
             FROM sandbox_events e JOIN sandbox_sessions s ON s.id = e.session_id
             WHERE s.novel_id = ? ORDER BY e.session_id, e.seq",
        )
        .bind(novel_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        (sessions, events)
    } else {
        (Vec::new(), Vec::new())
    };

    let counts = json!({
        "volumes": volumes.len(),
        "chapters": chapters.len(),
        "characters": characters.len(),
        "relations": relations.len(),
        "items": items.len(),
        "sessions": sessions.len(),
        "events": events.len(),
        "llm_usage": 0
    });
    let data = json!({
        "novel": novel,
        "volumes": volumes,
        "chapters": chapters,
        "characters": characters,
        "relations": relations,
        "items": items,
        "sessions": sessions,
        "events": events,
    });
    let data_bytes = serde_json::to_vec_pretty(&data)
        .map_err(|e| AppError::internal(format!("导出序列化失败: {e}")))?;
    let checksum = format!("sha256:{}", hex::encode(Sha256::digest(&data_bytes)));
    let schema_version = db::schema_version(pool).await.unwrap_or(0);
    let manifest = json!({
        "format_version": FORMAT_VERSION,
        "schema_version": schema_version,
        "app_version": "4.0.0",
        "exported_at": now_secs(),
        "novel": { "id": novel_id, "title": data["novel"]["title"], "summary": data["novel"]["summary"] },
        "counts": counts,
        "checksums": { "data.json": checksum },
        "options": { "include_sessions": include_sessions, "include_deleted": include_deleted }
    });

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", options)
            .map_err(|e| AppError::internal(format!("zip manifest: {e}")))?;
        zip.write_all(
            serde_json::to_string_pretty(&manifest)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|e| AppError::internal(format!("zip write: {e}")))?;
        zip.start_file("data.json", options)
            .map_err(|e| AppError::internal(format!("zip data: {e}")))?;
        zip.write_all(&data_bytes)
            .map_err(|e| AppError::internal(format!("zip write: {e}")))?;
        zip.finish()
            .map_err(|e| AppError::internal(format!("zip finish: {e}")))?;
    }
    let title = sanitize_filename(
        data["novel"]["title"]
            .as_str()
            .unwrap_or("novel")
            .to_string()
            .as_str(),
    );
    Ok((
        format!("{title}-{}.saexport", now_secs()),
        cursor.into_inner(),
    ))
}

fn sanitize_filename(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric()
                || c == '_'
                || c == '-'
                || ('\u{4e00}'..='\u{9fff}').contains(&c)
            {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out = "novel".to_string();
    }
    out.chars().take(60).collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

fn read_zip_entry(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    name: &str,
) -> Result<Vec<u8>, AppError> {
    let mut file = archive.by_name(name).map_err(|_| {
        AppError::validation(
            "导入包缺少文件",
            json!([{"field": name, "message": "missing"}]),
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| AppError::internal(format!("zip read: {e}")))?;
    Ok(bytes)
}

fn remap_value(value: &mut Value, map: &std::collections::HashMap<String, String>) {
    match value {
        Value::String(s) => {
            if let Some(new_id) = map.get(s) {
                *s = new_id.clone();
            }
        }
        Value::Array(items) => {
            for item in items {
                remap_value(item, map);
            }
        }
        Value::Object(obj) => {
            for item in obj.values_mut() {
                remap_value(item, map);
            }
        }
        _ => {}
    }
}

async fn import_name(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    novel_id: &str,
    name: &str,
) -> Result<String, AppError> {
    let sql = format!(
        "SELECT 1 FROM {table} WHERE novel_id = ? AND lower(name) = lower(?) AND deleted_at IS NULL"
    );
    let free: Option<i64> = sqlx::query_scalar(&sql)
        .bind(novel_id)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    if free.is_none() {
        return Ok(name.to_string());
    }
    for n in 1..=100 {
        let candidate = if n == 1 {
            format!("{name}（导入）")
        } else {
            format!("{name}（导入{n}）")
        };
        let taken: Option<i64> = sqlx::query_scalar(&sql)
            .bind(novel_id)
            .bind(&candidate)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        if taken.is_none() {
            return Ok(candidate);
        }
    }
    Err(AppError::validation(
        "导入重命名失败",
        json!([{"field":"name","message":"重命名次数超限"}]),
    ))
}

pub async fn import_package(pool: &SqlitePool, bytes: &[u8]) -> Result<ImportReport, AppError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|e| {
        AppError::validation(
            "导入包不是合法 zip",
            json!([{"field": "zip", "message": e.to_string()}]),
        )
    })?;
    let manifest_bytes = read_zip_entry(&mut archive, "manifest.json")?;
    let data_bytes = read_zip_entry(&mut archive, "data.json")?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        AppError::validation(
            "manifest.json 解析失败",
            json!([{"field": "manifest", "message": e.to_string()}]),
        )
    })?;
    let format_version = manifest["format_version"].as_u64().unwrap_or(0);
    if format_version > FORMAT_VERSION as u64 {
        return Err(AppError::new(
            ErrorCode::UnsupportedImportVersion,
            "导入包格式版本高于当前应用",
        ));
    }
    let expected = manifest["checksums"]["data.json"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches("sha256:");
    let actual = hex::encode(Sha256::digest(&data_bytes));
    if expected.is_empty() || expected != actual {
        return Err(AppError::validation(
            "data.json 校验失败",
            json!([{"field": "checksums", "message": "sha256 mismatch"}]),
        ));
    }
    let data: Value = serde_json::from_slice(&data_bytes).map_err(|e| {
        AppError::validation(
            "data.json 解析失败",
            json!([{"field": "data", "message": e.to_string()}]),
        )
    })?;

    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let source_novel = data["novel"]["id"].as_str().unwrap_or("");
    let novel_id = new_id();
    map.insert(source_novel.to_string(), novel_id.clone());
    for array in ["volumes", "chapters", "characters", "items", "sessions"] {
        if let Some(rows) = data[array].as_array() {
            for row in rows {
                if let Some(old) = row["id"].as_str() {
                    map.insert(old.to_string(), new_id());
                }
            }
        }
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let novel = &data["novel"];
    sqlx::query(
        "INSERT INTO novels(id, title, background, world_view, summary, status) VALUES(?, ?, ?, ?, ?, ?)",
    )
    .bind(&novel_id)
    .bind(novel["title"].as_str().unwrap_or("导入项目"))
    .bind(novel["background"].as_str())
    .bind(novel["world_view"].as_str())
    .bind(novel["summary"].as_str())
    .bind(novel["status"].as_str().unwrap_or("active"))
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;

    search::index_entity(
        &mut tx,
        "novel",
        &novel_id,
        &novel_id,
        novel["title"].as_str().unwrap_or("导入项目"),
        novel["summary"].as_str().unwrap_or(""),
    )
    .await?;
    let mut imported = json!({"volumes":0,"chapters":0,"characters":0,"relations":0,"items":0,"sessions":0,"events":0});

    if let Some(rows) = data["volumes"].as_array() {
        for (idx, row) in rows.iter().enumerate() {
            let old = row["id"].as_str().unwrap_or("");
            let id = map.get(old).cloned().unwrap_or_else(new_id);
            sqlx::query("INSERT INTO volumes(id, novel_id, volume_index, title, summary) VALUES(?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&novel_id)
                .bind((idx + 1) as i64)
                .bind(row["title"].as_str().unwrap_or(""))
                .bind(row["summary"].as_str())
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            search::index_entity(
                &mut tx,
                "volume",
                &id,
                &novel_id,
                row["title"].as_str().unwrap_or(""),
                row["summary"].as_str().unwrap_or(""),
            )
            .await?;
            imported["volumes"] = json!(imported["volumes"].as_i64().unwrap_or(0) + 1);
        }
    }

    if let Some(rows) = data["chapters"].as_array() {
        let mut per_volume: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for row in rows {
            let old = row["id"].as_str().unwrap_or("");
            let id = map.get(old).cloned().unwrap_or_else(new_id);
            let old_volume = row["volume_id"].as_str().unwrap_or("");
            let volume_id = map.get(old_volume).cloned().unwrap_or_default();
            let idx = per_volume.entry(volume_id.clone()).or_insert(0);
            *idx += 1;
            sqlx::query("INSERT INTO chapters(id, novel_id, volume_id, chapter_index, title, scene_prompt, final_outline, status) VALUES(?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&novel_id)
                .bind(&volume_id)
                .bind(*idx)
                .bind(row["title"].as_str().unwrap_or(""))
                .bind(row["scene_prompt"].as_str())
                .bind(row["final_outline"].as_str())
                .bind(row["status"].as_str().unwrap_or("draft"))
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            let body = [row["scene_prompt"].as_str(), row["final_outline"].as_str()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            search::index_entity(
                &mut tx,
                "chapter",
                &id,
                &novel_id,
                row["title"].as_str().unwrap_or(""),
                &body,
            )
            .await?;
            imported["chapters"] = json!(imported["chapters"].as_i64().unwrap_or(0) + 1);
        }
    }

    if let Some(rows) = data["characters"].as_array() {
        for row in rows {
            let old = row["id"].as_str().unwrap_or("");
            let id = map.get(old).cloned().unwrap_or_else(new_id);
            let base_name = row["name"].as_str().unwrap_or("角色");
            let name = import_name(&mut tx, "characters", &novel_id, base_name).await?;
            sqlx::query("INSERT INTO characters(id, novel_id, name, role_type, appearance, personality, hidden_personality, biography, current_goal, speaking_style) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&novel_id)
                .bind(&name)
                .bind(row["role_type"].as_str().unwrap_or("support"))
                .bind(row["appearance"].as_str())
                .bind(row["personality"].as_str())
                .bind(row["hidden_personality"].as_str())
                .bind(row["biography"].as_str())
                .bind(row["current_goal"].as_str())
                .bind(row["speaking_style"].as_str())
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            let body = [
                row["personality"].as_str(),
                row["biography"].as_str(),
                row["current_goal"].as_str(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
            search::index_entity(&mut tx, "character", &id, &novel_id, &name, &body).await?;
            imported["characters"] = json!(imported["characters"].as_i64().unwrap_or(0) + 1);
        }
    }

    if let Some(rows) = data["relations"].as_array() {
        for row in rows {
            let a = map.get(row["char_a_id"].as_str().unwrap_or("")).cloned();
            let b = map.get(row["char_b_id"].as_str().unwrap_or("")).cloned();
            if let (Some(a), Some(b)) = (a, b) {
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                sqlx::query("INSERT OR REPLACE INTO character_relations(novel_id, char_a_id, char_b_id, affinity, trust, conflict_desc) VALUES(?, ?, ?, ?, ?, ?)")
                    .bind(&novel_id)
                    .bind(&a)
                    .bind(&b)
                    .bind(row["affinity"].as_i64().unwrap_or(0))
                    .bind(row["trust"].as_i64().unwrap_or(0))
                    .bind(row["conflict_desc"].as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
                imported["relations"] = json!(imported["relations"].as_i64().unwrap_or(0) + 1);
            }
        }
    }

    if let Some(rows) = data["items"].as_array() {
        for row in rows {
            let old = row["id"].as_str().unwrap_or("");
            let id = map.get(old).cloned().unwrap_or_else(new_id);
            let owner = row["owner_id"].as_str().and_then(|v| map.get(v).cloned());
            let item_name = import_name(
                &mut tx,
                "items",
                &novel_id,
                row["name"].as_str().unwrap_or("物品"),
            )
            .await?;
            sqlx::query("INSERT INTO items(id, novel_id, name, features, description, owner_id, importance) VALUES(?, ?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&novel_id)
                .bind(&item_name)
                .bind(row["features"].as_str())
                .bind(row["description"].as_str())
                .bind(&owner)
                .bind(row["importance"].as_i64().unwrap_or(0))
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            imported["items"] = json!(imported["items"].as_i64().unwrap_or(0) + 1);
        }
    }

    if let Some(rows) = data["sessions"].as_array() {
        for row in rows {
            let old = row["id"].as_str().unwrap_or("");
            let id = map.get(old).cloned().unwrap_or_else(new_id);
            let chapter_id = row["chapter_id"].as_str().and_then(|v| map.get(v).cloned());
            let mut actor_ids: Value =
                serde_json::from_str(row["actor_ids"].as_str().unwrap_or("[]"))
                    .unwrap_or(json!([]));
            remap_value(&mut actor_ids, &map);
            let mut director_config = row["director_config"]
                .as_str()
                .and_then(|v| serde_json::from_str::<Value>(v).ok());
            if let Some(value) = director_config.as_mut() {
                remap_value(value, &map);
            }
            let status = match row["status"].as_str().unwrap_or("interrupted") {
                "running" | "paused" | "waiting_input" | "finalizing" | "created" => "interrupted",
                other => other,
            };
            sqlx::query("INSERT INTO sandbox_sessions(id, novel_id, chapter_id, scene_prompt, actor_ids, mode, status, step_limit, current_step, last_event_seq, summary_until_seq, rolling_summary, director_config, template_versions, session_deadline, summary_text, error_code, error_message, pending_directive, user_directive, created_at, updated_at, finished_at) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&novel_id)
                .bind(&chapter_id)
                .bind(row["scene_prompt"].as_str().unwrap_or(""))
                .bind(actor_ids.to_string())
                .bind(row["mode"].as_str().unwrap_or("roundrobin"))
                .bind(status)
                .bind(row["step_limit"].as_i64().unwrap_or(20))
                .bind(row["current_step"].as_i64().unwrap_or(0))
                .bind(row["last_event_seq"].as_i64().unwrap_or(0))
                .bind(row["summary_until_seq"].as_i64().unwrap_or(0))
                .bind(row["rolling_summary"].as_str())
                .bind(director_config.map(|v| v.to_string()))
                .bind(row["template_versions"].as_str())
                .bind(row["session_deadline"].as_i64())
                .bind(row["summary_text"].as_str())
                .bind(row["error_code"].as_str())
                .bind(row["error_message"].as_str())
                .bind(row["pending_directive"].as_str())
                .bind(row["user_directive"].as_str())
                .bind(row["created_at"].as_i64().unwrap_or_else(now_secs))
                .bind(row["updated_at"].as_i64().unwrap_or_else(now_secs))
                .bind(row["finished_at"].as_i64())
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            imported["sessions"] = json!(imported["sessions"].as_i64().unwrap_or(0) + 1);
        }
    }

    if let Some(rows) = data["events"].as_array() {
        for row in rows {
            let old_session = row["session_id"].as_str().unwrap_or("");
            let Some(session_id) = map.get(old_session).cloned() else {
                continue;
            };
            let mut payload: Value =
                serde_json::from_str(row["payload"].as_str().unwrap_or("{}")).unwrap_or(json!({}));
            remap_value(&mut payload, &map);
            sqlx::query("INSERT INTO sandbox_events(seq, session_id, event_type, payload, created_at) VALUES(?, ?, ?, ?, ?)")
                .bind(row["seq"].as_i64().unwrap_or(0))
                .bind(&session_id)
                .bind(row["event_type"].as_str().unwrap_or("unknown"))
                .bind(payload.to_string())
                .bind(row["created_at"].as_i64().unwrap_or_else(now_secs))
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            imported["events"] = json!(imported["events"].as_i64().unwrap_or(0) + 1);
        }
    }

    tx.commit().await.map_err(db_err)?;
    Ok(ImportReport {
        novel_id,
        imported,
        renamed: Vec::new(),
        skipped: Vec::new(),
    })
}
