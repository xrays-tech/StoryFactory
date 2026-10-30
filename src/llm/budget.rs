// src/llm/budget.rs — 三层超时预算（§7.2 / D-18）
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Budget {
    pub session_deadline: Instant,
    pub step_deadline: Instant,
    pub attempt_timeout: Duration,
}

impl Budget {
    pub fn new(session_secs: u64, step_secs: u64, attempt_secs: u64) -> Self {
        let now = Instant::now();
        Self {
            session_deadline: now + Duration::from_secs(session_secs.max(1)),
            step_deadline: now + Duration::from_secs(step_secs.max(1)),
            attempt_timeout: Duration::from_secs(attempt_secs.max(1)),
        }
    }

    pub fn with_deadlines(
        session_deadline: Instant,
        step_deadline: Instant,
        attempt_timeout: Duration,
    ) -> Self {
        Self {
            session_deadline,
            step_deadline,
            attempt_timeout,
        }
    }

    /// 单次尝试截止：min(now + attempt, step_deadline, session_deadline)。
    pub fn attempt_deadline(&self) -> Instant {
        let candidate = Instant::now() + self.attempt_timeout;
        candidate.min(self.step_deadline).min(self.session_deadline)
    }

    pub fn hard_deadline(&self) -> Instant {
        self.step_deadline.min(self.session_deadline)
    }

    pub fn exhausted(&self) -> bool {
        Instant::now() >= self.hard_deadline()
    }

    /// 重试前检查：退避 + 至少 2s 余量仍需落在硬截止内。
    pub fn can_retry(&self, backoff: Duration) -> bool {
        Instant::now() + backoff + Duration::from_secs(2) < self.hard_deadline()
    }

    pub fn remaining(&self) -> Duration {
        self.hard_deadline()
            .saturating_duration_since(Instant::now())
    }
}
