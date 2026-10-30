// tests/p2.rs — P2 CRUD / 回收站 / FTS / 关系 / 排序
mod common;

use axum::http::{Method, StatusCode};
use common::{
    call, create_chapter, create_character, create_novel, create_volume, test_app, test_state,
};
use serde_json::json;

#[tokio::test]
async fn novel_crud_lifecycle() {
    let app = test_app(test_state().await);
    let id = create_novel(&app, "测试小说").await;

    let (status, list) = call(&app, Method::GET, "/api/v1/novels", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total"], 1);

    let (status, updated) = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/novels/{id}"),
        Some(json!({ "title": "改名后", "summary": "简介" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["title"], "改名后");

    let (status, _) = call(&app, Method::DELETE, &format!("/api/v1/novels/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(&app, Method::GET, &format!("/api/v1/novels/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, bin) = call(&app, Method::GET, "/api/v1/recycle-bin", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bin["total"], 1);
    assert_eq!(bin["items"][0]["entity_type"], "novel");

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/recycle-bin/restore",
        Some(json!({ "entity_type": "novel", "ids": [id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, restored) = call(&app, Method::GET, &format!("/api/v1/novels/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["title"], "改名后");
}

#[tokio::test]
async fn character_name_unique_case_insensitive() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    create_character(&app, &nid, "Alice").await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/novels/{nid}/characters"),
        Some(json!({ "name": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn validation_errors_are_field_level() {
    let app = test_app(test_state().await);
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/novels",
        Some(json!({ "title": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(body["error"]["details"].as_array().is_some());
}

#[tokio::test]
async fn volume_and_chapter_index_reuse_after_delete() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let v1 = create_volume(&app, &nid, "第一卷").await;
    let c1 = create_chapter(&app, &nid, &v1, "第一章").await;
    let c2 = create_chapter(&app, &nid, &v1, "第二章").await;

    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/volumes/{v1}/chapters"),
        None,
    )
    .await;
    assert_eq!(list["total"], 2);

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/novels/{nid}/chapters/{c2}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 删除最高序号后，新章按 max+1 复用该序号（部分唯一索引只看未删行）
    let c3 = create_chapter(&app, &nid, &v1, "新章").await;
    let (_, got) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/chapters/{c3}"),
        None,
    )
    .await;
    assert_eq!(got["chapter_index"], 2);
    let (_, got2) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/chapters/{c1}"),
        None,
    )
    .await;
    assert_eq!(got2["chapter_index"], 1);
}

#[tokio::test]
async fn cross_volume_chapter_move() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let v1 = create_volume(&app, &nid, "卷一").await;
    let v2 = create_volume(&app, &nid, "卷二").await;
    let a = create_chapter(&app, &nid, &v1, "A").await;
    let b = create_chapter(&app, &nid, &v2, "B").await;

    let (status, moved) = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/novels/{nid}/chapters/{b}"),
        Some(json!({ "volume_id": v1, "chapter_index": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["volume_id"], v1);
    assert_eq!(moved["chapter_index"], 1);

    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/volumes/{v1}/chapters"),
        None,
    )
    .await;
    assert_eq!(list["total"], 2);
    let (_, a_row) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/chapters/{a}"),
        None,
    )
    .await;
    assert_eq!(a_row["chapter_index"], 2);
}

#[tokio::test]
async fn volume_move_up_down() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let v1 = create_volume(&app, &nid, "卷一").await;
    let v2 = create_volume(&app, &nid, "卷二").await;

    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/novels/{nid}/volumes/{v2}/move"),
        Some(json!({ "direction": "up" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/volumes"),
        None,
    )
    .await;
    assert_eq!(list[0]["id"], v2);
    assert_eq!(list[1]["id"], v1);
}

#[tokio::test]
async fn relations_put_normalizes_and_rejects_self() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let a = create_character(&app, &nid, "A").await;
    let b = create_character(&app, &nid, "B").await;

    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("/api/v1/novels/{nid}/relations"),
        Some(json!({ "relations": [{ "char_a_id": a, "char_b_id": b, "affinity": 20, "trust": 30 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, got) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/relations"),
        None,
    )
    .await;
    assert_eq!(got.as_array().unwrap().len(), 1);
    let row = &got[0];
    assert!(row["char_a_id"].as_str().unwrap() < row["char_b_id"].as_str().unwrap());

    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("/api/v1/novels/{nid}/relations"),
        Some(json!({ "relations": [{ "char_a_id": a, "char_b_id": a }] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn item_owner_must_belong_to_novel() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let other = create_novel(&app, "其他").await;
    let foreign = create_character(&app, &other, "外部角色").await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/novels/{nid}/items"),
        Some(json!({ "name": "剑", "owner_id": foreign })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn recycle_cascade_restore_and_purge() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let v1 = create_volume(&app, &nid, "卷一").await;
    let _c = create_chapter(&app, &nid, &v1, "章").await;
    let _ch = create_character(&app, &nid, "角色").await;
    let other = create_chapter(&app, &nid, &v1, "单独删除").await;

    // 单独删除一章（批次不同，父恢复时不应带回）
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/novels/{nid}/chapters/{other}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(&app, Method::DELETE, &format!("/api/v1/novels/{nid}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/recycle-bin/restore",
        Some(json!({ "entity_type": "novel", "ids": [nid] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, volumes) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/volumes"),
        None,
    )
    .await;
    assert_eq!(volumes.as_array().unwrap().len(), 1);
    let (_, chars) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/characters"),
        None,
    )
    .await;
    assert_eq!(chars["total"], 1);

    // 单独删除的章节仍在回收站
    let (_, bin) = call(&app, Method::GET, "/api/v1/recycle-bin", None).await;
    let has_chapter = bin["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == other);
    assert!(
        has_chapter,
        "individually deleted chapter must stay deleted"
    );

    // 再次软删后才能永久清除（purge 仅允许回收站内实体）
    let (status, _) = call(&app, Method::DELETE, &format!("/api/v1/novels/{nid}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/recycle-bin/novel/{nid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, bin) = call(&app, Method::GET, "/api/v1/recycle-bin", None).await;
    assert_eq!(bin["total"], 0);
}

#[tokio::test]
async fn fts_sync_on_create_update_delete_restore() {
    let app = test_app(test_state().await);
    let id = create_novel(&app, "中文小说测试").await;

    let (status, hits) = call(&app, Method::GET, "/api/v1/search?q=中文小说", None).await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    assert_eq!(hits["total"], 1);
    assert_eq!(hits["items"][0]["entity_id"], id);

    // 2 字走 LIKE 兜底
    let (_, short) = call(&app, Method::GET, "/api/v1/search?q=小说", None).await;
    assert_eq!(short["total"], 1);

    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/novels/{id}"),
        Some(json!({ "title": "完全不同的标题" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, old) = call(&app, Method::GET, "/api/v1/search?q=中文小说", None).await;
    assert_eq!(old["total"], 0);
    let (_, new) = call(&app, Method::GET, "/api/v1/search?q=完全不同", None).await;
    assert_eq!(new["total"], 1);

    let (status, _) = call(&app, Method::DELETE, &format!("/api/v1/novels/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, gone) = call(&app, Method::GET, "/api/v1/search?q=完全不同", None).await;
    assert_eq!(gone["total"], 0);

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/recycle-bin/restore",
        Some(json!({ "entity_type": "novel", "ids": [id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, back) = call(&app, Method::GET, "/api/v1/search?q=完全不同", None).await;
    assert_eq!(back["total"], 1);
}

#[tokio::test]
async fn pagination_bounds() {
    let app = test_app(test_state().await);
    for i in 0..3 {
        create_novel(&app, &format!("小说{i}")).await;
    }
    let (_, page) = call(
        &app,
        Method::GET,
        "/api/v1/novels?page=1&page_size=999",
        None,
    )
    .await;
    assert_eq!(page["page_size"], 100);
    assert_eq!(page["total"], 3);

    let (_, page2) = call(&app, Method::GET, "/api/v1/novels?page=2&page_size=2", None).await;
    assert_eq!(page2["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn purge_rejects_active_entity_and_unknown_type() {
    let app = test_app(test_state().await);
    let id = create_novel(&app, "活着的项目").await;

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/recycle-bin/novel/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::GET, &format!("/api/v1/novels/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "active entity must not be purged");

    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/recycle-bin/bogus/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_failed");
}

#[tokio::test]
async fn restore_keeps_original_name_when_free() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let cid = create_character(&app, &nid, "Alice").await;
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/novels/{nid}/characters/{cid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/recycle-bin/restore",
        Some(json!({ "entity_type": "character", "ids": [cid] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, got) = call(
        &app,
        Method::GET,
        &format!("/api/v1/novels/{nid}/characters/{cid}"),
        None,
    )
    .await;
    assert_eq!(got["name"], "Alice");
}

#[tokio::test]
async fn restore_character_reindexes_body() {
    let app = test_app(test_state().await);
    let nid = create_novel(&app, "小说").await;
    let (status, created) = call(
        &app,
        Method::POST,
        &format!("/api/v1/novels/{nid}/characters"),
        Some(json!({ "name": "剑客", "personality": "神秘剑客" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cid = created["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/novels/{nid}/characters/{cid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, gone) = call(&app, Method::GET, "/api/v1/search?q=神秘剑客", None).await;
    assert_eq!(gone["total"], 0);
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/recycle-bin/restore",
        Some(json!({ "entity_type": "character", "ids": [cid] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, back) = call(&app, Method::GET, "/api/v1/search?q=神秘剑客", None).await;
    assert_eq!(back["total"], 1);
}

#[tokio::test]
async fn malformed_json_uses_error_envelope() {
    let app = test_app(test_state().await);
    let (status, body) =
        common::call_raw(&app, Method::POST, "/api/v1/novels", "{\"title\":").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(body["error"]["request_id"].as_str().is_some());
}

#[tokio::test]
async fn create_returns_location_header() {
    let app = test_app(test_state().await);
    let (status, headers, body) = common::call_full(
        &app,
        Method::POST,
        "/api/v1/novels",
        Some(json!({ "title": "带位置" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_str().unwrap();
    assert_eq!(
        headers.get("location").and_then(|v| v.to_str().ok()),
        Some(format!("/api/v1/novels/{id}").as_str())
    );
}
