// tests/novel_import.rs — txt 小说逆向分析集成/单元测试
mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::StructuredMode;
use story_architect::models::import::{NovelImportStatus, PreviewCharacter, PreviewRelation};
use story_architect::services::novel_import::{
    layer_summaries, merge_characters, merge_relations, split_into_chunks, ImportRegistry,
};
use story_architect::state::AppState;

const CHUNK_ONE: &str = r#"{
  "summary": "第一章摘要：少年林默在山村长大，偶得祖传玉佩。",
  "characters": [
    {"name":"林默","role_type":"support","appearance":"黑衣少年","personality":"沉默","biography":"山村少年","current_goal":"寻找身世"},
    {"name":"苏晚","role_type":"deuteragonist","appearance":"白衣女子","personality":"冷静","biography":"神秘剑客"}
  ],
  "items": [
    {"name":"青玉玉佩","description":"祖传玉佩","features":"温润","owner":"林默","importance":4}
  ],
  "relations": [
    {"a":"林默","b":"苏晚","conflict_desc":"初次相遇互相试探","affinity":10,"trust":30}
  ]
}"#;

const CHUNK_TWO: &str = r#"{
  "summary": "第二章摘要：林默下山，与苏晚再度相遇。",
  "characters": [
    {"name":"林默","role_type":"protagonist","appearance":"","personality":"坚毅","hidden_personality":"重情","biography":"山村少年，身负血仇，踏上修行之路，逐渐成长为一代剑客","speaking_style":"寡言"},
    {"name":"苏晚","role_type":"support","biography":"神秘剑客"}
  ],
  "items": [],
  "relations": [
    {"a":"苏晚","b":"林默","conflict_desc":"","affinity":-40,"trust":20}
  ]
}"#;

const CHUNK_THREE: &str = r#"{
  "summary": "第三章摘要：两人联手对抗敌人。",
  "characters": [
    {"name":"林默","role_type":"protagonist"},
    {"name":"苏晚","role_type":"deuteragonist"}
  ],
  "items": [
    {"name":"青玉玉佩","importance":5,"owner":"林默"}
  ],
  "relations": []
}"#;

const STRUCTURE: &str = r#"{
  "title": "剑起山村",
  "world_view": "武道世界",
  "background": "少年复仇",
  "summary": "林默与苏晚的成长故事",
  "volumes": [
    {"title":"第一卷 山村少年","summary":"林默离乡","chapters":[
      {"title":"第一章 初入江湖","outline":"林默离开山村，在集市初遇苏晚，因玉佩被追兵盯上。"},
      {"title":"第二章 并肩而战","outline":"林默与苏晚在破庙联手退敌，互相信任。"}
    ]},
    {"title":"第二卷 风云再起","summary":"更大的阴谋","chapters":[
      {"title":"第三章 血仇浮现","outline":"林默得知灭门真相，决心复仇。"}
    ]}
  ]
}"#;

fn novel_text() -> String {
    let mut text = String::new();
    for (heading, body) in [
        (
            "第一章 山村少年",
            "林默自幼生活在偏僻山村，与病重的母亲相依为命。某日他在后山拾到一枚温润的青玉玉佩，玉佩背面刻着陌生的剑纹，令他心中生出许多疑问。",
        ),
        (
            "第二章 下山",
            "母亲病逝后，林默带着玉佩下山谋生。他在集市上第一次遇见白衣女子苏晚，对方似乎认得玉佩上的剑纹，却只留下一句意味深长的话便转身离去。",
        ),
        (
            "第三章 联手",
            "追兵循着玉佩的气息找到林默。危急时刻苏晚出手相助，两人在破庙中并肩退敌。林默第一次意识到，自己的身世远比想象中复杂。",
        ),
    ] {
        text.push_str(heading);
        text.push('\n');
        text.push_str(body);
        text.push('\n');
    }
    text
}

async fn build_ready_state() -> (AppState, Router, Arc<MockProvider>) {
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    for step in [CHUNK_ONE, CHUNK_TWO, CHUNK_THREE, STRUCTURE] {
        mock.push_ok(step);
    }
    let chain = ProviderChain::new(vec![mock.clone()], 0, Duration::from_secs(5));
    let mut state = common::test_state_with_chain(Some(chain.clone())).await;
    state.imports = Arc::new(ImportRegistry::with_chain(2, chain));
    let app = common::test_app(state.clone());
    (state, app, mock)
}

async fn upload(app: &Router, text: &str, filename: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/novel-imports")
        .header("host", "127.0.0.1:8080")
        .header("content-type", "text/plain")
        .header("x-filename", filename)
        .body(Body::from(text.to_string()))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn wait_terminal(state: &AppState, id: &str) -> Value {
    for _ in 0..200 {
        let job = story_architect::services::novel_import::load_job(&state.pool, id)
            .await
            .expect("load job")
            .expect("job exists");
        if job.status.is_terminal() {
            return serde_json::to_value(job).expect("job json");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("任务未在 10 秒内进入终态");
}

#[tokio::test]
async fn novel_import_end_to_end() {
    let (state, app, mock) = build_ready_state().await;
    // X-Filename 为 HTTP 头，中文按 URL 编码传递。
    let (status, created) = upload(
        &app,
        &novel_text(),
        "%E5%89%91%E8%B5%B7%E5%B1%B1%E6%9D%91.txt",
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "upload: {created}");
    let job_id = created["id"].as_str().expect("job id").to_string();
    assert_eq!(created["status"], "queued");
    assert_eq!(created["filename"], "剑起山村.txt");

    let job = wait_terminal(&state, &job_id).await;
    assert_eq!(job["status"], "ready", "job: {job}");
    assert_eq!(job["stage"], "ready");
    assert_eq!(job["progress"], 100);
    assert_eq!(job["chunks_total"], 3);
    assert_eq!(job["chunks_done"], 3);
    assert_eq!(mock.call_count(), 4, "3 分块 + 1 次结构生成");

    let (status, body) = common::call(
        &app,
        Method::GET,
        &format!("/api/v1/novel-imports/{job_id}/preview"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview: {body}");
    assert_eq!(body["title"], "剑起山村");
    assert_eq!(body["volumes"].as_array().expect("volumes").len(), 2);
    assert_eq!(body["characters"].as_array().expect("characters").len(), 2);
    assert_eq!(body["relations"].as_array().expect("relations").len(), 1);
    assert_eq!(body["items"].as_array().expect("items").len(), 1);
    let hero = body["characters"]
        .as_array()
        .expect("characters")
        .iter()
        .find(|c| c["name"] == "林默")
        .expect("林默");
    assert_eq!(hero["role_type"], "protagonist");
    assert!(hero["biography"]
        .as_str()
        .unwrap_or("")
        .contains("身负血仇"));
    assert_eq!(body["relations"][0]["affinity"], -40);
    assert_eq!(body["relations"][0]["conflict_desc"], "初次相遇互相试探");
    assert_eq!(body["items"][0]["importance"], 5);

    let project_body = json!({});
    let (status, _headers, novel) = common::call_with_headers(
        &app,
        Method::POST,
        &format!("/api/v1/novel-imports/{job_id}/project"),
        vec![("idempotency-key", "project-first")],
        Some(project_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "project: {novel}");
    let novel_id = novel["id"].as_str().expect("novel id").to_string();
    assert_eq!(novel["title"], "剑起山村");

    let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM volumes WHERE novel_id = ?),
                (SELECT COUNT(*) FROM chapters WHERE novel_id = ?),
                (SELECT COUNT(*) FROM characters WHERE novel_id = ?),
                (SELECT COUNT(*) FROM items WHERE novel_id = ?),
                (SELECT COUNT(*) FROM character_relations WHERE novel_id = ?)",
    )
    .bind(&novel_id)
    .bind(&novel_id)
    .bind(&novel_id)
    .bind(&novel_id)
    .bind(&novel_id)
    .fetch_one(&state.pool)
    .await
    .expect("counts");
    assert_eq!(counts, (2, 3, 2, 1, 1));

    let index_rows: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT v.volume_index, c.chapter_index, c.status FROM chapters c \
         JOIN volumes v ON v.id = c.volume_id WHERE c.novel_id = ? \
         ORDER BY v.volume_index, c.chapter_index",
    )
    .bind(&novel_id)
    .fetch_all(&state.pool)
    .await
    .expect("chapters");
    assert_eq!(index_rows.len(), 3);
    assert_eq!(index_rows[0].0, 1);
    assert_eq!(index_rows[1].0, 1);
    assert_eq!(index_rows[2].0, 2);
    assert_eq!(index_rows[2].1, 1, "chapter_index 必须卷内递增");
    assert!(index_rows.iter().all(|row| row.2 == "outlined"));

    let relation: (String, String) =
        sqlx::query_as("SELECT char_a_id, char_b_id FROM character_relations WHERE novel_id = ?")
            .bind(&novel_id)
            .fetch_one(&state.pool)
            .await
            .expect("relation");
    assert!(relation.0 < relation.1, "char_a_id 必须小于 char_b_id");

    let hits = story_architect::services::search::search(
        &state.pool,
        "初入江湖",
        None,
        Some(&novel_id),
        1,
        20,
        2,
    )
    .await
    .expect("search");
    assert!(
        hits.items
            .iter()
            .any(|hit| hit.entity_type == "chapter" && hit.title.contains("初入江湖")),
        "FTS 应能检索到新章节: {:?}",
        hits.items
    );

    let (status, _headers, conflict) = common::call_with_headers(
        &app,
        Method::POST,
        &format!("/api/v1/novel-imports/{job_id}/project"),
        vec![("idempotency-key", "project-second")],
        Some(project_body),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "duplicate: {conflict}");
    assert_eq!(conflict["error"]["code"], "import_conflict");

    // 终态任务不可取消。
    let (status, cancel) = common::call(
        &app,
        Method::DELETE,
        &format!("/api/v1/novel-imports/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "cancel: {cancel}");
}

#[tokio::test]
async fn novel_import_sse_emits_snapshot_and_done() {
    let (state, app, _mock) = build_ready_state().await;
    let (status, created) = upload(&app, &novel_text(), "sse.txt").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = created["id"].as_str().expect("job id").to_string();
    let job = wait_terminal(&state, &job_id).await;
    assert_eq!(job["status"], "ready");

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/novel-imports/{job_id}/events"))
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("sse response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("sse body");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("event: import.snapshot"), "sse: {text}");
    assert!(text.contains("event: import.done"), "sse: {text}");
}

#[tokio::test]
async fn filename_percent_decode_falls_back_to_raw() {
    let (_state, app, _mock) = build_ready_state().await;
    let (status, created) = upload(&app, &novel_text(), "50%ZZ.txt").await;
    assert_eq!(status, StatusCode::ACCEPTED, "upload: {created}");
    assert_eq!(created["filename"], "50%ZZ.txt");
}

#[tokio::test]
async fn upload_rejects_invalid_input() {
    let (_state, app, _mock) = build_ready_state().await;
    let (status, body) = upload(&app, "太短", "short.txt").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert_eq!(body["error"]["code"], "validation_failed");

    let mut invalid = vec![0xff, 0xfe, 0xfd];
    invalid.extend(std::iter::repeat_n(b'a', 300));
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/novel-imports")
        .header("host", "127.0.0.1:8080")
        .header("content-type", "application/octet-stream")
        .body(Body::from(invalid))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["error"]["details"][0]["field"], "file");
    assert_eq!(value["error"]["details"][0]["message"], "必须是 UTF-8 文本");
}

#[test]
fn chunking_prefers_chapter_headings() {
    let text = novel_text();
    let (chunks, truncated) = split_into_chunks(&text);
    assert!(!truncated);
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].contains("第一章 山村少年"));
    assert!(chunks[1].contains("第二章 下山"));
    assert!(chunks[2].contains("第三章 联手"));
}

#[test]
fn chunking_splits_long_text_with_overlap() {
    let text = "a".repeat(13_000);
    let (chunks, truncated) = split_into_chunks(&text);
    assert!(!truncated);
    assert_eq!(chunks.len(), 3);
    let first_tail: String = chunks[0].chars().skip(5_800).collect();
    let second_head: String = chunks[1].chars().take(200).collect();
    assert_eq!(first_tail, second_head, "相邻分块应有 200 字符重叠");
    assert_eq!(
        chunks.iter().map(|c| c.len()).sum::<usize>(),
        13_000 + 200 + 200
    );
}

#[test]
fn chunking_respects_max_chunks() {
    let text = "b".repeat(6000 * 250);
    let (chunks, truncated) = split_into_chunks(&text);
    assert!(truncated);
    assert_eq!(chunks.len(), 200);
}

#[test]
fn characters_merge_case_insensitively_and_keep_heaviest_role() {
    let input = vec![
        PreviewCharacter {
            name: "林默".to_string(),
            role_type: "support".to_string(),
            appearance: Some("黑衣".to_string()),
            personality: None,
            hidden_personality: None,
            biography: Some("短".to_string()),
            current_goal: None,
            speaking_style: None,
        },
        PreviewCharacter {
            name: "林默".to_string(),
            role_type: "protagonist".to_string(),
            appearance: None,
            personality: Some("坚毅".to_string()),
            hidden_personality: None,
            biography: Some("更长的完整生平".to_string()),
            current_goal: None,
            speaking_style: None,
        },
        PreviewCharacter {
            name: "Alice".to_string(),
            role_type: "support".to_string(),
            appearance: None,
            personality: None,
            hidden_personality: None,
            biography: None,
            current_goal: None,
            speaking_style: None,
        },
        PreviewCharacter {
            name: "alice".to_string(),
            role_type: "deuteragonist".to_string(),
            appearance: None,
            personality: None,
            hidden_personality: None,
            biography: None,
            current_goal: None,
            speaking_style: None,
        },
    ];
    let merged = merge_characters(input);
    assert_eq!(merged.len(), 2);
    let hero = merged.iter().find(|c| c.name == "林默").expect("林默");
    assert_eq!(hero.role_type, "protagonist");
    assert_eq!(hero.appearance.as_deref(), Some("黑衣"));
    assert_eq!(hero.personality.as_deref(), Some("坚毅"));
    assert_eq!(hero.biography.as_deref(), Some("更长的完整生平"));
    let alice = merged.iter().find(|c| c.name == "Alice").expect("Alice");
    assert_eq!(alice.role_type, "deuteragonist");
}

#[test]
fn relations_dedup_unordered_and_keep_strongest_affinity() {
    let input = vec![
        PreviewRelation {
            a: "林默".to_string(),
            b: "苏晚".to_string(),
            conflict_desc: Some("初次试探".to_string()),
            affinity: 10,
            trust: 30,
        },
        PreviewRelation {
            a: "苏晚".to_string(),
            b: "林默".to_string(),
            conflict_desc: None,
            affinity: -60,
            trust: 15,
        },
    ];
    let merged = merge_relations(input);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].affinity, -60);
    assert_eq!(merged[0].trust, 15);
    assert_eq!(merged[0].conflict_desc.as_deref(), Some("初次试探"));
}

#[test]
fn summaries_layer_until_under_limit() {
    let input: Vec<String> = (0..200)
        .map(|index| format!("第{index}段摘要{}", "内容".repeat(30)))
        .collect();
    let layered = layer_summaries(&input);
    assert!(layered.len() < input.len());
    let total: usize = layered.iter().map(|item| item.chars().count()).sum();
    assert!(total <= 12_000, "layered total = {total}");
}

#[test]
fn schemas_stay_compact() {
    let chunk = serde_json::to_string(&story_architect::llm::schemas::novel_chunk_schema())
        .expect("chunk schema");
    let structure = serde_json::to_string(&story_architect::llm::schemas::novel_structure_schema())
        .expect("structure schema");
    assert!(chunk.len() <= 1500, "chunk schema = {} bytes", chunk.len());
    assert!(
        structure.len() <= 1500,
        "structure schema = {} bytes",
        structure.len()
    );
}

#[tokio::test]
async fn novel_import_status_parses_all_values() {
    for raw in ["queued", "analyzing", "ready", "failed", "cancelled"] {
        let parsed = NovelImportStatus::from_db_or_default(raw);
        assert_eq!(parsed.to_db(), raw);
    }
}
