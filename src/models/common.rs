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
    // page 为用户输入的无界 i64；饱和运算保证超大页码返回 i64::MAX 偏移（空结果）而不是溢出 panic。
    (p, ps, ps, (p - 1).saturating_mul(ps))
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

#[cfg(test)]
mod tests {
    use super::resolve_page;

    #[test]
    fn resolve_page_keeps_defaults_for_invalid_input() {
        assert_eq!(resolve_page(0, 20), (1, 20, 20, 0));
        assert_eq!(resolve_page(-5, 20), (1, 20, 20, 0));
        assert_eq!(resolve_page(1, 0), (1, 20, 20, 0));
        assert_eq!(resolve_page(1, -3), (1, 20, 20, 0));
    }

    #[test]
    fn resolve_page_handles_normal_values() {
        assert_eq!(resolve_page(1, 20), (1, 20, 20, 0));
        assert_eq!(resolve_page(2, 20), (2, 20, 20, 20));
        assert_eq!(resolve_page(3, 999), (3, 100, 100, 200));
    }

    #[test]
    fn resolve_page_saturates_instead_of_overflowing() {
        let (p, ps, limit, offset) = resolve_page(i64::MAX, 100);
        assert_eq!(p, i64::MAX);
        assert_eq!(ps, 100);
        assert_eq!(limit, 100);
        assert_eq!(offset, i64::MAX);
    }
}
