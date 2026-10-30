// src/config.rs — 多层配置加载与校验（§5.2 + D-08 优先级）
// 优先级（高→低）: [DB app_config 热键(P2 运行时)] > APP_* env > .env > config.toml > 内置默认
// dotenvy 注入 .env 时不覆盖已有 env => 两者均走 APP_* 层, 优先级自动正确.
// D-28: http.dev_cors=true => CORS 仅放行 http://localhost:5173; 默认 false = 生产 deny-all.
// 密钥仅 env: APP_LLM_API_KEY / APP_LLM_FALLBACK_API_KEY_<N>; 文件层不可写入(serde skip); 展示掩码.
use std::path::Path;

use config::{Config, File, FileFormat};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;
use validator::Validate;

pub type SecretKey = secrecy::SecretString;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("配置校验失败:\n{0}")]
    Invalid(String),
    #[error("配置加载失败: {0}")]
    Load(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub bind: String,     // 127.0.0.1:8080（0.0.0.0 强制令牌 §12.1）
    pub data_dir: String, // "" = 平台默认（XDG_DATA_HOME / ~/.local/share）
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub recycle: RecycleConfig,
    #[serde(default)]
    pub cost: CostConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default = "default_fts_min")]
    pub fts_min_query_length: usize, // 附录 B: 2
    #[serde(default)]
    pub warnings: Vec<String>, // 加载期 warnings（未知键等), 启动时 log warn
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    pub provider: String,             // deepseek
    pub base_url: String,             // https://api.deepseek.com/v1
    pub model: String,                // deepseek-chat
    pub fallbacks: Vec<String>,       // ["openai:gpt-4o-mini"]
    pub connect_timeout_seconds: u32, // 5
    pub attempt_timeout_seconds: u32, // 45（§7.2）
    pub max_retries: u32,             // 2
    #[serde(skip, default = "no_api_key")]
    pub api_key: Option<SecretKey>,
    #[serde(skip, default = "no_fallback_keys")]
    pub fallback_api_keys: Vec<SecretKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct SandboxConfig {
    #[validate(range(min = 1, max = 16))] // §5.2 启动校验: 并发数 ≤ 16
    pub max_concurrent: u32, // 4
    pub max_steps: u32,                     // 20（创建可覆盖 1..=60）
    pub step_timeout_seconds: u32,          // 90
    pub session_timeout_seconds: u32,       // 1800
    pub history_max_tokens: u32,            // 6000
    pub context_hard_cap_tokens: u32,       // 12000
    pub rolling_summary_max_tokens: u32,    // 500
    pub director_repeat_limit: u32,         // 3
    pub waiting_input_timeout_seconds: u32, // 300
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpConfig {
    pub body_limit_bytes: u32,        // 2097152（2MB）
    pub import_body_limit_bytes: u32, // 67108864（64MB, import 路由级 §5.4）
    pub sse_heartbeat_seconds: u32,   // 15
    pub dev_cors: bool,               // D-28
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecycleConfig {
    pub retention_days: u32, // 30
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostConfig {
    pub monthly_budget_usd: f64, // 0.0 = 不限
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogConfig {
    pub level: String,      // info
    pub trace_prompt: bool, // false（§12.5）
}

fn default_fts_min() -> usize {
    2
}
fn no_api_key() -> Option<SecretKey> {
    None
}
fn no_fallback_keys() -> Vec<SecretKey> {
    Vec::new()
}

// ============================================================
// 内置默认（附录 B 全键）+ APP_* 映射
// ============================================================

pub fn defaults_toml() -> String {
    "
bind = \"127.0.0.1:8080\"
data_dir = \"\"
fts_min_query_length = 2

[llm]
provider = \"deepseek\"
base_url = \"https://api.deepseek.com/v1\"
model = \"deepseek-chat\"
fallbacks = []
connect_timeout_seconds = 5
attempt_timeout_seconds = 45
max_retries = 2

[sandbox]
max_concurrent = 4
max_steps = 20
step_timeout_seconds = 90
session_timeout_seconds = 1800
history_max_tokens = 6000
context_hard_cap_tokens = 12000
rolling_summary_max_tokens = 500
director_repeat_limit = 3
waiting_input_timeout_seconds = 300

[http]
body_limit_bytes = 2097152
import_body_limit_bytes = 67108864
sse_heartbeat_seconds = 15
dev_cors = false

[recycle]
retention_days = 30

[cost]
monthly_budget_usd = 0.0

[log]
level = \"info\"
trace_prompt = false

fts_min_query_length = 2
"
    .to_string()
}

// (dotted key, APP_* env name)
const ENV_MAP: [(&str, &str); 27] = [
    ("bind", "APP_BIND"),
    ("data_dir", "APP_DATA_DIR"),
    ("llm.provider", "APP_LLM__PROVIDER"),
    ("llm.base_url", "APP_LLM__BASE_URL"),
    ("llm.model", "APP_LLM__MODEL"),
    ("llm.fallbacks", "APP_LLM__FALLBACKS"),
    (
        "llm.connect_timeout_seconds",
        "APP_LLM__CONNECT_TIMEOUT_SECONDS",
    ),
    (
        "llm.attempt_timeout_seconds",
        "APP_LLM__ATTEMPT_TIMEOUT_SECONDS",
    ),
    ("llm.max_retries", "APP_LLM__MAX_RETRIES"),
    ("sandbox.max_concurrent", "APP_SANDBOX__MAX_CONCURRENT"),
    ("sandbox.max_steps", "APP_SANDBOX__MAX_STEPS"),
    (
        "sandbox.step_timeout_seconds",
        "APP_SANDBOX__STEP_TIMEOUT_SECONDS",
    ),
    (
        "sandbox.session_timeout_seconds",
        "APP_SANDBOX__SESSION_TIMEOUT_SECONDS",
    ),
    (
        "sandbox.history_max_tokens",
        "APP_SANDBOX__HISTORY_MAX_TOKENS",
    ),
    (
        "sandbox.context_hard_cap_tokens",
        "APP_SANDBOX__CONTEXT_HARD_CAP_TOKENS",
    ),
    (
        "sandbox.rolling_summary_max_tokens",
        "APP_SANDBOX__ROLLING_SUMMARY_MAX_TOKENS",
    ),
    (
        "sandbox.director_repeat_limit",
        "APP_SANDBOX__DIRECTOR_REPEAT_LIMIT",
    ),
    (
        "sandbox.waiting_input_timeout_seconds",
        "APP_SANDBOX__WAITING_INPUT_TIMEOUT_SECONDS",
    ),
    ("http.body_limit_bytes", "APP_HTTP__BODY_LIMIT_BYTES"),
    (
        "http.import_body_limit_bytes",
        "APP_HTTP__IMPORT_BODY_LIMIT_BYTES",
    ),
    (
        "http.sse_heartbeat_seconds",
        "APP_HTTP__SSE_HEARTBEAT_SECONDS",
    ),
    ("http.dev_cors", "APP_HTTP__DEV_CORS"),
    ("recycle.retention_days", "APP_RECYCLE__RETENTION_DAYS"),
    ("cost.monthly_budget_usd", "APP_COST__MONTHLY_BUDGET_USD"),
    ("log.level", "APP_LOG__LEVEL"),
    ("log.trace_prompt", "APP_LOG__TRACE_PROMPT"),
    ("fts_min_query_length", "APP_FTS_MIN_QUERY_LENGTH"),
];

// ============================================================
// load / env 层 / 校验
// ============================================================

pub fn load() -> Result<AppConfig, ConfigError> {
    // (1) .env -> env：优先数据目录（D-08），回退 CWD
    let data_env = Path::new(&data_dir_from_env_or_default()).join(".env");
    if dotenvy::from_path(&data_env).is_err() {
        let _ = dotenvy::from_path(".env");
    }

    // (2) config.toml 在数据目录（APP_DATA_DIR env 时确定）
    let data_dir_for_file = data_dir_from_env_or_default();
    let cfg_path = Path::new(data_dir_for_file.as_str()).join("config.toml");

    // (3) 默认层 -> 文件层
    let builder = Config::builder();
    let with_defaults =
        builder.add_source(File::from_str(defaults_toml().as_str(), FileFormat::Toml));
    let with_file = with_defaults.add_source(File::from(cfg_path).required(false));
    let built = with_file.build();
    let mut app = match built {
        Err(e) => return Err(ConfigError::Load(format!("config 合并失败: {e}", e = e))),
        Ok(cfg) => {
            let d = cfg.try_deserialize::<AppConfig>();
            match d {
                Err(e2) => {
                    return Err(ConfigError::Load(format!("config 类型错误: {e2}", e2 = e2)))
                }
                Ok(v) => v,
            }
        }
    };

    // (4) APP_* env 层（dotenvy 注入的 .env 值亦走此层 => env > .env）
    apply_env_overrides(&mut app);
    apply_secret_env(&mut app);

    // (5) 启动校验（失败拒绝启动）
    let msgs = validate_config(&app);
    if !msgs.is_empty() {
        let joined = msgs.join("\n  - ");
        return Err(ConfigError::Invalid(format!(
            "  - {joined}",
            joined = joined
        )));
    }
    Ok(app)
}

fn data_dir_from_env_or_default() -> String {
    if let Ok(v) = std::env::var("APP_DATA_DIR") {
        return v;
    }
    default_data_dir()
}

fn default_data_dir() -> String {
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        return format!("{v}/story-architect", v = v);
    }
    if let Ok(v) = std::env::var("HOME") {
        return format!("{v}/.local/share/story-architect", v = v);
    }
    "./story-architect-data".to_string()
}

fn apply_env_overrides(app: &mut AppConfig) {
    for entry in ENV_MAP {
        let dotted = entry.0;
        let env_name = entry.1;
        match std::env::var(env_name) {
            Err(_) => {}
            Ok(raw) => apply_kv(app, dotted.to_string(), raw),
        }
    }
}

fn apply_kv(app: &mut AppConfig, key: String, raw: String) {
    if key == "bind" {
        app.bind = raw.to_string();
    } else if key == "data_dir" {
        app.data_dir = raw.to_string();
    } else if key == "llm.provider" {
        app.llm.provider = raw.to_string();
    } else if key == "llm.base_url" {
        app.llm.base_url = raw.to_string();
    } else if key == "llm.model" {
        app.llm.model = raw.to_string();
    } else if key == "llm.fallbacks" {
        app.llm.fallbacks = split_list(raw);
    } else if key == "log.level" {
        app.log.level = raw.to_string();
    } else if key == "llm.connect_timeout_seconds" {
        assign_num(
            &mut app.llm.connect_timeout_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "llm.attempt_timeout_seconds" {
        assign_num(
            &mut app.llm.attempt_timeout_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "llm.max_retries" {
        assign_num(&mut app.llm.max_retries, raw, key, &mut app.warnings);
    } else if key == "sandbox.max_concurrent" {
        assign_num(&mut app.sandbox.max_concurrent, raw, key, &mut app.warnings);
    } else if key == "sandbox.max_steps" {
        assign_num(&mut app.sandbox.max_steps, raw, key, &mut app.warnings);
    } else if key == "sandbox.step_timeout_seconds" {
        assign_num(
            &mut app.sandbox.step_timeout_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.session_timeout_seconds" {
        assign_num(
            &mut app.sandbox.session_timeout_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.history_max_tokens" {
        assign_num(
            &mut app.sandbox.history_max_tokens,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.context_hard_cap_tokens" {
        assign_num(
            &mut app.sandbox.context_hard_cap_tokens,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.rolling_summary_max_tokens" {
        assign_num(
            &mut app.sandbox.rolling_summary_max_tokens,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.director_repeat_limit" {
        assign_num(
            &mut app.sandbox.director_repeat_limit,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "sandbox.waiting_input_timeout_seconds" {
        assign_num(
            &mut app.sandbox.waiting_input_timeout_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "http.body_limit_bytes" {
        assign_num(&mut app.http.body_limit_bytes, raw, key, &mut app.warnings);
    } else if key == "http.import_body_limit_bytes" {
        assign_num(
            &mut app.http.import_body_limit_bytes,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "http.sse_heartbeat_seconds" {
        assign_num(
            &mut app.http.sse_heartbeat_seconds,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "http.dev_cors" {
        assign_bool(&mut app.http.dev_cors, raw, key, &mut app.warnings);
    } else if key == "recycle.retention_days" {
        assign_num(&mut app.recycle.retention_days, raw, key, &mut app.warnings);
    } else if key == "log.trace_prompt" {
        assign_bool(&mut app.log.trace_prompt, raw, key, &mut app.warnings);
    } else if key == "cost.monthly_budget_usd" {
        assign_f64(
            &mut app.cost.monthly_budget_usd,
            raw,
            key,
            &mut app.warnings,
        );
    } else if key == "fts_min_query_length" {
        assign_sz(&mut app.fts_min_query_length, raw, key, &mut app.warnings);
    } else {
        app.warnings
            .push(format!("unknown env override key: {key}", key = key));
    }
}

fn split_list(raw: String) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for piece in raw.split(',') {
        let t = piece.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out
}

fn assign_num(target: &mut u32, raw: String, key: String, warnings: &mut Vec<String>) {
    let parsed = raw.parse::<u32>();
    match parsed {
        Ok(v) => *target = v,
        Err(_) => warnings.push(format!(
            "invalid numeric for {key}: {raw}",
            key = key,
            raw = raw
        )),
    }
}

fn assign_sz(target: &mut usize, raw: String, key: String, warnings: &mut Vec<String>) {
    let parsed = raw.parse::<usize>();
    match parsed {
        Ok(v) => *target = v,
        Err(_) => warnings.push(format!(
            "invalid numeric for {key}: {raw}",
            key = key,
            raw = raw
        )),
    }
}

fn assign_f64(target: &mut f64, raw: String, key: String, warnings: &mut Vec<String>) {
    let parsed = raw.parse::<f64>();
    match parsed {
        Ok(v) => *target = v,
        Err(_) => warnings.push(format!(
            "invalid numeric for {key}: {raw}",
            key = key,
            raw = raw
        )),
    }
}

fn assign_bool(target: &mut bool, raw: String, key: String, warnings: &mut Vec<String>) {
    match raw.parse::<bool>() {
        Ok(v) => *target = v,
        Err(_) => warnings.push(format!(
            "invalid bool for {key}: {raw}",
            key = key,
            raw = raw
        )),
    }
}

fn apply_secret_env(app: &mut AppConfig) {
    match std::env::var("APP_LLM_API_KEY") {
        Err(_) => {}
        Ok(v) => app.llm.api_key = Some(SecretKey::from(v)),
    }
    let mut idx = 1;
    while idx <= 8 {
        let name = format!("APP_LLM_FALLBACK_API_KEY_{idx}", idx = idx);
        match std::env::var(name) {
            Err(_) => {}
            Ok(v) => app.llm.fallback_api_keys.push(SecretKey::from(v)),
        }
        idx += 1;
    }
}

// ============================================================
// 启动校验（§5.2: 模型非空; attempt<step<=session; 并发<=16; retention>=1; bind 合法）
// ============================================================

fn validate_config(app: &AppConfig) -> Vec<String> {
    let mut msgs: Vec<String> = Vec::new();
    // validator(derive) 范围约束
    let vr = app.sandbox.validate();
    if vr.is_err() {
        msgs.push("sandbox.max_concurrent 必须介于 1..16（§5.2: 并发数 ≤16）".to_string());
    }
    if app.llm.model.trim().is_empty() {
        msgs.push("模型名称（llm.model）不能为空".to_string());
    }
    if app.llm.attempt_timeout_seconds >= app.sandbox.step_timeout_seconds {
        msgs.push(format!(
            "要求 llm.attempt_timeout_seconds < sandbox.step_timeout_seconds（当前 {a} >= {s}",
            a = app.llm.attempt_timeout_seconds,
            s = app.sandbox.step_timeout_seconds
        ));
    }
    if app.sandbox.step_timeout_seconds > app.sandbox.session_timeout_seconds {
        msgs.push(format!(
            "要求 sandbox.step_timeout_seconds <= sandbox.session_timeout_seconds（当前 {s} > {ss}",
            s = app.sandbox.step_timeout_seconds,
            ss = app.sandbox.session_timeout_seconds
        ));
    }
    if app.recycle.retention_days < 1 {
        msgs.push("recycle.retention_days 必须 ≥ 1".to_string());
    }
    if let Err(msg) = parse_bind(app.bind.clone()) {
        msgs.push(msg)
    }
    msgs
}

// bind 解析（host:port; 用于校验+host_guard）
pub struct BindAddr {
    pub host: String,
    pub port: u16,
}

pub fn parse_bind(bind: String) -> Result<BindAddr, String> {
    match bind.rfind(':') {
        None => Err(format!("bind 缺少 ':': {bind}", bind = bind)),
        Some(idx) => {
            let host = bind[0..idx].to_string();
            if host.is_empty() {
                return Err(format!("bind 缺少 host: {bind}", bind = bind));
            }
            let port_res = bind[idx + 1..].parse::<u16>();
            match port_res {
                Err(_) => Err(format!("bind port 非法: {bind}", bind = bind)),
                Ok(port) => {
                    if port == 0 {
                        return Err(format!("bind port 非法(0): {bind}", bind = bind));
                    }
                    Ok(BindAddr::new(host, port))
                }
            }
        }
    }
}

impl BindAddr {
    pub fn new(host: String, port: u16) -> BindAddr {
        BindAddr { host, port }
    }
}

// end of config.rs

// ============================================================
// 运行期配置补丁（PATCH /config 持久化到 app_config，启动时覆盖）
// ============================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimePatch {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub fallbacks: Option<Vec<String>>,
    pub max_concurrent: Option<u32>,
    pub monthly_budget_usd: Option<f64>,
    pub sse_heartbeat_seconds: Option<u32>,
    pub log_level: Option<String>,
    pub attempt_timeout_seconds: Option<u32>,
    pub step_timeout_seconds: Option<u32>,
    pub session_timeout_seconds: Option<u32>,
    pub retention_days: Option<u32>,
}

impl RuntimePatch {
    pub fn apply(&self, cfg: &mut AppConfig) -> Result<(), String> {
        if let Some(v) = &self.model {
            if v.trim().is_empty() {
                return Err("model 不能为空".to_string());
            }
            cfg.llm.model = v.clone();
        }
        if let Some(v) = &self.provider {
            cfg.llm.provider = v.clone();
        }
        if let Some(v) = &self.base_url {
            cfg.llm.base_url = v.clone();
        }
        if let Some(v) = &self.fallbacks {
            cfg.llm.fallbacks = v.clone();
        }
        if let Some(v) = self.max_concurrent {
            if !(1..=16).contains(&v) {
                return Err("max_concurrent 必须为 1..16".to_string());
            }
            cfg.sandbox.max_concurrent = v;
        }
        if let Some(v) = self.monthly_budget_usd {
            cfg.cost.monthly_budget_usd = v;
        }
        if let Some(v) = self.sse_heartbeat_seconds {
            cfg.http.sse_heartbeat_seconds = v.max(1);
        }
        if let Some(v) = &self.log_level {
            cfg.log.level = v.clone();
        }
        if let Some(v) = self.attempt_timeout_seconds {
            cfg.llm.attempt_timeout_seconds = v;
        }
        if let Some(v) = self.step_timeout_seconds {
            cfg.sandbox.step_timeout_seconds = v;
        }
        if let Some(v) = self.session_timeout_seconds {
            cfg.sandbox.session_timeout_seconds = v;
        }
        if let Some(v) = self.retention_days {
            cfg.recycle.retention_days = v.max(1);
        }
        if cfg.llm.attempt_timeout_seconds >= cfg.sandbox.step_timeout_seconds
            || cfg.sandbox.step_timeout_seconds > cfg.sandbox.session_timeout_seconds
        {
            return Err("要求 attempt < step <= session".to_string());
        }
        Ok(())
    }
}

pub async fn persist_runtime_patch(
    pool: &SqlitePool,
    patch: &RuntimePatch,
) -> Result<(), sqlx::Error> {
    let value = serde_json::to_string(patch).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO app_config(key, value, is_secret, updated_at) VALUES('runtime_config', ?, 0, strftime('%s','now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_runtime_patch(pool: &SqlitePool) -> Option<RuntimePatch> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM app_config WHERE key = 'runtime_config'")
            .fetch_optional(pool)
            .await
            .ok()?;
    serde_json::from_str(&value?).ok()
}
