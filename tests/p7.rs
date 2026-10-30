// tests/p7.rs — 导出/导入、配置、成本、client-errors、备份
mod common;

use std::io::{Cursor, Write};

use axum::http::{Method, StatusCode};
use serde_json::json;
use story_architect::config;
use story_architect::db;

use common::{
    call, create_chapter, create_character, create_novel, create_volume, test_app, test_state,
};

#[tokio::test]
async fn export_import_roundtrip_remaps_ids() {
    let state = test_state().await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "导出测试小说").await;
    let cid = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "第一卷").await;
    let _ch = create_chapter(&app, &nid, &vid, "第一章").await;
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/api/v1/novels/{nid}/items"),
        Some(json!({ "name": "古剑", "owner_id": cid })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, headers, bytes) = common::call_bytes(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/export"),
        "application/zip",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/zip")
    );
    assert!(bytes.starts_with(b"PK"));

    let (status, _headers, body) = common::call_bytes(
        &app,
        Method::POST,
        "/api/v1/import",
        "application/zip",
        bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let new_id = report["novel_id"].as_str().unwrap();
    assert_ne!(new_id, nid);
    assert_eq!(report["imported"]["characters"], 1);
    assert_eq!(report["imported"]["volumes"], 1);

    let (status, novel) = call(&app, Method::GET, &format!("/api/v1/novels/{new_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(novel["title"], "导出测试小说");
    let (_, chars) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{new_id}/characters"),
        None,
    )
    .await;
    assert_eq!(chars["total"], 1);
    let (_, items) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{new_id}/items"),
        None,
    )
    .await;
    assert_eq!(items["total"], 1);
    let (_, hits) = call(&app, Method::GET, "/api/v1/search?q=导出测试小说", None).await;
    assert!(
        hits["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["entity_id"] == new_id),
        "imported novel must be searchable: {hits}"
    );
    let owner = items["items"][0]["owner_id"].as_str().unwrap();
    let imported_char = chars["items"][0]["id"].as_str().unwrap();
    assert_eq!(owner, imported_char, "owner id must be remapped");
}

fn make_zip(format_version: u32, checksum: &str, data: &serde_json::Value) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let manifest = json!({
            "format_version": format_version,
            "schema_version": 1,
            "app_version": "4.0.0",
            "exported_at": 1,
            "novel": {"id":"n","title":"t","summary":""},
            "counts": {},
            "checksums": {"data.json": checksum},
            "options": {}
        });
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(manifest.to_string().as_bytes()).unwrap();
        zip.start_file("data.json", options).unwrap();
        zip.write_all(data.to_string().as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

#[tokio::test]
async fn import_rejects_future_version_and_bad_checksum() {
    let state = test_state().await;
    let app = test_app(state);
    let data = json!({"novel":{"id":"n","title":"t"},"volumes":[],"chapters":[],"characters":[],"relations":[],"items":[],"sessions":[],"events":[]});
    let future = make_zip(99, "sha256:bad", &data);
    let (status, _headers, body) = common::call_bytes(
        &app,
        Method::POST,
        "/api/v1/import",
        "application/zip",
        future,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"], "unsupported_import_version");

    let empty = make_zip(2, "", &data);
    let (status, _headers, _body) = common::call_bytes(
        &app,
        Method::POST,
        "/api/v1/import",
        "application/zip",
        empty,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let bad = make_zip(2, "sha256:deadbeef", &data);
    let (status, _headers, body) =
        common::call_bytes(&app, Method::POST, "/api/v1/import", "application/zip", bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"], "validation_failed");
}

#[tokio::test]
async fn config_patch_and_validation() {
    let state = test_state().await;
    let app = test_app(state.clone());
    let (status, cfg) = call(&app, Method::GET, "/api/v1/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cfg["llm"]["model"].as_str().is_some());
    let (status, updated) = call(
        &app,
        Method::PATCH,
        "/api/v1/config",
        Some(json!({"model":"deepseek-reasoner","max_concurrent":2})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["llm"]["model"], "deepseek-reasoner");
    assert_eq!(updated["sandbox"]["max_concurrent"], 2);

    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM app_config WHERE key = 'runtime_config'")
            .fetch_optional(&state.pool)
            .await
            .unwrap();
    assert!(stored.is_some(), "runtime config must persist");
    let patch = story_architect::config::load_runtime_patch(&state.pool)
        .await
        .expect("runtime patch");
    let mut loaded = story_architect::config::load().unwrap();
    patch.apply(&mut loaded).unwrap();
    assert_eq!(loaded.llm.model, "deepseek-reasoner");

    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/v1/config",
        Some(json!({"max_concurrent":99})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn usage_report_and_client_errors() {
    let state = test_state().await;
    let app = test_app(state.clone());
    sqlx::query(
        "INSERT INTO llm_usage(provider, model, purpose, prompt_tokens, completion_tokens, cost_usd, latency_ms, success) VALUES('deepseek','deepseek-chat','actor_step',100,50,0.001,10,1)",
    )
    .execute(&state.pool)
    .await
    .unwrap();
    let (status, usage) = call(&app, Method::GET, "/api/v1/usage", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(usage["total_calls"], 1);
    assert!((usage["total_cost_usd"].as_f64().unwrap() - 0.001).abs() < 1e-9);

    let (status, _headers, _body) = common::call_bytes(
        &app,
        Method::POST,
        "/api/v1/client-errors",
        "application/json",
        br#"{"message":"boom","stack":"x"}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn startup_backup_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().display().to_string();
    let pool = db::open_pool(&data_dir).await.unwrap();
    db::migrate(&pool).await.unwrap();
    db::backup(&pool, &data_dir).await.unwrap();
    let bak = format!("{}/story-architect.db.bak", data_dir);
    assert!(std::path::Path::new(&bak).exists());
    db::backup(&pool, &data_dir).await.unwrap();
    assert!(std::path::Path::new(&format!("{}/story-architect.db.bak.1", data_dir)).exists());
    let _ = config::load();
}
