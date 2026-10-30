// src/main.rs — 启动、迁移、优雅关闭（§3.3 / §5.7）
use std::path::PathBuf;

use story_architect::{config, db, jobs, llm, logging, router, state};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("StoryArchitect 启动失败: {e}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let mut cfg = config::load()?;
    let data_dir = effective_data_dir(&cfg.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(data_dir.join("logs"))?;

    let _log_guard = logging::init(&cfg.log.level, &data_dir.display().to_string());
    for warning in &cfg.warnings {
        tracing::warn!(warning = %warning, "config warning");
    }

    // 同一数据目录只允许一个进程：多进程共享 WAL 是历史丢数据的唯一异常条件。
    let data_dir_str = data_dir.display().to_string();
    let _instance_lock = match db::acquire_instance_lock(&data_dir_str) {
        Ok(lock) => {
            tracing::info!(lock = %lock.path().display(), "data dir lock acquired");
            lock
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context("无法获取数据目录独占锁"));
        }
    };

    let unclean = db::was_unclean(&data_dir_str);
    let pool = db::open_pool(&data_dir_str).await?;
    db::migrate(&pool).await?;
    if unclean {
        tracing::warn!("上次未正常关闭，正在检查数据库完整性");
        match db::quick_check(&pool).await {
            Ok(report) if report == "ok" => tracing::info!("sqlite quick_check ok"),
            Ok(report) => tracing::error!(report = %report, "sqlite quick_check 报告异常"),
            Err(e) => tracing::error!(error = %e, "sqlite quick_check 失败"),
        }
    }
    db::mark_running(&data_dir_str)?;
    if let Some(patch) = config::load_runtime_patch(&pool).await {
        if let Err(e) = patch.apply(&mut cfg) {
            tracing::warn!(error = %e, "runtime config patch rejected");
        }
    }
    llm::prompts::seed_defaults(&pool).await?;
    db::backup(&pool, &data_dir.display().to_string()).await?;
    let recovered = jobs::session_recovery::run_startup(&pool).await?;
    if recovered > 0 {
        tracing::warn!(recovered, "sessions marked interrupted after restart");
    }
    let interrupted_imports = jobs::novel_import_recovery::run_startup(&pool).await?;
    if interrupted_imports > 0 {
        tracing::warn!(
            interrupted_imports,
            "novel import jobs marked failed after restart"
        );
    }
    tracing::info!(db = %db::db_path(&data_dir.display().to_string()), "database ready");

    let addr = config::parse_bind(cfg.bind.clone()).map_err(anyhow::Error::msg)?;
    let token = if state::is_loopback(&addr.host) {
        String::new()
    } else {
        gen_token(&data_dir)?
    };
    let recycle_days = cfg.recycle.retention_days as i64;
    let app_state =
        state::new_state(cfg, pool.clone(), token.clone()).map_err(anyhow::Error::msg)?;
    let app = router::build_router(app_state);

    // 后台任务：回收站过期（每日）与幂等记录 TTL（10 分钟）
    let mut job_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let recycle_pool = pool.clone();
    job_handles.push(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let purged = jobs::recycle_expiry::run_once(&recycle_pool, recycle_days).await;
            if purged > 0 {
                tracing::info!(purged, "recycle expiry purged items");
            }
        }
    }));
    let zombie_pool = pool.clone();
    job_handles.push(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match jobs::session_zombie::run_once(&zombie_pool).await {
                Ok(n) if n > 0 => tracing::warn!(count = n, "zombie sessions marked interrupted"),
                Err(e) => tracing::warn!(error = %e, "zombie scan failed"),
                _ => {}
            }
        }
    }));
    let idem_pool = pool.clone();
    job_handles.push(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(600));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let removed = jobs::idempotency_cleanup::run_once(&idem_pool).await;
            if removed > 0 {
                tracing::info!(removed, "idempotency records expired");
            }
        }
    }));

    let listener = TcpListener::bind((addr.host.as_str(), addr.port)).await?;
    tracing::info!(address = %format!("{}:{}", addr.host, addr.port), "listening");

    if std::env::var("APP_NO_OPEN").unwrap_or_default() != "1" {
        // 0.0.0.0 不可直接浏览；改用回环地址并携带启动令牌（D-26）。
        let open_host = if addr.host == "0.0.0.0" {
            "127.0.0.1"
        } else {
            addr.host.as_str()
        };
        let url = if token.is_empty() {
            format!("http://{open_host}:{}", addr.port)
        } else {
            format!("http://{open_host}:{}?token={token}", addr.port)
        };
        std::thread::spawn(move || {
            let _ = webbrowser::open(&url);
        });
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // 关停序列：停后台任务 -> checkpoint -> 关闭连接池 -> 清 unclean 标记。
    for handle in job_handles {
        handle.abort();
    }
    if tokio::time::timeout(std::time::Duration::from_secs(5), db::close(pool))
        .await
        .is_err()
    {
        tracing::warn!("关闭连接池超时，交由强制退出兜底");
    }
    if let Err(e) = db::mark_clean(&data_dir_str) {
        tracing::warn!(error = %e, "清理 unclean 标记失败");
    }
    tracing::info!("shutdown complete");
    Ok(())
}

/// "" = 平台默认数据目录（§3.3）。实现委托给 `config::effective_data_dir`，
/// 保证配置层查找 `.env`/`config.toml` 的目录与数据写入目录始终一致。
pub fn effective_data_dir(configured: &str) -> PathBuf {
    config::effective_data_dir(configured)
}

/// 非回环绑定生成 32 hex 令牌并写入 0600 文件（D-29）。
pub fn gen_token(data_dir: &std::path::Path) -> std::io::Result<String> {
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    let path = data_dir.join("app.token");
    std::fs::write(&path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(token)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => tracing::warn!(error = %e, "SIGTERM handler unavailable"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
    // SSE 长连接可能让 graceful shutdown 无限等待：兜底强制退出，避免进程卡死后再被启动
    // 第二个实例（历史上正是这样出现了同一数据目录的双进程）。
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        tracing::warn!("graceful shutdown timed out; forcing exit");
        std::process::exit(0);
    });
}
