// src/llm/mock.rs — 测试用脚本化 Provider（无网络）
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;

use crate::llm::{
    CompletionReq, CompletionResp, LlmError, LlmErrorKind, LlmProvider, Message, Purpose,
    StructuredMode,
};

enum MockStep {
    Ok(String),
    DelayOk(std::time::Duration, String),
    Err(LlmError),
}

pub struct MockProvider {
    name: String,
    model: String,
    mode: StructuredMode,
    steps: Mutex<VecDeque<MockStep>>,
    calls: AtomicUsize,
    last_max_tokens: AtomicU32,
    captured: Mutex<Vec<(Purpose, Vec<Message>)>>,
}

impl MockProvider {
    pub fn new(name: impl Into<String>, mode: StructuredMode) -> Self {
        Self {
            name: name.into(),
            model: "mock-model".to_string(),
            mode,
            steps: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
            last_max_tokens: AtomicU32::new(0),
            captured: Mutex::new(Vec::new()),
        }
    }

    pub fn push_ok(&self, content: impl Into<String>) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push_back(MockStep::Ok(content.into()));
        }
    }

    pub fn push_delay_ok(&self, delay: std::time::Duration, content: impl Into<String>) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push_back(MockStep::DelayOk(delay, content.into()));
        }
    }

    pub fn push_err(&self, err: LlmError) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push_back(MockStep::Err(err));
        }
    }

    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// 最近一次请求使用的 max_tokens；用于断言预算升级。
    pub fn last_max_tokens(&self) -> u32 {
        self.last_max_tokens.load(Ordering::SeqCst)
    }

    /// 仅测试用：按调用顺序返回 (purpose, messages)，用于断言 Prompt 内容。
    pub fn captured_requests(&self) -> Vec<(Purpose, Vec<Message>)> {
        self.captured
            .lock()
            .map(|captured| captured.clone())
            .unwrap_or_default()
    }

    /// 仅测试用：最近一次请求的 system 消息。
    pub fn last_system_prompt(&self) -> Option<String> {
        self.captured.lock().ok().and_then(|captured| {
            captured
                .iter()
                .rev()
                .flat_map(|(_, messages)| messages.iter().rev())
                .find(|message| message.role == crate::llm::MessageRole::System)
                .map(|message| message.content.clone())
        })
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn structured_mode(&self) -> StructuredMode {
        self.mode
    }

    async fn complete(&self, req: CompletionReq) -> Result<CompletionResp, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.last_max_tokens.store(req.max_tokens, Ordering::SeqCst);
        if let Ok(mut captured) = self.captured.lock() {
            captured.push((req.purpose, req.messages.clone()));
        }
        let step = self
            .steps
            .lock()
            .ok()
            .and_then(|mut steps| steps.pop_front());
        match step {
            Some(MockStep::DelayOk(delay, content)) => {
                tokio::time::sleep(delay).await;
                Ok(CompletionResp {
                    content,
                    prompt_tokens: 10,
                    completion_tokens: 10,
                })
            }
            Some(MockStep::Ok(content)) => Ok(CompletionResp {
                content,
                prompt_tokens: 10,
                completion_tokens: 10,
            }),
            Some(MockStep::Err(err)) => Err(err),
            None => Err(LlmError::new(
                LlmErrorKind::InvalidOutput,
                "MockProvider 无剩余响应",
            )),
        }
    }
}
