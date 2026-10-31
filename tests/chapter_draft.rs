// tests/chapter_draft.rs — 章节正文生成（文风参考 / 场景切分 / 失败跳过）
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use sqlx::sqlite::SqlitePool;

use story_architect::config::{self, AppConfig};
use story_architect::db;
use story_architect::error::ErrorCode;
use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::{LlmError, LlmErrorKind, StructuredMode};
use story_architect::models::draft::{CreateDraftReq, DraftJob, DraftStatus};
use story_architect::services::chapter_draft::{load_job, DraftRegistry};

const OUTLINE_THREE_SCENES: &str = "\
## 剧情梗概
少年入城，老者赠匣，剑光乍现。

## 关键场景
- 少年推门而入
- 老者递出木匣
- 木匣中剑光乍现
";

struct Fixture {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
    config: Arc<ArcSwap<AppConfig>>,
    novel_id: String,
    chapter_id: String,
}

async fn setup(final_outline: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().display().to_string();
    let pool = db::open_pool(&data_dir).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    let novel_id: String = sqlx::query_scalar(
        "INSERT INTO novels(title, world_view, background, summary) \
         VALUES('剑气长街', '武道世界', '少年入城', '少年与老者的相遇') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("novel");

    let volume_id: String = sqlx::query_scalar(
        "INSERT INTO volumes(novel_id, volume_index, title, summary) \
         VALUES(?, 1, '第一卷', '少年初入江湖') RETURNING id",
    )
    .bind(&novel_id)
    .fetch_one(&pool)
    .await
    .expect("volume");

    sqlx::query(
        "INSERT INTO characters(novel_id, name, personality, speaking_style, current_goal) \
         VALUES(?, '少年', '谨慎', '寡言', '找到老者')",
    )
    .bind(&novel_id)
    .execute(&pool)
    .await
    .expect("character");

    let chapter_id: String = sqlx::query_scalar(
        "INSERT INTO chapters(novel_id, volume_id, chapter_index, title, final_outline) \
         VALUES(?, ?, 1, '第一章 夜雨长街', ?) RETURNING id",
    )
    .bind(&novel_id)
    .bind(&volume_id)
    .bind(final_outline)
    .fetch_one(&pool)
    .await
    .expect("chapter");

    let cfg = config::load().expect("config");
    Fixture {
        _dir: dir,
        pool,
        config: Arc::new(ArcSwap::from_pointee(cfg)),
        novel_id,
        chapter_id,
    }
}

fn mock_chain(mock: Arc<MockProvider>) -> ProviderChain {
    ProviderChain::new(vec![mock], 0, Duration::from_secs(5))
}

fn registry(fixture: &Fixture, chain: ProviderChain) -> DraftRegistry {
    DraftRegistry::with_chain(fixture.config.clone(), fixture.pool.clone(), 2, chain)
}

async fn wait_terminal(pool: &SqlitePool, id: &str) -> DraftJob {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let job = load_job(pool, id)
            .await
            .expect("load draft")
            .expect("draft row");
        if job.status.is_terminal() {
            return job;
        }
        assert!(
            Instant::now() < deadline,
            "draft {id} did not reach a terminal state in time"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn draft_request(target_words: i64) -> CreateDraftReq {
    CreateDraftReq {
        target_words: Some(target_words),
        ..CreateDraftReq::default()
    }
}

#[tokio::test]
async fn draft_generates_content_and_writes_chapter() {
    let fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    mock.push_ok("少年推门而入，雨水顺着屋檐落下。");
    mock.push_ok("老者从袖中取出木匣，放在桌上。");
    mock.push_ok("匣盖弹开，一道剑光映亮长街。");

    let registry = registry(&fixture, mock_chain(mock.clone()));
    let job = registry
        .create(&fixture.novel_id, &fixture.chapter_id, draft_request(1_200))
        .await
        .expect("create draft");
    assert_eq!(job.status, DraftStatus::Queued);

    let job = wait_terminal(&fixture.pool, &job.id).await;
    assert_eq!(job.status, DraftStatus::Ready, "{job:?}");
    assert_eq!(mock.call_count(), 3);

    let (chapter_content, word_count): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT content, word_count FROM chapters WHERE id = ?")
            .bind(&fixture.chapter_id)
            .fetch_one(&fixture.pool)
            .await
            .expect("chapter content");
    let draft_content: Option<String> =
        sqlx::query_scalar("SELECT content FROM chapter_drafts WHERE id = ?")
            .bind(&job.id)
            .fetch_one(&fixture.pool)
            .await
            .expect("draft content");

    let chapter_content = chapter_content.expect("chapter content must be written");
    assert!(!chapter_content.is_empty());
    assert!(word_count.unwrap_or_default() > 0);
    assert_eq!(Some(&chapter_content), draft_content.as_ref());
}

#[tokio::test]
async fn draft_prompt_includes_style_sample() {
    let fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let sample = "月光如刀，刀光如雪。\n他站在长街尽头，没有说话。";
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    mock.push_ok("第一场正文。");
    mock.push_ok("第二场正文。");
    mock.push_ok("第三场正文。");

    let registry = registry(&fixture, mock_chain(mock.clone()));
    let job = registry
        .create(
            &fixture.novel_id,
            &fixture.chapter_id,
            CreateDraftReq {
                style_name: Some("古龙".to_string()),
                style_sample: Some(sample.to_string()),
                ..draft_request(900)
            },
        )
        .await
        .expect("create draft");
    let job = wait_terminal(&fixture.pool, &job.id).await;
    assert_eq!(job.status, DraftStatus::Ready, "{job:?}");

    let system = mock.last_system_prompt().expect("system prompt");
    assert!(system.contains(sample), "system prompt: {system}");
    assert!(system.contains("不得复用"), "system prompt: {system}");
    assert!(system.contains("古龙"), "system prompt: {system}");
}

#[tokio::test]
async fn draft_rejects_chapter_without_outline() {
    let fixture = setup(None).await;
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let registry = registry(&fixture, mock_chain(mock));

    let error = registry
        .create(&fixture.novel_id, &fixture.chapter_id, draft_request(1_000))
        .await
        .expect_err("empty outline must be rejected");

    assert_eq!(error.status, 422);
    assert!(matches!(error.code, ErrorCode::ValidationFailed));
    assert!(error.message.contains("还没有大纲"));
}

#[tokio::test]
async fn draft_skips_failed_scene_but_finishes() {
    let fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    mock.push_ok("第一场正文。");
    mock.push_err(LlmError::new(LlmErrorKind::InvalidOutput, "第二场失败"));
    mock.push_ok("第三场正文。");

    let registry = registry(&fixture, mock_chain(mock.clone()));
    let job = registry
        .create(&fixture.novel_id, &fixture.chapter_id, draft_request(1_200))
        .await
        .expect("create draft");
    let job = wait_terminal(&fixture.pool, &job.id).await;

    assert_eq!(job.status, DraftStatus::Ready, "{job:?}");
    assert_eq!(mock.call_count(), 3);
    let message = job.message.as_deref().unwrap_or_default();
    assert!(message.contains("已跳过"), "message: {message}");

    let content: String = sqlx::query_scalar("SELECT content FROM chapters WHERE id = ?")
        .bind(&fixture.chapter_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("chapter content");
    assert!(content.contains("第一场正文"));
    assert!(content.contains("第三场正文"));
}

#[tokio::test]
async fn draft_escalates_truncated_scene_instead_of_skipping() {
    let fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    // 第一场在原始预算处被截断，升级预算后重试成功；其余两场正常。
    mock.push_err(LlmError::new(LlmErrorKind::Truncated, "输出被截断"));
    mock.push_ok("升级预算后的第一场正文。");
    mock.push_ok("第二场正文。");
    mock.push_ok("第三场正文。");

    let registry = registry(&fixture, mock_chain(mock.clone()));
    let job = registry
        .create(&fixture.novel_id, &fixture.chapter_id, draft_request(1_200))
        .await
        .expect("create draft");
    let job = wait_terminal(&fixture.pool, &job.id).await;

    assert_eq!(job.status, DraftStatus::Ready, "{job:?}");
    assert_eq!(job.message, None);
    assert_eq!(mock.call_count(), 4);
    let content: String = sqlx::query_scalar("SELECT content FROM chapters WHERE id = ?")
        .bind(&fixture.chapter_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("chapter content");
    assert!(content.contains("升级预算后的第一场正文"), "{content}");
    assert!(content.contains("第三场正文"));
}

#[tokio::test]
async fn draft_prompt_includes_pov_and_defaults_to_third_person() {
    let fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    for _ in 0..3 {
        mock.push_ok("第一人称正文。");
    }

    let draft_registry = registry(&fixture, mock_chain(mock.clone()));
    let job = draft_registry
        .create(
            &fixture.novel_id,
            &fixture.chapter_id,
            CreateDraftReq {
                pov: Some("第一人称".to_string()),
                ..draft_request(600)
            },
        )
        .await
        .expect("create draft");
    let job = wait_terminal(&fixture.pool, &job.id).await;
    assert_eq!(job.status, DraftStatus::Ready, "{job:?}");

    let prompts: Vec<String> = mock
        .captured_requests()
        .into_iter()
        .flat_map(|(_, messages)| messages)
        .filter(|message| message.role == story_architect::llm::MessageRole::User)
        .map(|message| message.content)
        .collect();
    assert!(!prompts.is_empty(), "expected prose requests");
    for prompt in &prompts {
        assert!(prompt.contains("# 叙述人称"), "prompt: {prompt}");
        assert!(prompt.contains("第一人称"), "prompt: {prompt}");
    }

    // 未指定人称时按第三人称，避免风格样本改变叙述人称
    let default_fixture = setup(Some(OUTLINE_THREE_SCENES)).await;
    let default_mock = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    for _ in 0..3 {
        default_mock.push_ok("第三人称正文。");
    }
    let default_draft_registry = registry(&default_fixture, mock_chain(default_mock.clone()));
    let default_job = default_draft_registry
        .create(
            &default_fixture.novel_id,
            &default_fixture.chapter_id,
            draft_request(600),
        )
        .await
        .expect("create default draft");
    assert_eq!(
        wait_terminal(&default_fixture.pool, &default_job.id)
            .await
            .status,
        DraftStatus::Ready
    );
    let first_prompt = default_mock
        .captured_requests()
        .into_iter()
        .flat_map(|(_, messages)| messages)
        .find(|message| message.role == story_architect::llm::MessageRole::User)
        .map(|message| message.content)
        .expect("default prose prompt");
    assert!(first_prompt.contains("第三人称"), "prompt: {first_prompt}");
}
