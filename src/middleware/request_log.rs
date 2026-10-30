// src/middleware/request_log.rs — request_id + 访问日志（§5.4）
// 位于链最外层（安全拒绝也留痕），与 §5.4 顺序为可接受偏差；SSE/token query 不记录。
use axum::extract::Request;
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

pub async fn request_log(mut req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get(&REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
        req.headers_mut().insert(REQUEST_ID.clone(), value);
    }

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(str::to_string);

    let started = std::time::Instant::now();
    let mut res = next.run(req).await;
    let elapsed = started.elapsed();

    if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
        res.headers_mut().insert(REQUEST_ID.clone(), value);
    }

    // SSE / 含 token 的 query 不记录 query（D-26 护栏）
    let log_query = match query {
        Some(q) if !is_sensitive_path(&path) && !q.contains("token=") => format!("?{q}"),
        _ => String::new(),
    };
    if elapsed.as_millis() > 1000 {
        tracing::warn!(
            request_id = %request_id,
            method = %method,
            path = %format!("{path}{log_query}"),
            status = res.status().as_u16(),
            elapsed_ms = elapsed.as_millis() as u64,
            "slow request"
        );
    } else {
        tracing::info!(
            request_id = %request_id,
            method = %method,
            path = %format!("{path}{log_query}"),
            status = res.status().as_u16(),
            elapsed_ms = elapsed.as_millis() as u64,
            "http"
        );
    }
    res
}

pub fn is_sensitive_path(path: &str) -> bool {
    path.ends_with("/events")
}
