// src/state.rs — 共享 AppState（热配置 + 连接池 + 绑定信息）
use std::sync::Arc;

use arc_swap::ArcSwap;
use sqlx::sqlite::SqlitePool;

use crate::config::{parse_bind, AppConfig};
use crate::services::chapter_draft::DraftRegistry;
use crate::services::novel_import::{ImportRegistry, IMPORT_SEMAPHORE};
use crate::services::sandbox::SessionManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<AppConfig>>,
    pub pool: SqlitePool,
    pub bind_host: String,
    pub bind_port: u16,
    pub is_public: bool,
    pub app_token: String,
    pub origins: Vec<String>,
    pub dev_cors: bool,
    pub sessions: std::sync::Arc<SessionManager>,
    pub imports: Arc<ImportRegistry>,
    pub drafts: Arc<DraftRegistry>,
}

pub fn new_state(config: AppConfig, pool: SqlitePool, token: String) -> Result<AppState, String> {
    let addr = parse_bind(config.bind.clone())?;

    let mut origins = vec![
        format!("http://127.0.0.1:{}", addr.port),
        format!("http://localhost:{}", addr.port),
        format!("http://[::1]:{}", addr.port),
    ];
    if config.http.dev_cors {
        origins.push("http://localhost:5173".to_string());
    }

    let config_arc = Arc::new(ArcSwap::from_pointee(config.clone()));
    let sessions = Arc::new(SessionManager::new(
        pool.clone(),
        config_arc.clone(),
        config.sandbox.max_concurrent as usize,
    ));
    let imports = Arc::new(ImportRegistry::new(IMPORT_SEMAPHORE));
    let drafts = Arc::new(DraftRegistry::new(config_arc.clone(), pool.clone(), None));
    Ok(AppState {
        config: config_arc,
        pool,
        bind_host: addr.host.clone(),
        bind_port: addr.port,
        is_public: !is_loopback(&addr.host),
        app_token: token,
        origins,
        dev_cors: config.http.dev_cors,
        sessions,
        imports,
        drafts,
    })
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}
