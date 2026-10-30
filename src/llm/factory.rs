// src/llm/factory.rs — 从 AppConfig 构建 Provider 链（§7.3）
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;

use crate::config::AppConfig;
use crate::llm::failover::ProviderChain;
use crate::llm::providers::OpenAiCompatProvider;
use crate::llm::{LlmProvider, StructuredMode};

fn mode_for(provider: &str) -> StructuredMode {
    match provider {
        "deepseek" => StructuredMode::JsonObject,
        _ => StructuredMode::JsonSchema,
    }
}

fn default_base_url(provider: &str) -> String {
    match provider {
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "openai" => "https://api.openai.com/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    }
}

fn default_model(provider: &str) -> String {
    match provider {
        "deepseek" => "deepseek-chat".to_string(),
        "openai" => "gpt-4o-mini".to_string(),
        _ => "gpt-4o-mini".to_string(),
    }
}

fn parse_fallback(spec: &str) -> (String, String) {
    match spec.split_once(':') {
        Some((provider, model)) => (provider.to_string(), model.to_string()),
        None => ("openai".to_string(), spec.to_string()),
    }
}

/// 无 API Key 时返回空链（会话创建时应报 LlmConfigMissing）。
pub fn build_chain(cfg: &AppConfig) -> ProviderChain {
    build_chain_with_attempt_timeout(
        cfg,
        Duration::from_secs(cfg.llm.attempt_timeout_seconds.max(1) as u64),
    )
}

/// 与 [`build_chain`] 相同，但可覆盖单次尝试的墙钟上限。
///
/// 逆向分析这类长输任务在推理模型上单次生成远超 45s，需要更宽的 hop 窗口。
pub fn build_chain_with_attempt_timeout(
    cfg: &AppConfig,
    attempt_timeout: Duration,
) -> ProviderChain {
    let mut providers: Vec<Arc<dyn LlmProvider>> = Vec::new();

    if let Some(key) = cfg.llm.api_key.clone() {
        providers.push(Arc::new(OpenAiCompatProvider::new(
            cfg.llm.provider.clone(),
            cfg.llm.base_url.clone(),
            cfg.llm.model.clone(),
            key,
            mode_for(&cfg.llm.provider),
        )));
    }

    for (index, spec) in cfg.llm.fallbacks.iter().enumerate() {
        let key: Option<SecretString> = cfg.llm.fallback_api_keys.get(index).cloned();
        let Some(key) = key else { continue };
        let (provider, model) = parse_fallback(spec);
        providers.push(Arc::new(OpenAiCompatProvider::new(
            provider.clone(),
            default_base_url(&provider),
            if model.is_empty() {
                default_model(&provider)
            } else {
                model
            },
            key,
            mode_for(&provider),
        )));
    }

    ProviderChain::new(providers, cfg.llm.max_retries, attempt_timeout)
}
