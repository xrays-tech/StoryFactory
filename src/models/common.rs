// src/models/common.rs — 分页容器与校验辅助（§6.2 通用约定）
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 列表返回容器：{ items, total, page, page_size }。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// 分页参数解析：默认 1/20, page_size 上限 100（§6.2）, 非法/0 归默认。
/// 返回 (page, page_size, limit, offset); 超限 page 仍返回空 items。
pub fn resolve_page(page: i64, page_size: i64) -> (i64, i64, i64, i64) {
    let ps = if page_size <= 0 {
        20
    } else if page_size > 100 {
        100
    } else {
        page_size
    };
    let p = if page <= 0 { 1 } else { page };
    (p, ps, ps, (p - 1) * ps)
}

/// 构造分页容器（表达式泛型构造受限, 使用工厂函数）。
pub fn mk_page<T>(items: Vec<T>, total: i64, page: i64, page_size: i64) -> Page<T> {
    Page {
        items,
        total,
        page,
        page_size,
    }
}

/// 校验明细条目（ValidationFailed.details 结构 [{field,message}]，§5.3）。
pub fn validation_details<A, B>(fields: Vec<(A, B)>) -> serde_json::Value
where
    A: Into<String>,
    B: Into<String>,
{
    let items: Vec<serde_json::Value> = fields
        .into_iter()
        .map(|(field, message)| {
            let field: String = field.into();
            let message: String = message.into();
            serde_json::json!({ "field": field, "message": message })
        })
        .collect();
    serde_json::json!(items)
}
