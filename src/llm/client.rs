// src/llm/client.rs — 全局 reqwest 客户端（连接池/超时/UA）
use std::time::Duration;

use reqwest::Client;

pub fn build_http_client() -> Client {
    Client::builder()
        .user_agent("StoryArchitect/4.0")
        .connect_timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(8)
        .build()
        .unwrap_or_else(|_| Client::new())
}
