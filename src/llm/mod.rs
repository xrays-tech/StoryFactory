// src/llm/mod.rs — LLM 抽象、消息与错误类型（§7）
pub mod budget;
pub mod client;
pub mod factory;
pub mod failover;
pub mod mock;
pub mod prompts;
pub mod providers;
pub mod schemas;
pub mod service;
pub mod structured;
pub mod usage;

use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    ActorStep,
    Director,
    Summary,
    Repair,
    Finalize,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::ActorStep => "actor_step",
            Purpose::Director => "director",
            Purpose::Summary => "summary",
            Purpose::Repair => "repair",
            Purpose::Finalize => "finalize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResponseFormat {
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
    },
}

#[derive(Debug, Clone)]
pub struct CompletionReq {
    pub messages: Vec<Message>,
    pub response_format: Option<ResponseFormat>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub deadline: Instant,
    pub purpose: Purpose,
}

impl CompletionReq {
    pub fn new(messages: Vec<Message>, deadline: Instant, purpose: Purpose) -> Self {
        Self {
            messages,
            response_format: None,
            max_tokens: 1500,
            temperature: 0.7,
            deadline,
            purpose,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionResp {
    pub content: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredMode {
    JsonSchema,
    JsonObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmErrorKind {
    Network,
    Timeout,
    RateLimited,
    InvalidOutput,
    Auth,
    BadRequest,
    Server,
}

impl LlmErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmErrorKind::Network => "network",
            LlmErrorKind::Timeout => "timeout",
            LlmErrorKind::RateLimited => "rate_limited",
            LlmErrorKind::InvalidOutput => "invalid_output",
            LlmErrorKind::Auth => "auth",
            LlmErrorKind::BadRequest => "bad_request",
            LlmErrorKind::Server => "server",
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct LlmError {
    pub kind: LlmErrorKind,
    pub message: String,
    pub retry_after: Option<std::time::Duration>,
}

impl LlmError {
    pub fn new(kind: LlmErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_after: None,
        }
    }

    pub fn with_retry_after(mut self, retry_after: Option<std::time::Duration>) -> Self {
        self.retry_after = retry_after;
        self
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self.kind,
            LlmErrorKind::Network
                | LlmErrorKind::Timeout
                | LlmErrorKind::RateLimited
                | LlmErrorKind::Server
        )
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    fn structured_mode(&self) -> StructuredMode;
    async fn complete(&self, req: CompletionReq) -> Result<CompletionResp, LlmError>;
}
