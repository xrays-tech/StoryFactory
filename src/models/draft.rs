// src/models/draft.rs — 章节正文生成草稿 DTO（ADR-07 ts-rs）
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftStatus {
    Queued,
    Generating,
    Ready,
    Failed,
    Cancelled,
}

impl DraftStatus {
    pub fn to_db(self) -> String {
        match self {
            DraftStatus::Queued => "queued".to_string(),
            DraftStatus::Generating => "generating".to_string(),
            DraftStatus::Ready => "ready".to_string(),
            DraftStatus::Failed => "failed".to_string(),
            DraftStatus::Cancelled => "cancelled".to_string(),
        }
    }

    pub fn from_db_or_default(raw: &str) -> DraftStatus {
        match raw {
            "generating" => DraftStatus::Generating,
            "ready" => DraftStatus::Ready,
            "failed" => DraftStatus::Failed,
            "cancelled" => DraftStatus::Cancelled,
            _ => DraftStatus::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DraftStatus::Ready | DraftStatus::Failed | DraftStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDraftReq {
    #[serde(default)]
    pub style_name: Option<String>,
    #[serde(default)]
    pub style_sample: Option<String>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub target_words: Option<i64>,
    /// 叙述人称（如「第一人称」「第三人称」）；风格样本不改变人称，留空按第三人称。
    #[serde(default)]
    pub pov: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftJob {
    pub id: String,
    pub chapter_id: String,
    pub status: DraftStatus,
    pub stage: Option<String>,
    pub progress: i64,
    pub style_name: Option<String>,
    pub target_words: i64,
    pub message: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChapterContent {
    pub content: String,
    pub word_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SaveChapterContentReq {
    pub content: String,
}
