// src/models/sandbox.rs — 沙盒会话/事件 DTO（§6.3 / §8.3 / §8.7）
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SessionStatus {
    Created,
    Running,
    Paused,
    WaitingInput,
    Finalizing,
    Completed,
    Failed,
    Interrupted,
    Aborted,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Created => "created",
            SessionStatus::Running => "running",
            SessionStatus::Paused => "paused",
            SessionStatus::WaitingInput => "waiting_input",
            SessionStatus::Finalizing => "finalizing",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
            SessionStatus::Interrupted => "interrupted",
            SessionStatus::Aborted => "aborted",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        match raw {
            "created" => Some(SessionStatus::Created),
            "running" => Some(SessionStatus::Running),
            "paused" => Some(SessionStatus::Paused),
            "waiting_input" => Some(SessionStatus::WaitingInput),
            "finalizing" => Some(SessionStatus::Finalizing),
            "completed" => Some(SessionStatus::Completed),
            "failed" => Some(SessionStatus::Failed),
            "interrupted" => Some(SessionStatus::Interrupted),
            "aborted" => Some(SessionStatus::Aborted),
            _ => None,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            SessionStatus::Created
                | SessionStatus::Running
                | SessionStatus::Paused
                | SessionStatus::WaitingInput
                | SessionStatus::Finalizing
                | SessionStatus::Interrupted
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Aborted
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DirectorMode {
    Roundrobin,
    Director,
    Scripted,
}

impl DirectorMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DirectorMode::Roundrobin => "roundrobin",
            DirectorMode::Director => "director",
            DirectorMode::Scripted => "scripted",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        match raw {
            "roundrobin" => Some(DirectorMode::Roundrobin),
            "director" => Some(DirectorMode::Director),
            "scripted" => Some(DirectorMode::Scripted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ControlAction {
    Start,
    Pause,
    Resume,
    Step,
    Abort,
    Recover,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScriptStep {
    pub actor_id: String,
    pub beat_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScriptedPlan {
    pub steps: Vec<ScriptStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateSessionReq {
    pub actor_ids: Vec<String>,
    pub mode: DirectorMode,
    pub scene_prompt: String,
    pub step_limit: Option<u32>,
    pub director_config: Option<ScriptedPlan>,
    pub user_directive: Option<String>,
    pub autostart: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ControlReq {
    pub action: ControlAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InputReq {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActorName {
    pub id: String,
    pub name: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PurposeCount {
    pub purpose: String,
    pub calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: f64,
    pub calls: u32,
    pub by_purpose: Vec<PurposeCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionSnapshot {
    pub id: String,
    pub novel_id: String,
    pub chapter_id: Option<String>,
    pub status: SessionStatus,
    pub mode: DirectorMode,
    pub current_step: u32,
    pub step_limit: u32,
    pub actor_ids: Vec<String>,
    pub actor_names: Vec<ActorName>,
    pub last_event_seq: u64,
    pub can_recover: bool,
    pub usage: UsageSummary,
    pub error: Option<SessionError>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
    pub session_deadline: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SandboxEventType {
    SessionStarted,
    SessionPaused,
    SessionResumed,
    DirectorDecision,
    DirectorFallback,
    StepStarted,
    ActorAction,
    ActorError,
    EmotionUpdated,
    RollingSummaryApplied,
    WaitingInput,
    UserInputReceived,
    FinalizeStarted,
    OutlineReady,
    SessionCompleted,
    SessionFailed,
    SessionAborted,
    SessionInterrupted,
}

impl SandboxEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxEventType::SessionStarted => "session_started",
            SandboxEventType::SessionPaused => "session_paused",
            SandboxEventType::SessionResumed => "session_resumed",
            SandboxEventType::DirectorDecision => "director_decision",
            SandboxEventType::DirectorFallback => "director_fallback",
            SandboxEventType::StepStarted => "step_started",
            SandboxEventType::ActorAction => "actor_action",
            SandboxEventType::ActorError => "actor_error",
            SandboxEventType::EmotionUpdated => "emotion_updated",
            SandboxEventType::RollingSummaryApplied => "rolling_summary_applied",
            SandboxEventType::WaitingInput => "waiting_input",
            SandboxEventType::UserInputReceived => "user_input_received",
            SandboxEventType::FinalizeStarted => "finalize_started",
            SandboxEventType::OutlineReady => "outline_ready",
            SandboxEventType::SessionCompleted => "session_completed",
            SandboxEventType::SessionFailed => "session_failed",
            SandboxEventType::SessionAborted => "session_aborted",
            SandboxEventType::SessionInterrupted => "session_interrupted",
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct SessionRow {
    pub id: String,
    pub novel_id: String,
    pub chapter_id: Option<String>,
    pub scene_prompt: String,
    pub actor_ids: String,
    pub mode: String,
    pub status: String,
    pub step_limit: i64,
    pub current_step: i64,
    pub last_event_seq: i64,
    pub summary_until_seq: i64,
    pub rolling_summary: Option<String>,
    pub director_config: Option<String>,
    pub template_versions: Option<String>,
    pub session_deadline: Option<i64>,
    pub summary_text: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub pending_directive: Option<String>,
    pub user_directive: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}
