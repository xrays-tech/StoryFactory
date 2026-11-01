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

/// 任务阶段（仅用于进度展示，DB 中是无约束文本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftStage {
    Queued,
    Preparing,
    Writing,
    Linting,
    Rewriting,
    Finished,
    Failed,
    Cancelled,
}

impl DraftStage {
    pub fn as_str(self) -> &'static str {
        match self {
            DraftStage::Queued => "queued",
            DraftStage::Preparing => "preparing",
            DraftStage::Writing => "writing",
            DraftStage::Linting => "linting",
            DraftStage::Rewriting => "rewriting",
            DraftStage::Finished => "finished",
            DraftStage::Failed => "failed",
            DraftStage::Cancelled => "cancelled",
        }
    }
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
    /// 是否在生成后做「去 AI 味」lint 与定点重写；缺省开启。
    #[serde(default)]
    pub deslop: Option<bool>,
}

/// 正文 lint 结果（after 报告，未重写时即 before）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftLint {
    pub before_total: i64,
    pub after_total: i64,
    pub rewritten: i64,
    pub issues: Vec<DraftLintIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftLintIssue {
    pub kind: String,
    pub label: String,
    pub count: i64,
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
    pub lint: Option<DraftLint>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChapterContent {
    pub content: String,
    pub word_count: i64,
    /// 该章节最近一条草稿的去 AI 味检查报告；无草稿或未检查时为 None。
    #[serde(default)]
    pub lint: Option<DraftLint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SaveChapterContentReq {
    pub content: String,
}
