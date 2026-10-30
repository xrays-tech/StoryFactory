// src/db.rs — 连接池 + 迁移（D-01 运行时查询 / D-25 sqlx::migrate!）
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqliteSynchronous};
use sqlx::Sqlite;

pub fn db_path(data_dir: &str) -> String {
    Path::new(data_dir)
        .join("story-architect.db")
        .display()
        .to_string()
}

/// 每个连接统一 PRAGMA（§4.5）。
pub fn connect_options(data_dir: &str) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(db_path(data_dir))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        // FULL: 本地单用户应用，宁可每次提交多一次 fsync，也不接受断电/系统崩溃丢已提交数据。
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_millis(5000))
}

pub async fn open_pool(data_dir: &str) -> Result<SqlitePool, sqlx::Error> {
    sqlx::pool::PoolOptions::<Sqlite>::new()
        .max_connections(4)
        // 常驻至少 1 个连接：SQLite 只允许「最后一个连接」在关闭时 checkpoint 并删除 -wal。
        // 若连接池把连接全部回收（sqlx 默认 idle_timeout 10 分钟、min_connections 0），
        // 外部工具/第二个进程就可能成为「最后一个连接」而删掉 -wal，导致池内新旧连接
        // 指向不同 -wal inode，出现「已提交数据查不到」的幻读与丢数据（历史故障根因）。
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(connect_options(data_dir))
        .await
}

/// 编译期嵌入 migrations（不依赖运行目录）；失败拒绝启动。
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    let version = schema_version(pool).await?;
    // PRAGMA user_version 作为外部工具可读的版本镜像
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(pool)
        .await?;
    Ok(())
}

/// 当前 schema 版本（D-03）。
pub async fn schema_version(pool: &SqlitePool) -> anyhow::Result<u64> {
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(version.unwrap_or(0).max(0) as u64)
}

/// WAL checkpoint。`mode` 取 `PASSIVE` / `FULL` / `RESTART` / `TRUNCATE`。
///
/// 返回是否「全部帧已落盘」。busy 时记警告而不是静默忽略：静默会让调用方误以为
/// 主库文件已经自洽（历史故障里 code 522 就是这样被吞掉的）。
pub async fn checkpoint(pool: &SqlitePool, mode: &str) -> Result<bool, sqlx::Error> {
    let row: Option<(i64, i64, i64)> = sqlx::query_as(&format!("PRAGMA wal_checkpoint({mode})"))
        .fetch_optional(pool)
        .await?;
    match row {
        Some((0, log, checkpointed)) => {
            tracing::debug!(mode, log, checkpointed, "wal checkpoint ok");
            Ok(true)
        }
        Some((busy, log, checkpointed)) => {
            tracing::warn!(mode, busy, log, checkpointed, "wal checkpoint busy");
            Ok(false)
        }
        None => Ok(true),
    }
}

/// 启动备份：WAL checkpoint 后复制，保留 3 份（§4.3）。
///
/// 用 FULL 而非 TRUNCATE：FULL 已保证主库文件自洽（可安全复制），而 TRUNCATE 会截断
/// WAL，在仍有其它连接/进程打开同一库时足以让读者读到被截断的 WAL
/// （SQLITE_IOERR_SHORT_READ，即 code 522）。
pub async fn backup(pool: &SqlitePool, data_dir: &str) -> std::io::Result<()> {
    let path = db_path(data_dir);
    if !Path::new(&path).exists() {
        return Ok(());
    }
    // checkpoint 失败不算致命：备份文件最多落后一个 WAL，但仍可恢复；
    // 拒绝启动反而会让用户完全用不了应用。
    if let Err(e) = checkpoint(pool, "FULL").await {
        tracing::warn!(error = %e, "backup checkpoint failed; copying as-is");
    }
    let bak = format!("{path}.bak");
    let bak1 = format!("{path}.bak.1");
    let bak2 = format!("{path}.bak.2");
    let _ = std::fs::remove_file(&bak2);
    if Path::new(&bak1).exists() {
        let _ = std::fs::rename(&bak1, &bak2);
    }
    if Path::new(&bak).exists() {
        let _ = std::fs::rename(&bak, &bak1);
    }
    std::fs::copy(&path, &bak)?;
    Ok(())
}

/// 多行写事务统一入口（§4.5 `BEGIN IMMEDIATE`）。
pub async fn begin_immediate(
    pool: &SqlitePool,
) -> Result<sqlx::Transaction<'static, Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

/// 干净关闭：先 checkpoint 再关闭连接池，避免把 WAL 清理交给进程退出时机。
pub async fn close(pool: SqlitePool) {
    if let Err(e) = checkpoint(&pool, "FULL").await {
        tracing::warn!(error = %e, "final wal checkpoint failed");
    }
    pool.close().await;
}

/// 完整性快速检查（SQLite `quick_check`，返回 "ok" 或问题描述）。
pub async fn quick_check(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(pool)
        .await
}

/// 单实例锁：同一数据目录只允许一个进程打开，避免多进程共享 WAL。
///
/// 多进程打开同一 WAL 库时，任一方的 checkpoint（尤其 TRUNCATE）都可能让另一方读到
/// 被截断的 WAL；串行化启动是唯一可靠的防护。锁是 OS 级文件锁（flock/LockFileEx），
/// 进程被 SIGKILL 时由内核自动释放，不会留下死锁。
#[derive(Debug)]
pub struct InstanceLock {
    _file: std::fs::File,
    path: PathBuf,
}

impl InstanceLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn acquire_instance_lock(data_dir: &str) -> std::io::Result<InstanceLock> {
    let path = Path::new(data_dir).join("story-architect.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "数据目录已被另一个 StoryArchitect 实例占用（锁文件 {}）",
                    path.display()
                ),
            ));
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(e),
    }
    use std::io::Write;
    let _ = file.set_len(0);
    let _ = writeln!(&file, "pid={}", std::process::id());
    let _ = file.sync_all();
    Ok(InstanceLock { _file: file, path })
}

fn dirty_path(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join(".unclean-shutdown")
}

/// 启动成功后落标记；只有干净关闭才移除，用于识别「上次是异常终止」。
pub fn mark_running(data_dir: &str) -> std::io::Result<()> {
    std::fs::write(dirty_path(data_dir), b"running\n")
}

pub fn was_unclean(data_dir: &str) -> bool {
    dirty_path(data_dir).exists()
}

pub fn mark_clean(data_dir: &str) -> std::io::Result<()> {
    match std::fs::remove_file(dirty_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
