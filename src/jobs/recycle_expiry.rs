// src/jobs/recycle_expiry.rs — 回收站自动过期（§4.7）
use sqlx::sqlite::SqlitePool;

use crate::models::enums::RecycleEntity;
use crate::services::recycle;

pub async fn run_once(pool: &SqlitePool, retention_days: i64) -> i64 {
    let cutoff = format!("strftime('%s','now') - {}", retention_days.max(1) * 86400);
    let tables = [
        ("novel", "novels"),
        ("volume", "volumes"),
        ("chapter", "chapters"),
        ("character", "characters"),
        ("item", "items"),
    ];
    let mut purged = 0i64;
    for (entity, table) in tables {
        let sql = format!(
            "SELECT id FROM {table} WHERE deleted_at IS NOT NULL AND deleted_at < {cutoff}"
        );
        let ids: Result<Vec<String>, _> = sqlx::query_scalar(&sql).fetch_all(pool).await;
        let ids = match ids {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, table, "recycle expiry scan failed");
                continue;
            }
        };
        let kind = RecycleEntity::from_db_or_default(entity.to_string());
        for id in ids {
            if recycle::purge(pool, kind, &id).await.is_ok() {
                purged += 1;
            }
        }
    }
    purged
}
