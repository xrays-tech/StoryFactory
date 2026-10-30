// tests/db.rs — 迁移幂等 + schema_version（D-25/D-03）
use story_architect::db;

#[tokio::test]
async fn migrate_and_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    let pool = db::open_pool(&data_dir).await.expect("open pool");
    db::migrate(&pool).await.expect("migrate");
    let v1 = db::schema_version(&pool).await.expect("schema version");
    assert!(v1 >= 1);

    // 幂等：第二遍不应改变版本
    db::migrate(&pool).await.expect("migrate twice");
    let v2 = db::schema_version(&pool)
        .await
        .expect("schema version twice");
    assert_eq!(v1, v2);

    // begin_immediate 可用
    let mut tx = db::begin_immediate(&pool).await.expect("begin immediate");
    sqlx::query("INSERT INTO novels (title) VALUES ('t')")
        .execute(&mut *tx)
        .await
        .expect("insert");
    tx.commit().await.expect("commit");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM novels")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn fts_trigram_cjk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    let pool = db::open_pool(&data_dir).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    sqlx::query(
        "INSERT INTO search_index(rowid, title, body, entity_type, entity_id, novel_id) VALUES (1, '中文小说标题', '这是一段中文正文内容', 'chapter', 'e1', 'n1')",
    )
    .execute(&pool)
    .await
    .expect("insert fts");

    let hit: Vec<String> = sqlx::query_scalar(
        "SELECT title FROM search_index WHERE search_index MATCH '\"中文小说\"'",
    )
    .fetch_all(&pool)
    .await
    .expect("trigram match");
    assert_eq!(hit.len(), 1, "trigram should match CJK substring");
}

#[tokio::test]
async fn instance_lock_is_exclusive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    let held = db::acquire_instance_lock(&data_dir).expect("first lock");
    assert!(held.path().exists());

    let second = db::acquire_instance_lock(&data_dir);
    assert!(
        second.is_err(),
        "同一数据目录的第二个实例必须被拒绝（多进程共享 WAL 会丢数据）"
    );

    drop(held);
    let third = db::acquire_instance_lock(&data_dir).expect("lock reacquired after release");
    drop(third);
}

#[tokio::test]
async fn unclean_marker_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    assert!(!db::was_unclean(&data_dir), "全新目录不应有残留标记");

    db::mark_running(&data_dir).expect("mark running");
    assert!(db::was_unclean(&data_dir));

    db::mark_clean(&data_dir).expect("mark clean");
    assert!(!db::was_unclean(&data_dir));

    // 重复清理是幂等的
    db::mark_clean(&data_dir).expect("mark clean twice");
}

#[tokio::test]
async fn checkpoint_and_quick_check_ok() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    let pool = db::open_pool(&data_dir).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    assert!(
        db::checkpoint(&pool, "FULL").await.expect("checkpoint"),
        "单连接下 FULL checkpoint 必须完整落盘"
    );
    let report = db::quick_check(&pool).await.expect("quick_check");
    assert_eq!(report, "ok");

    db::close(pool).await;
}
