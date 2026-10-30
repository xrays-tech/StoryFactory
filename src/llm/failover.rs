// src/llm/failover.rs — Provider 故障转移链（§7.3 / D-18）
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::llm::{CompletionReq, CompletionResp, LlmError, LlmErrorKind, LlmProvider, Purpose};

#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub provider: String,
    pub model: String,
    pub purpose: Purpose,
    pub latency_ms: u64,
    pub success: bool,
    pub error_kind: Option<&'static str>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ChainOutcome {
    pub resp: CompletionResp,
    pub provider: String,
    pub model: String,
    pub attempts: Vec<AttemptRecord>,
}

#[derive(Clone)]
pub struct ProviderChain {
    providers: Vec<Arc<dyn LlmProvider>>,
    max_retries: u32,
    attempt_timeout: Duration,
}

impl ProviderChain {
    pub fn new(
        providers: Vec<Arc<dyn LlmProvider>>,
        max_retries: u32,
        attempt_timeout: Duration,
    ) -> Self {
        Self {
            providers,
            max_retries,
            attempt_timeout,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn attempt_timeout(&self) -> Duration {
        self.attempt_timeout
    }

    pub async fn complete(&self, req: CompletionReq) -> Result<ChainOutcome, LlmError> {
        self.complete_tracked(req)
            .await
            .map_err(|(error, _attempts)| error)
    }

    /// 失败时同时返回 attempts，供上层落 llm_usage（§7.3 每跳均记录）。
    pub async fn complete_tracked(
        &self,
        req: CompletionReq,
    ) -> Result<ChainOutcome, (LlmError, Vec<AttemptRecord>)> {
        let mut attempts: Vec<AttemptRecord> = Vec::new();
        let mut last_error: Option<LlmError> = None;
        let overall_deadline = req.deadline;

        for provider in &self.providers {
            // 每跳独立 attempt 窗口（D-18）：now + attempt_timeout，且不超过整体截止。
            let hop_deadline = (Instant::now() + self.attempt_timeout).min(overall_deadline);
            for attempt in 0..=self.max_retries {
                if Instant::now() >= hop_deadline {
                    let err = LlmError::new(LlmErrorKind::Timeout, "当前 Provider 尝试预算耗尽");
                    attempts.push(AttemptRecord {
                        provider: provider.name().to_string(),
                        model: provider.model().to_string(),
                        purpose: req.purpose,
                        latency_ms: 0,
                        success: false,
                        error_kind: Some(err.kind.as_str()),
                        prompt_tokens: 0,
                        completion_tokens: 0,
                    });
                    last_error = Some(err);
                    break;
                }
                let mut hop_req = req.clone();
                hop_req.deadline = hop_deadline;
                let started = Instant::now();
                match provider.complete(hop_req).await {
                    Ok(resp) => {
                        attempts.push(AttemptRecord {
                            provider: provider.name().to_string(),
                            model: provider.model().to_string(),
                            purpose: req.purpose,
                            latency_ms: started.elapsed().as_millis() as u64,
                            success: true,
                            error_kind: None,
                            prompt_tokens: resp.prompt_tokens,
                            completion_tokens: resp.completion_tokens,
                        });
                        return Ok(ChainOutcome {
                            resp,
                            provider: provider.name().to_string(),
                            model: provider.model().to_string(),
                            attempts,
                        });
                    }
                    Err(err) => {
                        attempts.push(AttemptRecord {
                            provider: provider.name().to_string(),
                            model: provider.model().to_string(),
                            purpose: req.purpose,
                            latency_ms: started.elapsed().as_millis() as u64,
                            success: false,
                            error_kind: Some(err.kind.as_str()),
                            prompt_tokens: 0,
                            completion_tokens: 0,
                        });
                        last_error = Some(err.clone());
                        if !err.retryable() {
                            return Err((err, attempts));
                        }
                        if attempt == self.max_retries {
                            break;
                        }
                        // 429 无 Retry-After 时按设计缺省 5s；其它瞬时错误 1s/3s。
                        let backoff = err.retry_after.unwrap_or_else(|| {
                            if err.kind == LlmErrorKind::RateLimited {
                                Duration::from_secs(5)
                            } else if attempt == 0 {
                                Duration::from_secs(1)
                            } else {
                                Duration::from_secs(3)
                            }
                        });
                        if Instant::now() + backoff + Duration::from_secs(2) >= hop_deadline {
                            break;
                        }
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        Err((
            last_error.unwrap_or_else(|| {
                LlmError::new(LlmErrorKind::Network, "Provider 链为空或全部失败")
            }),
            attempts,
        ))
    }
}
